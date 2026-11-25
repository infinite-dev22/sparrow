//! Device chrome, launcher, app switcher, and embedded input.
use std::{cell::Cell, rc::Rc, sync::Arc};

use floem::{
    IntoView, View,
    event::{Event, EventListener, EventPropagation},
    keyboard::{Key, Modifiers, NamedKey},
    kurbo::Point,
    peniko::Color,
    reactive::{ReadSignal, RwSignal, SignalGet, SignalUpdate, create_effect},
    views::{Decorators, clip, dyn_container, dyn_stack, empty, img, label, stack},
};

use crate::{
    config::SparrowConfig,
    widgets::{ButtonKind, button, sparrow_scroll, tooltip_label},
};

use super::{
    device::{CutoutShape, NavigationIndicator},
    files,
    renderer::Input,
    session::{App, Screen, Session},
};

pub(super) type Config = ReadSignal<Arc<SparrowConfig>>;

pub(super) fn phone(
    session: Session,
    available: RwSignal<f64>,
    config: Config,
) -> impl View {
    let screen = session.screen;
    let content = session.clone();
    let sizing = session.clone();
    let dark = session.dark;
    let status = session.clone();
    let nav = session.clone();
    let cutout_state = session.clone();
    let frame_state = session.clone();
    let cutout = dyn_container(
        move || {
            cutout_state
                .device
                .get()
                .device()
                .map(|device| device.cutout)
        },
        move |shape| match shape {
            Some(shape) => cutout_shape(shape).into_any(),
            None => empty().into_any(),
        },
    )
    .style(|s| {
        s.absolute()
            .inset_top(0)
            .width_full()
            .height(32.0)
            .items_center()
            .pointer_events_none()
    });
    let screen_body = dyn_container(
        move || screen.get(),
        move |screen| match screen {
            Screen::App => surface(content.clone()).into_any(),
            Screen::Home => launcher(content.clone(), config).into_any(),
            Screen::Recents => recents(content.clone(), config).into_any(),
            Screen::Files => sparrow_scroll(files::view(content.clone(), config))
                .style(|s| {
                    s.size_full()
                        .min_width(0)
                        .padding_top(24.0)
                        .padding_bottom(32.0)
                })
                .into_any(),
        },
    )
    .style(|s| s.absolute().size_full());
    let frame = clip(
        stack((
            screen_body,
            stack((
                label(|| "Simulated device")
                    .style(|s| s.font_size(10).selectable(false)),
                empty().style(|s| s.flex_grow(1.0_f32)),
                label(move || if dark.get() { "Dark" } else { "Light" })
                    .style(|s| s.font_size(10)),
            ))
            .style(move |s| {
                s.absolute()
                    .inset_top(0)
                    .width_full()
                    .height(
                        status
                            .device
                            .get()
                            .device()
                            .map_or(24.0, |device| device.status_bar_height),
                    )
                    .padding_horiz(12.0)
                    .items_center()
                    .pointer_events_none()
                    .background(chrome(dark.get()))
                    .color(foreground(dark.get()))
                    .apply_if(
                        !status.status_sticky.get() && !status.bars.get(),
                        |s| s.hide(),
                    )
            }),
            cutout,
            navigation(nav, config),
        ))
        .style(|s| s.size_full()),
    )
    .style(move |s| {
        let (w, h) = sizing.viewport().map(|v| v.size).unwrap_or((400, 800));
        let width = available.get().min(520.0);
        let scale = (width / f64::from(w)).min(700.0 / f64::from(h));
        let (corner_radius, frame_color) = frame_state.device.get().device().map_or(
            (22.0, Color::from_rgb8(65, 69, 77)),
            |device| {
                (
                    device.corner_radius,
                    Color::from_rgb8(
                        device.frame_color.0,
                        device.frame_color.1,
                        device.frame_color.2,
                    ),
                )
            },
        );
        s.width(f64::from(w) * scale)
            .height(f64::from(h) * scale)
            .min_width(0)
            .border(3.0)
            .border_radius(corner_radius)
            .border_color(frame_color)
            .background(chrome(dark.get()))
            .color(foreground(dark.get()))
    });
    let id = frame.id();
    frame.on_event_cont(EventListener::PointerMove, move |event| {
        if let Event::PointerMove(pointer) = event
            && !session.bars.get_untracked()
            && let Some(size) = id.get_size()
            && ((!session.status_sticky.get_untracked() && pointer.pos.y < 10.0)
                || (!session.navigation_sticky.get_untracked()
                    && size.height - pointer.pos.y < 10.0))
        {
            session.reveal_bars();
        }
    })
}

