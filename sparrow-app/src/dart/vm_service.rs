//! Persistent WebSocket client for the Dart VM service.
//!
//! ## Lifecycle
//!
//! ```text
//! [flutter run prints URL]
//!          ↓
//! [terminal_update_process sends VmServiceUrl]
//!          ↓
//! [WindowTabData calls FlutterInspectorData::on_vm_url]
//!          ↓
//! [VmServiceClient::connect enqueues Command::Connect(url)]
//!          ↓
//! [Connection thread]: TCP connect → WS upgrade → getVM → streamListen(Extension) → ready
//!          ↓
//! [First widget tree fetched]
//!          ↓
//! [Flutter.Frame events trigger refresh (debounced 500 ms)]
//! ```
//!
//! ## Design notes
//!
//! - Single background thread owns the socket. One loop alternates between
//!   a short-timeout `read()` and draining a command channel. UI never
//!   blocks on the network.
//! - Timeouts on the underlying `TcpStream` bound read latency to 100 ms
//!   when idle — small enough that UI commands are effectively immediate.
//! - All state transitions (connect/disconnect/reconnect) live in that
//!   thread. The UI just calls `connect(url)` or `disconnect()`.

use std::{
    io,
    net::TcpStream,
    sync::{
        atomic::{AtomicBool, AtomicI64, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

use crossbeam_channel::{unbounded, Receiver, Sender};
use floem::reactive::{ReadSignal, RwSignal, Scope, SignalGet, SignalUpdate};
use serde_json::{json, Value};
use sparrow_rpc::terminal::TermId;
use tungstenite::{Message, WebSocket};

use crate::window_tab::CommonData;

type NotifSender = std::sync::mpsc::Sender<Notification>;
type NotifReceiver = std::sync::mpsc::Receiver<Notification>;

// ─────────────────────────────────────────────────────────────────────
//  Public API
// ─────────────────────────────────────────────────────────────────────

/// Handle to the background connection. Cheap to clone.
#[derive(Clone)]
pub struct VmServiceClient {
    tx: Sender<Command>,
    connected: Arc<AtomicBool>,
    next_ui_id: Arc<AtomicI64>,
}

/// Events emitted from the connection thread to the UI.
#[derive(Debug, Clone)]
pub enum Notification {
    /// WS upgraded + handshake complete.
    Connected,
    /// Connection closed (server exit, network error, or explicit
    /// disconnect).
    Disconnected,
    /// Transport / protocol failure. Auto-reconnect is not attempted.
    Error(String),
    /// Fresh widget tree from `ext.flutter.inspector.getRootWidgetSummaryTree`.
    WidgetTree(InspectorNode),
    /// Extension event (`Flutter.Frame`, `Flutter.ServiceExtensionStateChanged`,
    /// etc.). Not currently consumed but plumbed through for future use.
    Event { method: String, params: Value },
}

/// A single node in the live widget tree.
#[derive(Debug, Clone)]
pub struct InspectorNode {
    pub description: String,
    pub type_name: String,
    pub value_id: String,
    pub children: Vec<InspectorNode>,
}

impl VmServiceClient {
    pub fn new() -> (Self, NotifReceiver) {
        let (tx, rx) = unbounded();
        let (notif_tx, notif_rx) = std::sync::mpsc::channel();   // was: unbounded()
        let connected = Arc::new(AtomicBool::new(false));

        let worker = ConnectionWorker {
            rx,
            notif_tx,
            connected: connected.clone(),
        };
        thread::Builder::new()
            .name("DartVmService".into())
            .spawn(move || worker.run())
            .expect("spawn DartVmService worker");

        (
            Self {
                tx,
                connected,
                next_ui_id: Arc::new(AtomicI64::new(1_000_000)),
            },
            notif_rx,
        )
    }

    /// Connect to a VM service URL (`http://127.0.0.1:PORT/TOKEN=/`).
    /// Any existing connection is dropped first.
    pub fn connect(&self, url: String) {
        let _ = self.tx.send(Command::Connect(url));
    }

    /// Disconnect from the current URL, if any.
    pub fn disconnect(&self) {
        let _ = self.tx.send(Command::Disconnect);
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }
}

impl Drop for VmServiceClient {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Shutdown);
    }
}

// ─────────────────────────────────────────────────────────────────────
//  Commands (UI → worker)
// ─────────────────────────────────────────────────────────────────────

enum Command {
    Connect(String),
    Disconnect,
    Shutdown,
}

// ─────────────────────────────────────────────────────────────────────
//  Worker
// ─────────────────────────────────────────────────────────────────────

struct ConnectionWorker {
    rx: Receiver<Command>,
    notif_tx: NotifSender,
    connected: Arc<AtomicBool>,
}

impl ConnectionWorker {
    fn run(self) {
        loop {
            // Wait for a Connect / Disconnect / Shutdown while idle.
            let url = match self.rx.recv() {
                Ok(Command::Connect(url)) => url,
                Ok(Command::Disconnect) | Ok(Command::Shutdown) => continue,
                Err(_) => return,
            };

            if let Err(err) = self.run_connection(&url) {
                let _ = self.notif_tx.send(Notification::Error(err));
            }
            self.connected.store(false, Ordering::Relaxed);
            let _ = self.notif_tx.send(Notification::Disconnected);
        }
    }

    fn run_connection(&self, http_url: &str) -> Result<(), String> {
        let ws_url = to_ws_url(http_url)?;
        let (mut socket, isolate_id) = self.open_and_handshake(&ws_url)?;

        self.connected.store(true, Ordering::Relaxed);
        let _ = self.notif_tx.send(Notification::Connected);

        // Initial tree fetch, before any frame arrives.
        self.send_widget_tree_request(&mut socket, 10, &isolate_id)?;

        let mut last_refresh = Instant::now();
        loop {
            // Drain pending UI commands.
            while let Ok(cmd) = self.rx.try_recv() {
                match cmd {
                    Command::Connect(_new_url) => {
                        // Reconnect: bail out, outer loop will pick up the
                        // new URL from the queue.
                        return Ok(());
                    }
                    Command::Disconnect | Command::Shutdown => return Ok(()),
                }
            }

            // Read one message (blocking with a short timeout).
            match socket.read() {
                Ok(Message::Text(text)) => {
                    if let Ok(v) = serde_json::from_str::<Value>(&text) {
                        self.dispatch_message(v, &mut socket, &isolate_id, &mut last_refresh)?;
                    }
                }
                Ok(Message::Ping(_))
                | Ok(Message::Pong(_))
                | Ok(Message::Binary(_))
                | Ok(Message::Frame(_)) => {}
                Ok(Message::Close(_)) => return Ok(()),
                Err(tungstenite::Error::Io(e))
                if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock
                            | io::ErrorKind::TimedOut
                            | io::ErrorKind::Interrupted
                    ) => {}
                Err(e) => return Err(format!("websocket read: {e}")),
            }
        }
    }

    fn open_and_handshake(
        &self,
        ws_url: &str,
    ) -> Result<(WebSocket<TcpStream>, String), String> {
        // Parse host:port ourselves so we can set socket timeouts before
        // wrapping in `tungstenite`.
        let parsed = url::Url::parse(ws_url)
            .map_err(|e| format!("bad ws url: {e}"))?;
        let host = parsed
            .host_str()
            .ok_or_else(|| "ws url has no host".to_string())?;
        let port = parsed
            .port_or_known_default()
            .ok_or_else(|| "ws url has no port".to_string())?;
        let addr = format!("{host}:{port}");

        let stream = TcpStream::connect(&addr)
            .map_err(|e| format!("tcp connect {addr}: {e}"))?;
        stream
            .set_read_timeout(Some(Duration::from_millis(100)))
            .map_err(|e| format!("set_read_timeout: {e}"))?;
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| format!("set_write_timeout: {e}"))?;

        let (mut socket, _response) = tungstenite::client(ws_url, stream)
            .map_err(|e| format!("ws handshake: {e}"))?;

        // 1. getVM
        let isolate_id = self.send_blocking(&mut socket, 1, "getVM", json!({}))
            .and_then(|v| {
                v.get("result")
                    .and_then(|r| r.get("isolates"))
                    .and_then(|arr| arr.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|iso| iso.get("id"))
                    .and_then(|id| id.as_str())
                    .map(|s| s.to_string())
                    .ok_or_else(|| "getVM: no isolates".to_string())
            })?;

        // 2. streamListen("Extension") — required before extension events
        //    (Flutter.Frame) will flow.
        self.send_blocking(
            &mut socket,
            2,
            "streamListen",
            json!({ "streamId": "Extension" }),
        )?;

        // 3. streamListen("Debug") — for pause / resume events.
        let _ = self.send_blocking(
            &mut socket,
            3,
            "streamListen",
            json!({ "streamId": "Debug" }),
        );

        Ok((socket, isolate_id))
    }

    /// Send a JSON-RPC request and read until its response arrives.
    /// Extension events received in between are forwarded to `notif_tx`.
    fn send_blocking(
        &self,
        socket: &mut WebSocket<TcpStream>,
        id: i64,
        method: &str,
        params: Value,
    ) -> Result<Value, String> {
        let req = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let text = serde_json::to_string(&req)
            .map_err(|e| format!("serialize {method}: {e}"))?;
        socket
            .send(Message::Text(text.into()))
            .map_err(|e| format!("send {method}: {e}"))?;

        loop {
            match socket.read() {
                Ok(Message::Text(text)) => {
                    let v: Value = match serde_json::from_str(&text) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    if v.get("id").and_then(|x| x.as_i64()) == Some(id) {
                        return Ok(v);
                    }
                    if let Some(m) = v.get("method").and_then(|x| x.as_str()) {
                        let _ = self.notif_tx.send(Notification::Event {
                            method: m.to_string(),
                            params: v
                                .get("params")
                                .cloned()
                                .unwrap_or(Value::Null),
                        });
                    }
                }
                Ok(_) => {}
                Err(tungstenite::Error::Io(e))
                if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock
                            | io::ErrorKind::TimedOut
                            | io::ErrorKind::Interrupted
                    ) => {}
                Err(e) => return Err(format!("read {method}: {e}")),
            }
        }
    }

    fn send_widget_tree_request(
        &self,
        socket: &mut WebSocket<TcpStream>,
        id: i64,
        isolate_id: &str,
    ) -> Result<(), String> {
        let req = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "ext.flutter.inspector.getRootWidgetSummaryTree",
            "params": {
                "isolateId": isolate_id,
                "groupName": "sparrow",
                "isSummaryTree": "true",
            },
        });
        let text = serde_json::to_string(&req)
            .map_err(|e| format!("serialize tree req: {e}"))?;
        socket
            .send(Message::Text(text.into()))
            .map_err(|e| format!("send tree req: {e}"))?;
        Ok(())
    }

    fn dispatch_message(
        &self,
        v: Value,
        socket: &mut WebSocket<TcpStream>,
        isolate_id: &str,
        last_refresh: &mut Instant,
    ) -> Result<(), String> {
        // Response to `getRootWidgetSummaryTree`?
        if let Some(result) = v.get("result") {
            if let Some(tree) = parse_summary_tree(result) {
                let _ = self.notif_tx.send(Notification::WidgetTree(tree));
            }
            return Ok(());
        }

        // Extension event?
        if let Some(method) = v.get("method").and_then(|x| x.as_str()) {
            if method == "Flutter.Frame" && last_refresh.elapsed() >= Duration::from_millis(500) {
                *last_refresh = Instant::now();
                // Fire-and-forget refresh — response handled above.
                self.send_widget_tree_request(socket, next_id_for_tree(), isolate_id)?;
            }
            let _ = self.notif_tx.send(Notification::Event {
                method: method.to_string(),
                params: v.get("params").cloned().unwrap_or(Value::Null),
            });
        }

        Ok(())
    }
}

