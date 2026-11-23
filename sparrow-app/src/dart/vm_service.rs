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
//! - A short read timeout bounds idle command dispatch latency. Runtime
//!   changes wait for the preceding VM reply so Flutter applies them in order.
//! - All state transitions (connect/disconnect/reconnect) live in that
//!   thread. The UI just calls `connect(url)` or `disconnect()`.

use std::{
    collections::VecDeque,
    io,
    net::TcpStream,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, Sender, TryRecvError, unbounded};
use floem::reactive::{ReadSignal, RwSignal, Scope, SignalGet, SignalUpdate};
use serde_json::{Value, json};
use sparrow_rpc::terminal::TermId;
use tungstenite::{Message, WebSocket};

use crate::window_tab::CommonData;

type NotifSender = std::sync::mpsc::Sender<Notification>;
type NotifReceiver = std::sync::mpsc::Receiver<Notification>;

const COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(10);
const SERVICE_EXTENSION_TIMEOUT: Duration = Duration::from_secs(5);

// ─────────────────────────────────────────────────────────────────────
//  Public API
// ─────────────────────────────────────────────────────────────────────

/// Handle to the background connection. Cheap to clone.
#[derive(Clone)]
pub struct VmServiceClient {
    tx: Sender<Command>,
    ready_generation: Arc<AtomicU64>,
    generation: Arc<AtomicU64>,
}

use super::performance::{
    Frame, PerformanceData, PerformanceUpdate, Sample, SampleReply,
};

#[derive(Debug, Clone, Copy, Default)]
pub struct RuntimeFlags {
    pub debug_paint: bool,
    pub performance_overlay: bool,
    pub slow_animations: bool,
    pub repaint_rainbow: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct RuntimeSwitches {
    pub debug_paint: RwSignal<bool>,
    pub performance_overlay: RwSignal<bool>,
    pub slow_animations: RwSignal<bool>,
    pub repaint_rainbow: RwSignal<bool>,
}

impl RuntimeSwitches {
    fn new(cx: Scope) -> Self {
        Self {
            debug_paint: cx.create_rw_signal(false),
            performance_overlay: cx.create_rw_signal(false),
            slow_animations: cx.create_rw_signal(false),
            repaint_rainbow: cx.create_rw_signal(false),
        }
    }

    fn set(self, flags: RuntimeFlags) {
        floem::reactive::batch(|| {
            self.debug_paint.set(flags.debug_paint);
            self.performance_overlay.set(flags.performance_overlay);
            self.slow_animations.set(flags.slow_animations);
            self.repaint_rainbow.set(flags.repaint_rainbow);
        });
    }
}

/// Events emitted from the connection thread to the UI.
#[derive(Debug, Clone)]
pub enum Notification {
    /// WS upgraded + handshake complete.
    Connected(u64, RuntimeFlags),
    Restarting(u64),
    Performance(u64, PerformanceUpdate),
    /// Connection closed (server exit, network error, or explicit
    /// disconnect).
    Disconnected(u64),
    /// Transport / protocol failure. Auto-reconnect is not attempted.
    Error(u64, String),
    /// Fresh widget tree from `ext.flutter.inspector.getRootWidgetSummaryTree`.
    WidgetTree(InspectorNode),
    /// Extension event (`Flutter.Frame`, `Flutter.ServiceExtensionStateChanged`,
    /// etc.). Not currently consumed but plumbed through for future use.
    Event {
        method: String,
        params: Value,
    },
}

/// A single node in the live widget tree.
#[derive(Debug, Clone)]
pub struct InspectorNode {
    pub description: String,
    pub type_name: String,
    pub value_id: String,
    pub properties: Vec<(String, String)>,
    pub children: Vec<InspectorNode>,
}

impl VmServiceClient {
    pub fn new() -> (Self, NotifReceiver) {
        let (tx, rx) = unbounded();
        let (notif_tx, notif_rx) = std::sync::mpsc::channel(); // was: unbounded()
        let ready_generation = Arc::new(AtomicU64::new(0));
        let generation = Arc::new(AtomicU64::new(0));

        let worker = ConnectionWorker {
            rx,
            notif_tx,
            ready_generation: ready_generation.clone(),
            generation: generation.clone(),
        };
        thread::Builder::new()
            .name("DartVmService".into())
            .spawn(move || worker.run())
            .expect("spawn DartVmService worker");

        (
            Self {
                tx,
                ready_generation,
                generation,
            },
            notif_rx,
        )
    }