fn cutout_shape(shape: CutoutShape) -> impl View {
    let (width, height, radius) = match shape {
        CutoutShape::None => (0.0, 0.0, 0.0),
        CutoutShape::Notch => (116.0, 30.0, 12.0),
        CutoutShape::Pill => (104.0, 30.0, 15.0),
        CutoutShape::PunchHole => (12.0, 12.0, 6.0),
    };
    empty().style(move |s| {
        s.size(width, height)
            .border_radius(radius)
            .background(Color::from_rgb8(12, 14, 18))
    })
}

fn chrome(dark: bool) -> Color {
    if dark {
        Color::from_rgb8(25, 28, 34)
    } else {
        Color::from_rgb8(242, 245, 250)
    }
}

fn foreground(dark: bool) -> Color {
    if dark {
        Color::from_rgb8(233, 237, 246)
    } else {
        Color::from_rgb8(31, 40, 58)
    }
}

fn launcher(session: Session, config: Config) -> impl View {
    let active = session.active;
    let open = session.clone();
    let screen = session.screen;
    stack((
        label(|| "Home").style(|s| s.font_size(22).font_bold()),
        button(|| "Resume preview", ButtonKind::Outlined, config)
            .on_click_stop(move |_| {
                if let Some(app) = active.get_untracked() {
                    open.open(app);
                }
            })
            .style(move |s| {
                s.padding(7.0)
                    .apply_if(active.get().is_none(), |s| s.hide())
            }),
        button(|| "Files", ButtonKind::Outlined, config)
            .on_click_stop(move |_| screen.set(Screen::Files))
            .style(|s| s.padding(7.0)),
        button(|| "Recent apps", ButtonKind::Plain, config)
            .on_click_stop(move |_| screen.set(Screen::Recents))
            .style(|s| s.padding(7.0)),
    ))
    .style(|s| {
        s.size_full()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(8.0)
    })
}

fn recents(session: Session, config: Config) -> impl View {
    let recent = session.recents;
    let open = session.clone();
    stack((
        stack((
            label(|| "Recent apps").style(|s| s.font_bold().flex_grow(1.0_f32)),
            button(|| "Clear all", ButtonKind::Plain, config)
                .on_click_stop(move |_| session.clear_recents())
                .disabled(move || recent.get().is_empty()),
        ))
        .style(|s| s.items_center().width_full()),
        label(|| "No recent previews")
            .style(move |s| s.apply_if(!recent.get().is_empty(), |s| s.hide())),
        sparrow_scroll(
            dyn_stack(
                move || recent.get(),
                App::clone,
                move |app| {
                    let open = open.clone();
                    let title = if app.widgets {
                        "Flutter Widget Previews".into()
                    } else {
                        app.url.clone()
                    };
                    crate::widgets::button_with_content(
                        label(move || title.clone()).style(|s| {
                            s.text_ellipsis().width_full().selectable(false)
                        }),
                        ButtonKind::Outlined,
                        config,
                    )
                    .on_click_stop(move |_| open.open(app.clone()))
                    .style(|s| s.width_full().padding(6.0).min_width(0))
                },
            )
            .style(|s| s.flex_col().gap(5.0).width_full()),
        )
        .style(|s| {
            s.width_full()
                .flex_basis(0)
                .flex_grow(1.0_f32)
                .min_height(0)
        }),
    ))
    .style(|s| {
        s.size_full()
            .flex_col()
            .gap(9.0)
            .padding(12.0)
            .padding_top(33.0)
            .padding_bottom(38.0)
    })
}

