//! Flutter-specific quick fix assists (`Alt+Enter`).
//!
//! Provides context-aware Flutter assists (Wrap with Widget, Wrap with Padding,
//! Wrap with Center, Wrap with Column, Wrap with Row, Wrap with Container,
//! Wrap with SizedBox, Wrap with Builder, Remove this widget) matching Android Studio.

use std::{collections::HashMap, path::Path};

use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, Position, Range, TextEdit,
    Url, WorkspaceEdit,
};

use super::regex::analyze_dart_file;

/// Represents the target widget under cursor to be assisted.
#[derive(Debug, Clone)]
pub struct WidgetAssistTarget {
    pub name: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_pos: Position,
    pub end_pos: Position,
    pub indentation: String,
    pub full_snippet: String,
}

/// Find the deepest widget containing the given position (line, col) or byte offset.
pub fn find_widget_at_position(
    source: &str,
    line: usize,
    col: usize,
) -> Option<WidgetAssistTarget> {
    let analysis = analyze_dart_file(source);

    let line_offsets: Vec<usize> = std::iter::once(0)
        .chain(source.match_indices('\n').map(|(i, _)| i + 1))
        .collect();

    let offset_to_pos = |offset: usize| -> Position {
        let l = match line_offsets.binary_search(&offset) {
            Ok(idx) => idx,
            Err(idx) => idx.saturating_sub(1),
        };
        let l_start = line_offsets.get(l).copied().unwrap_or(0);
        let c = offset.saturating_sub(l_start);
        Position {
            line: l as u32,
            character: c as u32,
        }
    };

    let target_offset = line_offsets.get(line).copied().unwrap_or(0) + col;

    // Filter widgets whose instantiation range contains target_offset
    let mut matching: Vec<_> = analysis
        .widgets
        .iter()
        .filter_map(|w| {
            let (s, e) = w.instantiation_range?;
            if target_offset >= s && target_offset <= e {
                Some((w, s, e, e - s))
            } else {
                None
            }
        })
        .collect();

    // Pick the most specific (smallest span) containing widget
    matching.sort_by_key(|(_, _, _, len)| *len);

    let (widget, start_byte, end_byte, _) = matching.first()?;
    let start_pos = offset_to_pos(*start_byte);
    let end_pos = offset_to_pos(*end_byte);

    let line_start = line_offsets.get(start_pos.line as usize).copied().unwrap_or(0);
    let prefix = &source[line_start..*start_byte];
    let indentation: String = prefix.chars().take_while(|c| c.is_whitespace()).collect();
    let full_snippet = source[*start_byte..*end_byte].to_string();

    Some(WidgetAssistTarget {
        name: widget.class_name.clone(),
        start_byte: *start_byte,
        end_byte: *end_byte,
        start_pos,
        end_pos,
        indentation,
        full_snippet,
    })
}

/// Indent a multi-line string by `indent` spaces/characters.
fn indent_lines(text: &str, indent: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return String::new();
    }
    let mut result = String::new();
    for (i, line) in lines.iter().enumerate() {
        if i == 0 {
            result.push_str(line);
        } else {
            result.push('\n');
            if !line.trim().is_empty() {
                result.push_str(indent);
                result.push_str(line);
            }
        }
    }
    result
}