    /// Connect to a VM service URL (`http://127.0.0.1:PORT/TOKEN=/`).
    /// Any existing connection is dropped first.
    pub fn connect(&self, url: String) {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.ready_generation.store(0, Ordering::Release);
        let _ = self.tx.send(Command::Connect(Connection {
            url,
            generation,
            previous_isolate: None,
        }));
    }

    /// Disconnect from the current URL, if any.
    pub fn disconnect(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.ready_generation.store(0, Ordering::Release);
        let _ = self.tx.send(Command::Disconnect);
    }

    pub fn is_connected(&self) -> bool {
        let generation = self.generation.load(Ordering::SeqCst);
        generation != 0
            && self.ready_generation.load(Ordering::Acquire) == generation
    }

    /// Invoke a Flutter service extension (e.g. `ext.flutter.debugPaint`, `ext.flutter.showPerformanceOverlay`).
    pub fn call_service_extension(&self, method: impl Into<String>, params: Value) {
        let generation = self.generation.load(Ordering::SeqCst);
        if generation == 0
            || self.ready_generation.load(Ordering::Acquire) != generation
        {
            return;
        }
        let _ = self.tx.send(Command::CallServiceExtension {
            generation,
            method: method.into(),
            params,
        });
    }

    pub(crate) fn sample_performance(&self, sample: Sample) -> bool {
        let generation = self.generation.load(Ordering::SeqCst);
        self.is_connected()
            && self
                .tx
                .send(Command::SamplePerformance { generation, sample })
                .is_ok()
    }

    /// Toggle Flutter debug paint visual bounds.
    pub fn set_debug_paint(&self, isolate_id: Option<&str>, enabled: bool) {
        let mut params =
            json!({ "enabled": if enabled { "true" } else { "false" } });
        if let Some(id) = isolate_id {
            if let Some(map) = params.as_object_mut() {
                map.insert("isolateId".to_string(), json!(id));
            }
        }
        self.call_service_extension("ext.flutter.debugPaint", params);
    }

    /// Toggle Flutter performance overlay.
    pub fn set_performance_overlay(&self, isolate_id: Option<&str>, enabled: bool) {
        let mut params =
            json!({ "enabled": if enabled { "true" } else { "false" } });
        if let Some(id) = isolate_id {
            if let Some(map) = params.as_object_mut() {
                map.insert("isolateId".to_string(), json!(id));
            }
        }
        self.call_service_extension("ext.flutter.showPerformanceOverlay", params);
    }

    /// Toggle Flutter slow animations (5.0x dilation).
    pub fn set_slow_animations(&self, isolate_id: Option<&str>, enabled: bool) {
        let mut params =
            json!({ "timeDilation": if enabled { "5.0" } else { "1.0" } });
        if let Some(id) = isolate_id {
            if let Some(map) = params.as_object_mut() {
                map.insert("isolateId".to_string(), json!(id));
            }
        }
        self.call_service_extension("ext.flutter.timeDilation", params);
    }

    /// Toggle Flutter repaint rainbow.
    pub fn set_repaint_rainbow(&self, isolate_id: Option<&str>, enabled: bool) {
        let mut params =
            json!({ "enabled": if enabled { "true" } else { "false" } });
        if let Some(id) = isolate_id {
            if let Some(map) = params.as_object_mut() {
                map.insert("isolateId".to_string(), json!(id));
            }
        }
        self.call_service_extension("ext.flutter.repaintRainbow", params);
    }

