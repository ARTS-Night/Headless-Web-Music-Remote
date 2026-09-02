mod audio;

use std::{
    collections::HashMap,
    ffi::c_void,
    net::{Ipv4Addr, SocketAddr, UdpSocket},
    os::windows::io::AsRawHandle,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use axum::{
    Router,
    body::Body,
    extract::{
        ConnectInfo, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, HeaderValue, Method, Request, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use qrcodegen::{QrCode, QrCodeEcc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::{
    sync::{Mutex, oneshot, watch},
    time::sleep,
};
use tokio_tungstenite::tungstenite::Message as CdpMessage;
use tower_http::cors::CorsLayer;
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, POINT},
    System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    },
    UI::WindowsAndMessaging::{GetCursorPos, GetForegroundWindow},
};

const VIEWPORT: &str = "430,932";
const CDP_PORT: u16 = 9229;
const PWA_ORIGIN: &str = "https://arts-night.github.io";
const PWA_DEEP_LINK: &str = "https://arts-night.github.io/Headless-Web-Music-Remote/";

async fn local_network_access_header(request: Request<Body>, next: Next) -> Response {
    let pwa_request = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        == Some(PWA_ORIGIN);
    let mut response = next.run(request).await;
    if pwa_request {
        response.headers_mut().insert(
            "Access-Control-Allow-Private-Network",
            HeaderValue::from_static("true"),
        );
    }
    response
}
const MOBILE_USER_AGENT: &str = "Mozilla/5.0 (Linux; Android 15; Pixel 9) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/152.0.0.0 Mobile Safari/537.36";

struct BraveJob(HANDLE);

impl BraveJob {
    fn attach(child: &mut std::process::Child) -> Result<Self> {
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job.is_null() {
            bail!("create HWMR Brave job: {}", std::io::Error::last_os_error());
        }
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let configured = unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const c_void,
                std::mem::size_of_val(&limits) as u32,
            )
        };
        let assigned = configured != 0
            && unsafe { AssignProcessToJobObject(job, child.as_raw_handle() as HANDLE) } != 0;
        if !assigned {
            let error = std::io::Error::last_os_error();
            let _ = child.kill();
            unsafe { CloseHandle(job) };
            bail!("assign HWMR Brave to its lifetime job: {error}");
        }
        Ok(Self(job))
    }
}

impl Drop for BraveJob {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

fn host_port() -> u16 {
    std::env::var("HWMR_HOST_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|port| *port != 0)
        .unwrap_or(8787)
}

fn device_metrics(width: u32, height: u32) -> Value {
    let width = width.clamp(1, 1920);
    let height = height.clamp(1, 2400);
    json!({"width":width,"height":height,"screenWidth":width,"screenHeight":height,"deviceScaleFactor":1,"mobile":true})
}

fn profile_dir() -> Result<PathBuf> {
    let default = PathBuf::from(std::env::var("LOCALAPPDATA").context("LOCALAPPDATA unavailable")?)
        .join("HWMR")
        .join("browser-profile-v7");
    Ok(std::env::var_os("HWMR_PROFILE_DIR")
        .map(PathBuf::from)
        .unwrap_or(default))
}

fn brave_path() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("HWMR_BRAVE_PATH").map(PathBuf::from) {
        if path.is_file() {
            return Ok(path);
        }
        bail!("Brave not found at HWMR_BRAVE_PATH: {}", path.display());
    }
    let mut candidates = vec![PathBuf::from(
        r"C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe",
    )];
    if let Some(program_files) = std::env::var_os("ProgramFiles(x86)") {
        candidates.push(
            PathBuf::from(program_files)
                .join("BraveSoftware\\Brave-Browser\\Application\\brave.exe"),
        );
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(
            PathBuf::from(local).join("BraveSoftware\\Brave-Browser\\Application\\brave.exe"),
        );
    }
    candidates.into_iter().find(|path| path.is_file()).context(
        "Brave was not found. Install Brave in the standard location or set HWMR_BRAVE_PATH.",
    )
}

fn lan_address() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("1.1.1.1:80").ok()?;
    match socket.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(address) if address.is_private() || address.is_link_local() => {
            Some(address)
        }
        _ => None,
    }
}

