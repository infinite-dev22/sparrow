//! Inline file and directory naming editor.

use std::path::Path;

use floem::{
    View,
    event::EventListener,
    peniko::Color,
    reactive::{SignalGet, SignalUpdate, SignalWith},
    style::Position,
    views::{Container, Decorators, container, label, stack},
};
use lapce_xi_rope::Rope;
use sparrow_core::selection::Selection;

use crate::file_explorer::data::FileExplorerData;
use crate::{
    config::color::SparrowColor, panel::kind::PanelKind,
    widgets::text_input::TextInputBuilder, window_tab::Focus,
};

/// Blends `foreground` with `background`.
///
/// Uses the alpha channel from `foreground` - if `foreground` is opaque, `foreground` will be
/// returned unchanged.
///
/// The result is always opaque regardless of the transparency of the inputs.
fn blend_colors(background: Color, foreground: Color) -> Color {
    let background = background.to_rgba8();
    let foreground = foreground.to_rgba8();
    let a: u16 = foreground.a.into();
    let [r, g, b] = [
        [background.r, foreground.r],
        [background.g, foreground.g],
        [background.b, foreground.b],
    ]
    .map(|x| x.map(u16::from))
    .map(|[b, f]| (a * f + (255 - a) * b) / 255)
    .map(|x| x as u8);

    Color::from_rgba8(r, g, b, 255)
}

/// Initialize the file explorer's naming (renaming, creating, etc.) editor with the given path.
pub(super) fn initialize_naming_editor_with_path(data: &FileExplorerData, path: &Path) {
    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
    // Start with the part of the file or directory name before the extension
    // selected.
    let selection_end = {
        let without_leading_dot = file_name.strip_prefix('.').unwrap_or(&file_name);
        let idx = without_leading_dot
            .find('.')
            .unwrap_or(without_leading_dot.len());

        idx + file_name.len() - without_leading_dot.len()
    };

    initialize_naming_editor(data, &file_name, Some(selection_end));
}

pub(super) fn initialize_naming_editor(
    data: &FileExplorerData,
    text: &str,
    selection_end: Option<usize>,
) {
    let text = Rope::from(text);
    let selection_end = selection_end.unwrap_or(text.len());

    let doc = data.naming_editor_data.doc();
    doc.reload(text, true);
    data.naming_editor_data
        .cursor()
        .update(|cursor| cursor.set_insert(Selection::region(0, selection_end)));

    data.naming
        .update(|naming| naming.set_editor_needs_reset(false));
}

/// Input used for naming a file/directory
pub(super) fn file_node_input_view(data: FileExplorerData, err: Option<String>) -> Container {
    let ui_line_height = data.common.ui_line_height;

    let naming_editor_data = data.naming_editor_data.clone();
    let text_input_file_explorer_data = data.clone();
    let focus = data.common.focus;
    let config = data.common.config;

    let is_focused = move || {
        focus.with_untracked(|focus| focus == &Focus::Panel(PanelKind::FileExplorer))
    };
    let text_input_view = TextInputBuilder::new()
        .is_focused(is_focused)
        .key_focus(text_input_file_explorer_data)
        .build_editor(naming_editor_data.clone())
        .on_event_stop(EventListener::FocusLost, move |_| {
            data.finish_naming();
        })
        .style(move |s| {
            s.width_full()
                .height(ui_line_height.get())
                .padding(0.0)
                .margin(0.0)
                .border_radius(6.0)
                .border(1.0)
                .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
        });

    let text_input_id = text_input_view.id();
    text_input_id.request_focus();

    if let Some(err) = err {
        container(
            stack((
                text_input_view,
                label(move || err.clone()).style(move |s| {
                    let config = config.get();

                    let editor_background_color =
                        config.color(SparrowColor::PANEL_CURRENT_BACKGROUND);
                    let error_background_color =
                        config.color(SparrowColor::ERROR_LENS_ERROR_BACKGROUND);

                    let background_color = blend_colors(
                        editor_background_color,
                        error_background_color,
                    );

                    s.position(Position::Absolute)
                        .inset_top(ui_line_height.get())
                        .width_full()
                        .color(
                            config.color(SparrowColor::ERROR_LENS_ERROR_FOREGROUND),
                        )
                        .background(background_color)
                        .z_index(100)
                }),
            ))
            .style(|s| s.flex_grow(1.0f32)),
        )
    } else {
        container(text_input_view)
    }
    .style(move |s| s.width_full())
}
