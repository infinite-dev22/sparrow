use std::sync::Arc;

use floem::{
    View,
    event::{Event, EventListener, EventPropagation},
    reactive::{ReadSignal, SignalGet, SignalUpdate, create_rw_signal, use_context},
    style::{CursorStyle, FlexWrap, Style},
    views::{Decorators, empty, stack},
};

use crate::{
    config::{
        SparrowConfig, color::SparrowColor, color_theme::ThemeColorPreference,
    },
    db::SparrowDb,
    widgets::{Divider, DividerVariant, dialog_surface},
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
    let clamp_width = sidebar_width_clamp(rule_width);
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

fn sidebar_width_clamp(rule_width: f64) -> impl Fn(f64, f64) -> f64 + Copy {
    let [input_minimum, _] = tokens::control_minimum_size("TextField.minimumSize");
    let [icon_width, _] = tokens::size("search", "search_icon_grid");
    let [button_width, _] = tokens::control_minimum_size("Button.minimumSize");
    let [_, left, _, right] =
        tokens::insets("navigation", "search_container_insets");
    let gap = tokens::length("search", "icon_gap");
    let border = tokens::length("search", "inner_border_width");
    // Keep room for the query after Clear appears. Two gaps and two padding edges
    // use the same spacing in the search frame.
    let sidebar_minimum = input_minimum
        + icon_width
        + search_control_height()
        + 4.0 * gap
        + 2.0 * border
        + left
        + right;
    move |desired, available| {
        let maximum = (available - button_width - rule_width).max(0.0);
        let minimum = sidebar_minimum.min(maximum);
        desired.clamp(minimum, maximum)
    }
}

pub(super) fn search_control_height() -> f64 {
    let [_, height] = tokens::control_minimum_size("TextField.minimumSize");
    height
}

pub(super) fn search_field(style: Style) -> Style {
    let gap = tokens::length("search", "icon_gap");
    style
        .height(search_control_height())
        .padding_horiz(gap)
        .gap(gap)
        .border_radius(tokens::length("search", "inner_outline_radius_at_1x"))
}

pub(super) fn search_focus(style: Style, config: &SparrowConfig) -> Style {
    use ThemeColorPreference::*;
    let (width, color) = match config.color.color_preference {
        Dark => (
            tokens::length("dark", "Component.focusWidth"),
            tokens::color("dark", "Component.focusColor"),
        ),
        Light => (
            tokens::length("light", "Component.focusWidth"),
            tokens::color("light", "Component.focusColor"),
        ),
        HighContrastDark | HighContrastLight => (
            tokens::length("search", "focus_width_fallback"),
            config.color(SparrowColor::EDITOR_CARET),
        ),
    };
    style.outline(width).outline_color(color)
}

pub(super) fn footer_buttons(style: Style) -> Style {
    tokens::padding(style, "footer", "compact_footer_insets")
        .width_full()
        .items_center()
        .gap(tokens::length("footer", "button_base_gap"))
        .flex_wrap(FlexWrap::Wrap)
}

pub(super) fn button_minimum(style: Style) -> Style {
    let [width, height] = tokens::control_minimum_size("Button.minimumSize");
    style.min_width(width).min_height(height)
}

pub(super) fn dialog_card(style: Style, config: &SparrowConfig) -> Style {
    let [width, height] = tokens::size("dialog", "root_minimum_size");
    // Sparrow uses the source minimum as the preferred size of its overlay.
    dialog_surface(style, config)
        .font_family(config.ui.font_family.clone())
        .font_size(13.0)
        .width(width)
        .height(height)
        .max_width_pct(96.0)
        .max_height_pct(92.0)
        .min_height(0.0)
        .flex_col()
        .border_radius(crate::widgets::dialog_frame::DIALOG_RADIUS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use floem::{
        peniko::{Brush, Color},
        style::{
            BorderBottomLeftRadius, BorderBottomRightRadius, BorderTopLeftRadius,
            BorderTopRightRadius, ColGap, Height, MaxHeight, MaxWidth, MinHeight,
            MinWidth, Outline, OutlineColor, PaddingBottom, PaddingLeft,
            PaddingRight, PaddingTop, RowGap, Width,
        },
        unit::{PxPct, PxPctAuto},
    };

    #[test]
    fn settings_layout_card_uses_verified_size_as_preferred_overlay_size() {
        let style = dialog_card(Style::new(), &SparrowConfig::default());
        assert_eq!(style.get(Width), PxPctAuto::Px(900.0));
        assert_eq!(style.get(Height), PxPctAuto::Px(700.0));
        // Sparrow's workbench keeps the entire overlay inside smaller windows.
        assert_eq!(style.get(MaxWidth), PxPctAuto::Pct(96.0));
        assert_eq!(style.get(MaxHeight), PxPctAuto::Pct(92.0));
    }

    #[test]
    fn settings_layout_search_uses_declared_minimum_height_insets_gap_and_radius() {
        let field = search_field(Style::new());
        assert_eq!(field.get(Height), PxPctAuto::Px(28.0));
        assert_eq!(field.get(ColGap), PxPct::Px(2.0));
        assert_eq!(field.get(PaddingLeft), PxPct::Px(2.0));
        assert_eq!(field.get(PaddingRight), PxPct::Px(2.0));
        assert_eq!(field.get(BorderTopLeftRadius), PxPct::Px(4.0));
        assert_eq!(field.get(BorderTopRightRadius), PxPct::Px(4.0));
        assert_eq!(field.get(BorderBottomLeftRadius), PxPct::Px(4.0));
        assert_eq!(field.get(BorderBottomRightRadius), PxPct::Px(4.0));
        assert_eq!(field.get(Outline).0.width, 0.0);

        for (inset, expected) in [
            ("search_container_insets", [7.0, 5.0, 6.0, 5.0]),
            ("banner_insets", [11.0, 6.0, 0.0, 10.0]),
            ("history_toolbar_insets", [8.0, 2.0, 0.0, 0.0]),
        ] {
            assert_eq!(
                padding_edges(&tokens::padding(Style::new(), "navigation", inset)),
                expected.map(PxPct::Px),
                "{inset}",
            );
        }
    }

    #[test]
    fn settings_layout_search_focus_uses_the_active_theme() {
        let mut config = SparrowConfig::default();
        for preference in [ThemeColorPreference::Dark, ThemeColorPreference::Light] {
            config.color.color_preference = preference.clone();
            let focused = search_focus(search_field(Style::new()), &config);
            assert_eq!(focused.get(Outline).0.width, 2.0, "{preference:?}");
            assert_eq!(
                focused.get(OutlineColor),
                Brush::Solid(Color::from_rgb8(0x38, 0x71, 0xE1)),
                "{preference:?}",
            );
            assert_eq!(focused.get(Height), PxPctAuto::Px(28.0));
        }

        config.color.color_preference = ThemeColorPreference::HighContrastDark;
        let caret = Color::from_rgb8(0xFF, 0xFF, 0xFF);
        config
            .color
            .ui
            .insert(SparrowColor::EDITOR_CARET.into(), caret);
        let focused = search_focus(Style::new(), &config);
        assert_eq!(focused.get(Outline).0.width, 2.0);
        assert_eq!(focused.get(OutlineColor), Brush::Solid(caret));
    }

    #[test]
    fn settings_layout_footer_uses_verified_insets_gap_and_button_minimum() {
        let footer = footer_buttons(Style::new());
        assert_eq!(
            padding_edges(&footer),
            [8.0, 12.0, 8.0, 12.0].map(PxPct::Px)
        );
        assert_eq!(footer.get(ColGap), PxPct::Px(12.0));
        assert_eq!(footer.get(RowGap), PxPct::Px(12.0));

        let button = button_minimum(Style::new());
        assert_eq!(button.get(MinWidth), PxPctAuto::Px(72.0));
        assert_eq!(button.get(MinHeight), PxPctAuto::Px(28.0));
        assert_eq!(tokens::text("light", "Button.minimumSize"), "72,28");
        assert_eq!(tokens::text("light", "TextField.minimumSize"), "49,28");
    }

    #[test]
    fn settings_layout_split_preserves_default_and_clamps_dragging() {
        let proportion =
            tokens::proportion("navigation", "split_default_proportion");
        let rule_width = tokens::length("navigation", "split_divider_width");
        let clamp = sidebar_width_clamp(rule_width);

        assert_eq!(clamp(900.0 * proportion, 900.0), 180.0);
        assert_eq!(clamp(0.0, 900.0), 113.0);
        assert_eq!(clamp(60.0, 305.0), 113.0);
        assert_eq!(clamp(900.0, 900.0), 827.0);
        assert_eq!(clamp(18.0, 90.0), 17.0);
        assert_eq!(clamp(0.0, 0.0), 0.0);
    }

    fn padding_edges(style: &Style) -> [PxPct; 4] {
        [
            style.get(PaddingTop),
            style.get(PaddingLeft),
            style.get(PaddingBottom),
            style.get(PaddingRight),
        ]
    }
}
