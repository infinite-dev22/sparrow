//! Command palette preview surface.

use std::rc::Rc;

use crate::{
    config::color::SparrowColor, editor::view::editor_container_view,
    window_tab::WindowTabData,
};
use floem::{
    View,
    reactive::{SignalGet, create_rw_signal},
    style::{Display, Position},
    views::{Decorators, container},
};

pub(super) fn palette_preview(window_tab_data: Rc<WindowTabData>) -> impl View {
    let palette_data = window_tab_data.palette.clone();
    let workspace = palette_data.workspace.clone();
    let preview_editor = palette_data.preview_editor;
    let has_preview = palette_data.has_preview;
    let config = palette_data.common.config;
    let preview_editor = create_rw_signal(preview_editor);
    container(
        container(editor_container_view(
            window_tab_data,
            workspace,
            |_tracked: bool| true,
            preview_editor,
        ))
        .style(move |s| {
            let config = config.get();
            s.position(Position::Absolute)
                .border_top(1.0)
                .border_color(config.color(SparrowColor::SPARROW_BORDER))
                .size_full()
                .background(config.color(SparrowColor::EDITOR_BACKGROUND))
        }),
    )
    .style(move |s| {
        s.display(if has_preview.get() {
            Display::Flex
        } else {
            Display::None
        })
        .flex_grow(1.0f32)
    })
}
