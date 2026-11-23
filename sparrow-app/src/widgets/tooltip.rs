//! Shared tooltip lifetime and ownership for the IDE.
use floem::{
    IntoView, View, ViewId,
    action::{add_overlay, exec_after, remove_overlay},
    context::EventCx,
    event::{Event, EventListener, EventPropagation},
    reactive::{ReadSignal, SignalGet},
    views::{Decorators, container, label},
};
use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc, time::Duration};

use crate::config::{SparrowConfig, color::SparrowColor};

struct Hover {
    owner: ViewId,
    ticket: Rc<()>,
    overlay: Option<ViewId>,
}

thread_local! {
    static HOVERS: RefCell<HashMap<ViewId, Hover>> = RefCell::new(HashMap::new());
}

fn window_root(mut id: ViewId) -> ViewId {
    while let Some(parent) = id.parent() {
        id = parent;
    }
    id
}

fn dismiss(window: ViewId, owner: Option<ViewId>) {
    let removed = HOVERS.with_borrow_mut(|hovers| {
        if owner.is_none_or(|owner| {
            hovers.get(&window).is_some_and(|h| h.owner == owner)
        }) {
            hovers.remove(&window)
        } else {
            None
        }
    });
    if let Some(id) = removed.and_then(|h| h.overlay) {
        remove_overlay(id);
    }
}

pub(crate) fn dismiss_window(view: ViewId) {
    dismiss(window_root(view), None);
}

struct TooltipAnchor {
    id: ViewId,
    window: Rc<std::cell::Cell<Option<ViewId>>>,
}

impl View for TooltipAnchor {
    fn id(&self) -> ViewId {
        self.id
    }

    fn event_before_children(
        &mut self,
        _cx: &mut EventCx,
        event: &Event,
    ) -> EventPropagation {
        if matches!(
            event,
            Event::PointerLeave
                | Event::PointerDown(_)
                | Event::PointerWheel(_)
                | Event::KeyDown(_)
                | Event::WindowLostFocus
        ) {
            if let Some(window) = self.window.get() {
                dismiss(window, Some(self.id));
            }
        }
        EventPropagation::Continue
    }
}

pub(crate) fn tooltip<T: IntoView + 'static>(
    child: impl IntoView + 'static,
    tip: impl Fn() -> T + 'static,
) -> impl View {
    let owner = ViewId::new();
    owner.set_children([child.into_view()]);
    // Remember the root because cleanup can run after the view has been detached.
    let window = Rc::new(std::cell::Cell::new(None));
    let dismiss_owner = {
        let window = window.clone();
        move || {
            if let Some(window) = window.get() {
                dismiss(window, Some(owner));
            }
        }
    };
    let tip = Rc::new(tip);
    TooltipAnchor {
        id: owner,
        window: window.clone(),
    }
    .on_event_cont(EventListener::PointerEnter, move |event| {
        let Event::PointerMove(pointer) = event else {
            return;
        };
        let root = window_root(owner);
        window.set(Some(root));
        dismiss(root, None);
        let ticket = Rc::new(());
        HOVERS.with_borrow_mut(|hovers| {
            hovers.insert(
                root,
                Hover {
                    owner,
                    ticket: ticket.clone(),
                    overlay: None,
                },
            );
        });

        let position = pointer.pos + (10.0, 10.0);
        let tip = tip.clone();
        exec_after(Duration::from_millis(600), move |_| {
            HOVERS.with_borrow_mut(|hovers| {
                let Some(hover) = hovers.get_mut(&root) else {
                    return;
                };
                if !Rc::ptr_eq(&hover.ticket, &ticket) {
                    return;
                }
                if owner.is_hidden_recursive() {
                    hovers.remove(&root);
                    return;
                }
                hover.overlay = Some(add_overlay(position, move |_| {
                    tip().into_view().style(|s| s.pointer_events_none())
                }));
            });
        });
    })
    .on_move({
        let dismiss_owner = dismiss_owner.clone();
        move |_| dismiss_owner()
    })
    .on_cleanup(dismiss_owner)
}

pub(crate) trait TooltipExt: IntoView + Sized + 'static {
    fn tooltip<T: IntoView + 'static>(
        self,
        tip: impl Fn() -> T + 'static,
    ) -> impl View {
        tooltip(self, tip)
    }
}
impl<V: IntoView + 'static> TooltipExt for V {}

/// A tooltip with a label inside.
///
/// When styling an element that has the tooltip, it will style the child rather than the tooltip
/// label.
pub fn tooltip_label<S: std::fmt::Display + 'static, V: View + 'static>(
    config: ReadSignal<Arc<SparrowConfig>>,
    child: V,
    text: impl Fn() -> S + 'static + Clone,
) -> impl View {
    tooltip(child, move || {
        tooltip_tip(
            config,
            label(text.clone()).style(move |s| s.selectable(false)),
        )
    })
}

pub(crate) fn tooltip_tip<V: View + 'static>(
    config: ReadSignal<Arc<SparrowConfig>>,
    child: V,
) -> impl IntoView {
    container(child).style(move |s| {
        let config = config.get();
        s.padding_horiz(10.0)
            .padding_vert(5.0)
            .font_size(config.ui.font_size() as f32)
            .line_height(1.4)
            .font_family(config.ui.font_family.clone())
            .color(config.color(SparrowColor::TOOLTIP_FOREGROUND))
            .background(config.color(SparrowColor::TOOLTIP_BACKGROUND))
            .border(1)
            .border_radius(6)
            .border_color(config.color(SparrowColor::SPARROW_BORDER))
            .box_shadow_blur(3.0)
            .box_shadow_color(config.color(SparrowColor::SPARROW_DROPDOWN_SHADOW))
            .margin_left(0.0)
            .margin_top(4.0)
    })
}
