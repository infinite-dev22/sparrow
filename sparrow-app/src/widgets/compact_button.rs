use std::{fmt::Display, sync::Arc};

use floem::{View, reactive::ReadSignal, views::Decorators};

use crate::config::SparrowConfig;

use super::{ButtonKind, button};

pub(crate) fn compact_button<T: Display + 'static>(
    text: impl Fn() -> T + 'static,
    kind: ButtonKind,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    button(text, kind, config).style(|s| s.min_height(24.0).padding_horiz(8.0))
}
