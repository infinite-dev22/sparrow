use std::sync::Arc;

use floem::{
    View,
    event::{Event, EventListener, EventPropagation},
    reactive::{ReadSignal, SignalGet, SignalUpdate, create_rw_signal, use_context},
    style::CursorStyle,
    views::{Decorators, empty, stack},
};

use crate::{
    config::{SparrowConfig, color::SparrowColor},
    db::SparrowDb,
    widgets::{Divider, DividerVariant},
};

use super::tokens;

pub(super) fn settings_split(
    sidebar: impl View + 'static,
    content: impl View + 'static,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    let db = use_context::<Arc<SparrowDb>>();
    let default_proportion =
        tokens::proportion("navigation", "split_default_proportion");
    let initial = db
        .as_ref()
        .and_then(|db| db.settings_sidebar_proportion().ok())
        .unwrap_or(default_proportion);
    let proportion = create_rw_signal(initial);
    let available = create_rw_signal(0.0_f64);
    let anchor = create_rw_signal(None::<(f64, f64)>);
    let rule_width = tokens::length("navigation", "split_divider_width");
    let [field_minimum, _] = tokens::control_minimum_size("TextField.minimumSize");
    let [icon_width, _] = tokens::size("search", "search_icon_grid");
    let [button_width, _] = tokens::control_minimum_size("Button.minimumSize");
    let icon_gap = tokens::length("search", "icon_gap");
    let sidebar_minimum = field_minimum + icon_width * 2.0 + icon_gap * 2.0;

    let clamp_width = move |desired: f64, width: f64| {
        let minimum = sidebar_minimum.min(width * default_proportion);
        let maximum = (width - button_width - rule_width).max(minimum);
        desired.clamp(minimum, maximum)
    };
    let width = move || {
        let available = available.get();
        clamp_width(available * proportion.get(), available)
    };
    let handle = empty();
    let id = handle.id();
    let finish = move || {
        if anchor.get_untracked().is_some() {
            anchor.set(None);
            id.clear_active();
            if let Some(db) = &db {
                db.save_settings_sidebar_proportion(proportion.get_untracked());
            }
        }
    };
    let handle = handle
        .on_event(EventListener::PointerDown, move |event| {
            if let Event::PointerDown(pointer) = event
                && pointer.button.is_primary()
            {
                anchor.set(Some((id.layout_rect().x0 + pointer.pos.x, width())));
                id.request_active();
                return EventPropagation::Stop;
            }
            EventPropagation::Continue
        })
        .on_event(EventListener::PointerMove, move |event| {
            if let Event::PointerMove(pointer) = event
                && let Some((start_x, start_width)) = anchor.get_untracked()
            {
                let total = available.get_untracked();
                if total > 0.0 {
                    let width = clamp_width(
                        start_width + id.layout_rect().x0 + pointer.pos.x - start_x,
                        total,
                    );
                    proportion.set(width / total);
                }
                return EventPropagation::Stop;
            }
            EventPropagation::Continue
        })
        .on_event(EventListener::PointerUp, {
            let finish = finish.clone();
            move |event| {
                if let Event::PointerUp(pointer) = event
                    && pointer.button.is_primary()
                    && anchor.get_untracked().is_some()
                {
                    finish();
                    return EventPropagation::Stop;
                }
                EventPropagation::Continue
            }
        })
        .on_event_cont(EventListener::WindowLostFocus, move |_| finish())
        .style(move |s| {
            // The drag target uses the same 4 px hit width as the dock handle.
            s.absolute()
                .inset_left(width() - 2.0)
                .width(4.0)
                .height_full()
                .cursor(CursorStyle::ColResize)
                .z_index(1)
        })
        .debug_name("Settings sidebar resize handle");

    stack((
        sidebar.style(move |s| {
            s.width(width())
                .height_full()
                .min_width(0)
                .flex_shrink(0.0_f32)
        }),
        Divider::new(config)
            .variant(DividerVariant::Vertical)
            .thickness(rule_width as f32)
            .into_view(),
        content.style(|s| {
            s.height_full()
                .flex_grow(1.0_f32)
                .flex_basis(0)
                .min_width(0)
        }),
        handle,
    ))
    .style(|s| {
        s.width_full()
            .flex_grow(1.0_f32)
            .flex_basis(0)
            .min_height(0)
    })
    .on_resize(move |rect| available.set(rect.width()))
}

pub(super) fn sidebar_background(config: &SparrowConfig) -> floem::peniko::Color {
    config.color(SparrowColor::SPARROW_WINDOW_BACKGROUND)
}