    /// Force Flutter widget tree reassembly.
    pub fn reassemble(&self, isolate_id: Option<&str>) {
        let mut params = json!({});
        if let Some(id) = isolate_id {
            if let Some(map) = params.as_object_mut() {
                map.insert("isolateId".to_string(), json!(id));
            }
        }
        self.call_service_extension("ext.flutter.reassemble", params);
    }
}

// ─────────────────────────────────────────────────────────────────────
//  Commands (UI → worker)
// ─────────────────────────────────────────────────────────────────────

enum Command {
    Connect(Connection),
    Disconnect,
    SamplePerformance {
        generation: u64,
        sample: Sample,
    },
    CallServiceExtension {
        generation: u64,
        method: String,
        params: Value,
    },
}

struct Connection {
    url: String,
    generation: u64,
    // A hot restart can reuse the WebSocket URL while replacing the isolate.
    previous_isolate: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────
//  Worker
// ─────────────────────────────────────────────────────────────────────

struct ConnectionWorker {
    rx: Receiver<Command>,
    notif_tx: NotifSender,
    ready_generation: Arc<AtomicU64>,
    generation: Arc<AtomicU64>,
}

impl ConnectionWorker {
    fn run(self) {
        let mut next_connection = None;
        loop {
            // Wait for a connection while idle.
            let connection = match next_connection.take() {
                Some(connection) => connection,
                None => match self.rx.recv() {
                    Ok(Command::Connect(url)) => url,
                    Ok(
                        Command::Disconnect
                        | Command::CallServiceExtension { .. }
                        | Command::SamplePerformance { .. },
                    ) => continue,
                    Err(_) => return,
                },
            };

            if connection.generation != self.generation.load(Ordering::SeqCst) {
                continue;
            }
            match self.run_connection(&connection) {
                Ok(reconnect) => next_connection = reconnect,
                Err(err) => {
                    let _ = self
                        .notif_tx
                        .send(Notification::Error(connection.generation, err));
                }
            }
            self.ready_generation.store(0, Ordering::Release);
            let _ = self
                .notif_tx
                .send(Notification::Disconnected(connection.generation));
        }
    }

