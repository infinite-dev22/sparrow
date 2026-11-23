//! Native Flutter profiling dashboard.

use std::{rc::Rc, sync::Arc, time::Duration};

use floem::{
    View,
    action::exec_after,
    reactive::{
        ReadSignal, RwSignal, SignalGet, SignalUpdate, SignalWith, create_rw_signal,
    },
    views::{Decorators, dyn_container, label, stack, stack_from_iter},
};

use super::{kind::PanelKind, position::PanelPosition};
use crate::{
    config::{SparrowConfig, color::SparrowColor, icon::SparrowIcons},
    dart::{InspectorState, performance::Sample},
    widgets::{
        clickable_icon, panel_title_bar, sparrow_scroll, tab_strip, toggle_row,
    },
    window_tab::WindowTabData,
};

mod charts;
mod metrics;
mod network;

type Config = ReadSignal<Arc<SparrowConfig>>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Frames,
    Memory,
    Network,
    Diagnostics,
}

pub fn flutter_performance_panel(
    window: Rc<WindowTabData>,
    _position: PanelPosition,
) -> impl View {
    let inspector = window.flutter_inspector.clone();
    let config = window.common.config;
    let state = inspector.state;
    let page = create_rw_signal(Page::Frames);
    let data = inspector.performance;
    let panel = window.panel.clone();
    poll(window.clone(), page);

    let header = panel_title_bar(
        "Flutter Performance",
        floem::views::empty(),
        clickable_icon(
            || SparrowIcons::WINDOW_MINIMIZE,
            move || panel.hide_panel(&PanelKind::FlutterPerformance),
            || false,
            || false,
            || "Hide Flutter Performance",
            config,
        ),
    )
    .style(move |s| {
        s.border_bottom(1.0)
            .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
    });
    let status = stack((
        floem::views::empty().style(move |s| {
            s.size(7.0, 7.0)
                .border_radius(4.0)
                .flex_shrink(0.0_f32)
                .background(config.get().color(match state.get() {
                    InspectorState::Connected => SparrowColor::SOURCE_CONTROL_ADDED,
                    InspectorState::Connecting => SparrowColor::SPARROW_WARN,
                    InspectorState::Disconnected => {
                        SparrowColor::PANEL_FOREGROUND_DIM
                    }
                }))
        }),
        label(move || match state.get() {
            InspectorState::Connected => "Connected to Flutter",
            InspectorState::Connecting => "Connecting to Flutter…",
            InspectorState::Disconnected => {
                if data.frames.with(|history| !history.is_empty())
                    || data.memory.with(|history| !history.is_empty())
                    || data.network.with(|history| !history.is_empty())
                {
                    "Last session ended"
                } else {
                    "No Flutter session"
                }
            }
        })
        .style(|s| s.font_size(11.0).text_ellipsis().min_width(0.0)),
    ))
    .style(move |s| {
        s.padding_horiz(12.0)
            .padding_vert(8.0)
            .gap(7.0)
            .items_center()
            .color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
    });
    let tabs = tab_strip(|strip| {
        stack_from_iter(
            [
                (Page::Frames, "Frames"),
                (Page::Memory, "Memory"),
                (Page::Network, "Network"),
                (Page::Diagnostics, "Diagnostics"),
            ]
            .map(|(value, title)| {
                strip.button(
                    label(move || title),
                    move || page.get() == value,
                    move || page.set(value),
                    config,
                )
            }),
        )
        .style(|s| s.padding_horiz(10.0).height(30.0))
    })
    .style(move |s| {
        s.width_full()
            .flex_shrink(0.0_f32)
            .border_bottom(1.0)
            .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
    });

    let switches = inspector.runtime_switches;
    let client = inspector.client;
    let state_signal = state;
    let content = dyn_container(
        move || {
            let state = state.get();
            let page = page.get();
            let has_data = state == InspectorState::Disconnected
                && match page {
                    Page::Frames => data.frames.with(|history| !history.is_empty()),
                    Page::Memory => data.memory.with(|history| !history.is_empty()),
                    Page::Network => {
                        data.network.with(|history| !history.is_empty())
                    }
                    Page::Diagnostics => false,
                };
            (state, page, has_data)
        },
        move |(state, page, has_data)| {
            use floem::IntoView;
            if state != InspectorState::Connected
                && !has_data
                && page != Page::Diagnostics
            {
                let (title, hint) = if state == InspectorState::Connecting {
                    (
                        "Connecting to the VM service",
                        "Live profiling data will appear when Flutter connects.",
                    )
                } else {
                    (
                        "Waiting for a Flutter session",
                        "Run the workspace in debug or profile mode. Profile mode gives representative frame timings.",
                    )
                };
                return card(
                    stack((heading(title, config), secondary(move || hint, config)))
                        .style(|s| s.flex_col().gap(6.0)),
                    config,
                )
                .into_any();
            }
            match page {
                Page::Frames => metrics::frames(data, config).into_any(),
                Page::Memory => metrics::memory(data, config).into_any(),
                Page::Network => {
                    network::view(data, client.clone(), state_signal, config)
                        .into_any()
                }
                Page::Diagnostics => {
                    diagnostics(switches, state_signal, client.clone(), config)
                        .into_any()
                }
            }
        },
    );
    stack((
        header,
        status,
        tabs,
        sparrow_scroll(content.style(|s| s.width_full().min_width(0).padding(12.0)))
            .style(|s| {
                s.width_full()
                    .min_height(0)
                    .flex_basis(0)
                    .flex_grow(1.0_f32)
            }),
    ))
    .style(|s| s.absolute().size_full().flex_col().min_width(0))
    .debug_name("Flutter Performance Panel")
}