/// Generate Flutter quick-fix code actions for the given file and position.
pub fn generate_flutter_code_actions(
    file_path: &Path,
    source: &str,
    line: usize,
    col: usize,
) -> Vec<CodeActionOrCommand> {
    let Some(target) = find_widget_at_position(source, line, col) else {
        return Vec::new();
    };

    let Ok(uri) = Url::from_file_path(file_path) else {
        return Vec::new();
    };

    let base_range = Range {
        start: target.start_pos,
        end: target.end_pos,
    };

    let make_assist = |title: &str, new_text: String| -> CodeActionOrCommand {
        let text_edit = TextEdit {
            range: base_range,
            new_text,
        };
        let mut changes = HashMap::new();
        changes.insert(uri.clone(), vec![text_edit]);

        CodeActionOrCommand::CodeAction(CodeAction {
            title: title.to_string(),
            kind: Some(CodeActionKind::REFACTOR_REWRITE),
            diagnostics: None,
            edit: Some(WorkspaceEdit {
                changes: Some(changes),
                document_changes: None,
                change_annotations: None,
            }),
            command: None,
            is_preferred: Some(true),
            disabled: None,
            data: None,
        })
    };

    let mut actions = Vec::new();
    let child_indent = format!("{}  ", target.indentation);
    let indented_snippet = indent_lines(&target.full_snippet, "  ");

    // 1. Wrap with Widget...
    let wrap_widget = format!(
        "widget(\n{}child: {},\n{})",
        child_indent, indented_snippet, target.indentation
    );
    actions.push(make_assist("Wrap with widget...", wrap_widget));

    // 2. Wrap with Padding
    let wrap_padding = format!(
        "Padding(\n{}padding: const EdgeInsets.all(8.0),\n{}child: {},\n{})",
        child_indent, child_indent, indented_snippet, target.indentation
    );
    actions.push(make_assist("Wrap with Padding", wrap_padding));

    // 3. Wrap with Center
    let wrap_center = format!(
        "Center(\n{}child: {},\n{})",
        child_indent, indented_snippet, target.indentation
    );
    actions.push(make_assist("Wrap with Center", wrap_center));

    // 4. Wrap with Column
    let wrap_column = format!(
        "Column(\n{}children: [\n{}  {},\n{}],\n{})",
        child_indent, child_indent, indented_snippet, child_indent, target.indentation
    );
    actions.push(make_assist("Wrap with Column", wrap_column));

    // 5. Wrap with Row
    let wrap_row = format!(
        "Row(\n{}children: [\n{}  {},\n{}],\n{})",
        child_indent, child_indent, indented_snippet, child_indent, target.indentation
    );
    actions.push(make_assist("Wrap with Row", wrap_row));

    // 6. Wrap with Container
    let wrap_container = format!(
        "Container(\n{}child: {},\n{})",
        child_indent, indented_snippet, target.indentation
    );
    actions.push(make_assist("Wrap with Container", wrap_container));

    // 7. Wrap with SizedBox
    let wrap_sized_box = format!(
        "SizedBox(\n{}child: {},\n{})",
        child_indent, indented_snippet, target.indentation
    );
    actions.push(make_assist("Wrap with SizedBox", wrap_sized_box));

    // 8. Wrap with Builder
    let wrap_builder = format!(
        "Builder(\n{}builder: (context) {{\n{}  return {};\n{}}},\n{})",
        child_indent, child_indent, indented_snippet, child_indent, target.indentation
    );
    actions.push(make_assist("Wrap with Builder", wrap_builder));

    // 9. Remove this widget
    // If the widget has `child: X`, extract X; otherwise empty SizedBox / comment
    let remove_text = extract_child_widget(&target.full_snippet)
        .unwrap_or_else(|| "const SizedBox()".to_string());
    actions.push(make_assist(
        &format!("Remove this widget ({})", target.name),
        remove_text,
    ));

    actions
}

/// Helper to extract the `child: <Widget>` body from a widget instantiation snippet.
fn extract_child_widget(snippet: &str) -> Option<String> {
    let child_idx = snippet.find("child:")?;
    let after = &snippet[child_idx + "child:".len()..];
    let trimmed = after.trim_start();
    // Find either comma or end parenthesis
    let end_idx = trimmed.rfind(')').unwrap_or(trimmed.len());
    let candidate = trimmed[..end_idx].trim().trim_end_matches(',').trim();
    if candidate.is_empty() {
        None
    } else {
        Some(candidate.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_assists_for_widget() {
        let code = r#"
Widget build(BuildContext context) {
  return Text("Hello");
}
"#;
        let path = Path::new("/workspace/lib/main.dart");
        let actions = generate_flutter_code_actions(path, code, 2, 10);
        assert!(!actions.is_empty());

        let titles: Vec<&str> = actions
            .iter()
            .map(|a| match a {
                CodeActionOrCommand::CodeAction(c) => c.title.as_str(),
                CodeActionOrCommand::Command(c) => c.title.as_str(),
            })
            .collect();

        assert!(titles.contains(&"Wrap with widget..."));
        assert!(titles.contains(&"Wrap with Padding"));
        assert!(titles.contains(&"Wrap with Center"));
        assert!(titles.contains(&"Wrap with Column"));
        assert!(titles.contains(&"Wrap with Row"));
        assert!(titles.contains(&"Wrap with Container"));
    }

    #[test]
    fn extracts_child_widget_on_remove() {
        let code = r#"
Widget build(BuildContext context) {
  return Center(
    child: Text("Hello"),
  );
}
"#;
        let path = Path::new("/workspace/lib/main.dart");
        let actions = generate_flutter_code_actions(path, code, 2, 10);
        let remove = actions.iter().find(|a| match a {
            CodeActionOrCommand::CodeAction(c) => c.title.starts_with("Remove this widget"),
            _ => false,
        });
        assert!(remove.is_some());
        if let Some(CodeActionOrCommand::CodeAction(c)) = remove {
            let edit = c.edit.as_ref().unwrap();
            let changes = edit.changes.as_ref().unwrap();
            let edits = changes.values().next().unwrap();
            assert_eq!(edits[0].new_text, "Text(\"Hello\")");
        }
    }

    #[test]
    fn wraps_with_builder_and_sized_box() {
        let code = "Widget build(c) => Container();";
        let path = Path::new("/workspace/lib/main.dart");
        let actions = generate_flutter_code_actions(path, code, 0, 20);
        let titles: Vec<&str> = actions
            .iter()
            .map(|a| match a {
                CodeActionOrCommand::CodeAction(c) => c.title.as_str(),
                CodeActionOrCommand::Command(c) => c.title.as_str(),
            })
            .collect();
        assert!(titles.contains(&"Wrap with SizedBox"));
        assert!(titles.contains(&"Wrap with Builder"));
    }
}
