use floem::{
    View,
    reactive::{ReadSignal, SignalGet},
    views::{Decorators, svg},
};
use std::sync::Arc;

use crate::config::{SparrowConfig, color::SparrowColor, icon::SparrowIcons};

pub(crate) fn tree_chevron(
    config: ReadSignal<Arc<SparrowConfig>>,
    expanded: impl Fn() -> bool + 'static,
    has_children: impl Fn() -> bool + 'static,
) -> impl View {
    svg(move || {
        config.get().ui_svg(if expanded() {
            SparrowIcons::ITEM_OPENED
        } else {
            SparrowIcons::ITEM_CLOSED
        })
    })
    .style(move |s| {
        s.size(12, 12)
            .flex_shrink(0.0_f32)
            .color(if has_children() {
                config.get().color(SparrowColor::SPARROW_ICON_ACTIVE)
            } else {
                floem::peniko::Color::TRANSPARENT
            })
    })
}
