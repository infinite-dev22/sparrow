use std::sync::Arc;

use floem::{
    View,
    reactive::{ReadSignal, RwSignal, SignalGet, SignalUpdate},
    style::{CursorStyle, Foreground},
    views::{Decorators, label, stack, toggle_button},
};

use crate::config::{SparrowConfig, color::SparrowColor};

/// A labeled switch with an optional description. The label can also toggle it.
pub fn toggle_row(
    title: &'static str,
    description: Option<&'static str>,
    checked: RwSignal<bool>,
    on_change: impl Fn(bool) + Clone + 'static,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    let change = move |value| {
        checked.set(value);
        on_change(value);
    };
    let change_from_label = change.clone();

    stack((
        stack((
            label(move || title).style(|s| s.font_size(12.0).selectable(false)),
            label(move || description.unwrap_or_default()).style(move |s| {
                s.font_size(10.0)
                    .color(config.get().color(SparrowColor::EDITOR_DIM))
                    .selectable(false)
                    .apply_if(description.is_none(), |s| s.hide())
            }),
        ))
        .style(|s| s.flex_col().flex_grow(1.0_f32).min_width(0.0))
        .on_click_stop(move |_| change_from_label(!checked.get_untracked())),
        toggle_button(move || checked.get())
            .on_toggle(change)
            .style(move |s| {
                let config = config.get();
                let track = if checked.get() {
                    SparrowColor::SPARROW_BUTTON_PRIMARY_BACKGROUND
                } else {
                    SparrowColor::PANEL_HOVERED_ACTIVE_BACKGROUND
                };
                s.size(32.0, 16.0)
                    .flex_shrink(0.0_f32)
                    .border_radius(8.0)
                    .border(1.0)
                    .border_color(config.color(SparrowColor::SPARROW_BORDER))
                    .background(config.color(track))
                    .set(
                        Foreground,
                        Some(config.color(SparrowColor::PANEL_FOREGROUND).into()),
                    )
                    .focus_visible(|s| {
                        s.border_color(config.color(SparrowColor::EDITOR_CARET))
                    })
            }),
    ))
    .style(move |s| {
        s.items_center()
            .gap(8.0)
            .padding_horiz(12.0)
            .padding_vert(8.0)
            .cursor(CursorStyle::Pointer)
            .hover(|s| {
                s.background(
                    config.get().color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                )
            })
    })
}
