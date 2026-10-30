//! Persistent `dart language-server` client.
//!
//! Spawns one server per workspace, keeps it warm, sends incremental
//! `didChange` notifications on a 150 ms debounce, and routes
//! `publishDiagnostics` back to the UI.
//!
//! ## Wire diagram
//!
//! ```text
//!  [UI thread]                    [writer thread]              [dart language-server]
//!  send_change ──► Command::Change ──► debounce ──► didChange ──►
//!                                                                  ──► publishDiagnostics
//!  [UI thread]                    [reader thread]              ◄──
//!  effect ◄── Notification ◄───── LSP response / notif
//! ```
//!
//! ## Performance
//!
//! - Server boot:      ~500 ms (once per workspace)
//! - First didOpen:    immediate (no debounce)
//! - Debounced change: 150 ms
//! - Round-trip:       20–100 ms on warm summaries
//! - Old approach:     500 ms – 2 s **per keystroke**

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, unbounded};
use lsp_types::{CompletionItem, CompletionResponse, Diagnostic};
use parking_lot::Mutex;
use serde_json::{Value, json};
use std::io::BufWriter;
use std::process::Command;
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI64, Ordering},
    },
    thread,
    time::Duration,
};

/// A one-shot callback invoked from the reader thread when a response arrives.
/// `FnOnce + Send` is enough; nothing ever blocks on it.
type ResponseCallback = Box<dyn FnOnce(serde_json::Value) + Send + 'static>;

/// `create_signal_from_channel` in this floem rev is typed to
/// `std::sync::mpsc::Receiver`. The LSP's notification channel therefore
/// uses `std::sync::mpsc`, not the crossbeam channel used elsewhere.
type NotifSender = std::sync::mpsc::Sender<Notification>;
type NotifReceiver = std::sync::mpsc::Receiver<Notification>;

/// How long to wait after the last keystroke before sending `didChange`.
/// 150 ms is invisible to the human eye and far enough below the server's
/// own debounce that we don't fight it.
const DEBOUNCE: Duration = Duration::from_millis(150);

/// Upper bound on how long we block waiting for `initialize` to answer.
const INIT_TIMEOUT: Duration = Duration::from_secs(30);

// ─────────────────────────────────────────────────────────────────────
//  Public API
// ─────────────────────────────────────────────────────────────────────

/// Events emitted from the server to the UI.
#[derive(Debug, Clone)]
pub enum Notification {
    /// Diagnostics for a single file. `version` is the LSP document version
    /// we sent, or `-1` if the server didn't include one.
    Diagnostics {
        path: PathBuf,
        version: i64,
        diagnostics: Vec<Diagnostic>,
    },
    /// `window/logMessage` or parse noise.
    Log(String),
    /// Fatal — the server exited unexpectedly.
    Error(String),
}

/// Handle to the persistent Dart language server. Cheap to clone.
#[derive(Clone)]
pub struct DartLspService {
    tx: Sender<LspCommand>,
    state: Arc<SharedState>,
    last_sent: Arc<Mutex<HashMap<PathBuf, i64>>>,
    available: Arc<AtomicBool>,
}

impl DartLspService {
    /// Spawn a server for `workspace_root`. Returns `(service, receiver)`;
    /// the receiver should be handed to `floem::ext_event::create_signal_from_channel`
    /// **exactly once**.
    ///
    /// If `dart` is not on PATH the service is returned in the "unavailable"
    /// state and every method is a no-op — the caller never needs to check.
    pub fn spawn(workspace_root: PathBuf) -> (Self, NotifReceiver) {
        let (cmd_tx, cmd_rx) = unbounded();
        let (notif_tx, notif_rx) = std::sync::mpsc::channel(); // ← was unbounded()
        let available = Arc::new(AtomicBool::new(false));
        let last_sent = Arc::new(Mutex::new(HashMap::new()));
        let state = Arc::new(SharedState {
            next_id: AtomicI64::new(1),
            pending_requests: Mutex::new(HashMap::new()),
            open_files: Mutex::new(HashMap::new()),
            shutting_down: AtomicBool::new(false),
        });

        if !dart_available() {
            tracing::warn!(
                "`dart` not on PATH — Dart diagnostics disabled. \
                 Install the Dart SDK to enable them."
            );
            // `notif_tx` dropped here → the receiver closes → the UI
            // effect fires once with `None` and then never again. Clean.
            return (
                Self {
                    tx: cmd_tx,
                    state: state.clone(),
                    last_sent,
                    available,
                },
                notif_rx,
            );
        }

        let mut child = match Command::new("dart")
            .arg("language-server")
            .arg("--protocol=lsp")
            .current_dir(&workspace_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("failed to spawn `dart language-server`: {e}");
                return (
                    Self {
                        tx: cmd_tx,
                        state: state.clone(),
                        last_sent,
                        available,
                    },
                    notif_rx,
                );
            }
        };

