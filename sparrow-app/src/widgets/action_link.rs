use std::{fmt::Display, sync::Arc};

use floem::{
    View,
    reactive::{ReadSignal, SignalGet},
    views::Decorators,
};

use crate::config::{SparrowConfig, color::SparrowColor};

use super::{ButtonKind, button};

/// An inline action with the same hover and keyboard focus as other buttons.
pub(crate) fn action_link<T: Display + 'static>(
    text: impl Fn() -> T + 'static,
    action: impl Fn() + 'static,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    button(text, ButtonKind::Plain, config)
        .on_click_stop(move |_| action())
        .style(move |s| {
            s.max_width_full()
                .padding(0)
                .color(config.get().color(SparrowColor::EDITOR_LINK))
        })
}
