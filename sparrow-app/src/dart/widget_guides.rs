//! Flutter widget tree guideline extractor and layout geometry.
//!
//! Provides hierarchical indentation guides linking widget constructor calls
//! to their corresponding closing brackets and child widgets, matching the
//! visual guide lines in Android Studio.

use super::regex::analyze_dart_file;

/// Represents a vertical widget guideline connecting a parent widget constructor
/// to its closing bracket / children.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WidgetGuide {
    /// 0-indexed line number where the widget starts.
    pub start_line: usize,
    /// 0-indexed column where the widget constructor begins.
    pub start_col: usize,
    /// 0-indexed line number where the widget's closing parenthesis is placed.
    pub end_line: usize,
    /// 0-indexed column of the closing parenthesis.
    pub end_col: usize,
    /// Name of the widget (e.g. `"Scaffold"`, `"Column"`, `"Padding"`).
    pub widget_name: String,
    /// Nesting depth of this widget in the hierarchy.
    pub depth: usize,
}

/// Extract widget guidelines from Dart source text.
pub fn extract_widget_guides(source: &str) -> Vec<WidgetGuide> {
    let analysis = analyze_dart_file(source);
    let mut guides = Vec::new();

    let line_offsets: Vec<usize> = std::iter::once(0)
        .chain(source.match_indices('\n').map(|(i, _)| i + 1))
        .collect();

    let offset_to_line_col = |offset: usize| -> (usize, usize) {
        let line = match line_offsets.binary_search(&offset) {
            Ok(idx) => idx,
            Err(idx) => idx.saturating_sub(1),
        };
        let line_start = line_offsets.get(line).copied().unwrap_or(0);
        let col = offset.saturating_sub(line_start);
        (line, col)
    };

    for widget in &analysis.widgets {
        let Some((start_byte, end_byte)) = widget.instantiation_range else {
            continue;
        };

        let (start_line, start_col) = offset_to_line_col(start_byte);
        let (end_line, end_col) = offset_to_line_col(end_byte);

        // Only draw guide lines for widgets spanning multiple lines
        if end_line > start_line {
            guides.push(WidgetGuide {
                start_line,
                start_col,
                end_line,
                end_col,
                widget_name: widget.class_name.clone(),
                depth: widget.depth,
            });
        }
    }

    guides.sort_by_key(|g| (g.start_line, g.start_col));
    guides
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_widget_guides_for_nested_tree() {
        let code = r#"
Widget build(BuildContext context) {
  return Scaffold(
    body: Center(
      child: Column(
        children: [
          Text("A"),
          Text("B"),
        ],
      ),
    ),
  );
}
"#;
        let guides = extract_widget_guides(code);
        assert_eq!(guides.len(), 3);
        assert_eq!(guides[0].widget_name, "Scaffold");
        assert_eq!(guides[1].widget_name, "Center");
        assert_eq!(guides[2].widget_name, "Column");

        assert!(guides[0].end_line > guides[0].start_line);
        assert!(guides[1].depth > guides[0].depth);
        assert!(guides[2].depth > guides[1].depth);
    }
}