        let stdin = child.stdin.take().expect("child stdin");
        let stdout = child.stdout.take().expect("child stdout");
        let stderr = child.stderr.take().expect("child stderr");

        let state = Arc::new(SharedState {
            next_id: AtomicI64::new(1),
            pending_requests: Mutex::new(HashMap::new()),
            open_files: Mutex::new(HashMap::new()),
            shutting_down: AtomicBool::new(false),
        });

        // ── Writer thread ────────────────────────────────────────────
        {
            let state = state.clone();
            let notif_tx = notif_tx.clone();
            let root = workspace_root.clone();
            thread::Builder::new()
                .name("DartLspWriter".into())
                .spawn(move || {
                    if let Err(e) =
                        run_writer(state, BufWriter::new(stdin), cmd_rx, &root)
                    {
                        let _ = notif_tx.send(Notification::Error(e));
                    }
                })
                .expect("spawn DartLspWriter");
        }

        // ── Reader thread ────────────────────────────────────────────
        {
            let state = state.clone();
            thread::Builder::new()
                .name("DartLspReader".into())
                .spawn(move || {
                    run_reader(state, BufReader::new(stdout), notif_tx);
                })
                .expect("spawn DartLspReader");
        }

        // ── Stderr drain (prevents pipe-buffer stall) ────────────────
        thread::Builder::new()
            .name("DartLspStderr".into())
            .spawn(move || {
                let mut reader = BufReader::new(stderr);
                let mut line = String::new();
                while reader.read_line(&mut line).is_ok() {
                    if !line.trim().is_empty() {
                        tracing::debug!("dart language-server: {}", line.trim_end());
                    }
                    line.clear();
                }
            })
            .expect("spawn DartLspStderr");

        // ── Reaper (blocks until server exits; frees OS resources) ───
        thread::Builder::new()
            .name("DartLspReaper".into())
            .spawn(move || {
                let _ = child.wait();
                tracing::debug!("dart language-server exited");
            })
            .expect("spawn DartLspReaper");

