//! Theme colors for diagnostic labels, counts, and status indicators.
use std::sync::Arc;

use floem::{
    View,
    peniko::{Color, color::palette},
    reactive::{ReadSignal, SignalGet},
    views::{Decorators, dyn_stack, label, stack, svg},
};

use crate::config::{SparrowConfig, color::SparrowColor, icon::SparrowIcons};

use super::{Issue, rank};

const SEVERITIES: [(&str, &str, &str); 4] = [
    ("Error", SparrowColor::SPARROW_ERROR, SparrowIcons::ERROR),
    ("Warning", SparrowColor::SPARROW_WARN, SparrowIcons::WARNING),
    ("Info", SparrowColor::EDITOR_LINK, SparrowIcons::INFO),
    (
        "Hint",
        SparrowColor::PANEL_FOREGROUND_DIM,
        SparrowIcons::LIGHTBULB,
    ),
];

pub(super) fn severity_name(rank: usize) -> &'static str {
    SEVERITIES[rank].0
}

pub(super) fn severity_color(config: &SparrowConfig, rank: usize) -> Color {
    config.color(SEVERITIES[rank].1)
}

pub(super) fn severity_icon(rank: usize) -> &'static str {
    SEVERITIES[rank].2
}

pub(super) fn counts(issues: &[Issue]) -> [usize; 4] {
    let mut counts = [0; 4];
    for issue in issues {
        counts[rank(&issue.diagnostic)] += 1;
    }
    counts
}

pub(super) fn surface(config: &SparrowConfig) -> Color {
    config
        .color(SparrowColor::PANEL_BACKGROUND)
        .lerp_rect(config.color(SparrowColor::PANEL_FOREGROUND), 0.045)
}

pub(super) fn badge(
    config: ReadSignal<Arc<SparrowConfig>>,
    text: String,
    severity: usize,
) -> impl View {
    stack((
        svg(move || config.get().ui_svg(severity_icon(severity)))
            .style(|s| s.size(10.0, 10.0).flex_shrink(0.0_f32)),
        label(move || text.clone()).style(|s| s.selectable(false)),
    ))
    .style(move |s| {
        let config = config.get();
        let tone = severity_color(&config, severity);
        let background = surface(&config).lerp_rect(tone, 0.14);
        s.background(background)
            .color(readable(tone, background))
            .font_size(12.0)
            .font_weight(floem::text::Weight(550))
            .padding_horiz(4.0)
            .gap(3.0)
            .height(18.0)
            .items_center()
            .border_radius(4.0)
            .flex_shrink(0.0_f32)
    })
}

pub(super) fn badges(
    config: ReadSignal<Arc<SparrowConfig>>,
    counts: impl Fn() -> [usize; 4] + 'static,
    compact: bool,
) -> impl View {
    dyn_stack(
        move || {
            counts()
                .into_iter()
                .enumerate()
                .filter(|(_, count)| *count != 0)
        },
        |item| *item,
        move |(severity, count)| {
            let name = match (severity, count) {
                (0, 1) => "error",
                (0, _) => "errors",
                (1, 1) => "warning",
                (1, _) => "warnings",
                (2, _) => "info",
                (_, 1) => "hint",
                _ => "hints",
            };
            let text = format!("{count} {name}");
            let caption = if compact {
                count.to_string()
            } else {
                text.clone()
            };
            crate::widgets::tooltip_label(
                config,
                badge(config, caption, severity),
                move || text.clone(),
            )
        },
    )
    .style(move |s| {
        s.items_center()
            .gap(5.0)
            .min_width(0.0)
            .apply_if(!compact, |s| s.flex_wrap(floem::style::FlexWrap::Wrap))
    })
}

/// Keep small text readable even with bright warning colors in light themes.
pub(super) fn readable(foreground: Color, background: Color) -> Color {
    if contrast(foreground, background) >= 4.5 {
        return foreground;
    }
    let black = palette::css::BLACK;
    let white = palette::css::WHITE;
    let target = if contrast(black, background) >= contrast(white, background) {
        black
    } else {
        white
    };
    for step in 1..=10 {
        let candidate = foreground.lerp_rect(target, step as f32 / 10.0);
        if contrast(candidate, background) >= 4.5 {
            return candidate;
        }
    }
    target
}

fn contrast(a: Color, b: Color) -> f32 {
    let a = a.discard_alpha().relative_luminance();
    let b = b.discard_alpha().relative_luminance();
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_remains_readable_on_light_dark_and_selection_surfaces() {
        for surface in [
            Color::from_rgb8(250, 250, 250),
            Color::from_rgb8(26, 28, 31),
            Color::from_rgb8(42, 67, 113),
        ] {
            for tone in [
                Color::from_rgb8(233, 167, 0),
                Color::from_rgb8(229, 20, 0),
                Color::from_rgb8(84, 138, 247),
            ] {
                let fill = surface.lerp_rect(tone, 0.14);
                assert!(contrast(readable(tone, fill), fill) >= 4.5);
            }
        }
    }
}
