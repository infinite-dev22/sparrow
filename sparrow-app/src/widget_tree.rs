//! Reactive state for the widget-tree panel.

use std::{path::PathBuf, rc::Rc};
use std::sync::{Arc, Mutex};
use floem::reactive::{ReadSignal, RwSignal, Scope, SignalGet, SignalUpdate, SignalWith};

use crate::{
    config::SparrowConfig,
    dart::analyze_dart_file,
    editor::EditorData,
    widget_kind::FlutterWidgetInfo,
    window_tab::CommonData,
};
use crate::dart::DartLspService;
use crate::dart::widget_lsp::analyze_with_symbols;

/// One node in the widget tree. Wraps [`FlutterWidgetInfo`] with the UI state
/// the panel needs (expand/collapse, parent-child links).
#[derive(Clone)]
pub struct WidgetNodeData {
    pub info: FlutterWidgetInfo,
    pub open: RwSignal<bool>,
    pub children: Vec<RwSignal<WidgetNodeData>>,
}

impl WidgetNodeData {
    /// Number of *visible* rows in the subtree rooted here (self + open
    /// descendants).
    pub fn visible_count(&self) -> usize {
        let mut count = 1;
        if self.open.get_untracked() {
            for child in &self.children {
                count += child.with_untracked(|c| c.visible_count());
            }
        }
        count
    }
}

#[derive(Clone)]
pub struct WidgetTreeData {
    pub roots: RwSignal<Vec<RwSignal<WidgetNodeData>>>,
    pub source_path: RwSignal<Option<PathBuf>>,
    /// Receives `(path, widgets)` from background analysis.
    pub updates: ReadSignal<Option<(PathBuf, Vec<FlutterWidgetInfo>)>>,
    sender: Arc<Mutex<std::sync::mpsc::Sender<(PathBuf, Vec<FlutterWidgetInfo>)>>>,
    pub common: Rc<CommonData>,
    cx: Scope,
}

impl WidgetTreeData {
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

    /// Request an analysis of `editor`'s current content. LSP-first,
    /// regex fallback.
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
        let source = doc.buffer.with_untracked(|b| b.to_string());
        self.source_path.set(Some(path.clone()));

        if lsp.is_available() {
            let sender = self.sender.clone();
            let path_for_cb = path.clone();
            let source_arc = Arc::new(source);
            lsp.document_symbols(&path, move |symbols| {
                let widgets = analyze_with_symbols(&source_arc, &symbols);
                if let Ok(tx) = sender.lock() {
                    let _ = tx.send((path_for_cb, widgets));
                }
            });
        } else {
            let analysis = analyze_dart_file(&source);
            if let Ok(tx) = self.sender.lock() {
                let _ = tx.send((path, analysis.widgets.into_iter().collect()));
            }
        }
    }

    /// Runs on the UI thread after `DEBOUNCE` of no edits. Matches
    /// `OutlineData::schedule_refresh` — same pattern, same constants.
    pub(crate) fn schedule_refresh(
        &self,
        timer: RwSignal<Option<floem::action::TimerToken>>,
        main_split: crate::main_split::MainSplitData,
        lsp: crate::dart::DartLspService,
    ) {
        use std::time::Duration;
        const DEBOUNCE: Duration = Duration::from_millis(300);

        if timer.with_untracked(|t| t.is_some()) {
            return;
        }
        let tree = self.clone();
        let token = floem::action::exec_after(DEBOUNCE, move |_| {
            timer.set(None);
            let Some(editor) = main_split
                .active_editor
                .try_get_untracked()
                .flatten()
            else {
                return;
            };
            tree.analyze(&editor, &lsp);
        });
        timer.set(Some(token));
    }

    /// Push the results of a completed analysis into the tree signal.
    /// Ignores results whose source no longer matches `source_path`.
    pub fn apply_update(&self, for_path: PathBuf, widgets: Vec<FlutterWidgetInfo>) {
        if self.source_path.with_untracked(|p| p.as_ref() == Some(&for_path)) {
            let roots = build_tree(self.cx, widgets.into_iter().collect());
            self.roots.set(roots);
        }
    }

    /// Drop the current analysis.
    pub fn clear(&self) {
        if self.source_path.with_untracked(|p| p.is_some()) {
            self.roots.set(Vec::new());
            self.source_path.set(None);
        }
    }

    pub fn config(&self) -> floem::reactive::ReadSignal<std::sync::Arc<SparrowConfig>> {
        self.common.config
    }
}