fn jpeg_quality() -> u8 {
    std::env::var("HWMR_JPEG_QUALITY")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|v| (1..=100).contains(v))
        .unwrap_or(60)
}

fn screencast_params(width: u32, height: u32) -> Value {
    let every_nth = std::env::var("HWMR_EVERY_NTH_FRAME")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|v| *v > 0)
        .unwrap_or(2);
    json!({"format":"jpeg", "quality":jpeg_quality(), "maxWidth":width.clamp(1, 1920), "maxHeight":height.clamp(1, 2400), "everyNthFrame":every_nth})
}

#[derive(Clone)]
struct App {
    port: u16,
    active_id: Arc<Mutex<String>>,
    active: Arc<Mutex<Cdp>>,
    frame_tx: watch::Sender<Vec<u8>>,
    frames: watch::Receiver<Vec<u8>>,
    viewport: Arc<Mutex<(u32, u32)>>,
    auth: Arc<Mutex<Auth>>,
}
struct Auth {
    code: String,
    token: Option<String>,
    qr_nonce: Option<QrNonce>,
}
struct QrNonce {
    nonce: String,
    expires_at: Instant,
}
#[derive(Deserialize)]
struct PairRequest {
    code: Option<String>,
    nonce: Option<String>,
}

#[derive(Clone)]
struct Cdp {
    writer: Arc<
        Mutex<
            futures_util::stream::SplitSink<
                tokio_tungstenite::WebSocketStream<
                    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
                >,
                CdpMessage,
            >,
        >,
    >,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>,
    next_id: Arc<Mutex<u64>>,
}

#[derive(Deserialize)]
struct Target {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    title: String,
    url: String,
    #[serde(rename = "webSocketDebuggerUrl")]
    websocket: String,
}

#[derive(Deserialize)]
struct BrowserVersion {
    #[serde(rename = "webSocketDebuggerUrl")]
    websocket: String,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Control {
    Tap { x: f64, y: f64 },
    Scroll { x: f64, y: f64, delta_y: f64 },
    Resize { width: u32, height: u32 },
    Back,
    Forward,
    Reload,
    Navigate { url: String },
    Go { text: String },
    Key { key: String },
    InsertText { text: String },
    Screencast { enabled: bool },
    SelectTab { id: String },
}

#[derive(Serialize)]
struct Status<'a> {
    status: &'a str,
}

