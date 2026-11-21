//! Window messages reported by language servers and tools.

use std::sync::{Arc, atomic::AtomicU64};

use crate::config::{SparrowConfig, color::SparrowColor, icon::SparrowIcons};
use floem::{
    View,
    event::EventListener,
    reactive::{ReadSignal, RwSignal, SignalGet, SignalUpdate},
    text::Weight,
    views::{
        Decorators, container, dyn_stack,
        editor::{core::register::Clipboard, text::SystemClipboard},
        scroll::PropagatePointerWheel,
        stack, svg, text,
    },
};
use lsp_types::{MessageType, ShowMessageParams};

use crate::widgets::{clickable_icon, sparrow_scroll};

pub(super) fn window_message_view(
    messages: RwSignal<Vec<(String, ShowMessageParams)>>,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    let view_fn =
        move |(i, (title, message)): (usize, (String, ShowMessageParams))| {
            stack((
                svg(move || {
                    if let MessageType::ERROR = message.typ {
                        config.get().ui_svg(SparrowIcons::ERROR)
                    } else {
                        config.get().ui_svg(SparrowIcons::WARNING)
                    }
                })
                .style(move |s| {
                    let config = config.get();
                    let size = config.ui.icon_size() as f32;
                    let color = if let MessageType::ERROR = message.typ {
                        config.color(SparrowColor::SPARROW_ERROR)
                    } else {
                        config.color(SparrowColor::SPARROW_WARN)
                    };
                    s.min_width(size)
                        .size(size, size)
                        .margin_right(10.0)
                        .margin_top(4.0)
                        .color(color)
                }),
                stack((
                    text(title.clone()).style(|s| {
                        s.min_width(0.0).line_height(1.8).font_weight(Weight::BOLD)
                    }),
                    text(message.message.clone()).style(|s| {
                        s.min_width(0.0).line_height(1.8).margin_top(5.0)
                    }),
                ))
                .style(move |s| {
                    s.flex_col()
                        .min_width(0.0)
                        .flex_basis(0.0)
                        .flex_grow(1.0f32)
                }),
                clickable_icon(
                    || SparrowIcons::CLOSE,
                    move || {
                        messages.update(|messages| {
                            messages.remove(i);
                        });
                    },
                    || false,
                    || false,
                    || "Close",
                    config,
                )
                .style(|s| s.margin_left(6.0)),
            ))
            .on_double_click_stop(move |_| {
                messages.update(|messages| {
                    messages.remove(i);
                });
            })
            .on_secondary_click_stop({
                let message = message.message.clone();
                move |_| {
                    let mut clipboard = SystemClipboard::new();
                    if !message.is_empty() {
                        clipboard.put_string(&message);
                    }
                }
            })
            .on_event_stop(EventListener::PointerDown, |_| {})
            .style(move |s| {
                let config = config.get();
                s.width_full()
                    .items_start()
                    .padding(10.0)
                    .border(1.0)
                    .border_radius(6.0)
                    .border_color(config.color(SparrowColor::SPARROW_BORDER))
                    .background(config.color(SparrowColor::PANEL_BACKGROUND))
                    .apply_if(i > 0, |s| s.margin_top(10.0))
            })
        };

    let id = AtomicU64::new(0);
    container(
        container(
            container(
                sparrow_scroll(
                    dyn_stack(
                        move || messages.get().into_iter().enumerate(),
                        move |_| {
                            id.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                        },
                        view_fn,
                    )
                    .style(|s| s.flex_col().width_full()),
                )
                .style(|s| {
                    s.absolute()
                        .pointer_events_auto()
                        .width_full()
                        .min_height(0.0)
                        .max_height_full()
                        .set(PropagatePointerWheel, false)
                }),
            )
            .style(|s| s.size_full()),
        )
        .style(|s| {
            s.width(360.0)
                .max_width_pct(80.0)
                .padding(10.0)
                .height_full()
        }),
    )
    .style(|s| s.absolute().size_full().justify_end().pointer_events_none())
    .debug_name("Window Message View")
}
