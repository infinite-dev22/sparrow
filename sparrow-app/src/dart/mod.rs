pub mod analyzer;
pub mod assists;
pub mod bootstrap;
pub mod closing_labels;
pub mod daemon;
pub mod hot_reload;
pub mod lsp;
pub mod regex;
pub mod sdk;
pub mod semantic_tokens;
pub mod signature_help;
pub mod swatches;
pub mod test_runner;
pub mod test_service;
pub mod vm_service;
pub mod widget_guides;
pub mod widget_lsp;

pub use assists::{generate_flutter_code_actions, WidgetAssistTarget};
pub use bootstrap::{
    bootstrap_workspace, classify_project, is_dart_or_flutter_project,
    is_flutter_project, is_package_resolution_needed, run_pub_get, ProjectType,
};
pub use closing_labels::{extract_closing_labels_from_source, ClosingLabel};
pub use daemon::{DaemonCommand, DaemonNotification, FlutterDaemonService};
pub use hot_reload::{FlutterHotReloadData, FlutterRunState};
pub use lsp::{DartLspService, Notification as DartNotification};
pub use regex::{analyze_dart_file, ClassInfo, DartAnalysis};
pub use swatches::{extract_swatches, ColorSwatch, IconSwatch};
pub use widget_guides::{extract_widget_guides, WidgetGuide};
pub use sdk::{
    find_dart_binary, find_dart_sdk, find_flutter_binary, find_flutter_sdk,
    is_dart_available, is_flutter_available, DartSdk, FlutterSdk,
};
pub use signature_help::{SignatureHelpData, SignatureStatus};
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

pub mod assets;
pub mod project_wizard;

pub mod devtools;