#[tokio::main]
async fn main() -> Result<()> {
    if std::env::args().skip(1).any(|arg| arg == "--show-qr") {
        return show_qr().await;
    }
    let profile = profile_dir()?;
    std::fs::create_dir_all(&profile)?;
    let brave = brave_path()?;
    let (port, _brave_job) = launch_brave(&brave, &profile).await?;
    let target = page_target(port).await?;
    let active_id = Arc::new(Mutex::new(target.id.clone()));
    let code = random_hex(3);
    let (frame_tx, frames) = watch::channel(Vec::new());
    let cdp = Cdp::connect(
        target.websocket,
        target.id.clone(),
        active_id.clone(),
        frame_tx.clone(),
    )
    .await?;
    cdp.command("Page.navigate", json!({"url":"https://www.youtube.com/"}))
        .await?;
    cdp.command("Page.enable", json!({})).await?;
    cdp.command("Page.startScreencast", screencast_params(430, 932))
        .await?;
    let visibility = cdp
        .command(
            "Runtime.evaluate",
            json!({"expression":"JSON.stringify({visibilityState:document.visibilityState,hidden:document.hidden})","returnByValue":true}),
        )
        .await?;
    println!(
        "Headless page visibility: {}",
        visibility["result"]["result"]["value"]
    );
    let app = App {
        port,
        active_id,
        active: Arc::new(Mutex::new(cdp)),
        frame_tx,
        frames,
        viewport: Arc::new(Mutex::new((430, 932))),
        auth: Arc::new(Mutex::new(Auth {
            code: code.clone(),
            token: None,
            qr_nonce: None,
        })),
    };
    let reconcile = app.clone();
    tokio::spawn(async move {
        loop {
            if let Ok(targets) = page_targets(reconcile.port).await {
                let active = reconcile.active_id.lock().await.clone();
                if !targets.iter().any(|target| target.id == active) {
                    if let Some(target) = targets.first() {
                        let _ = select_tab(&reconcile, &target.id).await;
                    } else {
                        *reconcile.active_id.lock().await = String::new();
                    }
                }
            }
            sleep(Duration::from_secs(1)).await;
        }
    });
    let router = Router::new()
        .route("/", get(index))
        .route("/health", get(health))
        .route("/pair", post(pair))
        .route("/pair/qr", post(qr_pair))
        .route("/logout", post(logout))
        .route("/audio", get(audio_status))
        .route("/isolation", get(isolation_status))
        .route("/tabs", get(tabs))
        .route("/ws/frame", get(frame_ws))
        .route("/ws/control", get(control_ws))
        .layer(
            CorsLayer::new()
                .allow_origin(PWA_ORIGIN.parse::<HeaderValue>().unwrap())
                .allow_methods([Method::GET, Method::POST])
                .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE]),
        )
        .layer(middleware::from_fn(local_network_access_header))
        .with_state(app.clone());
    let host_port = host_port();
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", host_port)).await?;
    println!(
        "HWMR started\nProfile: {}\nLocal: http://127.0.0.1:{host_port}",
        profile.display()
    );
    if let Some(address) = lan_address() {
        println!("Phone (trusted LAN): http://{address}:{host_port}");
    } else {
        println!(
            "Phone: no private LAN address detected; use this PC's trusted-LAN IPv4 address and port {host_port}"
        );
    }
    println!(
        "Pairing code: {code}\nQR pairing: printed below (expires in 2 minutes)\nCDP: 127.0.0.1:{port} (loopback only)"
    );
    match issue_qr(&app).await {
        Ok(qr) => println!("QR pairing (expires in 2 minutes):\n{qr}"),
        Err(error) => println!("QR pairing unavailable: {error}"),
    }
    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await?;
    Ok(())
}