fn poll(window: Rc<WindowTabData>, page: RwSignal<Page>) {
    exec_after(Duration::from_secs(1), move |_| {
        let Some(page_value) = page.try_get_untracked() else {
            return;
        };
        let inspector = &window.flutter_inspector;
        if inspector.state.get_untracked() == InspectorState::Connected
            && window
                .panel
                .is_panel_visible(&PanelKind::FlutterPerformance)
        {
            let request = match page_value {
                Page::Memory => Some(Sample::Memory),
                Page::Network
                    if (inspector.performance.recording.get_untracked()
                        || inspector
                            .performance
                            .network_refresh
                            .get_untracked())
                        && !inspector.performance.network_busy.get_untracked() =>
                {
                    Some(Sample::Network)
                }
                _ => None,
            };
            if let Some(request) = request {
                inspector.client.sample_performance(request);
            }
        }
        poll(window, page);
    });
}

fn diagnostics(
    switches: crate::dart::vm_service::RuntimeSwitches,
    state: RwSignal<InspectorState>,
    client: crate::dart::vm_service::VmServiceClient,
    config: Config,
) -> impl View {
    let disabled = move || state.get() != InspectorState::Connected;
    let paint = client.clone();
    let overlay = client.clone();
    let animations = client.clone();
    let reassemble = client.clone();
    card(
        stack((
            heading("Runtime diagnostics", config),
            secondary(|| "Some controls are available only in debug mode.", config),
            toggle_row(
                "Debug paint",
                Some("Draw layout bounds and padding"),
                switches.debug_paint,
                move |on| paint.set_debug_paint(None, on),
                config,
            )
            .disabled(disabled),
            toggle_row(
                "Performance overlay",
                Some("Draw Flutter’s timing graphs in the app"),
                switches.performance_overlay,
                move |on| overlay.set_performance_overlay(None, on),
                config,
            )
            .disabled(disabled),
            toggle_row(
                "Slow animations",
                Some("Stretch animations to five times their duration"),
                switches.slow_animations,
                move |on| animations.set_slow_animations(None, on),
                config,
            )
            .disabled(disabled),
            toggle_row(
                "Repaint rainbow",
                Some("Change layer colors on repaint"),
                switches.repaint_rainbow,
                move |on| client.set_repaint_rainbow(None, on),
                config,
            )
            .disabled(disabled),
            crate::widgets::button(
                || "Reassemble widget tree",
                crate::widgets::ButtonKind::Outlined,
                config,
            )
            .on_click_stop(move |_| reassemble.reassemble(None))
            .disabled(disabled),
        ))
        .style(|s| s.flex_col().gap(3.0)),
        config,
    )
}

fn heading(title: &'static str, config: Config) -> impl View {
    label(move || title).style(move |s| {
        s.font_size(13.0)
            .font_bold()
            .color(config.get().color(SparrowColor::PANEL_FOREGROUND))
    })
}

fn secondary(
    text: impl Fn() -> &'static str + 'static,
    config: Config,
) -> impl View {
    label(text).style(move |s| {
        s.font_size(11.0)
            .text_overflow(floem::style::TextOverflow::Wrap)
            .min_width(0)
            .color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
    })
}

fn card(content: impl View + 'static, config: Config) -> impl View {
    floem::views::container(content).style(move |s| {
        s.width_full()
            .min_width(0)
            .padding(10.0)
            .border(1.0)
            .border_radius(8.0)
            .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
    })
}
