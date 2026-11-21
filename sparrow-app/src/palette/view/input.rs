//! Command palette query field.

use std::rc::Rc;

use crate::{
    config::color::SparrowColor,
    widgets::text_input::TextInputBuilder,
    window_tab::{Focus, WindowTabData},
};
use floem::{
    View,
    reactive::SignalGet,
    views::{Decorators, container},
};

pub(super) fn palette_input(window_tab_data: Rc<WindowTabData>) -> impl View {
    let editor = window_tab_data.palette.input_editor.clone();
    let config = window_tab_data.common.config;
    let focus = window_tab_data.common.focus;
    let is_focused = move || focus.get() == Focus::Palette;

    let input = TextInputBuilder::new()
        .is_focused(is_focused)
        .build_editor(editor)
        .placeholder(move || window_tab_data.palette.placeholder_text().to_owned())
        .style(|s| s.width_full());

    container(container(input).style(move |s| {
        let config = config.get();
        s.width_full()
            .height(25.0)
            .items_center()
            .border_bottom(1.0)
            .border_color(config.color(SparrowColor::SPARROW_BORDER))
            .background(config.color(SparrowColor::EDITOR_BACKGROUND))
    }))
    .style(|s| s.padding_bottom(5.0))
}