async fn launch_brave(brave: &Path, profile: &Path) -> Result<(u16, BraveJob)> {
    let port_file = profile.join("DevToolsActivePort");
    let port_guard = std::net::TcpListener::bind(("127.0.0.1", CDP_PORT))
        .with_context(|| format!("CDP loopback port {CDP_PORT} is already in use"))?;
    if port_file.exists() {
        std::fs::remove_file(&port_file)
            .with_context(|| format!("remove stale {}", port_file.display()))?;
    }
    drop(port_guard);
    let mut child = Command::new(brave)
        .args([
            "--headless",
            "--remote-debugging-address=127.0.0.1",
            "--remote-debugging-port=9229",
            "--no-first-run",
            &format!("--user-agent={MOBILE_USER_AGENT}"),
            &format!("--user-data-dir={}", profile.display()),
            &format!("--window-size={VIEWPORT}"),
            "about:blank",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("launch Headless Brave")?;
    let job = BraveJob::attach(&mut child)?;
    for _ in 0..40 {
        if let Ok(value) = std::fs::read_to_string(&port_file) {
            if let Some(port) = value.lines().next().and_then(|line| line.parse().ok()) {
                return Ok((port, job));
            }
        }
        if let Ok(response) = reqwest::get(format!("http://127.0.0.1:{CDP_PORT}/json/list")).await {
            if response.status().is_success() {
                let version: BrowserVersion =
                    reqwest::get(format!("http://127.0.0.1:{CDP_PORT}/json/version"))
                        .await?
                        .json()
                        .await?;
                let browser_path = url::Url::parse(&version.websocket)?.path().to_owned();
                std::fs::write(&port_file, format!("{CDP_PORT}\n{browser_path}\n"))?;
                return Ok((CDP_PORT, job));
            }
        }
        sleep(Duration::from_millis(250)).await;
    }
    bail!("Brave did not create DevToolsActivePort; check profile ownership and logs")
}

async fn page_target(port: u16) -> Result<Target> {
    for _ in 0..30 {
        if let Ok(response) = reqwest::get(format!("http://127.0.0.1:{port}/json/list")).await {
            let targets: Vec<Target> = response.json().await?;
            if let Some(target) = targets.into_iter().find(|target| target.kind == "page") {
                return Ok(target);
            }
        }
        sleep(Duration::from_millis(250)).await;
    }
    bail!("No CDP page target created")
}

async fn page_targets(port: u16) -> Result<Vec<Target>> {
    Ok(reqwest::get(format!("http://127.0.0.1:{port}/json/list"))
        .await?
        .json::<Vec<Target>>()
        .await?
        .into_iter()
        .filter(|target| target.kind == "page")
        .collect())
}

impl Cdp {
    async fn connect(
        url: String,
        target_id: String,
        active_id: Arc<Mutex<String>>,
        frame_tx: watch::Sender<Vec<u8>>,
    ) -> Result<Self> {
        let (socket, _) = tokio_tungstenite::connect_async(url).await?;
        let (writer, mut reader) = socket.split();
        let cdp = Self {
            writer: Arc::new(Mutex::new(writer)),
            pending: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(Mutex::new(0)),
        };
        let responses = cdp.pending.clone();
        let acker = cdp.writer.clone();
        tokio::spawn(async move {
            while let Some(Ok(CdpMessage::Text(text))) = reader.next().await {
                let Ok(message): Result<Value, _> = serde_json::from_str(&text) else {
                    continue;
                };
                if let Some(id) = message.get("id").and_then(Value::as_u64) {
                    if let Some(tx) = responses.lock().await.remove(&id) {
                        let _ = tx.send(message);
                    }
                } else if message.get("method").and_then(Value::as_str)
                    == Some("Page.screencastFrame")
                {
                    let params = &message["params"];
                    if let Some(data) = params["data"].as_str().and_then(|data| {
                        base64::engine::general_purpose::STANDARD.decode(data).ok()
                    }) {
                        if *active_id.lock().await == target_id {
                            let _ = frame_tx.send(data);
                        }
                    }
                    if let Some(session_id) = params["sessionId"].as_u64() {
                        let ack = json!({"id": 0, "method":"Page.screencastFrameAck", "params":{"sessionId":session_id}});
                        let _ = acker
                            .lock()
                            .await
                            .send(CdpMessage::Text(ack.to_string().into()))
                            .await;
                    }
                }
            }
        });
        cdp.command(
            "Emulation.setDeviceMetricsOverride",
            device_metrics(430, 932),
        )
        .await?;
        Ok(cdp)
    }

    async fn command(&self, method: &str, params: Value) -> Result<Value> {
        let mut id = self.next_id.lock().await;
        *id += 1;
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(*id, tx);
        self.writer
            .lock()
            .await
            .send(CdpMessage::Text(
                json!({"id":*id,"method":method,"params":params})
                    .to_string()
                    .into(),
            ))
            .await?;
        let reply = tokio::time::timeout(Duration::from_secs(10), rx)
            .await
            .context("CDP command timed out")??;
        if reply.get("error").is_some() {
            bail!("CDP {method} failed: {}", reply["error"]);
        }
        Ok(reply)
    }
}

async fn index() -> Html<&'static str> {
    Html(include_str!("../web/index.html"))
}
async fn health() -> impl IntoResponse {
    axum::Json(Status { status: "ok" })
}
fn qr_text(payload: &str) -> Result<String> {
    let qr = QrCode::encode_text(payload, QrCodeEcc::Medium)
        .map_err(|_| anyhow::anyhow!("QR payload is too large"))?;
    let size = qr.size();
    let mut text = String::new();
    // Terminals normally use a black background, so render dark modules as
    // spaces and light modules as blocks. Four modules of quiet zone are
    // required by QR readers.
    for y in -4..size + 4 {
        let row: String = (-4..size + 4)
            .map(|x| {
                let dark = x >= 0 && y >= 0 && x < size && y < size && qr.get_module(x, y);
                if dark { "  " } else { "██" }
            })
            .collect();
        text.push_str(&row);
        text.push('\n');
    }
    Ok(text)
}

fn deep_link(host: Ipv4Addr, port: u16, nonce: &str) -> String {
    let payload = json!({
        "v": 1,
        "host": host.to_string(),
        "port": port,
        "nonce": nonce
    })
    .to_string();
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload);
    format!("{PWA_DEEP_LINK}#connect={encoded}")
}

