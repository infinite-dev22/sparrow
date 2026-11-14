//! Closing labels for Dart & Flutter widget trees.
//!
//! Provides inline closing labels (e.g. `// Scaffold`, `// Column`, `// Padding`)
//! at the closing parentheses of multi-line widget instantiations, matching the
//! behavior of Android Studio and the Dart Analysis Server.

use lsp_types::{Position, Range};
use serde::{Deserialize, Serialize};

use super::regex::analyze_dart_file;

/// A single closing label annotation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClosingLabel {
    /// Label text to display (e.g. `"Scaffold"`, `"Column"`, `"Padding"`).
    pub label: String,
    /// Source range of the entity being closed: `start` is the constructor call,
    /// `end` is the closing parenthesis.
    pub range: Range,
}

impl ClosingLabel {
    pub fn new(label: impl Into<String>, range: Range) -> Self {
        Self {
            label: label.into(),
            range,
        }
    }

    /// Format as an inline comment (e.g. `"// Scaffold"`).
    pub fn display_text(&self) -> String {
        format!("// {}", self.label)
    }
}

/// Extract closing labels from raw Dart source code using fast regex & paren matching.
///
/// This provides immediate, real-time closing labels on every keystroke even
/// when the language server is offline or busy.
pub fn extract_closing_labels_from_source(source: &str) -> Vec<ClosingLabel> {
    let analysis = analyze_dart_file(source);
    let mut labels = Vec::new();

    // Line start offsets to convert byte indices to (line, col)
    let line_offsets: Vec<usize> = std::iter::once(0)
        .chain(source.match_indices('\n').map(|(i, _)| i + 1))
        .collect();

    let offset_to_pos = |offset: usize| -> Position {
        let line = match line_offsets.binary_search(&offset) {
            Ok(idx) => idx,
            Err(idx) => idx.saturating_sub(1),
        };
        let line_start = line_offsets.get(line).copied().unwrap_or(0);
        let col = offset.saturating_sub(line_start);
        Position {
            line: line as u32,
            character: col as u32,
        }
    };

    for widget in &analysis.widgets {
        let Some((start_byte, end_byte)) = widget.instantiation_range else {
            continue;
        };

        let start_pos = offset_to_pos(start_byte);
        let end_pos = offset_to_pos(end_byte);

        // Only show closing label if the instantiation spans across lines
        if end_pos.line > start_pos.line {
            labels.push(ClosingLabel {
                label: widget.class_name.clone(),
                range: Range {
                    start: start_pos,
                    end: end_pos,
                },
            });
        }
    }

    labels.sort_by_key(|l| (l.range.end.line, l.range.end.character));
    labels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_multiline_widget_closing_labels() {
        let code = r#"
Widget build(BuildContext context) {
  return Scaffold(
    appBar: AppBar(
      title: Text("Title"),
    ),
    body: Center(
      child: Column(
        children: [
          Text("Hello"),
          Text("World"),
        ],
      ),
    ),
  );
}
"#;
        let labels = extract_closing_labels_from_source(code);
        let names: Vec<&str> = labels.iter().map(|l| l.label.as_str()).collect();

        assert!(names.contains(&"Scaffold"));
        assert!(names.contains(&"AppBar"));
        assert!(names.contains(&"Center"));
        assert!(names.contains(&"Column"));
    }

    #[test]
    fn skips_single_line_widgets() {
        let code = r#"
Widget build(BuildContext context) {
  return SizedBox(child: Text("Single line"));
}
"#;
        let labels = extract_closing_labels_from_source(code);
        assert!(labels.is_empty());
    }

    #[test]
    fn formats_display_text_correctly() {
        let label = ClosingLabel {
            label: "Scaffold".to_string(),
            range: Range::default(),
        };
        assert_eq!(label.display_text(), "// Scaffold");
    }
}
