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
    cdp: Cdp,
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
    #[serde(rename = "type")]
    kind: String,
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
    let (cdp, frames) = Cdp::connect(target.websocket).await?;
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
    let app = App { cdp, frames };
    let router = Router::new()
        .route("/", get(index))
        .route("/health", get(health))
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

impl Cdp {
    async fn connect(url: String) -> Result<(Self, watch::Receiver<Vec<u8>>)> {
        let (socket, _) = tokio_tungstenite::connect_async(url).await?;
        let (writer, mut reader) = socket.split();
        let cdp = Self {
            writer: Arc::new(Mutex::new(writer)),
            pending: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(Mutex::new(0)),
        };
        let (frame_tx, frame_rx) = watch::channel(Vec::new());
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
                        let _ = frame_tx.send(data);
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
        Ok((cdp, frame_rx))
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
async fn frame_ws(ws: WebSocketUpgrade, State(app): State<App>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| frames(socket, app.frames))
}
async fn control_ws(ws: WebSocketUpgrade, State(app): State<App>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| controls(socket, app.cdp))
}
async fn frames(mut socket: WebSocket, mut frames: watch::Receiver<Vec<u8>>) {
    while frames.changed().await.is_ok() {
        let frame = frames.borrow().clone();
        if socket.send(Message::Binary(frame.into())).await.is_err() {
            break;
        }
    }
}
async fn controls(mut socket: WebSocket, cdp: Cdp) {
    while let Some(Ok(Message::Text(text))) = socket.recv().await {
        let before = window_state();
        let result = match serde_json::from_str::<Control>(&text) {
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
            Err(error) => Err(error.into()),
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