async fn show_qr() -> Result<()> {
    let response = reqwest::Client::new()
        .post(format!("http://127.0.0.1:{}/pair/qr", host_port()))
        .send()
        .await
        .context("request QR pairing from HWMR")?
        .error_for_status()
        .context("start HWMR first, then run hwmr.exe --show-qr")?;
    let body = response.json::<Value>().await?;
    let qr = body["qr"].as_str().context("HWMR returned no QR code")?;
    println!("{qr}\nScan this QR with your phone. It expires in 2 minutes.");
    Ok(())
}

async fn issue_qr(app: &App) -> Result<String> {
    let address = lan_address().context("no trusted LAN address detected")?;
    let nonce = random_hex(16);
    let qr = qr_text(&deep_link(address, host_port(), &nonce))?;
    let mut auth = app.auth.lock().await;
    auth.qr_nonce = Some(QrNonce {
        nonce,
        expires_at: Instant::now() + Duration::from_secs(120),
    });
    Ok(qr)
}

async fn qr_pair(
    ConnectInfo(client): ConnectInfo<SocketAddr>,
    State(app): State<App>,
) -> impl IntoResponse {
    if !client.ip().is_loopback() {
        return StatusCode::FORBIDDEN.into_response();
    }
    match issue_qr(&app).await {
        Ok(qr) => axum::Json(json!({"ok": true, "qr": qr})).into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, error.to_string()).into_response(),
    }
}