fn navigation(session: Session, config: Config) -> impl View {
    let screen = session.screen;
    let gestures = session.gestures;
    let dark = session.dark;
    let indicator_state = session.clone();
    let sizing = session.clone();
    let sticky = session.navigation_sticky;
    let bars = session.bars;
    let back = session.clone();
    let origin = Rc::new(Cell::new(None::<Point>));
    let start = origin.clone();
    let indicator = dyn_container(
        move || {
            indicator_state
                .device
                .get()
                .device()
                .map(|device| device.navigation)
        },
        move |indicator| match indicator.unwrap_or(NavigationIndicator::GesturePill)
        {
            NavigationIndicator::HomeIndicator => empty()
                .style(move |s| {
                    s.size(108, 4)
                        .border_radius(2)
                        .background(foreground(dark.get()))
                })
                .into_any(),
            NavigationIndicator::GesturePill => empty()
                .style(move |s| {
                    s.size(65, 4)
                        .border_radius(2)
                        .background(foreground(dark.get()))
                })
                .into_any(),
            NavigationIndicator::NavigationBar => stack((
                empty().style(move |s| {
                    s.size(13, 3)
                        .border_radius(2)
                        .background(foreground(dark.get()))
                }),
                empty().style(move |s| {
                    s.size(13, 3)
                        .border_radius(2)
                        .background(foreground(dark.get()))
                }),
                empty().style(move |s| {
                    s.size(13, 3)
                        .border_radius(2)
                        .background(foreground(dark.get()))
                }),
            ))
            .style(|s| s.gap(8.0).items_center())
            .into_any(),
        },
    );
    let pill = tooltip_label(
        config,
        indicator,
        || "Tap for Home · Swipe up for Recent apps",
    );
    let gesture = stack((pill,)).style(move |s| {
        s.width_full()
            .height(
                sizing
                    .device
                    .get()
                    .device()
                    .map_or(31.0, |device| device.navigation_height),
            )
            .flex_shrink(0.0_f32)
            .justify_center()
            .items_center()
            .apply_if(!gestures.get(), |s| s.hide())
    });
    let target = gesture.id();
    let gesture = gesture
        .on_event_stop(EventListener::PointerDown, move |event| {
            if let Event::PointerDown(pointer) = event {
                target.request_active();
                start.set(Some(pointer.pos));
            }
        })
        .on_event_stop(EventListener::PointerUp, move |event| {
            if let Event::PointerUp(pointer) = event
                && let Some(start) = origin.take()
            {
                screen.set(if start.y - pointer.pos.y > 12.0 {
                    Screen::Recents
                } else {
                    Screen::Home
                });
            }
        });
    stack((
        stack((
            button(|| "Back", ButtonKind::Plain, config)
                .on_click_stop(move |_| back.back()),
            button(|| "Home", ButtonKind::Plain, config)
                .on_click_stop(move |_| screen.set(Screen::Home)),
            button(|| "Recents", ButtonKind::Plain, config)
                .on_click_stop(move |_| screen.set(Screen::Recents)),
        ))
        .style(move |s| {
            s.width_full()
                .items_center()
                .justify_evenly()
                .apply_if(gestures.get(), |s| s.hide())
        }),
        gesture,
    ))
    .style(move |s| {
        s.absolute()
            .inset_bottom(0)
            .width_full()
            .height(
                sizing
                    .device
                    .get()
                    .device()
                    .map_or(31.0, |device| device.navigation_height),
            )
            .items_center()
            .background(chrome(dark.get()))
            .color(foreground(dark.get()))
            .apply_if(!sticky.get() && !bars.get(), |s| s.hide())
    })
}

