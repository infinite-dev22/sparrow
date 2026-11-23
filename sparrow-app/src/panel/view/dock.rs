//! Docked tool window layout, resize handling and panel content dispatch.

use std::rc::Rc;

use floem::{
    IntoView, View,
    event::{EventListener, EventPropagation},
    reactive::{Memo, SignalGet, SignalUpdate, SignalWith, create_memo, create_rw_signal},
    views::{Decorators, container, empty, stack, tab},
};

use crate::panel::{
    debug_view::debug_panel,
    global_search_view::global_search_panel,
    kind::PanelKind,
    plugin_view::plugin_panel,
    position::{PanelContainerPosition, PanelPosition},
    problem_view::problem_panel,
    source_control_view::source_control_panel,
    terminal_view::terminal_panel,
};
use crate::{
    config::color::SparrowColor,
    file_explorer::view::file_explorer_panel,
    panel::{
        call_hierarchy_view::show_hierarchy_panel,
        implementation_view::implementation_panel,
        references_view::references_panel,
    },
    window_tab::{DragContent, WindowTabData},
};

use super::island::{ISLAND_RADIUS, island_corners};

pub fn panel_container_view(
    window_tab_data: Rc<WindowTabData>,
    position: PanelContainerPosition,
    dock_width_scale: Memo<f64>,
) -> impl View {
    let panel = window_tab_data.panel.clone();
    let config = window_tab_data.common.config;
    let dragging = window_tab_data.common.dragging;
    let is_dragging_panel = move || {
        dragging
            .with(|d| d.as_ref().map(|d| d.is_panel()))
            .unwrap_or(false)
    };
    let drop_view = {
        let panel = panel.clone();
        move |position: PanelPosition| {
            let panel = panel.clone();
            let dragging_over = create_rw_signal(false);
            empty()
                .on_event(EventListener::DragEnter, move |_| {
                    if is_dragging_panel() {
                        dragging_over.set(true);
                        EventPropagation::Stop
                    } else {
                        EventPropagation::Continue
                    }
                })
                .on_event(EventListener::DragLeave, move |_| {
                    if is_dragging_panel() {
                        dragging_over.set(false);
                        EventPropagation::Stop
                    } else {
                        EventPropagation::Continue
                    }
                })
                .on_event(EventListener::Drop, move |_| {
                    if let Some(DragContent::Panel(kind)) = dragging.get_untracked()
                    {
                        dragging_over.set(false);
                        panel.move_panel_to_position(kind, &position);
                        EventPropagation::Stop
                    } else {
                        EventPropagation::Continue
                    }
                })
                .style(move |s| {
                    s.size_pct(100.0, 100.0).apply_if(dragging_over.get(), |s| {
                        s.background(
                            config
                                .get()
                                .color(SparrowColor::EDITOR_DRAG_DROP_BACKGROUND),
                        )
                    })
                })
        }
    };

    let panel_size = panel.size;
    let available = panel.available_size;
    let extent = create_memo(move |_| {
        panel_size.with(|size| match position {
            PanelContainerPosition::Left => size.left.max(0.0) * dock_width_scale.get(),
            PanelContainerPosition::Right => size.right.max(0.0) * dock_width_scale.get(),
            PanelContainerPosition::Bottom => size.bottom.clamp(0.0, (available.get().height - 103.0).max(0.0)),
        })
    });

    let is_bottom = position.is_bottom();
    stack((
        panel_view(window_tab_data.clone(), position.first()),
        panel_view(window_tab_data.clone(), position.second()),
        super::resize::handle(panel.clone(), position, config),
        island_corners(config, window_tab_data.common.project_color),
        stack((drop_view(position.first()), drop_view(position.second()))).style(
            move |s| {
                let is_dragging_panel = is_dragging_panel();
                s.absolute()
                    .size_pct(100.0, 100.0)
                    .apply_if(!is_bottom, |s| s.flex_col())
                    .apply_if(!is_dragging_panel, |s| s.pointer_events_none())
            },
        ),
    ))
    .style(move |s| {
        let size = extent.get();
        let is_maximized = is_bottom && panel.panel_bottom_maximized(true);
        let config = config.get();
        s.apply_if(!panel.is_container_shown(&position, true), |s| s.hide())
            .apply_if(position == PanelContainerPosition::Bottom, |s| {
                s.width_pct(100.0)
                    .flex_shrink(0.0_f32)
                    .apply_if(!is_maximized, |s| s.height(size as f32))
                    .apply_if(is_maximized, |s| s.flex_grow(1.0_f32).flex_basis(0.0))
                    .background(config.color(SparrowColor::PANEL_BACKGROUND))
            })
            .apply_if(position == PanelContainerPosition::Left, |s| {
                s.width(size as f32)
                    .height_pct(100.0)
                    .background(config.color(SparrowColor::PANEL_BACKGROUND))
            })
            .apply_if(position == PanelContainerPosition::Right, |s| {
                s.width(size as f32)
                    .height_pct(100.0)
                    .background(config.color(SparrowColor::PANEL_BACKGROUND))
            })
            .apply_if(!is_bottom, |s| s.flex_col())
            .border_radius(ISLAND_RADIUS)
            .color(config.color(SparrowColor::PANEL_FOREGROUND))
    })
    .debug_name(format!("{:?} Panel Container View", position))
}

