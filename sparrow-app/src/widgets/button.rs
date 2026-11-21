use std::{cell::RefCell, collections::HashMap, fmt::Display, sync::Arc};

use floem::{
    View, ViewId,
    event::{Event, EventListener, EventPropagation},
    keyboard::{KeyCode, PhysicalKey},
    reactive::{ReadSignal, SignalGet},
    style::CursorStyle,
    views::{Decorators, button as floem_button, label},
};

use crate::config::{SparrowConfig, color::SparrowColor};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    Plain,
    Outlined,
    Primary,
}

/// A themed text button. Attach the click handler and register the returned
/// view with the dialog's focus manager where needed.
pub fn button<T: Display + 'static>(
    text: impl Fn() -> T + 'static,
    kind: ButtonKind,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    native_button_keyboard(floem_button(label(text).style(|s| s.selectable(false))))
        .style(move |s| {
            let config = config.get();
            s.items_center()
                .justify_center()
                .border_radius(4.0)
                .apply_if(kind != ButtonKind::Plain, |s| {
                    s.border(1.0)
                        .border_color(config.color(SparrowColor::SPARROW_BORDER))
                })
                .apply_if(kind == ButtonKind::Primary, |s| {
                    s.background(
                        config
                            .color(SparrowColor::SPARROW_BUTTON_PRIMARY_BACKGROUND),
                    )
                })
                .hover(|s| {
                    s.cursor(CursorStyle::Pointer).background(
                        config.color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                    )
                })
                .active(|s| {
                    s.background(
                        config.color(SparrowColor::PANEL_HOVERED_ACTIVE_BACKGROUND),
                    )
                })
                .focus_visible(|s| {
                    s.outline(1.0)
                        .outline_color(config.color(SparrowColor::EDITOR_CARET))
                })
                .disabled(|s| {
                    s.cursor(CursorStyle::Default)
                        .color(config.color(SparrowColor::PANEL_FOREGROUND_DIM))
                })
        })
}

// Floem dispatches the native button click before its KeyDown listeners. Stop
// activation from also reaching Sparrow's editor, and leave Tab traversal to Floem.
pub(super) fn native_button_keyboard(view: impl View + 'static) -> impl View {
    let id = view.id();
    view.on_event_cont(EventListener::FocusGained, move |_| {
        FOCUSED_BUTTONS.with_borrow_mut(|buttons| buttons.insert(root_view(id), id));
    })
    .on_event_cont(EventListener::FocusLost, move |_| forget_button(id))
    .on_cleanup(move || forget_button(id))
    .on_event(EventListener::KeyDown, move |event| {
        if let Event::KeyDown(key) = event {
            if matches!(
                key.key.physical_key,
                PhysicalKey::Code(
                    KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Space
                )
            ) {
                return EventPropagation::Stop;
            }
        }
        EventPropagation::Continue
    })
}

thread_local! {
    static FOCUSED_BUTTONS: RefCell<HashMap<ViewId, ViewId>> = RefCell::new(HashMap::new());
}

fn root_view(mut id: ViewId) -> ViewId {
    while let Some(parent) = id.parent() {
        id = parent;
    }
    id
}

fn forget_button(id: ViewId) {
    FOCUSED_BUTTONS
        .with_borrow_mut(|buttons| buttons.retain(|_, button| *button != id));
}

pub(crate) fn native_button_has_focus(window: ViewId) -> bool {
    FOCUSED_BUTTONS.with_borrow(|buttons| buttons.contains_key(&root_view(window)))
}
