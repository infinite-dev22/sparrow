//! Workspace tab strip and window actions.

use std::rc::Rc;

use crate::{
    command::WindowCommand,
    config::{color::SparrowColor, icon::SparrowIcons},
    title::window_controls_view,
    window::WindowData,
    window_tab::WindowTabData,
    workspace::{SparrowWorkspace, SparrowWorkspaceType},
};
use floem::{
    View,
    event::{Event, EventListener, EventPropagation},
    reactive::{
        RwSignal, SignalGet, SignalUpdate, SignalWith, create_memo, create_rw_signal,
    },
    style::Position,
    views::{
        Decorators, container, drag_window_area, dyn_stack, empty, stack, text,
    },
};

use crate::widgets::{TabStrip, clickable_icon, tab_strip};

fn workspace_title(workspace: &SparrowWorkspace) -> Option<String> {
    let p = workspace.path.as_ref()?;
    let dir = p.file_name().unwrap_or(p.as_os_str()).to_string_lossy();
    Some(match &workspace.kind {
        SparrowWorkspaceType::Local => format!("{dir}"),
        SparrowWorkspaceType::RemoteSSH(remote) => format!("{dir} [{remote}]"),
        #[cfg(windows)]
        SparrowWorkspaceType::RemoteWSL(remote) => format!("{dir} [{remote}]"),
    })
}