        available.store(true, Ordering::Relaxed);
        (
            Self {
                tx: cmd_tx,
                state: state.clone(), // ← NEW
                last_sent,
                available,
            },
            notif_rx,
        )
    }

    /// A service that does nothing. Used for remote workspaces, projectless
    /// workspaces, or when `dart` is not on PATH.
    pub fn spawn_unavailable() -> (Self, NotifReceiver) {
        let (tx, _rx) = unbounded();
        let (notif_tx, notif_rx) = std::sync::mpsc::channel();
        drop(notif_tx);
        (
            Self {
                tx,
                state: Arc::new(SharedState {
                    next_id: AtomicI64::new(1),
                    pending_requests: Mutex::new(HashMap::new()),
                    open_files: Mutex::new(HashMap::new()),
                    shutting_down: AtomicBool::new(false),
                }),
                last_sent: Arc::new(Mutex::new(HashMap::new())),
                available: Arc::new(AtomicBool::new(false)),
            },
            notif_rx,
        )
    }

    pub fn send_change(&self, path: PathBuf, content: String, version: i64) {
        if !self.available.load(Ordering::Relaxed) {
            return;
        }
        let mut last = self.last_sent.lock();
        if let Some(&prev) = last.get(&path) {
            if prev >= version {
                return;
            }
        }
        last.insert(path.clone(), version);
        drop(last);
        let _ = self.tx.send(LspCommand::Change {
            path,
            content,
            version,
        });
    }

    pub fn send_close(&self, path: PathBuf) {
        self.last_sent.lock().remove(&path);
        if self.available.load(Ordering::Relaxed) {
            let _ = self.tx.send(LspCommand::Close { path });
        }
    }

    pub fn is_available(&self) -> bool {
        self.available.load(Ordering::Relaxed)
    }

    // ── generic request plumbing ──────────────────────────────────

    /// Send an LSP request. The callback fires from the reader thread
    /// with the full JSON-RPC response object (`{ result, error? }`), so
    /// callers can distinguish success from failure. Returns the request
    /// id (useful for logging).
    ///
    /// No timeout is applied. If the server never responds, the callback
    /// never fires — the caller's closure captures whatever it needs, and
    /// gets dropped when `WindowTabData` tears down (closing the pipe
    /// causes the reader thread to exit).
    pub fn request_async<F>(
        &self,
        method: impl Into<String>,
        params: Value,
        cb: F,
    ) -> i64
    where
        F: FnOnce(Value) + Send + 'static,
    {
        let id = self.state.next_id.fetch_add(1, Ordering::Relaxed);
        self.state.pending_requests.lock().insert(id, Box::new(cb));
        let _ = self.tx.send(LspCommand::Request {
            id,
            method: method.into(),
            params,
        });
        id
    }

    // ── typed methods ─────────────────────────────────────────────

    /// `textDocument/documentSymbol` — returns the hierarchical symbol
    /// tree for `path`. Empty vec if the server errors or the file isn't
    /// known.
    pub fn document_symbols<F>(&self, path: &Path, cb: F) -> i64
    where
        F: FnOnce(Vec<lsp_types::DocumentSymbol>) + Send + 'static,
    {
        let uri = path_to_uri(path);
        self.request_async(
            "textDocument/documentSymbol",
            json!({ "textDocument": { "uri": uri } }),
            move |resp| {
                let symbols = resp
                    .get("result")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();
                cb(symbols);
            },
        )
    }

    /// `textDocument/hover`. Positions are 0-based (LSP convention).
    pub fn hover<F>(&self, path: &Path, line: u32, character: u32, cb: F) -> i64
    where
        F: FnOnce(Option<lsp_types::Hover>) + Send + 'static,
    {
        let uri = path_to_uri(path);
        self.request_async(
            "textDocument/hover",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }),
            move |resp| {
                let hover = resp
                    .get("result")
                    .and_then(|v| serde_json::from_value(v.clone()).ok());
                cb(hover);
            },
        )
    }

    /// `textDocument/definition`. Handles all three LSP shapes
    /// (Location, Location[], LocationLink[]) transparently.
    pub fn definition<F>(&self, path: &Path, line: u32, character: u32, cb: F) -> i64
    where
        F: FnOnce(Vec<lsp_types::Location>) + Send + 'static,
    {
        let uri = path_to_uri(path);
        self.request_async(
            "textDocument/definition",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }),
            move |resp| {
                cb(parse_definition_response(
                    resp.get("result").cloned().unwrap_or(Value::Null),
                ));
            },
        )
    }

    /// `textDocument/references`. Returns all locations the server knows about
    /// (may span multiple files). `include_declaration: true` matches the
    /// behavior of every other IDE.
    pub fn references<F>(
        &self,
        path: &Path,
        line: u32,
        character: u32,
        include_declaration: bool,
        cb: F,
    ) -> i64
    where
        F: FnOnce(Vec<lsp_types::Location>) + Send + 'static,
    {
        let uri = path_to_uri(path);
        self.request_async(
            "textDocument/references",
            json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character },
            "context": { "includeDeclaration": include_declaration }
        }),
            move |resp| {
                let locations = resp
                    .get("result")
                    .filter(|v| !v.is_null())
                    .and_then(|v| {
                        serde_json::from_value::<Vec<lsp_types::Location>>(v.clone()).ok()
                    })
                    .unwrap_or_default();
                cb(locations);
            },
        )
    }

    /// `textDocument/prepareRename`. `Some(_)` means "the server has a name
    /// at this position and will accept a rename"; `None` means "cursor isn't
    /// on an identifier" (or a transport error).
    pub fn prepare_rename<F>(
        &self,
        path: &Path,
        line: u32,
        character: u32,
        cb: F,
    ) -> i64
    where
        F: FnOnce(Option<lsp_types::PrepareRenameResponse>) + Send + 'static,
    {
        let uri = path_to_uri(path);
        self.request_async(
            "textDocument/prepareRename",
            json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character }
        }),
            move |resp| {
                let parsed = resp
                    .get("result")
                    .filter(|v| !v.is_null())
                    .and_then(|v| {
                        serde_json::from_value::<lsp_types::PrepareRenameResponse>(
                            v.clone(),
                        )
                            .ok()
                    });
                cb(parsed);
            },
        )
    }

    /// `textDocument/rename`. Returns a `WorkspaceEdit` that must be applied
    /// via [`crate::main_split::MainSplitData::apply_workspace_edit`].
    pub fn rename<F>(
        &self,
        path: &Path,
        line: u32,
        character: u32,
        new_name: &str,
        cb: F,
    ) -> i64
    where
        F: FnOnce(Option<lsp_types::WorkspaceEdit>) + Send + 'static,
    {
        let uri = path_to_uri(path);
        let new_name = new_name.to_string();
        self.request_async(
            "textDocument/rename",
            json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character },
            "newName": new_name
        }),
            move |resp| {
                let edit = resp
                    .get("result")
                    .filter(|v| !v.is_null())
                    .and_then(|v| {
                        serde_json::from_value::<lsp_types::WorkspaceEdit>(v.clone())
                            .ok()
                    });
                cb(edit);
            },
        )
    }

    /// `textDocument/completion` at the given 0-based position.
    ///
    /// The server decides what's relevant given the current position and
    /// document content — we don't need to compute a prefix on this side.
    /// Returns the flattened list of items (or an empty vec on error / null).
    pub fn completion<F>(&self, path: &Path, line: u32, character: u32, cb: F) -> i64
    where
        F: FnOnce(Option<Vec<CompletionItem>>) + Send + 'static,
    {
        let uri = path_to_uri(path);
        self.request_async(
            "textDocument/completion",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character },
                // LSP spec: omitting `context` = Invoked. The Dart server
                // treats that as "compute completions for this exact
                // position", which is what we want.
                "context": { "triggerKind": 1 }
            }),
            move |resp| {
                // `result: null` means "no completions"; distinguish from
                // transport / server error (`error` present) so the caller
                // can clear stale state either way.
                let items = resp
                    .get("result")
                    .filter(|v| !v.is_null())
                    .and_then(|v| {
                        serde_json::from_value::<CompletionResponse>(v.clone()).ok()
                    })
                    .map(|r| match r {
                        CompletionResponse::Array(items) => items,
                        CompletionResponse::List(list) => list.items,
                    });
                cb(items);
            },
        )
    }

    /// `textDocument/codeAction`. Returns refactorings, quick-fixes, etc.
    /// for the given position + diagnostics.
    pub fn code_actions<F>(
        &self,
        path: &Path,
        line: u32,
        character: u32,
        diagnostics: &[lsp_types::Diagnostic],
        cb: F,
    ) -> i64
    where
        F: FnOnce(Vec<lsp_types::CodeActionOrCommand>) + Send + 'static,
    {
        let uri = path_to_uri(path);
        let diagnostics = diagnostics.to_vec();
        self.request_async(
            "textDocument/codeAction",
            json!({
            "textDocument": { "uri": uri },
            "range": {
                "start": { "line": line, "character": character },
                "end": { "line": line, "character": character }
            },
            "context": {
                "diagnostics": diagnostics,
                "triggerKind": 1
            }
        }),
            move |resp| {
                let actions = resp
                    .get("result")
                    .filter(|v| !v.is_null())
                    .and_then(|v| {
                        serde_json::from_value::<Vec<lsp_types::CodeActionOrCommand>>(
                            v.clone(),
                        )
                            .ok()
                    })
                    .unwrap_or_default();
                cb(actions);
            },
        )
    }

    /// `codeAction/resolve`. Some actions only provide their `edit` after
    /// explicit resolution.
    pub fn resolve_code_action<F>(
        &self,
        action: lsp_types::CodeAction,
        cb: F,
    ) -> i64
    where
        F: FnOnce(Option<lsp_types::WorkspaceEdit>) + Send + 'static,
    {
        self.request_async(
            "codeAction/resolve",
            serde_json::to_value(&action).unwrap_or(Value::Null),
            move |resp| {
                let edit = resp
                    .get("result")
                    .filter(|v| !v.is_null())
                    .and_then(|v| {
                        serde_json::from_value::<lsp_types::CodeAction>(v.clone()).ok()
                    })
                    .and_then(|a| a.edit);
                cb(edit);
            },
        )
    }
}

