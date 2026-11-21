use std::{fmt::Display, rc::Rc, sync::Arc};

use floem::{
    View,
    reactive::{ReadSignal, SignalGet},
    style::CursorStyle,
    views::{
        Checkbox, CheckboxClass, Decorators, LabeledCheckboxClass, ValueContainer,
    },
};

use crate::config::{SparrowConfig, color::SparrowColor};

fn themed(
    checkbox: ValueContainer<bool>,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> ValueContainer<bool> {
    checkbox.style(move |s| {
        let config = config.get();
        let size = config.ui.font_size() as f32;
        let color = config.color(SparrowColor::EDITOR_FOREGROUND);

        s.class(CheckboxClass, |s| {
            s.min_width(size)
                .size(size, size)
                .flex_shrink(0.0_f32)
                .color(color)
                .border_color(color)
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
    themed(Checkbox::new(checked), config)
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
        config,
    )
    .on_click_stop(move |_| on_update(!checked()))
    .style(|s| s.class(LabeledCheckboxClass, |s| s.gap(8.0)))
}