fn next_id_for_tree() -> i64 {
    // Simple monotonic tick — any i64 works as long as it's unique per
    // outstanding request. The state machine is single-threaded, so a
    // thread-local counter isn't needed; we just bump a static.
    static COUNTER: std::sync::atomic::AtomicI64 =
        std::sync::atomic::AtomicI64::new(100_000);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// Convert the printed HTTP URL to the WebSocket endpoint.
///
/// `http://127.0.0.1:PORT/TOKEN=/` → `ws://127.0.0.1:PORT/TOKEN=/ws`
fn to_ws_url(http_url: &str) -> Result<String, String> {
    let trimmed = http_url.trim_end_matches('/');
    let base = trimmed
        .strip_prefix("http://")
        .ok_or_else(|| format!("not an http url: {http_url}"))?;
    Ok(format!("ws://{base}/ws"))
}

fn parse_summary_tree(response: &Value) -> Option<InspectorNode> {
    // Two shapes possible depending on server version:
    //   1. { result: { value: { ... } } }       (VM service standard)
    //   2. { value: { ... } }                    (some builds)
    let inner = response
        .get("value")
        .or_else(|| response.get("result").and_then(|r| r.get("value")))?;
    parse_node(inner)
}

fn parse_node(v: &Value) -> Option<InspectorNode> {
    // A node is either a plain object or a `DiagnosticsNode` wrapper.
    let root = v.get("value").unwrap_or(v);

    let description = root
        .get("description")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let type_name = root
        .get("type")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let value_id = root
        .get("valueId")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let children = root
        .get("children")
        .and_then(|x| x.as_array())
        .map(|arr| arr.iter().filter_map(parse_node).collect())
        .unwrap_or_default();

    if description.is_empty() {
        return None;
    }
    Some(InspectorNode {
        description,
        type_name,
        value_id,
        children,
    })
}

// ─────────────────────────────────────────────────────────────────────
//  Reactive state (used by the panel + WindowTabData)
// ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InspectorState {
    Disconnected,
    Connecting,
    Connected,
}

