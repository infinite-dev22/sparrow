//! Live Markdown document preview sharing the editor's unsaved buffer.
use crate::editor::EditorData;
use floem::reactive::{
        SignalWith,
    };
use std::path::Path;
mod document;
mod toolbar;

pub use document::preview;
pub use toolbar::toolbar;

pub fn is_markdown(path: &Path) -> bool {
    path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
        matches!(s.to_lowercase().as_str(), "md" | "markdown" | "mdown")
    })
}
pub fn mode(editor: &EditorData) -> String {
    let doc = editor.doc_signal().get();
    doc.content
        .with(|content| {
            content.path().and_then(|path| {
                editor
                    .common
                    .document_views
                    .with(|views| views.get(path).cloned())
            })
        })
        .unwrap_or_else(|| "source".into())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn markdown_extensions_are_case_insensitive() {
        for file in ["README.md", "guide.markdown", "doc.MD"] {
            assert!(is_markdown(Path::new(file)));
        }
        assert!(!is_markdown(Path::new("main.dart")));
    }
}