impl Drop for DartLspService {
    fn drop(&mut self) {
        let _ = self.tx.send(LspCommand::Shutdown);
    }
}

fn parse_definition_response(value: Value) -> Vec<lsp_types::Location> {
    if value.is_null() {
        return Vec::new();
    }
    if let Ok(loc) = serde_json::from_value::<lsp_types::Location>(value.clone()) {
        return vec![loc];
    }
    if let Ok(locs) =
        serde_json::from_value::<Vec<lsp_types::Location>>(value.clone())
    {
        return locs;
    }
    if let Ok(links) = serde_json::from_value::<Vec<lsp_types::LocationLink>>(value)
    {
        return links
            .into_iter()
            .map(|link| lsp_types::Location {
                uri: link.target_uri,
                range: link.target_selection_range,
            })
            .collect();
    }
    Vec::new()
}

// ─────────────────────────────────────────────────────────────────────
//  Commands (UI → writer)
// ─────────────────────────────────────────────────────────────────────

enum LspCommand {
    Change {
        path: PathBuf,
        content: String,
        version: i64,
    },
    Close {
        path: PathBuf,
    },
    /// A JSON-RPC *request* to the server. Response is delivered to the
    /// callback registered under `id` in `SharedState::pending_requests`.
    Request {
        id: i64,
        method: String,
        params: Value,
    },
    Shutdown,
}

