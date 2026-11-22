use std::{fmt::Display, sync::Arc};

use floem::{
    IntoView, View,
    kurbo::Rect,
    reactive::{ReadSignal, SignalGet},
    style::CursorStyle,
    views::{Decorators, label, stack},
};

use crate::config::{SparrowConfig, color::SparrowColor};

const ROW_HEIGHT: f64 = 26.0;

pub(crate) fn search_result_row<T: Display + 'static>(
    content: impl IntoView + 'static,
    hint: impl Fn() -> T + 'static,
    hint_width_pct: f64,
    is_selected: impl Fn() -> bool + 'static,
    select: impl Fn() + Clone + 'static,
    open: impl Fn() + 'static,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    let click = select.clone();
    stack((
        content,
        label(hint).style(move |s| {
            s.max_width_pct(hint_width_pct)
                .text_ellipsis()
                .color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
        }),
    ))
    .on_click_stop(move |_| click())
    .on_double_click_stop(move |_| {
        select();
        open();
    })
    .style(move |s| {
        let config = config.get();
        let selected = is_selected();
        let selection = config.color(SparrowColor::PANEL_SELECTION_BACKGROUND);
        let hover = config.color(SparrowColor::PANEL_HOVERED_BACKGROUND);
        s.height(ROW_HEIGHT)
            .width_full()
            .padding_horiz(8)
            .items_center()
            .gap(8)
            .border_radius(4)
            .cursor(CursorStyle::Pointer)
            .apply_if(selected, |s| s.background(selection))
            .hover(move |s| s.background(if selected { selection } else { hover }))
    })
}

pub(crate) fn search_result_bounds(index: usize) -> Rect {
    let top = index as f64 * ROW_HEIGHT;
    Rect::new(0.0, top, 1.0, top + ROW_HEIGHT)
}
