use std::{fmt::Display, rc::Rc, sync::Arc};

use floem::{
    IntoView, View,
    event::{Event, EventListener, EventPropagation},
    keyboard::{Key, NamedKey},
    peniko::Color,
    reactive::{ReadSignal, RwSignal, SignalGet, SignalUpdate},
    style::CursorStyle,
    taffy::FlexWrap,
    views::{
        Decorators, RadioButton, RadioButtonClass, RadioButtonDotClass,
        stack_from_iter,
    },
};

use crate::config::{SparrowConfig, color::SparrowColor};

/// A themed group of radio buttons sharing one selected value.
pub fn radio_group<T>(
    selected: RwSignal<T>,
    options: impl IntoIterator<Item = T>,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View
where
    T: Clone + Display + Eq + 'static,
{
    let options: Rc<[T]> = options.into_iter().collect();
    let buttons: Vec<_> = options
        .iter()
        .map(|value| {
            let label = value.clone();
            let value = value.clone();
            RadioButton::new_labeled_rw(value.clone(), selected, move || {
                label.clone()
            })
            .into_view()
            .style(move |s| {
                let config = config.get();
                let size = config.ui.font_size() as f32;
                let accent = config.color(SparrowColor::EDITOR_LINK);
                let border = if selected.get() == value {
                    accent
                } else {
                    config.color(SparrowColor::EDITOR_FOREGROUND)
                };
                s.padding_horiz(4.0)
                    .padding_vert(2.0)
                    .gap(6.0)
                    .border_radius(4.0)
                    .class(RadioButtonClass, |s| {
                        s.size(size, size)
                            .flex_shrink(0.0_f32)
                            .items_center()
                            .justify_center()
                            .background(Color::TRANSPARENT)
                            .border(1.0)
                            .border_color(border)
                            .border_radius(size / 2.0)
                    })
                    .class(RadioButtonDotClass, |s| {
                        s.size(size / 2.0, size / 2.0)
                            .background(accent)
                            .border_radius(size / 4.0)
                    })
                    .hover(|s| {
                        s.cursor(CursorStyle::Pointer).background(
                            config.color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                        )
                    })
                    .focus_visible(|s| s.outline(1.0).outline_color(accent))
            })
        })
        .collect();
    let ids: Rc<[_]> = buttons.iter().map(View::id).collect();

    stack_from_iter(buttons.into_iter().enumerate().map(|(index, button)| {
        let options = options.clone();
        let ids = ids.clone();
        button.on_event(EventListener::KeyDown, move |event| {
            let Event::KeyDown(key) = event else {
                return EventPropagation::Continue;
            };
            if !key.modifiers.is_empty() {
                return EventPropagation::Continue;
            }
            let len = options.len();
            let next = match key.key.logical_key {
                Key::Named(NamedKey::ArrowLeft | NamedKey::ArrowUp) => {
                    (index + len - 1) % len
                }
                Key::Named(NamedKey::ArrowRight | NamedKey::ArrowDown) => {
                    (index + 1) % len
                }
                _ => return EventPropagation::Continue,
            };
            selected.set(options[next].clone());
            ids[next].request_focus();
            EventPropagation::Stop
        })
    }))
    .style(|s| s.items_center().flex_wrap(FlexWrap::Wrap).gap(8.0))
}
