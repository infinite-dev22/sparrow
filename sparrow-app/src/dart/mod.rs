pub mod hot_reload;
pub mod lsp;
pub mod regex;
pub mod semantic_tokens;
pub mod test_runner;
pub mod vm_service;
pub mod widget_lsp;

pub use hot_reload::{FlutterHotReloadData, FlutterRunState};
pub use lsp::{DartLspService, Notification as DartNotification};
pub use regex::{analyze_dart_file, ClassInfo, DartAnalysis};
pub use vm_service::{
    FlutterInspectorData, InspectorNode, InspectorState,
    Notification as VmServiceNotification,
};

pub(crate) fn is_dart_path(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("dart"))
        .unwrap_or(false)
}