fn panel_view(
    window_tab_data: Rc<WindowTabData>,
    position: PanelPosition,
) -> impl View {
    let panel = window_tab_data.panel.clone();
    let panels = move || {
        panel
            .panels
            .with(|p| p.get(&position).cloned().unwrap_or_default())
    };
    let active_fn = move || {
        panel
            .styles
            .with(|s| s.get(&position).map(|s| s.active).unwrap_or(0))
    };
    tab(
        active_fn,
        panels,
        |p| *p,
        move |kind| {
            let view = match kind {
                PanelKind::Notifications => crate::panel::workflow_tools::notifications(window_tab_data.clone()).into_any(),
                PanelKind::PullRequests => crate::panel::workflow_tools::pull_requests(window_tab_data.clone()).into_any(),
                PanelKind::MobilePreview => crate::panel::mobile_preview::panel(window_tab_data.clone()).into_any(),
                PanelKind::RunningDevices => crate::panel::workflow_tools::running_devices(window_tab_data.clone()).into_any(),
                PanelKind::Problems => crate::panel::reference_tools::problems(window_tab_data.clone()).into_any(),
                PanelKind::Logcat => crate::panel::reference_tools::logcat(window_tab_data.clone()).into_any(),
                PanelKind::ResourceManager => crate::panel::resource_manager::panel(window_tab_data.clone()).into_any(),
                PanelKind::GitHistory => crate::panel::reference_tools::history(window_tab_data.clone()).into_any(),
                PanelKind::Terminal => {
                    terminal_panel(window_tab_data.clone()).into_any()
                }
                PanelKind::FileExplorer => {
                    file_explorer_panel(window_tab_data.clone(), position).into_any()
                }
                PanelKind::SourceControl => {
                    source_control_panel(window_tab_data.clone(), position)
                        .into_any()
                }
                PanelKind::Plugin => {
                    plugin_panel(window_tab_data.clone(), position).into_any()
                }
                PanelKind::Search => {
                    global_search_panel(window_tab_data.clone(), position).into_any()
                }
                PanelKind::Problem => {
                    problem_panel(window_tab_data.clone(), position).into_any()
                }
                PanelKind::Debug => {
                    debug_panel(window_tab_data.clone(), position).into_any()
                }
                PanelKind::CallHierarchy => {
                    show_hierarchy_panel(window_tab_data.clone(), position)
                        .into_any()
                }
                PanelKind::DocumentSymbol => {
                    crate::panel::dart_outline::dart_outline_panel(window_tab_data.clone(), position).into_any()
                }
                PanelKind::References => {
                    references_panel(window_tab_data.clone(), position).into_any()
                }
                PanelKind::Implementation => {
                    implementation_panel(window_tab_data.clone(), position)
                        .into_any()
                }
                PanelKind::WidgetTree => {
                    crate::panel::widget_tree::widget_tree_panel(
                        window_tab_data.clone(),
                        position,
                    )
                        .into_any()
                }
                PanelKind::DartOutline => {
                    crate::panel::dart_outline::dart_outline_panel(
                        window_tab_data.clone(),
                        position,
                    )
                        .into_any()
                }
                PanelKind::FlutterInspector => {
                    crate::panel::flutter_inspector::flutter_inspector_panel(
                        window_tab_data.clone(),
                        position,
                    )
                        .into_any()
                }
                PanelKind::FlutterTest => {
                    crate::panel::flutter_test_view::flutter_test_panel(window_tab_data.clone(), position).into_any()
                }
                PanelKind::FlutterRun => {
                    crate::panel::flutter_run_view::flutter_run_panel(
                        window_tab_data.clone(),
                        position,
                    )
                        .into_any()
                }
                PanelKind::DeviceManager => {
                    crate::panel::device_manager_view::device_manager_panel(
                        window_tab_data.clone(),
                        position,
                    )
                        .into_any()
                }
                PanelKind::FlutterPerformance => {
                    crate::panel::flutter_performance_view::flutter_performance_panel(
                        window_tab_data.clone(),
                        position,
                    )
                        .into_any()
                }
            };
            let needs_header = match kind {
                PanelKind::Search
                | PanelKind::CallHierarchy
                | PanelKind::References
                | PanelKind::Implementation
                | PanelKind::FlutterInspector
                | PanelKind::FlutterTest => true,
                PanelKind::Problem => !position.is_bottom(),
                _ => false,
            };
            let view = view.style(|s| s.size_full());
            if needs_header {
                stack((
                    crate::panel::chrome::header(
                        window_tab_data.clone(), kind, kind.title(), empty(),
                    ),
                    container(view).style(|s| s.width_full().flex_grow(1.0_f32).flex_basis(0).min_height(0)),
                ))
                .style(|s| s.flex_col().size_full())
                .into_any()
            } else {
                view.into_any()
            }
        },
    )
        .style(move |s| {
            s.size_pct(100.0, 100.0).apply_if(
                !panel.is_position_shown(&position, true)
                    || panel.is_position_empty(&position, true),
                |s| s.hide(),
            )
        })
}
