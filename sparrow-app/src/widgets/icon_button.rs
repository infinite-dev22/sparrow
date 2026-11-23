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

/// Overrides for an icon button. The default target adds five pixels of
/// clearance around the configured icon on each side.
#[derive(Clone, Copy, Debug, Default)]
pub struct IconButtonStyle {
    pub size: Option<f64>,
    pub background: Option<Color>,
    pub foreground: Option<Color>,
    pub radius: Option<f64>,
    pub icon_size: Option<f64>,
}

pub fn icon_button<S: std::fmt::Display + 'static>(
    icon: impl Fn() -> &'static str + 'static,
    tooltip: impl Fn() -> S + 'static + Clone,
    config: ReadSignal<Arc<SparrowConfig>>,
    appearance: impl Fn() -> IconButtonStyle + Clone + 'static,
) -> impl View {
    tooltip_label(
        config,
        icon_button_view(icon, || false, || false, config, appearance),
        tooltip,
    )
}

fn contrasting_icon(foreground: Color, background: Color) -> Color {
    let luminance = background.discard_alpha().relative_luminance();
    let foreground_luminance = foreground.discard_alpha().relative_luminance();
    if (luminance.max(foreground_luminance) + 0.05)
        / (luminance.min(foreground_luminance) + 0.05)
        >= 3.0
    {
        foreground
    } else if luminance < 0.179 {
        Color::WHITE
    } else {
        Color::BLACK
    }
}

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
    let view = icon_button_view(
        icon,
        active_fn,
        disabled_fn,
        config,
        IconButtonStyle::default,
    );
    if let Some(on_click) = on_click {
        view.on_click_stop(move |_| on_click())
    } else {
        view
    }
}

fn icon_button_view(
    icon: impl Fn() -> &'static str + 'static,
    active: impl Fn() -> bool + 'static,
    disabled: impl Fn() -> bool + Copy + 'static,
    config: ReadSignal<Arc<SparrowConfig>>,
    appearance: impl Fn() -> IconButtonStyle + Clone + 'static,
) -> impl View {
    let icon_appearance = appearance.clone();
    container(
        svg(move || config.get().ui_svg(icon()))
            .style(move |s| {
                let config = config.get();
                let appearance = icon_appearance();
                let size =
                    appearance.icon_size.unwrap_or(config.ui.icon_size() as f64);
                let foreground = appearance.foreground.unwrap_or_else(|| {
                    contrasting_icon(
                        config.color(SparrowColor::SPARROW_ICON_ACTIVE),
                        appearance
                            .background
                            .filter(|background| background.components[3] > 0.0)
                            .unwrap_or_else(|| {
                                config.color(SparrowColor::PANEL_BACKGROUND)
                            }),
                    )
                });
                s.size(size, size)
                    .flex_shrink(0.0_f32)
                    .color(foreground)
                    .pointer_events_none()
                    .disabled(|s| {
                        s.color(config.color(SparrowColor::SPARROW_ICON_INACTIVE))
                    })
            })
            .disabled(disabled),
    )
    .disabled(disabled)
    .style(move |s| {
        let config = config.get();
        let appearance = appearance();
        let icon_size = appearance.icon_size.unwrap_or(config.ui.icon_size() as f64);
        let size = appearance.size.unwrap_or(icon_size + 10.0);
        s.size(size, size)
            .flex_shrink(0.0_f32)
            .items_center()
            .justify_center()
            .border_radius(appearance.radius.unwrap_or(4.0))
            .background(appearance.background.unwrap_or(Color::TRANSPARENT))
            .border(1.0)
            .border_color(Color::TRANSPARENT)
            .apply_if(active(), |s| {
                s.border_color(config.color(SparrowColor::EDITOR_CARET))
            })
            .apply_if(!disabled(), |s| {
                s.hover(|s| {
                    s.cursor(CursorStyle::Pointer).background(
                        appearance.background.unwrap_or_else(|| {
                            config.color(SparrowColor::PANEL_HOVERED_BACKGROUND)
                        }),
                    )
                })
                .active(|s| {
                    s.background(appearance.background.unwrap_or_else(|| {
                        config.color(SparrowColor::PANEL_HOVERED_ACTIVE_BACKGROUND)
                    }))
                })
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn low_contrast_icons_remain_visible_on_light_and_dark_surfaces() {
        let dark = Color::from_rgb8(26, 27, 29);
        let light = Color::from_rgb8(244, 245, 247);
        assert_eq!(contrasting_icon(Color::BLACK, dark), Color::WHITE);
        assert_eq!(contrasting_icon(Color::WHITE, light), Color::BLACK);
        let muted = Color::from_rgb8(154, 158, 164);
        assert_eq!(contrasting_icon(muted, dark), muted);
    }
}
