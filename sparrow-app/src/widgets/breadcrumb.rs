use std::sync::Arc;

use floem::{
    View,
    reactive::{ReadSignal, SignalGet},
    views::{Decorators, stack, svg},
};

use crate::config::{SparrowConfig, color::SparrowColor, icon::SparrowIcons};

/// Places a themed separator before each breadcrumb after the first.
pub fn breadcrumb_item(
    first: bool,
    content: impl View + 'static,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    stack((
        svg(move || config.get().ui_svg(SparrowIcons::BREADCRUMB_SEPARATOR)).style(
            move |s| {
                s.size(12.0, 12.0)
                    .flex_shrink(0.0_f32)
                    .apply_if(first, |s| s.hide())
                    .color(config.get().color(SparrowColor::SPARROW_ICON_INACTIVE))
            },
        ),
        content,
    ))
    .style(|s| s.items_center())
}