    fn run_connection(
        &self,
        connection: &Connection,
    ) -> Result<Option<Connection>, String> {
        let ws_url = to_ws_url(&connection.url)?;
        let (mut socket, isolate_id, extensions) =
            self.open_and_handshake(&ws_url, connection)?;
        let flags =
            self.read_runtime_flags(&mut socket, &isolate_id, &extensions)?;
        socket
            .get_ref()
            .set_read_timeout(Some(COMMAND_POLL_INTERVAL))
            .map_err(|e| format!("set VM service polling interval: {e}"))?;

        if connection.generation != self.generation.load(Ordering::SeqCst) {
            return Ok(None);
        }
        self.ready_generation
            .store(connection.generation, Ordering::Release);
        let _ = self
            .notif_tx
            .send(Notification::Connected(connection.generation, flags));

        // Initial tree fetch, before any frame arrives.
        self.send_widget_tree_request(&mut socket, 10, &isolate_id)?;

        let mut frames = Vec::new();
        let mut last_frame_batch = Instant::now();
        let mut sample_in_flight: Option<(i64, Sample, Instant)> = None;
        let mut last_network_timestamp = None;
        let mut last_refresh = Instant::now();
        let mut pending_extensions = VecDeque::new();
        let mut in_flight: Option<(i64, String, Instant)> = None;
        loop {
            // Drain pending UI commands.
            loop {
                match self.rx.try_recv() {
                    Ok(Command::Connect(url)) => return Ok(Some(url)),
                    Ok(Command::Disconnect) | Err(TryRecvError::Disconnected) => {
                        return Ok(None);
                    }
                    Err(TryRecvError::Empty) => break,
                    Ok(Command::SamplePerformance { generation, sample }) => {
                        if generation != connection.generation
                            || generation != self.generation.load(Ordering::SeqCst)
                        {
                            continue;
                        }
                        let manual = matches!(
                            sample,
                            Sample::Recording(_) | Sample::ClearNetwork
                        );
                        if manual || sample_in_flight.is_none() {
                            let id = next_request_id();
                            let (method, params) =
                                sample.rpc(&isolate_id, last_network_timestamp);
                            self.send_nonblocking(&mut socket, id, method, params)?;
                            sample_in_flight = Some((
                                id,
                                sample,
                                Instant::now() + SERVICE_EXTENSION_TIMEOUT,
                            ));
                        }
                    }
                    Ok(Command::CallServiceExtension {
                        generation,
                        method,
                        params,
                    }) => {
                        if generation == connection.generation
                            && generation == self.generation.load(Ordering::SeqCst)
                        {
                            pending_extensions.push_back((method, params));
                        }
                    }
                }
            }

            if connection.generation != self.generation.load(Ordering::SeqCst) {
                return Ok(None);
            }

            if let Some((_, sample, deadline)) = sample_in_flight
                && Instant::now() >= deadline
            {
                sample_in_flight = None;
                let _ = self.notif_tx.send(Notification::Performance(
                    connection.generation,
                    PerformanceUpdate::Reply(SampleReply {
                        sample,
                        result: Err("The VM service did not respond.".into()),
                    }),
                ));
            }
            if !frames.is_empty()
                && last_frame_batch.elapsed() >= Duration::from_millis(100)
            {
                let _ = self.notif_tx.send(Notification::Performance(
                    connection.generation,
                    PerformanceUpdate::Frames(std::mem::take(&mut frames)),
                ));
                last_frame_batch = Instant::now();
            }

            if let Some((_, method, deadline)) = &in_flight {
                if Instant::now() >= *deadline {
                    return Err(format!(
                        "VM service timed out waiting for {method}"
                    ));
                }
            } else if let Some((method, mut params)) = pending_extensions.pop_front()
            {
                if let Some(params) = params.as_object_mut() {
                    params
                        .entry("isolateId")
                        .or_insert_with(|| json!(isolate_id));
                }
                let id = next_request_id();
                self.send_nonblocking(&mut socket, id, &method, params)?;
                in_flight =
                    Some((id, method, Instant::now() + SERVICE_EXTENSION_TIMEOUT));
            }

            match socket.read() {
                Ok(Message::Text(text)) => {
                    if let Ok(v) = serde_json::from_str::<Value>(&text) {
                        if v["method"] == "streamNotify"
                            && v["params"]["streamId"] == "Isolate"
                            && v["params"]["event"]["kind"] == "IsolateExit"
                            && v["params"]["event"]["isolate"]["id"].as_str()
                                == Some(&isolate_id)
                        {
                            let generation = connection.generation + 1;
                            if self
                                .generation
                                .compare_exchange(
                                    connection.generation,
                                    generation,
                                    Ordering::SeqCst,
                                    Ordering::SeqCst,
                                )
                                .is_err()
                            {
                                return Ok(None);
                            }
                            self.ready_generation.store(0, Ordering::Release);
                            let _ = self
                                .notif_tx
                                .send(Notification::Restarting(generation));
                            return Ok(Some(Connection {
                                url: connection.url.clone(),
                                generation,
                                previous_isolate: Some(isolate_id),
                            }));
                        }
                        if let Some((id, method, _)) = &in_flight
                            && v.get("id").and_then(Value::as_i64) == Some(*id)
                        {
                            if let Some(error) = v.get("error") {
                                tracing::warn!(%method, %error, "Flutter service extension failed");
                            }
                            in_flight = None;
                            continue;
                        }
                        if let Some((id, sample, _)) = sample_in_flight
                            && v["id"].as_i64() == Some(id)
                        {
                            sample_in_flight = None;
                            let result = match v.get("result") {
                                Some(value) => Ok(value.clone()),
                                None => Err(v["error"]["message"]
                                    .as_str()
                                    .unwrap_or("Invalid VM service response.")
                                    .to_owned()),
                            };
                            if let Ok(value) = &result {
                                match sample {
                                    Sample::Network => {
                                        last_network_timestamp =
                                            value["timestamp"].as_u64()
                                    }
                                    Sample::ClearNetwork
                                    | Sample::Recording(true) => {
                                        last_network_timestamp = None
                                    }
                                    _ => {}
                                }
                            }
                            let _ = self.notif_tx.send(Notification::Performance(
                                connection.generation,
                                PerformanceUpdate::Reply(SampleReply {
                                    sample,
                                    result,
                                }),
                            ));
                            continue;
                        }
                        if let Some(frame) = Frame::from_event(&v, &isolate_id) {
                            frames.push(frame);
                            continue;
                        }
                        self.dispatch_message(
                            v,
                            &mut socket,
                            &isolate_id,
                            &mut last_refresh,
                        )?;
                    }
                }
                Ok(Message::Ping(_))
                | Ok(Message::Pong(_))
                | Ok(Message::Binary(_))
                | Ok(Message::Frame(_)) => {}
                Ok(Message::Close(_)) => return Ok(None),
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
        connection: &Connection,
    ) -> Result<(WebSocket<TcpStream>, String, Vec<String>), String> {
        // Parse host:port ourselves so we can set socket timeouts before
        // wrapping in `tungstenite`.
        let parsed =
            url::Url::parse(ws_url).map_err(|e| format!("bad ws url: {e}"))?;
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
            .set_nodelay(true)
            .map_err(|e| format!("set_nodelay: {e}"))?;
        stream
            .set_read_timeout(Some(Duration::from_millis(100)))
            .map_err(|e| format!("set_read_timeout: {e}"))?;
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| format!("set_write_timeout: {e}"))?;

        let (mut socket, _response) = tungstenite::client(ws_url, stream)
            .map_err(|e| format!("ws handshake: {e}"))?;

        for stream in ["Isolate", "Extension", "Debug"] {
            self.send_blocking(
                &mut socket,
                next_request_id(),
                "streamListen",
                json!({ "streamId": stream }),
            )?;
        }

        let deadline = Instant::now() + SERVICE_EXTENSION_TIMEOUT;
        while Instant::now() < deadline {
            if self.generation.load(Ordering::SeqCst) != connection.generation {
                return Err("Flutter VM connection was replaced".into());
            }
            let vm = self.send_blocking(
                &mut socket,
                next_request_id(),
                "getVM",
                json!({}),
            )?;
            if let Some(isolate_id) = vm["result"]["isolates"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|isolate| isolate["id"].as_str())
                .find(|id| Some(*id) != connection.previous_isolate.as_deref())
            {
                let isolate = self.send_blocking(
                    &mut socket,
                    next_request_id(),
                    "getIsolate",
                    json!({ "isolateId": isolate_id }),
                )?;
                let extensions: Vec<_> = isolate["result"]["extensionRPCs"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect();
                if extensions
                    .iter()
                    .any(|extension| extension == "ext.flutter.timeDilation")
                {
                    return Ok((socket, isolate_id.to_owned(), extensions));
                }
            }
            thread::sleep(Duration::from_millis(25));
        }
        Err("Flutter VM did not register its runtime extensions".into())
    }

    fn read_runtime_flags(
        &self,
        socket: &mut WebSocket<TcpStream>,
        isolate_id: &str,
        extensions: &[String],
    ) -> Result<RuntimeFlags, String> {
        let mut flags = RuntimeFlags::default();
        for (method, key, flag) in [
            ("ext.flutter.debugPaint", "enabled", &mut flags.debug_paint),
            (
                "ext.flutter.showPerformanceOverlay",
                "enabled",
                &mut flags.performance_overlay,
            ),
            (
                "ext.flutter.timeDilation",
                "timeDilation",
                &mut flags.slow_animations,
            ),
            (
                "ext.flutter.repaintRainbow",
                "enabled",
                &mut flags.repaint_rainbow,
            ),
        ] {
            if extensions.iter().any(|extension| extension == method) {
                let reply = self.send_blocking(
                    socket,
                    next_request_id(),
                    method,
                    json!({ "isolateId": isolate_id }),
                )?;
                let result = &reply["result"][key];
                *flag = if key == "enabled" {
                    result.as_bool().unwrap_or_else(|| result == "true")
                } else {
                    result.as_f64().or_else(|| result.as_str()?.parse().ok())
                        == Some(5.0)
                };
            }
        }
        Ok(flags)
    }

    fn send_nonblocking(
        &self,
        socket: &mut WebSocket<TcpStream>,
        id: i64,
        method: &str,
        params: Value,
    ) -> Result<(), String> {
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
        Ok(())
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
                            params: v.get("params").cloned().unwrap_or(Value::Null),
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
                "objectGroup": "sparrow",
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
            if method == "Flutter.Frame"
                && last_refresh.elapsed() >= Duration::from_millis(500)
            {
                *last_refresh = Instant::now();
                // Fire-and-forget refresh — response handled above.
                self.send_widget_tree_request(
                    socket,
                    next_request_id(),
                    isolate_id,
                )?;
            }
            let _ = self.notif_tx.send(Notification::Event {
                method: method.to_string(),
                params: v.get("params").cloned().unwrap_or(Value::Null),
            });
        }

        Ok(())
    }
}

fn next_request_id() -> i64 {
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
fn to_ws_url(vm_url: &str) -> Result<String, String> {
    if vm_url.starts_with("ws://") {
        return Ok(vm_url.to_owned());
    }
    let trimmed = vm_url.trim_end_matches('/');
    let base = trimmed.strip_prefix("http://").ok_or_else(|| {
        "Expected a local VM service HTTP or WebSocket URL".to_string()
    })?;
    Ok(format!("ws://{base}/ws"))
}

fn parse_summary_tree(response: &Value) -> Option<InspectorNode> {
    // Flutter wraps the tree in the extension's result, inside the VM reply.
    response
        .get("result")
        .and_then(parse_node)
        .or_else(|| response.get("value").and_then(parse_node))
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
    let mut properties = Vec::new();
    if let Some(props) = root.get("properties").and_then(|x| x.as_array()) {
        for prop in props {
            let name = prop
                .get("name")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let desc = prop
                .get("description")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            if !name.is_empty() && !desc.is_empty() {
                properties.push((name, desc));
            }
        }
    }

    if description.is_empty() {
        return None;
    }
    Some(InspectorNode {
        description,
        type_name,
        value_id,
        properties,
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
    pub selected_node: RwSignal<Option<RwSignal<InspectorNodeData>>>,
    pub updates: ReadSignal<Option<Notification>>,
    pub(crate) runtime_switches: RuntimeSwitches,
    pub(crate) performance: PerformanceData,
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
            selected_node: cx.create_rw_signal(None),
            updates,
            runtime_switches: RuntimeSwitches::new(cx),
            performance: PerformanceData::new(cx),
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
        self.runtime_switches.set(RuntimeFlags::default());
        self.performance.reset();
        self.client.connect(url);
    }

    pub fn disconnect(&self) {
        self.client.disconnect();
        self.performance.end_session();
        self.vm_url.set(None);
        self.root.set(None);
        self.selected_node.set(None);
        self.state.set(InspectorState::Disconnected);
        self.runtime_switches.set(RuntimeFlags::default());
    }

    /// Apply a notification received from the connection thread.
    pub fn apply(&self, notif: &Notification) {
        match notif {
            Notification::Performance(generation, update)
                if self.is_current(*generation) =>
            {
                self.performance.apply(update);
            }
            Notification::Connected(generation, flags)
                if self.is_current(*generation) =>
            {
                self.runtime_switches.set(*flags);
                self.state.set(InspectorState::Connected);
            }
            Notification::Restarting(generation) if self.is_current(*generation) => {
                self.performance.reset();
                self.state.set(InspectorState::Connecting);
                self.runtime_switches.set(RuntimeFlags::default());
                self.root.set(None);
                self.selected_node.set(None);
            }
            Notification::Disconnected(generation)
                if self.is_current(*generation) =>
            {
                self.performance.end_session();
                self.state.set(InspectorState::Disconnected);
                self.runtime_switches.set(RuntimeFlags::default());
                self.root.set(None);
            }
            Notification::Error(generation, e) if self.is_current(*generation) => {
                tracing::warn!("VM service error: {e}");
                self.state.set(InspectorState::Disconnected);
            }
            Notification::WidgetTree(tree) => {
                let root = InspectorNodeData::from_node(self.cx, tree.clone());
                self.root.set(Some(root));
            }
            _ => {}
        }
    }

    fn is_current(&self, generation: u64) -> bool {
        self.client.generation.load(Ordering::SeqCst) == generation
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
    fn to_ws_url_preserves_debug_adapter_websocket() {
        let url = "ws://127.0.0.1:41677/TOKEN=/ws";
        assert_eq!(to_ws_url(url).unwrap(), url);
    }

    #[test]
    fn to_ws_url_rejects_https() {
        assert!(to_ws_url("https://127.0.0.1:41677/TOKEN=/").is_err());
    }

    #[test]
    fn service_extensions_wait_for_the_matching_reply_in_order() {
        fn request(socket: &mut WebSocket<TcpStream>) -> Value {
            serde_json::from_str(
                socket.read().unwrap().into_text().unwrap().as_str(),
            )
            .unwrap()
        }
        fn respond(socket: &mut WebSocket<TcpStream>, id: &Value, result: Value) {
            let response = json!({ "jsonrpc": "2.0", "id": id, "result": result });
            socket
                .send(Message::Text(response.to_string().into()))
                .unwrap();
        }

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let (client, _notifications) = VmServiceClient::new();
        client.connect(format!("http://{}/", listener.local_addr().unwrap()));
        let connect_fixture = |isolate_id: &str| {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut socket = tungstenite::accept(stream).unwrap();
            for _ in 0..3 {
                let listen = request(&mut socket);
                respond(&mut socket, &listen["id"], json!({ "type": "Success" }));
            }
            let vm = request(&mut socket);
            respond(
                &mut socket,
                &vm["id"],
                json!({ "isolates": [{"id": isolate_id}] }),
            );
            let isolate = request(&mut socket);
            respond(
                &mut socket,
                &isolate["id"],
                json!({ "extensionRPCs": ["ext.flutter.timeDilation"] }),
            );
            let query = request(&mut socket);
            respond(&mut socket, &query["id"], json!({ "timeDilation": "1.0" }));
            socket
        };
        let mut socket = connect_fixture("isolates/1");
        let tree = request(&mut socket);
        assert_eq!(
            tree["params"],
            json!({ "isolateId": "isolates/1", "objectGroup": "sparrow" })
        );
        client.set_debug_paint(None, true);
        client.set_debug_paint(None, false);
        client.set_slow_animations(None, true);
        let on = request(&mut socket);
        assert_eq!(on["method"], "ext.flutter.debugPaint");
        assert_eq!(
            on["params"],
            json!({ "enabled": "true", "isolateId": "isolates/1" })
        );
        respond(&mut socket, &tree["id"], json!({}));
        socket
            .get_ref()
            .set_read_timeout(Some(Duration::from_millis(50)))
            .unwrap();
        assert!(matches!(
            socket.read(),
            Err(tungstenite::Error::Io(e))
                if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut)
        ));
        socket
            .get_ref()
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        respond(&mut socket, &on["id"], json!({ "enabled": "true" }));
        let off = request(&mut socket);
        assert_eq!(off["method"], "ext.flutter.debugPaint");
        assert_eq!(off["params"]["enabled"], "false");
        respond(&mut socket, &off["id"], json!({ "enabled": "false" }));
        let slow = request(&mut socket);
        assert_eq!(slow["method"], "ext.flutter.timeDilation");
        assert_eq!(slow["params"]["timeDilation"], "5.0");
        respond(&mut socket, &slow["id"], json!({ "timeDilation": "5.0" }));
        client.set_debug_paint(None, true);
        let in_flight = request(&mut socket);
        assert_eq!(in_flight["params"]["isolateId"], "isolates/1");
        client.set_repaint_rainbow(None, true);
        let exit = json!({
            "jsonrpc": "2.0",
            "method": "streamNotify",
            "params": {
                "streamId": "Isolate",
                "event": { "kind": "IsolateExit", "isolate": { "id": "isolates/1" } }
            }
        });
        socket.send(Message::Text(exit.to_string().into())).unwrap();

        let mut replacement = connect_fixture("isolates/2");
        let tree = request(&mut replacement);
        respond(&mut replacement, &tree["id"], json!({}));
        replacement
            .get_ref()
            .set_read_timeout(Some(Duration::from_millis(50)))
            .unwrap();
        assert!(matches!(
            replacement.read(),
            Err(tungstenite::Error::Io(e))
                if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut)
        ));
        replacement
            .get_ref()
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        client.set_repaint_rainbow(None, false);
        let current = request(&mut replacement);
        assert_eq!(current["method"], "ext.flutter.repaintRainbow");
        assert_eq!(
            current["params"],
            json!({ "isolateId": "isolates/2", "enabled": "false" })
        );
        respond(
            &mut replacement,
            &current["id"],
            json!({ "enabled": "false" }),
        );

        client.set_debug_paint(None, true);
        let in_flight = request(&mut replacement);
        assert_eq!(in_flight["params"]["isolateId"], "isolates/2");
        client.set_slow_animations(None, true);
        client.connect(format!("http://{}/", listener.local_addr().unwrap()));

        let mut rerun = connect_fixture("isolates/3");
        let tree = request(&mut rerun);
        respond(&mut rerun, &tree["id"], json!({}));
        rerun
            .get_ref()
            .set_read_timeout(Some(Duration::from_millis(50)))
            .unwrap();
        assert!(matches!(
            rerun.read(),
            Err(tungstenite::Error::Io(e))
                if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut)
        ));
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
                "description": "Root",
                "type": "Root",
                "valueId": "r",
                "children": [{ "description": "Text" }]
            }
        });
        let root = parse_summary_tree(&standard).unwrap();
        assert_eq!(root.description, "Root");
        assert_eq!(root.children[0].description, "Text");
        assert!(parse_summary_tree(&json!({ "result": null })).is_none());

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
