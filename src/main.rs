mod audio;

use std::{collections::HashMap, path::PathBuf, process::Stdio, sync::Arc, time::Duration};

use anyhow::{Context, Result, bail};
use axum::{
    Router,
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::{Html, IntoResponse},
    routing::get,
};
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::{
    process::Command,
    sync::{Mutex, oneshot, watch},
    time::sleep,
};
use tokio_tungstenite::tungstenite::Message as CdpMessage;
use windows_sys::Win32::{
    Foundation::POINT,
    UI::WindowsAndMessaging::{GetCursorPos, GetForegroundWindow},
};

const BRAVE: &str = r"C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe";
const VIEWPORT: &str = "430,932";

#[derive(Clone)]
struct App {
    port: u16,
    active_id: Arc<Mutex<String>>,
    active: Arc<Mutex<Cdp>>,
    frame_tx: watch::Sender<Vec<u8>>,
    frames: watch::Receiver<Vec<u8>>,
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
#[serde(tag = "type", rename_all = "snake_case")]
enum Control {
    Tap { x: f64, y: f64 },
    Scroll { x: f64, y: f64, delta_y: f64 },
    Back,
    Forward,
    Reload,
    Navigate { url: String },
    Go { text: String },
    Key { key: String },
    Screencast { enabled: bool },
    SelectTab { id: String },
}

#[derive(Serialize)]
struct Status<'a> {
    status: &'a str,
}

#[tokio::main]
async fn main() -> Result<()> {
    let profile = std::env::var("LOCALAPPDATA").context("LOCALAPPDATA unavailable")?;
    let profile = PathBuf::from(profile).join("HWMR").join("browser-profile");
    std::fs::create_dir_all(&profile)?;
    let port = launch_brave(&profile).await?;
    let target = page_target(port).await?;
    let active_id = Arc::new(Mutex::new(target.id.clone()));
    let (frame_tx, frames) = watch::channel(Vec::new());
    let cdp = Cdp::connect(
        target.websocket,
        target.id.clone(),
        active_id.clone(),
        frame_tx.clone(),
    )
    .await?;
    cdp.command("Page.enable", json!({})).await?;
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
    cdp.command(
        "Page.startScreencast",
        json!({"format":"jpeg", "quality":70, "maxWidth":430, "maxHeight":932, "everyNthFrame":1}),
    )
    .await?;
    let app = App {
        port,
        active_id,
        active: Arc::new(Mutex::new(cdp)),
        frame_tx,
        frames,
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
        .route("/audio", get(audio_status))
        .route("/isolation", get(isolation_status))
        .route("/tabs", get(tabs))
        .route("/ws/frame", get(frame_ws))
        .route("/ws/control", get(control_ws))
        .with_state(app);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8787").await?;
    println!("HWMR viewer: http://127.0.0.1:8787  (CDP: 127.0.0.1:{port})");
    axum::serve(listener, router).await?;
    Ok(())
}

async fn launch_brave(profile: &PathBuf) -> Result<u16> {
    if !std::path::Path::new(BRAVE).exists() {
        bail!("Brave not found at {BRAVE}");
    }
    let port_file = profile.join("DevToolsActivePort");
    if port_file.exists() {
        bail!(
            "Existing DevToolsActivePort found; profile may already be owned. Stop its Brave instance before starting HWMR"
        )
    }
    Command::new(BRAVE)
        .args([
            "--headless",
            "--remote-debugging-address=127.0.0.1",
            "--remote-debugging-port=0",
            &format!("--user-data-dir={}", profile.display()),
            &format!("--window-size={VIEWPORT}"),
            "https://www.youtube.com/",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("launch Headless Brave")?;
    for _ in 0..40 {
        if let Ok(value) = std::fs::read_to_string(&port_file) {
            if let Some(port) = value.lines().next().and_then(|line| line.parse().ok()) {
                return Ok(port);
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
async fn audio_status() -> impl IntoResponse {
    match audio::sessions() {
        Ok(sessions) => axum::Json(serde_json::json!({"sessions": sessions})).into_response(),
        Err(error) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            error.to_string(),
        )
            .into_response(),
    }
}
async fn isolation_status() -> impl IntoResponse {
    let state = window_state();
    axum::Json(
        serde_json::json!({"foreground_window": state.0, "cursor": {"x": state.1, "y": state.2}}),
    )
}
async fn tabs(State(app): State<App>) -> impl IntoResponse {
    match page_targets(app.port).await {
        Ok(items) => {
            let active = app.active_id.lock().await.clone();
            axum::Json(serde_json::json!({"type":"tabs", "items": items.into_iter().map(|target| json!({"id":target.id,"title":target.title,"url":target.url,"active":target.id == active})).collect::<Vec<_>>() })).into_response()
        }
        Err(error) => (axum::http::StatusCode::BAD_GATEWAY, error.to_string()).into_response(),
    }
}
async fn frame_ws(ws: WebSocketUpgrade, State(app): State<App>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| frames(socket, app.frames))
}
async fn control_ws(ws: WebSocketUpgrade, State(app): State<App>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| controls(socket, app))
}
async fn frames(mut socket: WebSocket, mut frames: watch::Receiver<Vec<u8>>) {
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
    *app.active_id.lock().await = target.id;
    next.command(
        "Page.startScreencast",
        json!({"format":"jpeg", "quality":70, "maxWidth":430, "maxHeight":932, "everyNthFrame":1}),
    )
    .await?;
    *app.active.lock().await = next;
    Ok(json!({"selected":id}))
}
async fn controls(mut socket: WebSocket, app: App) {
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
                match down { Ok(_) => cdp.command("Input.dispatchMouseEvent", json!({"type":"mouseReleased","x":x,"y":y,"button":"left","clickCount":1})).await, Err(error) => Err(error) }
            }
            Ok(Control::Scroll { x, y, delta_y }) => {
                cdp.command(
                    "Input.dispatchMouseEvent",
                    json!({"type":"mouseWheel","x":x,"y":y,"deltaX":0,"deltaY":delta_y}),
                )
                .await
            }
            Ok(Control::Back) => cdp.command("Page.goBack", json!({})).await,
            Ok(Control::Forward) => cdp.command("Page.goForward", json!({})).await,
            Ok(Control::Reload) => cdp.command("Page.reload", json!({})).await,
            Ok(Control::Navigate { url }) => cdp.command("Page.navigate", json!({"url":url})).await,
            Ok(Control::Go { text }) => cdp.command("Page.navigate", json!({"url": navigation_url(&text)})).await,
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
            Ok(Control::Screencast { enabled }) => cdp.command(
                if enabled { "Page.startScreencast" } else { "Page.stopScreencast" },
                if enabled { json!({"format":"jpeg", "quality":70, "maxWidth":430, "maxHeight":932, "everyNthFrame":1}) } else { json!({}) },
            ).await,
            Err(error) => Err(error.into()),
            Ok(Control::SelectTab { .. }) => unreachable!(),
                }
            }
        };
        let after = window_state();
        let text = match result {
            Ok(_) => json!({
                "ok": true,
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

#[cfg(test)]
mod tests {
    use super::navigation_url;
    #[test]
    fn url_or_search() {
        assert_eq!(navigation_url("https://example.com"), "https://example.com");
        assert_eq!(
            navigation_url("hello world"),
            "https://www.google.com/search?q=hello+world"
        );
    }
}
