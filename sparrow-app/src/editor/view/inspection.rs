//! Editor inspection overlay and diagnostic navigation.

use std::rc::Rc;

use floem::{
    View,
    reactive::{RwSignal, SignalGet, SignalWith, create_memo},
    views::{Decorators, dyn_stack, label, stack, svg},
};
use sparrow_core::rope_text_pos::RopeTextPosition;

use crate::editor::EditorData;
use crate::{
    command::InternalCommand,
    config::{color::SparrowColor, icon::SparrowIcons},
    widgets::clickable_icon,
    window_tab::WindowTabData,
};

/// Current-file inspection counts and navigation, independent of the Problems scope.
pub(super) fn inspection_widget(
    window: Rc<WindowTabData>,
    editor: RwSignal<EditorData>,
) -> impl View {
    let config = window.common.config;
    let diagnostics = create_memo(move |_| {
        let doc = editor.get().doc_signal().get();
        doc.diagnostics().diagnostics.get()
    });
    let counts = create_memo(move |_| {
        let mut counts = [0usize; 3];
        for diagnostic in diagnostics.get() {
            let index = match diagnostic.severity {
                Some(lsp_types::DiagnosticSeverity::ERROR) => 0,
                Some(lsp_types::DiagnosticSeverity::WARNING) => 1,
                _ => 2,
            };
            counts[index] += 1;
        }
        counts
    });
    let open_window = window.clone();
    let previous_window = window.clone();
    let next_window = window;
    let navigate = move |window: &WindowTabData, forward: bool| {
        let editor = editor.get_untracked();
        let doc = editor.doc();
        let offset = editor.cursor().with_untracked(|c| c.offset());
        let mut positions: Vec<_> = doc
            .diagnostics()
            .diagnostics
            .get_untracked()
            .iter()
            .map(|d| d.range.start)
            .collect();
        positions.sort();
        positions.dedup();
        let next = if forward {
            positions
                .iter()
                .find(|p| {
                    doc.buffer.with_untracked(|b| b.offset_of_position(p)) > offset
                })
                .or_else(|| positions.first())
        } else {
            positions
                .iter()
                .rev()
                .find(|p| {
                    doc.buffer.with_untracked(|b| b.offset_of_position(p)) < offset
                })
                .or_else(|| positions.last())
        };
        if let (Some(position), Some(path)) =
            (next, doc.content.with_untracked(|c| c.path().cloned()))
        {
            window
                .common
                .internal_command
                .send(InternalCommand::JumpToLocation {
                    location: crate::editor::location::EditorLocation {
                        path,
                        position: Some(
                            crate::editor::location::EditorPosition::Position(
                                *position,
                            ),
                        ),
                        scroll_offset: None,
                        ignore_unconfirmed: false,
                        same_editor_tab: false,
                    },
                });
        }
    };
    stack((
        dyn_stack(
            || 0..3,
            |i| *i,
            move |i| {
                let window = open_window.clone();
                stack((
                    svg(move || {
                        config.get().ui_svg(if i == 0 {
                            SparrowIcons::ERROR
                        } else if i == 1 {
                            SparrowIcons::WARNING
                        } else {
                            SparrowIcons::LIGHTBULB
                        })
                    })
                    .style(move |s| {
                        s.size(12.0, 12.0).margin_right(3.0).color(
                            config.get().color(if i == 0 {
                                SparrowColor::SPARROW_ERROR
                            } else {
                                SparrowColor::SPARROW_WARN
                            }),
                        )
                    }),
                    label(move || counts.get()[i].to_string()),
                ))
                .on_click_stop(move |_| {
                    window.show_panel(crate::panel::kind::PanelKind::Problems)
                })
                .style(move |s| {
                    s.items_center()
                        .margin_right(6.0)
                        .apply_if(counts.get()[i] == 0, |s| s.hide())
                })
            },
        ),
        clickable_icon(
            || SparrowIcons::FILE_PICKER_UP,
            move || navigate(&previous_window, false),
            || false,
            move || diagnostics.with(|d| d.is_empty()),
            || "Previous Problem",
            config,
        ),
        clickable_icon(
            || SparrowIcons::ITEM_OPENED,
            move || navigate(&next_window, true),
            || false,
            move || diagnostics.with(|d| d.is_empty()),
            || "Next Problem",
            config,
        ),
    ))
    .style(move |s| {
        s.absolute()
            .inset_right(12.0)
            .inset_top(0.0)
            .height(24.0)
            .items_center()
            .padding_left(6.0)
            .font_size(11.0)
            .background(config.get().color(SparrowColor::EDITOR_BACKGROUND))
            .apply_if(diagnostics.with(|d| d.is_empty()), |s| s.hide())
    })
}
