//! Dart-specific tooling.
//!
//! - [`lsp`] — persistent `dart language-server`, source of all diagnostics.
//! - [`regex`] — best-effort widget-tree analyzer used until the LSP's
//!   `documentSymbol` is wired in. Sub-millisecond; not the bottleneck.

pub mod lsp;
pub mod regex;
pub mod widget_lsp;
pub mod hot_reload;

pub use hot_reload::{FlutterHotReloadData, FlutterRunState};
pub use lsp::{DartLspService, Notification as DartNotification};
pub use regex::{analyze_dart_file, ClassInfo, DartAnalysis};

pub(crate) fn is_dart_path(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("dart"))
        .unwrap_or(false)
}