fn surface(session: Session) -> impl View {
    let image = img(Vec::new);
    let id = image.id();
    let source = session.frame;
    create_effect(move |_| {
        if let Some(frame) = source.get() {
            id.update_state(frame);
        }
    });
    let origin = Rc::new(Cell::new(None::<Point>));
    let edge = Rc::new(Cell::new(false));
    let controller = session.clone();
    image
        .style(|s| s.absolute().size_full())
        .keyboard_navigable()
        .on_event(EventListener::PointerDown, {
            let origin = origin.clone();
            let edge = edge.clone();
            let session = session.clone();
            move |event| {
                let Event::PointerDown(pointer) = event else {
                    return EventPropagation::Continue;
                };
                if !pointer.button.is_primary() {
                    return EventPropagation::Continue;
                }
                id.request_focus();
                id.request_active();
                let Some(size) = id.get_size() else {
                    return EventPropagation::Continue;
                };
                let from_edge = session.gestures.get_untracked()
                    && (pointer.pos.x < 12.0 || size.width - pointer.pos.x < 12.0)
                    || (!session.navigation_sticky.get_untracked()
                        && size.height - pointer.pos.y < 12.0)
                    || (!session.status_sticky.get_untracked()
                        && pointer.pos.y < 12.0);
                origin.set(Some(pointer.pos));
                edge.set(from_edge);
                if !from_edge {
                    touch(&session, id, "touchStart", pointer.pos);
                }
                EventPropagation::Stop
            }
        })
        .on_event(EventListener::PointerMove, {
            let origin = origin.clone();
            let edge = edge.clone();
            let session = session.clone();
            move |event| {
                if origin.get().is_some()
                    && !edge.get()
                    && let Event::PointerMove(pointer) = event
                {
                    touch(&session, id, "touchMove", pointer.pos);
                }
                EventPropagation::Continue
            }
        })
        .on_event(EventListener::PointerUp, {
            let session = session.clone();
            move |event| {
                let Event::PointerUp(pointer) = event else {
                    return EventPropagation::Continue;
                };
                let Some(start) = origin.take() else {
                    return EventPropagation::Continue;
                };
                if edge.get() {
                    let movement = pointer.pos - start;
                    if movement.x.abs() > 24.0 && movement.x.abs() > movement.y.abs()
                    {
                        session.back();
                    } else if movement.y.abs() > 15.0 {
                        session.reveal_bars();
                    } else {
                        touch(&session, id, "touchStart", pointer.pos);
                        touch(&session, id, "touchEnd", pointer.pos);
                    }
                } else {
                    touch(&session, id, "touchEnd", pointer.pos);
                }
                EventPropagation::Stop
            }
        })
        .on_event(EventListener::PointerWheel, {
            let session = session.clone();
            move |event| {
                if let Event::PointerWheel(pointer) = event
                    && let Some((x, y)) = position(&session, id, pointer.pos)
                {
                    session.send(Input::Wheel(
                        x,
                        y,
                        pointer.delta.x,
                        pointer.delta.y,
                    ));
                    return EventPropagation::Stop;
                }
                EventPropagation::Continue
            }
        })
        .on_event(EventListener::KeyDown, move |event| {
            let Event::KeyDown(key) = event else {
                return EventPropagation::Continue;
            };
            match &key.key.logical_key {
                Key::Character(text)
                    if !key.modifiers.intersects(
                        Modifiers::ALT | Modifiers::CONTROL | Modifiers::META,
                    ) =>
                {
                    controller.send(Input::Text(text.to_string()));
                }
                Key::Named(named) => {
                    let code = match named {
                        NamedKey::Enter => 13,
                        NamedKey::Backspace => 8,
                        NamedKey::Delete => 46,
                        NamedKey::ArrowLeft => 37,
                        NamedKey::ArrowUp => 38,
                        NamedKey::ArrowRight => 39,
                        NamedKey::ArrowDown => 40,
                        NamedKey::Home => 36,
                        NamedKey::End => 35,
                        _ => return EventPropagation::Continue,
                    };
                    controller.send(Input::Key(format!("{named:?}"), code));
                }
                _ => return EventPropagation::Continue,
            }
            EventPropagation::Stop
        })
}

fn position(
    session: &Session,
    id: floem::ViewId,
    point: Point,
) -> Option<(f64, f64)> {
    let viewport = session.viewport()?;
    let size = id.get_size()?;
    if size.width <= 0.0 || size.height <= 0.0 {
        return None;
    }
    let w = f64::from(viewport.size.0);
    let h = f64::from(viewport.size.1);
    Some((
        (point.x / size.width * w).clamp(0.0, w - 1.0),
        (point.y / size.height * h).clamp(0.0, h - 1.0),
    ))
}

fn touch(session: &Session, id: floem::ViewId, kind: &'static str, point: Point) {
    if let Some((x, y)) = position(session, id, point) {
        session.send(Input::Touch(kind, x, y));
    }
}