// ─────────────────────────────────────────────────────────────────────
//  Shared between threads
// ─────────────────────────────────────────────────────────────────────

struct SharedState {
    next_id: AtomicI64,
    pending_requests: Mutex<HashMap<i64, ResponseCallback>>,
    open_files: Mutex<HashMap<PathBuf, i64>>,
    shutting_down: AtomicBool,
}

// ─────────────────────────────────────────────────────────────────────
//  Writer thread
// ─────────────────────────────────────────────────────────────────────

fn run_writer(
    state: Arc<SharedState>,
    mut out: impl Write,
    rx: Receiver<LspCommand>,
    workspace_root: &Path,
) -> Result<(), String> {
    initialize(&state, &mut out, workspace_root)?;

    loop {
        // First command of a burst
        let first = match rx.recv() {
            Ok(c) => c,
            Err(_) => return Ok(()),
        };
        let mut pending: HashMap<PathBuf, (String, i64)> = HashMap::new();
        match first {
            LspCommand::Request { id, method, params } => {
                let msg = json!({
                    "jsonrpc": "2.0", "id": id, "method": method, "params": params
                });
                if write_lsp_message(&mut out, &msg).is_err() {
                    // Wake the callback with an error-shaped response.
                    if let Some(cb) = state.pending_requests.lock().remove(&id) {
                        cb(
                            json!({"error": {"code": -1, "message": "write failed"}}),
                        );
                    }
                }
            }
            LspCommand::Change {
                path,
                content,
                version,
            } => {
                // First ever change for a file: send didOpen immediately
                // so the server starts analyzing without our debounce on top.
                let is_open = state.open_files.lock().contains_key(&path);
                if !is_open {
                    apply_change(&state, &mut out, &path, content, version);
                    continue;
                }
                pending.insert(path, (content, version));
            }
            LspCommand::Close { path } => {
                close_file(&state, &mut out, &path);
                continue;
            }
            LspCommand::Shutdown => {
                shutdown(&state, &mut out);
                return Ok(());
            }
        }

        // Debounce: keep collecting until 150 ms of quiet.
        loop {
            match rx.recv_timeout(DEBOUNCE) {
                Ok(LspCommand::Request { id, method, params }) => {
                    let msg = json!({
                        "jsonrpc": "2.0", "id": id, "method": method, "params": params
                    });
                    if write_lsp_message(&mut out, &msg).is_err() {
                        // Wake the callback with an error-shaped response.
                        if let Some(cb) = state.pending_requests.lock().remove(&id) {
                            cb(
                                json!({"error": {"code": -1, "message": "write failed"}}),
                            );
                        }
                    }
                }
                Ok(LspCommand::Change {
                    path,
                    content,
                    version,
                }) => {
                    pending.insert(path, (content, version));
                }
                Ok(LspCommand::Close { path }) => {
                    pending.remove(&path);
                    close_file(&state, &mut out, &path);
                }
                Ok(LspCommand::Shutdown) => {
                    shutdown(&state, &mut out);
                    return Ok(());
                }
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            }
        }

        for (path, (content, version)) in pending.drain() {
            apply_change(&state, &mut out, &path, content, version);
        }
    }
}