async fn pair(
    State(app): State<App>,
    axum::Json(request): axum::Json<PairRequest>,
) -> impl IntoResponse {
    let mut auth = app.auth.lock().await;
    let valid_code = request.code.as_deref() == Some(&auth.code);
    let valid_nonce = if let Some(nonce) = &request.nonce {
        auth.qr_nonce
            .as_ref()
            .is_some_and(|qn| &qn.nonce == nonce && qn.expires_at > Instant::now())
    } else {
        false
    };

    if !valid_code && !valid_nonce {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            axum::Json(json!({"ok":false,"error":"invalid_pairing_code"})),
        )
            .into_response();
    }
    let token = random_hex(32);
    auth.code = random_hex(3);
    auth.token = Some(token.clone());
    auth.qr_nonce = None;
    axum::Json(json!({"ok":true,"token":token})).into_response()
}
async fn logout(State(app): State<App>, headers: HeaderMap) -> impl IntoResponse {
    if !authorized(&headers, &app).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let mut auth = app.auth.lock().await;
    auth.code = random_hex(3);
    auth.token = None;
    auth.qr_nonce = None;
    println!("Logged out. New pairing code: {}", auth.code);
    axum::Json(json!({"ok": true})).into_response()
}
async fn authorized(headers: &HeaderMap, app: &App) -> bool {
    let Some(token) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
    else {
        return false;
    };
    app.auth.lock().await.token.as_deref() == Some(token)
}
async fn audio_status(State(app): State<App>, headers: HeaderMap) -> impl IntoResponse {
    if !authorized(&headers, &app).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match audio::sessions() {
        Ok(sessions) => axum::Json(serde_json::json!({"sessions": sessions})).into_response(),
        Err(error) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            error.to_string(),
        )
            .into_response(),
    }
}
async fn isolation_status(State(app): State<App>, headers: HeaderMap) -> impl IntoResponse {
    if !authorized(&headers, &app).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let state = window_state();
    axum::Json(
        serde_json::json!({"foreground_window": state.0, "cursor": {"x": state.1, "y": state.2}}),
    )
    .into_response()
}
async fn tabs(State(app): State<App>, headers: HeaderMap) -> impl IntoResponse {
    if !authorized(&headers, &app).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match page_targets(app.port).await {
        Ok(items) => {
            let active = app.active_id.lock().await.clone();
            axum::Json(serde_json::json!({"type":"tabs", "items": items.into_iter().map(|target| json!({"id":target.id,"title":target.title,"url":target.url,"active":target.id == active})).collect::<Vec<_>>() })).into_response()
        }
        Err(error) => (axum::http::StatusCode::BAD_GATEWAY, error.to_string()).into_response(),
    }
}
fn ws_origin_allowed(headers: &HeaderMap) -> bool {
    let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    else {
        return true;
    };

    origin == PWA_ORIGIN
        || headers
            .get(header::HOST)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|host| origin == format!("http://{host}"))
}
async fn frame_ws(
    ws: WebSocketUpgrade,
    State(app): State<App>,
    headers: HeaderMap,
) -> axum::response::Response {
    if !ws_origin_allowed(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    ws.on_upgrade(move |socket| frames(socket, app))
        .into_response()
}
async fn control_ws(
    ws: WebSocketUpgrade,
    State(app): State<App>,
    headers: HeaderMap,
) -> axum::response::Response {
    if !ws_origin_allowed(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    ws.on_upgrade(move |socket| controls(socket, app))
        .into_response()
}
#[derive(Deserialize)]
struct WsAuth {
    #[serde(rename = "type")]
    kind: String,
    token: String,
}
async fn authenticate(socket: &mut WebSocket, app: &App) -> bool {
    let Some(Ok(Message::Text(text))) = socket.recv().await else {
        return reject(socket).await;
    };
    let Ok(auth) = serde_json::from_str::<WsAuth>(&text) else {
        return reject(socket).await;
    };
    if auth.kind == "auth" && app.auth.lock().await.token.as_deref() == Some(&auth.token) {
        true
    } else {
        reject(socket).await
    }
}
async fn reject(socket: &mut WebSocket) -> bool {
    let _ = socket.send(Message::Close(None)).await;
    let _ = socket.close().await;
    false
}
async fn frames(mut socket: WebSocket, app: App) {
    if !authenticate(&mut socket, &app).await {
        return;
    }
    let mut frames = app.frames;
    let initial = frames.borrow().clone();
    if !initial.is_empty() && socket.send(Message::Binary(initial.into())).await.is_err() {
        return;
    }
    while frames.changed().await.is_ok() {
        let frame = frames.borrow().clone();
        if socket.send(Message::Binary(frame.into())).await.is_err() {
            break;
        }
    }
}
async fn select_tab(app: &App, id: &str) -> Result<Value> {
    let target = page_targets(app.port)
        .await?
        .into_iter()
        .find(|target| target.id == id)
        .context("unknown or closed page target")?;
    if *app.active_id.lock().await == target.id {
        return Ok(json!({"already_active":true}));
    }
    let _ = app
        .active
        .lock()
        .await
        .command("Page.stopScreencast", json!({}))
        .await;
    let next = Cdp::connect(
        target.websocket,
        target.id.clone(),
        app.active_id.clone(),
        app.frame_tx.clone(),
    )
    .await?;
    next.command("Page.enable", json!({})).await?;
    let (width, height) = *app.viewport.lock().await;
    next.command(
        "Emulation.setDeviceMetricsOverride",
        device_metrics(width, height),
    )
    .await?;
    *app.active_id.lock().await = target.id;
    next.command("Page.startScreencast", screencast_params(width, height))
        .await?;
    if let Ok(frame) = next
        .command(
            "Page.captureScreenshot",
            json!({"format":"jpeg","quality":jpeg_quality()}),
        )
        .await
    {
        if let Some(data) = frame["result"]["data"]
            .as_str()
            .and_then(|data| base64::engine::general_purpose::STANDARD.decode(data).ok())
        {
            let _ = app.frame_tx.send(data);
        }
    }
    *app.active.lock().await = next;
    Ok(json!({"selected":id}))
}
async fn navigate_history(cdp: &Cdp, offset: i64) -> Result<Value> {
    let history = cdp.command("Page.getNavigationHistory", json!({})).await?;
    let result = &history["result"];
    let entries = result["entries"]
        .as_array()
        .context("CDP returned no navigation history")?;
    let current = result["currentIndex"]
        .as_i64()
        .context("CDP returned no current navigation entry")?;
    let next = current + offset;
    let entry = entries
        .get(
            usize::try_from(next)
                .ok()
                .context("no navigation history entry")?,
        )
        .context("no navigation history entry")?;
    cdp.command(
        "Page.navigateToHistoryEntry",
        json!({"entryId":entry["id"]}),
    )
    .await
}
async fn controls(mut socket: WebSocket, app: App) {
    if !authenticate(&mut socket, &app).await {
        return;
    }
    while let Some(Ok(Message::Text(text))) = socket.recv().await {
        let before = window_state();
        let result = match serde_json::from_str::<Control>(&text) {
            Ok(Control::SelectTab { id }) => select_tab(&app, &id).await,
            command => {
                let cdp = app.active.lock().await.clone();
                match command {
                    Ok(Control::Tap { x, y }) => {
                        let down = cdp
                    .command(
                        "Input.dispatchMouseEvent",
                        json!({"type":"mousePressed","x":x,"y":y,"button":"left","clickCount":1}),
                    )
                    .await;
                        match down {
                    Ok(_) => {
                        match cdp.command("Input.dispatchMouseEvent", json!({"type":"mouseReleased","x":x,"y":y,"button":"left","clickCount":1})).await {
                            Ok(_) => cdp.command("Runtime.evaluate", json!({"expression":format!("(()=>{{const e=document.elementFromPoint({x},{y});return !!e&&(e.matches('input,textarea')||e.isContentEditable)}})()"),"returnByValue":true})).await.map(|hit| json!({"text_input":hit["result"]["result"]["value"].as_bool().unwrap_or(false)})),
                            Err(error) => Err(error),
                        }
                    }
                    Err(error) => Err(error),
                }
                    }
                    Ok(Control::Scroll { x, y, delta_y }) => {
                        cdp.command(
                            "Input.dispatchMouseEvent",
                            json!({"type":"mouseWheel","x":x,"y":y,"deltaX":0,"deltaY":delta_y}),
                        )
                        .await
                    }
                    Ok(Control::Resize { width, height }) => {
                        let width = width.clamp(1, 1920);
                        let height = height.clamp(1, 2400);
                        let _ = cdp.command("Page.stopScreencast", json!({})).await;
                        match cdp
                            .command(
                                "Emulation.setDeviceMetricsOverride",
                                device_metrics(width, height),
                            )
                            .await
                        {
                            Ok(_) => {
                                *app.viewport.lock().await = (width, height);
                                cdp.command(
                                    "Page.startScreencast",
                                    screencast_params(width, height),
                                )
                                .await
                            }
                            Err(error) => Err(error),
                        }
                    }
                    Ok(Control::Back) => navigate_history(&cdp, -1).await,
                    Ok(Control::Forward) => navigate_history(&cdp, 1).await,
                    Ok(Control::Reload) => cdp.command("Page.reload", json!({})).await,
                    Ok(Control::Navigate { url }) => {
                        cdp.command("Page.navigate", json!({"url":url})).await
                    }
                    Ok(Control::Go { text }) => {
                        cdp.command("Page.navigate", json!({"url": navigation_url(&text)}))
                            .await
                    }
                    Ok(Control::Key { key }) => {
                        let (key_value, code, virtual_key) = cdp_key(&key);
                        let down = cdp
                    .command(
                        "Input.dispatchKeyEvent",
                        json!({"type":"keyDown","key":key_value,"code":code,"windowsVirtualKeyCode":virtual_key,"nativeVirtualKeyCode":virtual_key}),
                    )
                    .await;
                        match down {
                    Ok(_) => {
                        cdp.command("Input.dispatchKeyEvent", json!({"type":"keyUp","key":key_value,"code":code,"windowsVirtualKeyCode":virtual_key,"nativeVirtualKeyCode":virtual_key}))
                            .await
                    }
                    Err(error) => Err(error),
                }
                    }
                    Ok(Control::InsertText { text }) => {
                        cdp.command("Input.insertText", json!({"text":text})).await
                    }
                    Ok(Control::Screencast { enabled }) => {
                        cdp.command(
                            if enabled {
                                "Page.startScreencast"
                            } else {
                                "Page.stopScreencast"
                            },
                            if enabled {
                                screencast_params(430, 932)
                            } else {
                                json!({})
                            },
                        )
                        .await
                    }
                    Err(error) => Err(error.into()),
                    Ok(Control::SelectTab { .. }) => unreachable!(),
                }
            }
        };
        let after = window_state();
        let text = match result {
            Ok(result) => json!({
                "ok": true,
                "result": result,
                "foreground_unchanged": before.0 == after.0,
                "cursor_unchanged": before.1 == after.1 && before.2 == after.2,
            })
            .to_string(),
            Err(error) => json!({"ok":false,"error":error.to_string()}).to_string(),
        };
        println!(
            "CDP input isolation: foreground {} -> {}, cursor ({}, {}) -> ({}, {})",
            before.0, after.0, before.1, before.2, after.1, after.2
        );
        if socket.send(Message::Text(text.into())).await.is_err() {
            break;
        }
    }
}

fn window_state() -> (isize, i32, i32) {
    let mut point = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut point) };
    (unsafe { GetForegroundWindow() } as isize, point.x, point.y)
}

