use std::{fmt::Display, sync::Arc};

use floem::{
    View,
    reactive::{ReadSignal, SignalGet},
    views::{Decorators, container, label},
};

use crate::config::{SparrowConfig, color::SparrowColor};

/// Centers a muted message in the available space.
pub fn empty_state<T: Display + 'static>(
    message: impl Fn() -> T + 'static,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    container(label(message).style(|s| s.selectable(false))).style(move |s| {
        s.size_full()
            .items_center()
            .justify_center()
            .color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
    })
}