/// Convert a flat, depth-annotated widget list into a hierarchical tree
/// with one `RwSignal` per node (so the panel can toggle expansion).
fn build_tree(
    cx: Scope,
    widgets: im::Vector<FlutterWidgetInfo>,
) -> Vec<RwSignal<WidgetNodeData>> {
    let mut roots: Vec<RwSignal<WidgetNodeData>> = Vec::new();
    let mut stack: Vec<(usize, RwSignal<WidgetNodeData>)> = Vec::new();

    for info in widgets {
        let depth = info.depth;
        // Pop siblings and "uncles" — anything at depth >= our depth.
        while stack.last().map(|(d, _)| *d >= depth).unwrap_or(false) {
            stack.pop();
        }
        let node = cx.create_rw_signal(WidgetNodeData {
            info,
            open: cx.create_rw_signal(true),
            children: Vec::new(),
        });
        match stack.last() {
            Some((_, parent)) => parent.update(|p| p.children.push(node)),
            None => roots.push(node),
        }
        stack.push((depth, node));
    }
    roots
}

fn is_dart_path(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("dart"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use floem::reactive::Scope;

    /// Flatten a tree into `(depth, class_name)` pairs, respecting expansion.
    fn flatten(
        nodes: &[RwSignal<WidgetNodeData>],
        depth: usize,
        out: &mut Vec<(usize, String)>,
    ) {
        for n in nodes {
            let node = n.get_untracked();
            out.push((depth, node.info.class_name.clone()));
            if node.open.get_untracked() {
                flatten(&node.children, depth + 1, out);
            }
        }
    }

    fn dummy(name: &str, depth: usize) -> FlutterWidgetInfo {
        FlutterWidgetInfo {
            class_name: name.into(),
            depth,
            ..Default::default()
        }
    }

    #[test]
    fn builds_simple_tree() {
        let cx = Scope::new();
        let flat = im::vector![
            dummy("Column", 0),
            dummy("Padding", 1),
            dummy("Text", 2),
            dummy("SizedBox", 1),
        ];
        let roots = build_tree(cx, flat);
        assert_eq!(roots.len(), 1);
        let column = roots[0].get_untracked();
        assert_eq!(column.children.len(), 2);
        assert_eq!(column.children[0].get_untracked().children.len(), 1);

        let mut flat_out = Vec::new();
        flatten(&roots, 0, &mut flat_out);
        assert_eq!(
            flat_out,
            vec![
                (0, "Column".into()),
                (1, "Padding".into()),
                (2, "Text".into()),
                (1, "SizedBox".into()),
            ]
        );
    }

    #[test]
    fn builds_multiple_roots() {
        let cx = Scope::new();
        let flat = im::vector![dummy("A", 0), dummy("B", 0), dummy("C", 0)];
        let roots = build_tree(cx, flat);
        assert_eq!(roots.len(), 3);
    }

    #[test]
    fn collapse_hides_children_from_flatten() {
        let cx = Scope::new();
        let flat = im::vector![dummy("Outer", 0), dummy("Inner", 1)];
        let roots = build_tree(cx, flat);
        roots[0].get_untracked().open.set(false);

        let mut flat_out = Vec::new();
        flatten(&roots, 0, &mut flat_out);
        assert_eq!(flat_out, vec![(0, "Outer".into())]);
    }

    #[test]
    fn depth_jump_is_tolerated() {
        // Malformed input (depth skips a level) — should nest under nearest
        // ancestor rather than panic.
        let cx = Scope::new();
        let flat = im::vector![dummy("A", 0), dummy("B", 2)];
        let roots = build_tree(cx, flat);
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].get_untracked().children.len(), 1);
    }
}