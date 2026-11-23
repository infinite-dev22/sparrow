//! Dart HTTP requests captured on demand.

use floem::{
    View,
    reactive::{RwSignal, SignalGet, SignalUpdate, SignalWith, create_rw_signal},
    views::{Decorators, dyn_stack, label, stack},
};

use super::{Config, card, heading, secondary};
use crate::{
    config::color::SparrowColor,
    dart::{
        InspectorState,
        performance::{PerformanceData, Sample},
        vm_service::VmServiceClient,
    },
    widgets::{ButtonKind, button, text_input},
};

pub(super) fn view(
    data: PerformanceData,
    client: VmServiceClient,
    state: RwSignal<InspectorState>,
    config: Config,
) -> impl View {
    let filter = create_rw_signal(String::new());
    let selected = create_rw_signal(None::<String>);
    let capture = client.clone();
    let busy =
        move || state.get() != InspectorState::Connected || data.network_busy.get();
    let rows = dyn_stack(
        move || {
            let text = filter.get().to_lowercase();
            data.network.with(|rows| {
                rows.iter()
                    .filter(|row| {
                        text.is_empty()
                            || format!(
                                "{} {} {}",
                                row.method,
                                row.url,
                                row.status.map_or(String::new(), |value| value
                                    .to_string())
                            )
                            .to_lowercase()
                            .contains(&text)
                    })
                    .cloned()
                    .collect::<Vec<_>>()
            })
        },
        |row| row.clone(),
        move |row| {
            let id = row.id.clone();
            let active = row.id.clone();
            let title = format!("{}  {}", row.method, row.url);
            let result = match (row.status, row.duration_us) {
                (Some(status), Some(us)) => {
                    format!("HTTP {status}  ·  {:.1} ms", us as f64 / 1000.0)
                }
                (Some(status), None) => format!("HTTP {status}  ·  In progress"),
                (None, Some(us)) => {
                    format!("No response  ·  {:.1} ms", us as f64 / 1000.0)
                }
                _ => "In progress".into(),
            };
            crate::widgets::button_with_content(
                stack((
                    label(move || title.clone())
                        .style(|s| s.text_ellipsis().selectable(false)),
                    label(move || result.clone()).style(move |s| {
                        s.font_size(10.0).color(
                            config.get().color(SparrowColor::PANEL_FOREGROUND_DIM),
                        )
                    }),
                ))
                .style(|s| s.flex_col().width_full().min_width(0).gap(2.0)),
                ButtonKind::Plain,
                config,
            )
            .on_click_stop(move |_| selected.set(Some(id.clone())))
            .style(move |s| {
                s.width_full().min_width(0).padding(6.0).apply_if(
                    selected.with(|value| value.as_ref() == Some(&active)),
                    |s| {
                        s.background(
                            config
                                .get()
                                .color(SparrowColor::PANEL_SELECTION_BACKGROUND),
                        )
                    },
                )
            })
        },
    )
    .style(|s| s.flex_col().width_full().gap(2.0).min_width(0));
    let detail = label(move || {
        selected.with(|id| {
            data.network.with(|rows| {
                rows.iter()
                    .find(|row| Some(&row.id) == id.as_ref())
                    .map_or(String::new(), |row| {
                        format!("{} {}", row.method, row.url)
                    })
            })
        })
    })
    .style(move |s| {
        s.selectable(true)
            .text_overflow(floem::style::TextOverflow::Wrap)
            .font_size(11.0)
            .color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
    });
    stack((
        heading("HTTP activity", config),
        secondary(
            || "Capture Dart HTTP traffic from the connected app.",
            config,
        ),
        stack((
            button(
                move || {
                    if data.recording.get() {
                        "Stop capture"
                    } else {
                        "Start capture"
                    }
                },
                ButtonKind::Outlined,
                config,
            )
            .on_click_stop(move |_| {
                if capture.sample_performance(Sample::Recording(
                    !data.recording.get_untracked(),
                )) {
                    data.network_busy.set(true);
                }
            })
            .disabled(busy),
            button(|| "Clear", ButtonKind::Plain, config)
                .on_click_stop(move |_| {
                    if client.sample_performance(Sample::ClearNetwork) {
                        data.network_busy.set(true);
                        selected.set(None);
                    }
                })
                .disabled(move || busy() || data.network.with(Vec::is_empty)),
        ))
        .style(|s| s.items_center().gap(4.0)),
        text_input(filter, config)
            .placeholder("Filter by method, URL or status")
            .style(|s| s.width_full()),
        label(move || {
            data.network_error.get().unwrap_or_else(|| {
                if data.network.with(Vec::is_empty) {
                    if data.recording.get() {
                        "Recording. Make an HTTP request in the app.".into()
                    } else {
                        "Start capture to record requests.".into()
                    }
                } else {
                    data.network
                        .with(|rows| format!("{} captured requests", rows.len()))
                }
            })
        })
        .style(|s| {
            s.font_size(11.0)
                .text_overflow(floem::style::TextOverflow::Wrap)
        }),
        card(
            stack((detail, rows)).style(|s| s.flex_col().gap(4.0).width_full()),
            config,
        ),
    ))
    .style(|s| s.flex_col().gap(9.0).width_full())
}