fn initialize(
    state: &SharedState,
    out: &mut impl Write,
    workspace_root: &Path,
) -> Result<(), String> {
    let id = state.next_id.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = crossbeam_channel::bounded(1);
    state.pending_requests.lock().insert(
        id,
        Box::new(move |v| {
            let _ = tx.send(v);
        }),
    );

    let root_uri = path_to_uri(workspace_root);
    let params = json!({
        "processId": std::process::id(),
        "clientInfo": { "name": "sparrow", "version": env!("CARGO_PKG_VERSION") },
        "rootUri": root_uri,
        "workspaceFolders": [{ "uri": root_uri, "name": workspace_root
            .file_name().and_then(|s| s.to_str()).unwrap_or("workspace") }],
        "capabilities": {
            "textDocument": {
                "synchronization": {
                    "dynamicRegistration": false,
                    "didSave": false, "willSave": false, "willSaveWaitUntil": false
                },
                "publishDiagnostics": {
                    "relatedInformation": true, "versionSupport": true,
                    "tagSupport": { "valueSet": [1, 2] }
                },
                "documentSymbol": {
                    "hierarchicalDocumentSymbolSupport": true,
                    "symbolKind": { "valueSet": [1,2,3,4,5,6,7,8,9,10,11,12,
                        13,14,15,16,17,18,19,20,21,22,23,24,25,26] }
                },
                "hover": { "contentFormat": ["markdown", "plaintext"] },
                "definition": { "linkSupport": true }
            },
            "workspace": { "workspaceFolders": true, "configuration": false },
            "window": { "workDoneProgress": true }
        }
    });

    write_lsp_message(
        out,
        &json!({ "jsonrpc": "2.0", "id": id, "method": "initialize", "params": params }),
    )
        .map_err(|e| format!("write initialize: {e}"))?;

    match rx.recv_timeout(INIT_TIMEOUT) {
        Ok(resp) => {
            if let Some(err) = resp.get("error") {
                return Err(format!("initialize error: {err}"));
            }
        }
        Err(_) => return Err("initialize timed out".into()),
    }

    write_lsp_message(
        out,
        &json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }),
    )
    .map_err(|e| format!("write initialized: {e}"))?;

    tracing::debug!("dart language-server initialized");
    Ok(())
}

fn apply_change(
    state: &SharedState,
    out: &mut impl Write,
    path: &Path,
    content: String,
    version: i64,
) {
    let uri = path_to_uri(path);
    let mut open = state.open_files.lock();
    let is_open = open.get(path).copied();
    match is_open {
        Some(prev) if prev >= version => return,
        Some(_) => {
            open.insert(path.to_path_buf(), version);
            drop(open);
            let _ = write_lsp_message(
                out,
                &json!({
                    "jsonrpc": "2.0", "method": "textDocument/didChange",
                    "params": {
                        "textDocument": { "uri": uri, "version": version },
                        "contentChanges": [{ "text": content }]
                    }
                }),
            );
        }
        None => {
            open.insert(path.to_path_buf(), version);
            drop(open);
            let _ = write_lsp_message(
                out,
                &json!({
                    "jsonrpc": "2.0", "method": "textDocument/didOpen",
                    "params": {
                        "textDocument": {
                            "uri": uri, "languageId": "dart",
                            "version": version, "text": content
                        }
                    }
                }),
            );
        }
    }
}

fn close_file(state: &SharedState, out: &mut impl Write, path: &Path) {
    if state.open_files.lock().remove(path).is_some() {
        let _ = write_lsp_message(
            out,
            &json!({
                "jsonrpc": "2.0", "method": "textDocument/didClose",
                "params": { "textDocument": { "uri": path_to_uri(path) } }
            }),
        );
    }
}

fn shutdown(state: &SharedState, out: &mut impl Write) {
    state.shutting_down.store(true, Ordering::Relaxed);
    let id = state.next_id.fetch_add(1, Ordering::Relaxed);
    let _ = write_lsp_message(
        out,
        &json!({
            "jsonrpc": "2.0", "id": id, "method": "shutdown", "params": null
        }),
    );
    let _ = write_lsp_message(
        out,
        &json!({
            "jsonrpc": "2.0", "method": "exit", "params": null
        }),
    );
}

// ─────────────────────────────────────────────────────────────────────
//  Reader thread
// ─────────────────────────────────────────────────────────────────────

fn run_reader(
    state: Arc<SharedState>,
    mut input: impl BufRead,
    notif_tx: NotifSender, // ← was Sender<Notification>
) {
    loop {
        let msg = match read_lsp_message(&mut input) {
            Ok(m) => m,
            Err(_) => {
                if !state.shutting_down.load(Ordering::Relaxed) {
                    let _ = notif_tx.send(Notification::Error(
                        "dart language-server exited unexpectedly".into(),
                    ));
                }
                return;
            }
        };
        let value: Value = match serde_json::from_str(&msg) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("malformed LSP message: {e}");
                continue;
            }
        };
        dispatch(state.as_ref(), &notif_tx, value);
    }
}

