use std::sync::Arc;

use floem::{
    View,
    reactive::{ReadSignal, RwSignal, SignalGet, SignalUpdate},
    views::{Decorators, stack_from_iter},
};

use crate::config::{SparrowConfig, icon::SparrowIcons};

use super::modal_focus::ModalFocus;

pub(crate) fn search_match_options(
    focus: &ModalFocus,
    case_sensitive: RwSignal<bool>,
    whole_words: RwSignal<bool>,
    regex: RwSignal<bool>,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View + use<> {
    stack_from_iter(
        [
            (
                case_sensitive,
                SparrowIcons::SEARCH_CASE_SENSITIVE,
                "Match case",
            ),
            (whole_words, SparrowIcons::SEARCH_WHOLE_WORD, "Whole words"),
            (regex, SparrowIcons::SEARCH_REGEX, "Regular expression"),
        ]
        .map(|(signal, icon, tip)| {
            focus.icon(
                move || icon,
                move || signal.update(|on| *on = !*on),
                move || signal.get(),
                || false,
                move || tip,
                config,
            )
        }),
    )
    .style(|s| s.items_center().gap(4))
}