fn cdp_key(key: &str) -> (&str, &str, u16) {
    match key {
        "Space" => (" ", "Space", 32),
        "ArrowLeft" => ("ArrowLeft", "ArrowLeft", 37),
        "ArrowRight" => ("ArrowRight", "ArrowRight", 39),
        "Backspace" => ("Backspace", "Backspace", 8),
        "Enter" => ("Enter", "Enter", 13),
        _ => (key, key, 0),
    }
}

fn navigation_url(text: &str) -> String {
    let text = text.trim();
    if text.starts_with("https://") || text.starts_with("http://") {
        text.into()
    } else {
        format!(
            "https://www.google.com/search?q={}",
            url::form_urlencoded::byte_serialize(text.as_bytes()).collect::<String>()
        )
    }
}
fn random_hex(bytes: usize) -> String {
    let mut value = vec![0; bytes];
    getrandom::fill(&mut value).expect("OS random unavailable");
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qr_text_generation() {
        let text = super::qr_text("http://example.com").unwrap();
        assert!(text.contains("██"));
        assert!(text.lines().count() > 10);
    }
    #[test]
    fn deep_link_contains_only_bootstrap_data() {
        let link = super::deep_link(Ipv4Addr::new(192, 168, 1, 20), 8787, &"a".repeat(32));
        assert!(link.starts_with(PWA_DEEP_LINK));
        assert!(link.contains("#connect="));
        assert!(!link.contains("token"));
    }
    #[test]
    fn url_or_search() {
        assert_eq!(navigation_url("https://example.com"), "https://example.com");
        assert_eq!(
            navigation_url("hello world"),
            "https://www.google.com/search?q=hello+world"
        );
    }
    #[test]
    fn mobile_viewport_metrics() {
        let metrics = device_metrics(390, 760);
        assert_eq!(metrics["mobile"], true);
        assert_eq!(metrics["width"], 390);
        assert_eq!(metrics["height"], 760);
    }
}
