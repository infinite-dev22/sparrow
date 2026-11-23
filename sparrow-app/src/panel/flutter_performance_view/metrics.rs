//! Frame timing and isolate memory summaries.

use floem::{
    View,
    reactive::{SignalGet, SignalUpdate, SignalWith},
    views::{Decorators, label, stack},
};

use super::{Config, card, charts::plot, heading, secondary};
use crate::{
    config::color::SparrowColor,
    dart::performance::PerformanceData,
    widgets::{ButtonKind, button},
};

const FRAME_BUDGET: f64 = 1000.0 / 60.0;

pub(super) fn frames(data: PerformanceData, config: Config) -> impl View {
    let stats = move |raster| {
        data.frames.with(|frames| {
            if frames.is_empty() {
                return "—".into();
            }
            let total: f64 = frames
                .iter()
                .map(|f| if raster { f.raster_ms } else { f.build_ms })
                .sum();
            format!("{:.1} ms", total / frames.len() as f64)
        })
    };
    let chart = plot(
        move |track| {
            if track {
                data.frames.with(|_| ());
                return Vec::new();
            }
            data.frames.with_untracked(|frames| {
                frames.iter().map(|f| [f.build_ms, f.raster_ms]).collect()
            })
        },
        Some(FRAME_BUDGET),
        config,
    );
    stack((
        stack((
            heading("Frame timing", config).style(|s| s.flex_grow(1.0_f32)),
            button(move || if data.paused.get() { "Resume" } else { "Pause" }, ButtonKind::Outlined, config)
                .on_click_stop(move |_| data.paused.update(|paused| *paused = !*paused)),
            button(|| "Clear", ButtonKind::Plain, config)
                .on_click_stop(move |_| data.frames.update(|frames| frames.clear())),
        )).style(|s| s.items_center().gap(4.0).flex_wrap(floem::style::FlexWrap::Wrap)),
        secondary(|| "Reference budget  ·  16.7 ms at 60 Hz", config),
        stack((
            metric("UI build average", move || stats(false), config),
            metric("Raster average", move || stats(true), config),
        )).style(|s| s.gap(6.0).width_full()),
        card(stack((
            chart,
            legend("UI build", "Raster", config),
            label(move || data.frames.with(|frames| {
                let Some(last) = frames.back() else {
                    return "Waiting for rendered frames. Interact with the app to generate a sample.".into();
                };
                let over = frames.iter().filter(|f| f.build_ms > FRAME_BUDGET || f.raster_ms > FRAME_BUDGET).count();
                format!("{over} of {} over budget · Latest #{}: {:.1} ms total", frames.len(), last.number, last.elapsed_ms)
            })).style(|s| s.font_size(11.0).text_overflow(floem::style::TextOverflow::Wrap).min_width(0)),
        )).style(|s| s.flex_col().gap(6.0).width_full()), config),
    )).style(|s| s.flex_col().gap(9.0).width_full())
}

pub(super) fn memory(data: PerformanceData, config: Config) -> impl View {
    let current = move |field: fn(crate::dart::performance::Memory) -> u64| {
        data.memory.with(|samples| {
            samples
                .back()
                .copied()
                .map_or("—".into(), |m| bytes(field(m)))
        })
    };
    let chart = plot(
        move |track| {
            if track {
                data.memory.with(|_| ());
                return Vec::new();
            }
            data.memory.with_untracked(|samples| {
                samples
                    .iter()
                    .map(|m| [m.heap as f64, m.external as f64])
                    .collect()
            })
        },
        None,
        config,
    );
    stack((
        heading("Isolate memory", config),
        secondary(|| "Samples once per second while this tab is open.", config),
        stack((
            metric("Heap used", move || current(|m| m.heap), config),
            metric("Heap capacity", move || current(|m| m.capacity), config),
        ))
        .style(|s| s.gap(6.0).width_full()),
        metric("External memory", move || current(|m| m.external), config),
        card(
            stack((chart, legend("Heap used", "External", config)))
                .style(|s| s.flex_col().gap(6.0).width_full()),
            config,
        ),
        label(move || {
            data.memory_error.get().unwrap_or_else(|| {
                if data.memory.with(|samples| samples.is_empty()) {
                    "Waiting for memory samples…".into()
                } else {
                    String::new()
                }
            })
        })
        .style(|s| {
            s.font_size(11.0)
                .text_overflow(floem::style::TextOverflow::Wrap)
        }),
    ))
    .style(|s| s.flex_col().gap(9.0).width_full())
}

fn bytes(bytes: u64) -> String {
    format!("{:.1} MiB", bytes as f64 / 1_048_576.0)
}

fn metric(
    title: &'static str,
    value: impl Fn() -> String + 'static,
    config: Config,
) -> impl View {
    card(
        stack((
            secondary(move || title, config),
            label(value).style(move |s| {
                s.font_size(18.0)
                    .font_bold()
                    .text_ellipsis()
                    .color(config.get().color(SparrowColor::EDITOR_LINK))
            }),
        ))
        .style(|s| s.flex_col().gap(4.0).width_full()),
        config,
    )
    .style(|s| s.flex_basis(0).flex_grow(1.0_f32).min_width(0))
}

fn legend(left: &'static str, right: &'static str, config: Config) -> impl View {
    let item = |text, color| {
        stack((
            floem::views::empty().style(move |s| {
                s.size(7, 7)
                    .border_radius(4)
                    .background(config.get().color(color))
            }),
            label(move || text).style(|s| s.font_size(10.0)),
        ))
        .style(|s| s.items_center().gap(5.0))
    };
    stack((
        item(left, SparrowColor::EDITOR_LINK),
        item(right, SparrowColor::SOURCE_CONTROL_ADDED),
    ))
    .style(|s| s.gap(12.0).flex_wrap(floem::style::FlexWrap::Wrap))
}
