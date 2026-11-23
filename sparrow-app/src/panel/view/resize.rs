use std::{cell::Cell, rc::Rc, sync::Arc, time::Duration};

use floem::{
    View,
    action::{TimerToken, exec_after},
    event::{Event, EventListener, EventPropagation},
    kurbo::{Point, Size},
    reactive::{ReadSignal, SignalGet, SignalUpdate, batch, create_rw_signal},
    style::CursorStyle,
    views::{Decorators, empty},
};

use crate::{
    config::{SparrowConfig, color::SparrowColor},
    panel::{
        data::{PanelData, PanelSize},
        position::PanelContainerPosition,
    },
};

#[derive(Clone, Copy)]
struct Anchor {
    cursor: Point,
    size: Size,
}

#[derive(Default)]
struct Drag {
    anchor: Cell<Option<Anchor>>,
    latest: Cell<Option<Point>>,
    timer: Cell<Option<TimerToken>>,
}

impl Drag {
    fn stop(&self) -> bool {
        if let Some(timer) = self.timer.take() {
            timer.cancel();
        }
        self.latest.take();
        self.anchor.take().is_some()
    }

    fn apply(&self, panel: &PanelData, position: PanelContainerPosition) {
        let (Some(anchor), Some(cursor)) = (self.anchor.get(), self.latest.take())
        else {
            return;
        };
        let delta = cursor - anchor.cursor;
        let (requested, available) = match position {
            PanelContainerPosition::Left => (
                anchor.size.width + delta.x,
                panel.available_size.get_untracked().width,
            ),
            PanelContainerPosition::Right => (
                anchor.size.width - delta.x,
                panel.available_size.get_untracked().width,
            ),
            PanelContainerPosition::Bottom => (
                anchor.size.height - delta.y,
                panel.available_size.get_untracked().height,
            ),
        };
        let current = panel.size.get_untracked();
        let mut next = current.clone();
        resize(&mut next, position, requested, available);
        batch(|| {
            if next != current {
                panel.size.set(next);
            }
            if position.is_bottom()
                && delta.y >= 12.0
                && panel.panel_bottom_maximized(false)
            {
                panel.toggle_bottom_maximize();
            }
        });
    }
}

fn resize(
    size: &mut PanelSize,
    position: PanelContainerPosition,
    requested: f64,
    available: f64,
) {
    if position.is_bottom() {
        let minimum = (available / 2.0).clamp(0.0, 100.0);
        size.bottom = requested.clamp(minimum, (available - minimum).max(minimum));
        return;
    }
    let (width, opposite) = if position == PanelContainerPosition::Left {
        (&mut size.left, &mut size.right)
    } else {
        (&mut size.right, &mut size.left)
    };
    let minimum = (available / 3.0).clamp(0.0, 150.0);
    *width = requested.clamp(minimum, (available - 2.0 * minimum).max(minimum));
    *opposite = opposite.min((available - *width - minimum).max(minimum));
}

pub(super) fn handle(
    panel: PanelData,
    position: PanelContainerPosition,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    let drag = Rc::new(Drag::default());
    let sizing = panel.resizing;
    let active = create_rw_signal(false);
    let view = empty();
    let id = view.id();
    // Floem sends pointer events relative to the current position of the handle.
    let window_point =
        move |point: Point| id.layout_rect().origin() + point.to_vec2();
    let finish = {
        let drag = drag.clone();
        move || {
            if drag.stop() {
                id.clear_active();
                active.try_update(|value| *value = false);
                sizing.try_update(|value| *value = false);
            }
        }
    };

    view.on_event(EventListener::PointerDown, {
        let drag = drag.clone();
        move |event| {
            let Event::PointerDown(pointer) = event else {
                return EventPropagation::Continue;
            };
            if !pointer.button.is_primary() {
                return EventPropagation::Continue;
            }
            let Some(size) = id.parent_size() else {
                return EventPropagation::Continue;
            };
            drag.anchor.set(Some(Anchor {
                cursor: window_point(pointer.pos),
                size,
            }));
            batch(|| {
                sizing.set(true);
                active.set(true);
            });
            id.request_active();
            EventPropagation::Stop
        }
    })
    .on_event(EventListener::PointerMove, {
        let drag = drag.clone();
        let panel = panel.clone();
        move |event| {
            let Event::PointerMove(pointer) = event else {
                return EventPropagation::Continue;
            };
            if drag.anchor.get().is_none() {
                return EventPropagation::Continue;
            }
            drag.latest.set(Some(window_point(pointer.pos)));
            if drag.timer.get().is_none() {
                let pending = Rc::downgrade(&drag);
                let panel = panel.clone();
                drag.timer.set(Some(exec_after(
                    Duration::from_millis(16),
                    move |_| {
                        if let Some(drag) = pending.upgrade() {
                            drag.timer.take();
                            drag.apply(&panel, position);
                        }
                    },
                )));
            }
            EventPropagation::Stop
        }
    })
    .on_event(EventListener::PointerUp, {
        let drag = drag.clone();
        let finish = finish.clone();
        move |event| {
            let Event::PointerUp(pointer) = event else {
                return EventPropagation::Continue;
            };
            if !pointer.button.is_primary() || drag.anchor.get().is_none() {
                return EventPropagation::Continue;
            }
            let cursor = window_point(pointer.pos);
            if drag
                .anchor
                .get()
                .is_some_and(|anchor| cursor != anchor.cursor)
                || drag.latest.get().is_some()
            {
                drag.latest.set(Some(cursor));
                drag.apply(&panel, position);
            }
            finish();
            EventPropagation::Stop
        }
    })
    .on_event_cont(EventListener::WindowLostFocus, {
        let finish = finish.clone();
        move |_| finish()
    })
    .on_cleanup(finish)
    .style(move |s| {
        let bottom = position.is_bottom();
        let foreground = config.get().color(SparrowColor::EDITOR_CARET);
        let highlight = |s: floem::style::Style| s.background(foreground).z_index(2);
        s.absolute()
            .cursor(if bottom {
                CursorStyle::RowResize
            } else {
                CursorStyle::ColResize
            })
            .apply_if(bottom, |s| s.width_full().height(4.0).inset_top(-2.0))
            .apply_if(!bottom, |s| s.width(4.0).height_full())
            .apply_if(position == PanelContainerPosition::Left, |s| {
                s.inset_right(-2.0)
            })
            .apply_if(position == PanelContainerPosition::Right, |s| {
                s.inset_left(-2.0)
            })
            .apply_if(active.get(), highlight)
            .hover(highlight)
    })
    .debug_name("Dock resize handle")
}