pub(super) fn workspace_tab_header(window_data: WindowData) -> impl View {
    let tabs = window_data.window_tabs;
    let active = window_data.active;
    let config = window_data.config;
    let window_tab_header_height = window_data.common.window_tab_header_height;
    let available_width = create_rw_signal(0.0);
    let add_icon_width = create_rw_signal(0.0);
    let window_control_width = create_rw_signal(0.0);
    let window_maximized = window_data.common.window_maximized;
    let num_window_tabs = window_data.num_window_tabs;
    let window_command = window_data.common.window_command;

    let tab_width = create_memo(move |_| {
        let window_control_width = if !cfg!(target_os = "macos")
            && config.get_untracked().core.custom_titlebar
        {
            window_control_width.get()
        } else {
            0.0
        };
        let available_width = available_width.get()
            - add_icon_width.get()
            - if cfg!(target_os = "macos") { 75.0 } else { 0.0 }
            - window_control_width
            - 30.0;
        let tabs_len = tabs.with(|tabs| tabs.len());
        if tabs_len > 0 {
            (available_width / tabs_len as f64).clamp(104.0, 200.0)
        } else {
            available_width.max(0.0)
        }
    });

    let local_window_data = window_data.clone();
    let dragging_index: RwSignal<Option<RwSignal<usize>>> = create_rw_signal(None);
    let view_fn =
        move |strip: TabStrip,
              (index, tab): (RwSignal<usize>, Rc<WindowTabData>)| {
            let drag_over_left = create_rw_signal(None);
            let window_data = local_window_data.clone();
            let tab_view = stack((
                container({
                    stack((
                        stack((
                            text(
                                workspace_title(&tab.workspace)
                                    .unwrap_or_else(|| String::from("New Tab")),
                            )
                            .style(|s| {
                                s.margin_left(10.0)
                                    .min_width(0.0)
                                    .flex_basis(0.0)
                                    .flex_grow(1.0f32)
                                    .selectable(false)
                                    .text_ellipsis()
                            }),
                            {
                                let window_data = local_window_data.clone();
                                clickable_icon(
                                    || SparrowIcons::WINDOW_CLOSE,
                                    move || {
                                        window_data.run_window_command(
                                            WindowCommand::CloseWorkspaceTab {
                                                index: Some(index.get_untracked()),
                                            },
                                        );
                                    },
                                    || false,
                                    || false,
                                    || "Close",
                                    config.read_only(),
                                )
                                .style(|s| s.margin_horiz(6.0))
                            },
                        ))
                        .on_event_stop(EventListener::DragOver, move |event| {
                            if dragging_index.get_untracked().is_some() {
                                if let Event::PointerMove(pointer_event) = event {
                                    let left = pointer_event.pos.x
                                        < tab_width.get_untracked() / 2.0;
                                    if drag_over_left.get_untracked() != Some(left) {
                                        drag_over_left.set(Some(left));
                                    }
                                }
                            }
                        })
                        .on_event(EventListener::Drop, move |event| {
                            if dragging_index.get_untracked().is_some() {
                                drag_over_left.set(None);
                                if let Event::PointerUp(pointer_event) = event {
                                    let left = pointer_event.pos.x
                                        < tab_width.get_untracked() / 2.0;
                                    let index = index.get_untracked();
                                    let new_index =
                                        if left { index } else { index + 1 };
                                    if let Some(from_index) =
                                        dragging_index.get_untracked()
                                    {
                                        window_data.move_tab(
                                            from_index.get_untracked(),
                                            new_index,
                                        );
                                    }
                                    dragging_index.set(None);
                                }
                                EventPropagation::Stop
                            } else {
                                EventPropagation::Continue
                            }
                        })
                        .on_event_stop(EventListener::DragLeave, move |_| {
                            drag_over_left.set(None);
                        })
                        .style(move |s| {
                            let config = config.get();
                            s.width_full()
                                .min_width(0.0)
                                .apply_if(active.get() == index.get(), |s| {
                                    s.background(config.color(
                                        SparrowColor::SPARROW_TAB_ACTIVE_BACKGROUND,
                                    ))
                                })
                                .items_center()
                                .border_right(1.0)
                                .border_color(
                                    config.color(SparrowColor::SPARROW_BORDER),
                                )
                                .apply_if(
                                    cfg!(target_os = "macos") && index.get() == 0,
                                    |s| s.border_left(1.0),
                                )
                        }),
                        container(empty().style(move |s| {
                            s.size_full()
                                .apply_if(active.get() == index.get(), |s| {
                                    s.border_bottom(2.0)
                                })
                                .border_color(config.get().color(
                                    SparrowColor::SPARROW_TAB_ACTIVE_UNDERLINE,
                                ))
                        }))
                        .style(move |s| {
                            s.position(Position::Absolute)
                                .padding_horiz(3.0)
                                .size_full()
                                .pointer_events_none()
                        }),
                    ))
                    .style(move |s| s.size_full().items_center())
                })
                .draggable()
                .on_event_stop(EventListener::DragStart, move |_| {
                    dragging_index.set(Some(index));
                })
                .on_event_stop(EventListener::DragEnd, move |_| {
                    dragging_index.set(None);
                })
                .dragging_style(move |s| {
                    let config = config.get();
                    s.border(1.0)
                        .border_radius(6.0)
                        .border_color(config.color(SparrowColor::SPARROW_BORDER))
                        .color(
                            config
                                .color(SparrowColor::EDITOR_FOREGROUND)
                                .multiply_alpha(0.7),
                        )
                        .background(
                            config
                                .color(SparrowColor::PANEL_BACKGROUND)
                                .multiply_alpha(0.7),
                        )
                })
                .on_click_stop(move |_| {
                    active.set(index.get_untracked());
                })
                .style(move |s| s.size_full()),
                empty().style(move |s| {
                    let index = index.get();
                    s.absolute()
                        .margin_left(if index == 0 { 0.0 } else { -2.0 })
                        .width(
                            tab_width.get() as f32
                                + if index == 0 { 1.0 } else { 3.0 },
                        )
                        .height_full()
                        .border_color(
                            config
                                .get()
                                .color(SparrowColor::SPARROW_TAB_ACTIVE_UNDERLINE),
                        )
                        .apply_if(drag_over_left.get().is_some(), move |s| {
                            let drag_over_left =
                                drag_over_left.get_untracked().unwrap();
                            if drag_over_left {
                                s.border_left(3.0)
                            } else {
                                s.border_right(3.0)
                            }
                        })
                        .apply_if(drag_over_left.get().is_none(), move |s| s.hide())
                }),
            ))
            .style(move |s| s.height_full().width(tab_width.get() as f32));
            strip.item(tab_view, move || active.get() == index.get())
        };

    stack((
        empty().style(move |s| {
            let is_macos = cfg!(target_os = "macos");
            s.min_width(75.0)
                .width(75.0)
                .flex_shrink(0.0_f32)
                .apply_if(!is_macos, |s| s.hide())
        }),
        tab_strip(|strip| {
            dyn_stack(
                move || {
                    let tabs = tabs.get();
                    for (i, (index, _)) in tabs.iter().enumerate() {
                        if index.get_untracked() != i {
                            index.set(i);
                        }
                    }
                    tabs
                },
                |(_, tab)| tab.window_tab_id,
                move |item| view_fn(strip, item),
            )
            .style(|s| s.height_full())
        })
        .style(|s| s.height_full()),
        container(clickable_icon(
            || SparrowIcons::ADD,
            move || {
                window_data.run_window_command(WindowCommand::NewWorkspaceTab {
                    workspace: SparrowWorkspace::default(),
                    end: true,
                });
            },
            || false,
            || false,
            || "New Workspace Tab",
            config.read_only(),
        ))
        .on_resize(move |rect| {
            let current = add_icon_width.get_untracked();
            if rect.width() != current {
                add_icon_width.set(rect.width());
            }
        })
        .style(|s| {
            s.height_full()
                .flex_shrink(0.0_f32)
                .padding_left(10.0)
                .padding_right(10.0)
                .items_center()
        }),
        drag_window_area(empty())
            .style(|s| s.height_full().flex_basis(0.0).flex_grow(1.0f32)),
        window_controls_view(
            window_command,
            false,
            num_window_tabs,
            window_maximized,
            config.read_only(),
        )
        .style(|s| s.flex_shrink(0.0_f32))
        .on_resize(move |rect| {
            let width = rect.width();
            if window_control_width.get_untracked() != width {
                window_control_width.set(width);
            }
        }),
    ))
    .on_resize(move |rect| {
        let current = available_width.get_untracked();
        if rect.width() != current {
            available_width.set(rect.width());
        }
        window_tab_header_height.set(rect.height());
    })
    .style(move |s| {
        let config = config.get();
        s.border_bottom(1.0)
            .width_full()
            .height(37.0)
            .font_size(config.ui.font_size() as f32)
            .line_height(1.4)
            .apply_if(!config.ui.font_family.is_empty(), |s| {
                s.font_family(config.ui.font_family.clone())
            })
            .apply_if(tabs.with(|tabs| tabs.len() < 2), |s| s.hide())
            .color(config.color(SparrowColor::EDITOR_FOREGROUND))
            .border_color(config.color(SparrowColor::SPARROW_BORDER))
            .background(config.color(SparrowColor::PANEL_BACKGROUND))
            .items_center()
    })
    .debug_name("Workspace Tab Header")
}