fn dispatch(state: &SharedState, notif_tx: &NotifSender, msg: Value) {
    // Response?
    if msg.get("method").is_none() {
        if let Some(id) = msg.get("id").and_then(Value::as_i64) {
            if let Some(cb) = state.pending_requests.lock().remove(&id) {
                // Pass the full message so callbacks can inspect `error`.
                cb(msg);
            }
        }
        return;
    }

    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    let params = msg.get("params").cloned().unwrap_or(Value::Null);

    match method {
        "textDocument/publishDiagnostics" => {
            let Some(uri) = params.get("uri").and_then(Value::as_str) else {
                return;
            };
            let Some(path) = uri_to_path(uri) else { return };
            let version =
                params.get("version").and_then(Value::as_i64).unwrap_or(-1);
            let diagnostics = params
                .get("diagnostics")
                .and_then(|v| {
                    serde_json::from_value::<Vec<Diagnostic>>(v.clone()).ok()
                })
                .unwrap_or_default();
            let _ = notif_tx.send(Notification::Diagnostics {
                path,
                version,
                diagnostics,
            });
        }
        "window/logMessage" | "window/showMessage" => {
            let text = params
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            if !text.is_empty() {
                let _ = notif_tx.send(Notification::Log(text));
            }
        }
        // Ignored: $/progress, window/workDoneProgress/create, telemetry, etc.
        _ => {}
    }
}

// ─────────────────────────────────────────────────────────────────────
//  Wire protocol
// ─────────────────────────────────────────────────────────────────────

fn write_lsp_message(out: &mut impl Write, msg: &Value) -> std::io::Result<()> {
    let body = serde_json::to_string(msg)?;
    write!(out, "Content-Length: {}\r\n\r\n{}", body.len(), body)?;
    out.flush()
}

fn read_lsp_message(input: &mut impl BufRead) -> std::io::Result<String> {
    let mut content_length: usize = 0;
    loop {
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "server closed stdout",
            ));
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some(rest) = trimmed.strip_prefix("Content-Length: ") {
            content_length = rest.parse().unwrap_or(0);
        }
    }
    let mut buf = vec![0u8; content_length];
    input.read_exact(&mut buf)?;
    String::from_utf8(buf)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

fn path_to_uri(path: &Path) -> String {
    url::Url::from_file_path(path)
        .map(|u| u.to_string())
        .unwrap_or_else(|_| format!("file://{}", path.display()))
}

fn uri_to_path(uri: &str) -> Option<PathBuf> {
    url::Url::parse(uri).ok()?.to_file_path().ok()
}

