//! Resource category tabs and horizontal scrolling.

use crate::config::{SparrowConfig, color::SparrowColor};
use crate::panel::resource_manager::Kind;
use floem::{
    View,
    reactive::{ReadSignal, RwSignal, SignalGet, SignalUpdate},
    views::{Decorators, dyn_stack, label},
};
use std::sync::Arc;

pub(super) fn resource_categories(
    kind: RwSignal<Kind>,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    crate::widgets::tab_strip(|strip| {
        dyn_stack(
            || {
                vec![
                    Kind::Asset,
                    Kind::Drawable,
                    Kind::Color,
                    Kind::Layout,
                    Kind::MipMap,
                    Kind::String,
                    Kind::Font,
                ]
            },
            |k| *k,
            move |tab| {
                strip
                    .button(
                        label(move || tab.title()),
                        move || kind.get() == tab,
                        move || kind.set(tab),
                        config,
                    )
                    .style(|s| s.padding_horiz(9.0))
            },
        )
        .style(|s| s.items_center())
    })
    .style(move |s| {
        s.width_full()
            .height(30.0)
            .border_bottom(1.0)
            .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
    })
}
