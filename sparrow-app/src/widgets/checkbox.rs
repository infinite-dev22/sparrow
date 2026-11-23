use std::{fmt::Display, rc::Rc, sync::Arc};

use floem::{
    View,
    reactive::{ReadSignal, SignalGet},
    style::CursorStyle,
    views::{
        Checkbox, CheckboxClass, Decorators, LabeledCheckboxClass, ValueContainer,
        container, empty, stack,
    },
};

use crate::config::{SparrowConfig, color::SparrowColor};

/// The aggregate state of a checkbox representing several items.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CheckboxState {
    Unchecked,
    Mixed,
    Checked,
}

impl CheckboxState {
    pub(crate) fn from_selection(total: usize, selected: usize) -> Self {
        if selected == 0 {
            Self::Unchecked
        } else if selected == total {
            Self::Checked
        } else {
            Self::Mixed
        }
    }
}

fn themed(
    checkbox: ValueContainer<bool>,
    filled: impl Fn() -> bool + 'static,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> ValueContainer<bool> {
    checkbox.style(move |s| {
        let config = config.get();
        let size = config.ui.font_size() as f32;
        let filled = filled();
        let accent = config.color(SparrowColor::SPARROW_BUTTON_PRIMARY_BACKGROUND);
        let foreground =
            config.color(SparrowColor::SPARROW_BUTTON_PRIMARY_FOREGROUND);
        let border = if filled {
            accent
        } else {
            config.color(SparrowColor::EDITOR_DIM)
        };

        s.class(CheckboxClass, |s| {
            s.min_width(size)
                .size(size, size)
                .flex_shrink(0.0_f32)
                .color(foreground)
                .border_color(border)
                .apply_if(filled, |s| s.background(accent))
                .border(1.0)
                .border_radius(2.0)
                .hover(|s| s.cursor(CursorStyle::Pointer))
        })
    })
}

/// A themed Floem checkbox. Use `on_update` to store the new checked value.
pub fn checkbox(
    checked: impl Fn() -> bool + 'static,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> ValueContainer<bool> {
    let checked = Rc::new(checked);
    let style_checked = checked.clone();
    themed(
        Checkbox::new(move || checked()),
        move || style_checked(),
        config,
    )
}

/// A themed labeled checkbox that can also be registered with ModalFocus.
/// Floem handles clicks on the label and checkbox. The wrapper handles
/// keyboard activation when the dialog focuses the row.
pub fn labeled_checkbox<T: Display + 'static>(
    checked: impl Fn() -> bool + Clone + 'static,
    label: impl Fn() -> T + 'static,
    on_update: impl Fn(bool) + 'static,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    let on_update = Rc::new(on_update);
    let update_from_checkbox = on_update.clone();
    themed(
        Checkbox::labeled(checked.clone(), label)
            .on_update(move |value| update_from_checkbox(value)),
        checked.clone(),
        config,
    )
    .on_click_stop(move |_| on_update(!checked()))
    .style(|s| s.class(LabeledCheckboxClass, |s| s.gap(8.0)))
}

/// A group checkbox uses Floem's Boolean input for activation. Activating a
/// mixed group checks every item, while activating a checked group clears it.
pub(crate) fn group_checkbox(
    state: impl Fn() -> CheckboxState + Clone + 'static,
    on_update: impl Fn(bool) + 'static,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    let checked = state.clone();
    let filled = state.clone();
    let input = themed(
        Checkbox::new(move || checked() == CheckboxState::Checked)
            .on_update(on_update),
        move || filled() != CheckboxState::Unchecked,
        config,
    );
    let dash = container(empty().style(move |s| {
        s.width_pct(50.0).height(2.0).border_radius(1.0).background(
            config
                .get()
                .color(SparrowColor::SPARROW_BUTTON_PRIMARY_FOREGROUND),
        )
    }))
    .style(move |s| {
        s.absolute()
            .size_full()
            .items_center()
            .justify_center()
            .pointer_events_none()
            .apply_if(state() != CheckboxState::Mixed, |s| s.hide())
    });
    stack((input, dash)).style(|s| s.flex_shrink(0.0_f32))
}

#[cfg(test)]
mod tests {
    use super::CheckboxState::{self, *};

    #[test]
    fn group_checkbox_reports_empty_none_partial_and_full_selection() {
        for (total, selected, state) in [
            (0, 0, Unchecked),
            (2, 0, Unchecked),
            (2, 1, Mixed),
            (2, 2, Checked),
        ] {
            assert_eq!(CheckboxState::from_selection(total, selected), state);
        }
    }
}