#[derive(Clone)]
pub struct InspectorNodeData {
    pub node: InspectorNode,
    pub open: RwSignal<bool>,
    pub children: Vec<RwSignal<InspectorNodeData>>,
}

impl InspectorNodeData {
    fn from_node(cx: Scope, node: InspectorNode) -> RwSignal<InspectorNodeData> {
        let children: Vec<RwSignal<InspectorNodeData>> = node
            .children
            .clone()
            .into_iter()
            .map(|c| Self::from_node(cx, c))
            .collect();
        cx.create_rw_signal(InspectorNodeData {
            node,
            open: cx.create_rw_signal(true),
            children,
        })
    }
}

#[derive(Clone)]
pub struct FlutterInspectorData {
    pub client: VmServiceClient,
    pub state: RwSignal<InspectorState>,
    pub vm_url: RwSignal<Option<(TermId, String)>>,
    pub root: RwSignal<Option<RwSignal<InspectorNodeData>>>,
    pub updates: ReadSignal<Option<Notification>>,
    pub common: std::rc::Rc<CommonData>,
    cx: Scope,
}

impl FlutterInspectorData {
    pub fn new(cx: Scope, common: std::rc::Rc<CommonData>) -> Self {
        let (client, rx) = VmServiceClient::new();
        let updates = floem::ext_event::create_signal_from_channel(rx);
        Self {
            client,
            state: cx.create_rw_signal(InspectorState::Disconnected),
            vm_url: cx.create_rw_signal(None),
            root: cx.create_rw_signal(None),
            updates,
            common,
            cx,
        }
    }