fn dart_available() -> bool {
    Command::new("dart")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

// ─────────────────────────────────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_round_trip() {
        let mut buf = Vec::new();
        let msg = json!({"jsonrpc": "2.0", "method": "test", "params": {"x": 1}});
        write_lsp_message(&mut buf, &msg).unwrap();

        // Round-trip through the reader
        let mut cursor = std::io::Cursor::new(buf);
        let parsed = read_lsp_message(&mut cursor).unwrap();
        let parsed: Value = serde_json::from_str(&parsed).unwrap();
        assert_eq!(parsed, msg);
    }

    #[test]
    fn uri_round_trip() {
        let path = PathBuf::from("/tmp/hello.dart");
        let uri = path_to_uri(&path);
        assert_eq!(uri_to_path(&uri), Some(path));
    }

    #[test]
    fn uri_handles_spaces() {
        let path = PathBuf::from("/tmp/hello world.dart");
        let uri = path_to_uri(&path);
        assert!(uri.contains("%20"));
        assert_eq!(uri_to_path(&uri), Some(path));
    }

    #[test]
    fn unavailable_service_is_inert() {
        let (tx, _rx) = unbounded();
        let svc = DartLspService {
            tx,
            state: Arc::new(SharedState {
                next_id: AtomicI64::new(1),
                pending_requests: Mutex::new(HashMap::new()),
                open_files: Mutex::new(HashMap::new()),
                shutting_down: AtomicBool::new(false),
            }),
            last_sent: Arc::new(Mutex::new(HashMap::new())),
            available: Arc::new(AtomicBool::new(false)),
        };
        svc.send_change(PathBuf::from("/x.dart"), String::new(), 1);
        svc.send_close(PathBuf::from("/x.dart"));
        assert!(!svc.is_available());
    }

    #[test]
    fn request_id_is_monotonic() {
        let (tx, _rx) = unbounded();
        let state = Arc::new(SharedState {
            next_id: AtomicI64::new(1),
            pending_requests: Mutex::new(HashMap::new()),
            open_files: Mutex::new(HashMap::new()),
            shutting_down: AtomicBool::new(false),
        });
        let svc = DartLspService {
            tx,
            state: state.clone(),
            last_sent: Arc::new(Mutex::new(HashMap::new())),
            available: Arc::new(AtomicBool::new(true)),
        };
        let a = svc.request_async("a", Value::Null, |_| {});
        let b = svc.request_async("b", Value::Null, |_| {});
        assert!(b > a);
    }

    #[test]
    fn parse_definition_handles_all_shapes() {
        use lsp_types::Url;

        // Scalar
        let scalar = json!({
            "uri": "file:///a.dart",
            "range": { "start": {"line":0,"character":0}, "end": {"line":0,"character":5} }
        });
        let scalar_clone = scalar.clone();
        assert_eq!(parse_definition_response(scalar_clone).len(), 1);

        // Array
        let array = json!([scalar.clone(), scalar.clone()]);
        assert_eq!(parse_definition_response(array).len(), 2);

        // LocationLink
        let link = json!({
            "targetUri": "file:///b.dart",
            "targetRange": { "start": {"line":0,"character":0}, "end": {"line":0,"character":5} },
            "targetSelectionRange": { "start": {"line":0,"character":0}, "end": {"line":0,"character":5} }
        });
        let links = parse_definition_response(json!([link]));
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].uri, Url::parse("file:///b.dart").unwrap());

        // Null / unknown
        assert!(parse_definition_response(Value::Null).is_empty());
        assert!(parse_definition_response(json!({})).is_empty());
    }

    #[test]
    fn parse_completion_handles_both_shapes() {
        use lsp_types::CompletionResponse;

        let array: CompletionResponse = serde_json::from_value(json!([
            { "label": "print" },
            { "label": "printf" }
        ]))
        .unwrap();
        let items = match array {
            CompletionResponse::Array(v) => v,
            _ => panic!("expected array"),
        };
        assert_eq!(items.len(), 2);

        let list: CompletionResponse = serde_json::from_value(json!({
            "isIncomplete": false,
            "items": [{ "label": "print" }]
        }))
        .unwrap();
        let items = match list {
            CompletionResponse::List(l) => l.items,
            _ => panic!("expected list"),
        };
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn references_parses_location_array() {
        let locations: Vec<lsp_types::Location> = serde_json::from_value(json!([
        { "uri": "file:///a.dart", "range": { "start": {"line":0,"character":0},
            "end": {"line":0,"character":5} } },
        { "uri": "file:///b.dart", "range": { "start": {"line":2,"character":3},
            "end": {"line":2,"character":8} } }
    ])).unwrap();
        assert_eq!(locations.len(), 2);
        assert_eq!(locations[1].range.start.line, 2);
    }

    #[test]
    fn prepare_rename_parses_all_shapes() {
        use lsp_types::PrepareRenameResponse;

        // Range
        let r: PrepareRenameResponse = serde_json::from_value(json!({
        "start": {"line":0,"character":0}, "end": {"line":0,"character":5}
    })).unwrap();
        assert!(matches!(r, PrepareRenameResponse::Range(_)));

        // Range with placeholder
        let r: PrepareRenameResponse = serde_json::from_value(json!({
        "range": { "start": {"line":0,"character":0},
            "end": {"line":0,"character":5} },
        "placeholder": "foo"
    })).unwrap();
        assert!(matches!(r, PrepareRenameResponse::RangeWithPlaceholder { .. }));

        // Default behavior
        let r: PrepareRenameResponse = serde_json::from_value(json!({
        "defaultBehavior": true
    })).unwrap();
        assert!(matches!(r, PrepareRenameResponse::DefaultBehavior { .. }));
    }

    #[test]
    fn rename_parses_workspace_edit() {
        let edit: lsp_types::WorkspaceEdit = serde_json::from_value(json!({
        "changes": {
            "file:///a.dart": [{
                "range": { "start": {"line":0,"character":0},
                    "end": {"line":0,"character":5} },
                "newText": "bar"
            }]
        }
    })).unwrap();
        assert!(edit.changes.is_some());
    }
}
