//! Reactive state for the Dart outline panel.
//!
//! Tree nodes are `RwSignal`s so individual chevrons can toggle without
//! rebuilding the whole tree. Re-analysis is debounced (300 ms) so typing
//! doesn't spam `documentSymbol`; the tree reflects the file within ~300 ms
//! of the last keystroke.

use std::{
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex},
    time::Duration,
};

use floem::{
    action::{exec_after, TimerToken},
    reactive::{
        ReadSignal, RwSignal, Scope, SignalGet, SignalUpdate, SignalWith,
    },
};
use lsp_types::DocumentSymbol;

use crate::{
    config::SparrowConfig,
    dart::DartLspService,
    editor::EditorData,
    window_tab::CommonData,
};

const DEBOUNCE: Duration = Duration::from_millis(300);

/// One node in the outline tree.
#[derive(Clone)]
pub struct OutlineNodeData {
    pub symbol: DocumentSymbol,
    pub open: RwSignal<bool>,
    pub children: Vec<RwSignal<OutlineNodeData>>,
}

impl OutlineNodeData {
    fn from_symbol(cx: Scope, symbol: DocumentSymbol) -> RwSignal<OutlineNodeData> {
        let children: Vec<RwSignal<OutlineNodeData>> = symbol
            .children
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(|c| Self::from_symbol(cx, c))
            .collect();
        cx.create_rw_signal(OutlineNodeData {
            symbol,
            open: cx.create_rw_signal(true),
            children,
        })
    }
}

/// Build the tree from a flat list of top-level `DocumentSymbol`s.
pub fn build_outline(
    cx: Scope,
    symbols: Vec<DocumentSymbol>,
) -> Vec<RwSignal<OutlineNodeData>> {
    symbols
        .into_iter()
        .map(|s| OutlineNodeData::from_symbol(cx, s))
        .collect()
}

/// Flatten `nodes` (respecting expansion) into `(depth, node)` pairs for
/// `virtual_stack`. Tracking `.get()` on the node and its `open` signal
/// makes the caller re-run when either changes.
pub fn flatten(
    nodes: &[RwSignal<OutlineNodeData>],
    depth: usize,
    out: &mut Vec<(usize, RwSignal<OutlineNodeData>)>,
) {
    for node in nodes {
        out.push((depth, *node));
        let n = node.get();
        if n.open.get() {
            flatten(&n.children, depth + 1, out);
        }
    }
}

#[derive(Clone)]
pub struct OutlineData {
    pub roots: RwSignal<Vec<RwSignal<OutlineNodeData>>>,
    pub source_path: RwSignal<Option<PathBuf>>,
    /// Receives `(path, symbols)` from the LSP reader thread.
    pub updates: ReadSignal<Option<(PathBuf, Vec<DocumentSymbol>)>>,
    sender: Arc<Mutex<std::sync::mpsc::Sender<(PathBuf, Vec<DocumentSymbol>)>>>,
    pub common: Rc<CommonData>,
    cx: Scope,
}

impl OutlineData {
    pub fn new(cx: Scope, common: Rc<CommonData>) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let updates = floem::ext_event::create_signal_from_channel(rx);
        Self {
            roots: cx.create_rw_signal(Vec::new()),
            source_path: cx.create_rw_signal(None),
            updates,
            sender: Arc::new(Mutex::new(tx)),
            common,
            cx,
        }
    }

    /// Request a fresh outline. Called from the debounce timer on the UI
    /// thread.
    pub fn analyze(&self, editor: &EditorData, lsp: &DartLspService) {
        let doc = editor.doc();
        let Some(path) = doc.content.with_untracked(|c| c.path().cloned()) else {
            self.clear();
            return;
        };
        if !crate::dart::is_dart_path(&path) {
            self.clear();
            return;
        }
        self.source_path.set(Some(path.clone()));
        if !lsp.is_available() {
            self.roots.set(Vec::new());
            return;
        }
        let sender = self.sender.clone();
        let path_for_cb = path.clone();
        lsp.document_symbols(&path, move |symbols| {
            if let Ok(tx) = sender.lock() {
                let _ = tx.send((path_for_cb, symbols));
            }
        });
    }

    /// Apply a completed analysis. Discarded if the user has since switched
    /// to a different file.
    pub fn apply_update(&self, for_path: PathBuf, symbols: Vec<DocumentSymbol>) {
        if self
            .source_path
            .with_untracked(|p| p.as_ref() == Some(&for_path))
        {
            let roots = build_outline(self.cx, symbols);
            self.roots.set(roots);
        }
    }

    pub fn clear(&self) {
        if self.source_path.with_untracked(|p| p.is_some()) {
            self.roots.set(Vec::new());
            self.source_path.set(None);
        }
    }

    pub fn config(&self) -> ReadSignal<Arc<SparrowConfig>> {
        self.common.config
    }

    /// Runs on the UI thread after `DEBOUNCE` of no edits. Re-reads the
    /// active editor at fire time so file switches are handled correctly.
    pub(crate) fn schedule_refresh(
        &self,
        timer: RwSignal<Option<TimerToken>>,
        main_split: crate::main_split::MainSplitData,
        lsp: DartLspService,
    ) {
        if timer.with_untracked(|t| t.is_some()) {
            return;
        }
        let outline = self.clone();
        let token = exec_after(DEBOUNCE, move |_| {
            timer.set(None);
            let Some(editor) = main_split
                .active_editor
                .try_get_untracked()
                .flatten()
            else {
                return;
            };
            outline.analyze(&editor, &lsp);
        });
        timer.set(Some(token));
    }
}