    /// Called from `WindowTabData` when the terminal scanner detects a
    /// `flutter run` banner for the active Flutter terminal.
    pub fn on_vm_url(&self, term_id: TermId, url: String) {
        // Ignore if we already have this exact URL for this term.
        if let Some((existing_term, existing_url)) = self.vm_url.get_untracked() {
            if existing_term == term_id && existing_url == url {
                return;
            }
        }
        self.vm_url.set(Some((term_id, url.clone())));
        self.state.set(InspectorState::Connecting);
        self.client.connect(url);
    }

    pub fn disconnect(&self) {
        self.client.disconnect();
        self.vm_url.set(None);
        self.root.set(None);
        self.state.set(InspectorState::Disconnected);
    }

    /// Apply a notification received from the connection thread.
    pub fn apply(&self, notif: &Notification) {
        match notif {
            Notification::Connected => {
                self.state.set(InspectorState::Connected);
            }
            Notification::Disconnected => {
                self.state.set(InspectorState::Disconnected);
                self.root.set(None);
            }
            Notification::Error(e) => {
                tracing::warn!("VM service error: {e}");
                self.state.set(InspectorState::Disconnected);
            }
            Notification::WidgetTree(tree) => {
                let root = InspectorNodeData::from_node(self.cx, tree.clone());
                self.root.set(Some(root));
            }
            Notification::Event { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_ws_url_appends_ws() {
        assert_eq!(
            to_ws_url("http://127.0.0.1:41677/TOKEN=/").unwrap(),
            "ws://127.0.0.1:41677/TOKEN=/ws"
        );
    }

    #[test]
    fn to_ws_url_strips_trailing_slash() {
        assert_eq!(
            to_ws_url("http://127.0.0.1:41677/TOKEN=/").unwrap(),
            to_ws_url("http://127.0.0.1:41677/TOKEN=").unwrap()
        );
    }

    #[test]
    fn to_ws_url_rejects_https() {
        assert!(to_ws_url("https://127.0.0.1:41677/TOKEN=/").is_err());
    }

    #[test]
    fn parse_node_basic() {
        let v = json!({
            "description": "Column",
            "type": "Column",
            "valueId": "inspector-1",
            "children": [
                { "description": "Text(\"hi\")", "type": "Text", "valueId": "inspector-2" }
            ]
        });
        let node = parse_node(&v).unwrap();
        assert_eq!(node.description, "Column");
        assert_eq!(node.children.len(), 1);
        assert_eq!(node.children[0].description, "Text(\"hi\")");
    }

    #[test]
    fn parse_node_handles_wrapper() {
        let v = json!({
            "value": {
                "description": "App",
                "type": "App",
                "valueId": "inspector-0",
                "children": []
            }
        });
        let node = parse_node(&v).unwrap();
        assert_eq!(node.description, "App");
    }

    #[test]
    fn parse_node_empty_returns_none() {
        assert!(parse_node(&json!({})).is_none());
    }

    #[test]
    fn parse_summary_tree_shapes() {
        let standard = json!({
            "result": {
                "value": {
                    "description": "Root",
                    "type": "Root",
                    "valueId": "r"
                }
            }
        });
        assert_eq!(parse_summary_tree(&standard).unwrap().description, "Root");

        let alternative = json!({
            "value": {
                "description": "Root",
                "type": "Root",
                "valueId": "r"
            }
        });
        assert_eq!(
            parse_summary_tree(&alternative).unwrap().description,
            "Root"
        );
    }
}