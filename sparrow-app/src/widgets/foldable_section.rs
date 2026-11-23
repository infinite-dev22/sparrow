use std::sync::Arc;

use floem::{
    View,
    reactive::{ReadSignal, RwSignal, SignalGet, SignalUpdate},
    style::CursorStyle,
    taffy::AlignItems,
    views::{Decorators, h_stack, stack},
};

use crate::config::{SparrowConfig, color::SparrowColor, icon::SparrowIcons};

use super::clickable_icon_base;

pub fn foldable_panel_section(
    header: impl View + 'static,
    child: impl View + 'static,
    open: RwSignal<bool>,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    stack((
        h_stack((
            clickable_icon_base(
                move || {
                    if open.get() {
                        SparrowIcons::PANEL_FOLD_DOWN
                    } else {
                        SparrowIcons::PANEL_FOLD_UP
                    }
                },
                None::<Box<dyn Fn()>>,
                || false,
                || false,
                config,
            ),
            header.style(|s| s.align_items(AlignItems::Center).padding_left(3.0)),
        ))
        .style(move |s| {
            s.padding_horiz(10.0)
                .padding_vert(6.0)
                .width_pct(100.0)
                .items_center()
                .cursor(CursorStyle::Pointer)
                .background(config.get().color(SparrowColor::PANEL_BACKGROUND))
        })
        .on_click_stop(move |_| {
            open.update(|open| *open = !*open);
        }),
        child.style(move |s| s.apply_if(!open.get(), |s| s.hide())),
    ))
}
