use floem::{
    View,
    reactive::{RwSignal, SignalGet, SignalWith},
    style::CursorStyle,
    views::{Decorators, dyn_stack, empty},
};
use lsp_types::DiagnosticSeverity;
use sparrow_core::buffer::rope_text::RopeText;

use crate::{
    config::color::SparrowColor,
    editor::{EditorData, location::EditorPosition},
    widgets::tooltip::{tooltip, tooltip_tip},
};

use super::diff_markers::{go_to_line, hunks};

#[derive(PartialEq, Eq, Hash)]
struct Marker {
    line: usize,
    end: usize,
    column: u32,
    color: &'static str,
    text: String,
    problem: bool,
}

pub(super) fn overview(editor: RwSignal<EditorData>) -> impl View {
    let config = editor.get_untracked().common.config;
    dyn_stack(
        move || {
            let editor = editor.get();
            if !editor.kind.get().is_normal()
                || editor.doc().content.with(|c| c.path().is_none())
            {
                return Vec::new();
            }
            let doc = editor.doc_signal().get();
            let mut markers: Vec<_> = doc.head_changes().with(|changes| {
                hunks(&doc, changes)
                    .into_iter()
                    .map(|hunk| Marker {
                        line: hunk.lines.start,
                        end: hunk.lines.end,
                        column: 0,
                        color: hunk.color,
                        text: hunk.description.into(),
                        problem: false,
                    })
                    .collect()
            });
            doc.diagnostics.diagnostics.with(|diagnostics| {
                markers.extend(diagnostics.iter().filter_map(|diagnostic| {
                    let color = match diagnostic.severity {
                        Some(DiagnosticSeverity::ERROR) => {
                            SparrowColor::SPARROW_ERROR
                        }
                        Some(DiagnosticSeverity::WARNING) => {
                            SparrowColor::SPARROW_WARN
                        }
                        _ => return None,
                    };
                    Some(Marker {
                        line: diagnostic.range.start.line as usize,
                        end: diagnostic.range.start.line as usize,
                        column: diagnostic.range.start.character,
                        color,
                        text: diagnostic.message.clone(),
                        problem: true,
                    })
                }));
            });
            let mut seen = std::collections::HashSet::new();
            markers.retain(|marker| {
                seen.insert((
                    marker.line,
                    marker.end,
                    marker.column,
                    marker.color,
                    marker.text.clone(),
                ))
            });
            markers
        },
        |marker| {
            (
                marker.line,
                marker.end,
                marker.column,
                marker.color,
                marker.text.clone(),
            )
        },
        move |marker| {
            let line = marker.line;
            let color = marker.color;
            let problem = marker.problem;
            let column = marker.column;
            let end = marker.end;
            let description = marker.text;
            tooltip(
                empty().style(|s| s.size_full()).on_click_stop(move |_| {
                    let editor = editor.get_untracked();
                    if problem {
                        editor.go_to_position(
                            EditorPosition::Position(lsp_types::Position::new(
                                line as u32,
                                column,
                            )),
                            None,
                            None,
                        );
                        if let Some(view) =
                            editor.editor.editor_view_id.get_untracked()
                        {
                            view.request_focus();
                        }
                    } else {
                        go_to_line(&editor, line);
                    }
                }),
                move || {
                    tooltip_tip(
                        config,
                        floem::views::label({
                            let description = description.clone();
                            move || format!("Line {} · {}", line + 1, description)
                        }),
                    )
                },
            )
            .style(move |s| {
                let editor = editor.get();
                let viewport = editor.editor.viewport.get();
                let total =
                    editor.doc_signal().get().buffer.with(|b| b.last_line() + 1);
                let ratio = line.min(total - 1) as f64 / total as f64;
                let height = ((end.saturating_sub(line) as f64 / total as f64)
                    * viewport.height())
                .max(4.0);
                s.absolute()
                    .inset_top(
                        (ratio * viewport.height())
                            .min((viewport.height() - height).max(0.0)),
                    )
                    .inset_left(if problem { 4.0 } else { 0.0 })
                    .width(if problem { 6.0 } else { 4.0 })
                    .height(height)
                    .border_radius(1.0)
                    .background(config.get().color(color))
                    .cursor(CursorStyle::Pointer)
            })
        },
    )
    .style(|s| {
        s.absolute()
            .inset_right(0.0)
            .inset_top(0.0)
            .width(10.0)
            .height_full()
    })
}
