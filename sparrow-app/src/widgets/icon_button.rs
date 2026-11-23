use std::sync::Arc;

use floem::{
    View,
    peniko::Color,
    reactive::{ReadSignal, SignalGet},
    style::CursorStyle,
    views::{Decorators, container, svg},
};

use crate::config::{SparrowConfig, color::SparrowColor};

use super::tooltip_label;

pub fn not_clickable_icon<S: std::fmt::Display + 'static>(
    icon: impl Fn() -> &'static str + 'static,
    active_fn: impl Fn() -> bool + 'static,
    disabled_fn: impl Fn() -> bool + 'static + Copy,
    tooltip_: impl Fn() -> S + 'static + Clone,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    tooltip_label(
        config,
        clickable_icon_base(
            icon,
            None::<Box<dyn Fn()>>,
            active_fn,
            disabled_fn,
            config,
        ),
        tooltip_,
    )
    .debug_name("Not Clickable Icon")
}

pub fn clickable_icon<S: std::fmt::Display + 'static>(
    icon: impl Fn() -> &'static str + 'static,
    on_click: impl Fn() + 'static,
    active_fn: impl Fn() -> bool + 'static,
    disabled_fn: impl Fn() -> bool + 'static + Copy,
    tooltip_: impl Fn() -> S + 'static + Clone,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    tooltip_label(
        config,
        clickable_icon_base(icon, Some(on_click), active_fn, disabled_fn, config),
        tooltip_,
    )
}

pub fn clickable_icon_base(
    icon: impl Fn() -> &'static str + 'static,
    on_click: Option<impl Fn() + 'static>,
    active_fn: impl Fn() -> bool + 'static,
    disabled_fn: impl Fn() -> bool + 'static + Copy,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    let view = container(
        svg(move || config.get().ui_svg(icon()))
            .style(move |s| {
                let config = config.get();
                let size = config.ui.icon_size() as f32;
                s.size(size, size)
                    .color(config.color(SparrowColor::SPARROW_ICON_ACTIVE))
                    .disabled(|s| {
                        s.color(config.color(SparrowColor::SPARROW_ICON_INACTIVE))
                            .cursor(CursorStyle::Default)
                    })
            })
            .disabled(disabled_fn),
    )
    .disabled(disabled_fn)
    .style(move |s| {
        let config = config.get();
        s.padding(4.0)
            .items_center()
            .justify_center()
            .border_radius(6.0)
            .border(1.0)
            .border_color(Color::TRANSPARENT)
            .apply_if(active_fn(), |s| {
                s.border_color(config.color(SparrowColor::EDITOR_CARET))
            })
            .hover(|s| {
                s.cursor(CursorStyle::Pointer)
                    .background(config.color(SparrowColor::PANEL_HOVERED_BACKGROUND))
            })
            .active(|s| {
                s.background(
                    config.color(SparrowColor::PANEL_HOVERED_ACTIVE_BACKGROUND),
                )
            })
    });

    if let Some(on_click) = on_click {
        view.on_click_stop(move |_| {
            on_click();
        })
    } else {
        view
    }
}
