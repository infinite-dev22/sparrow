//! Project switcher popup and project action rows.

use floem::{
    View,
    kurbo::Point,
    peniko::Color,
    prelude::create_rw_signal,
    reactive::{
        ReadSignal, RwSignal, SignalGet, SignalUpdate, SignalWith, create_effect,
        use_context,
    },
    style::{CursorStyle, Display},
    text::Weight,
    views::{Decorators, container, dyn_stack, empty, label, scroll, stack, svg},
};
use std::{path::Path, rc::Rc, sync::Arc};

use crate::{
    command::{SparrowWorkbenchCommand, WindowCommand},
    config::{SparrowConfig, color::SparrowColor, icon::SparrowIcons},
    db::SparrowDb,
    widgets::divider::Divider,
    window_tab::WindowTabData,
    workspace::SparrowWorkspace,
};

pub fn project_switcher(
    window_tab_data: Rc<WindowTabData>,
    window_tabs: ReadSignal<im::Vector<(RwSignal<usize>, Rc<WindowTabData>)>>,
    origin: RwSignal<Point>,
) -> impl View {
    let visible = window_tab_data.project_menu_visible;
    let config = window_tab_data.common.config;
    let window_command = window_tab_data.common.window_common.window_command;
    let workbench_command = window_tab_data.common.workbench_command;
    let window_origin = window_tab_data.common.window_origin;
    let title_height = window_tab_data.title_height;
    let current_tab_id = window_tab_data.window_tab_id; // copied out: no Rc in closures
    let db: Arc<SparrowDb> = use_context().unwrap();

    // Recents (minus already-open projects), each with its git branch, refreshed on open.
    let recent_workspaces =
        create_rw_signal(Vec::<(Arc<SparrowWorkspace>, String)>::new());
    create_effect(move |_| {
        if visible.get() {
            if let Ok(recent) = db.recent_workspaces() {
                let open: Vec<_> = window_tabs.with_untracked(|tabs| {
                    tabs.iter().map(|(_, t)| t.workspace.path.clone()).collect()
                });
                recent_workspaces.set(
                    recent
                        .into_iter()
                        .filter(|ws| !open.contains(&ws.path))
                        .map(|ws| {
                            let branch = ws
                                .path
                                .as_deref()
                                .map(read_git_branch)
                                .unwrap_or_default();
                            (Arc::new(ws), branch)
                        })
                        .collect(),
                );
            }
        }
    });

    let close_menu = {
        let visible = window_tab_data.project_menu_visible;
        move || visible.set(false)
    };

    let section_label = move |text: &'static str| {
        label(move || text.to_string()).style(move |s| {
            s.padding_horiz(8.0)
                .padding_top(6.0)
                .padding_bottom(4.0)
                .font_size(11.0)
                .color(config.get().color(SparrowColor::EDITOR_DIM))
        })
    };

    let divider = move || Divider::new(config).spacing(6.0).into_view();

    let build_project_item =
        move |workspace: Arc<SparrowWorkspace>, branch: String, is_current: bool| {
            let path_str = workspace
                .path
                .as_deref()
                .map(display_path)
                .unwrap_or_default();
            let name = workspace
                .path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "Untitled".into());
            let initials: String =
                name.chars().take(2).collect::<String>().to_uppercase();

            let hash = name.bytes().fold(0u32, |a, b| a.wrapping_add(b as u32));
            let color = match hash % 5 {
                0 => Color::from_rgb8(0xE0, 0x7A, 0x5F),
                1 => Color::from_rgb8(0x2A, 0xA1, 0xB8),
                2 => Color::from_rgb8(0xB0, 0x4F, 0xC0),
                3 => Color::from_rgb8(0x50, 0xA1, 0x4F),
                _ => Color::from_rgb8(0x7A, 0x4F, 0xE0),
            };

            let has_branch = !branch.is_empty();
            let cmd = window_command;
            let close = close_menu;
            let ws = workspace.clone();
            let config_get = config;

            stack((
                container(label(move || initials.clone()).style(|s| {
                    s.font_size(13.0)
                        .font_weight(Weight::BOLD)
                        .color(Color::WHITE)
                        .selectable(false)
                }))
                .style(move |s| {
                    s.size(32.0, 32.0)
                        .flex_shrink(0.0_f32)
                        .border_radius(8.0)
                        .items_center()
                        .justify_center()
                        .background(color)
                        .margin_right(12.0)
                }),
                stack((
                    label(move || name.clone()).style(move |s| {
                        s.font_size(13.0).color(
                            config_get.get().color(SparrowColor::EDITOR_FOREGROUND),
                        )
                    }),
                    label(move || path_str.clone()).style(move |s| {
                        s.font_size(11.0)
                            .text_ellipsis()
                            .color(config_get.get().color(SparrowColor::EDITOR_DIM))
                    }),
                    stack((
                        svg(move || config_get.get().ui_svg(SparrowIcons::SCM))
                            .style(move |s| {
                                let c = config_get.get();
                                let size = c.ui.icon_size() as f32 - 4.0;
                                s.size(size, size)
                                    .margin_right(4.0)
                                    .color(c.color(SparrowColor::EDITOR_DIM))
                            }),
                        label(move || branch.clone()).style(move |s| {
                            s.font_size(11.0).color(
                                config_get.get().color(SparrowColor::EDITOR_DIM),
                            )
                        }),
                    ))
                    .style(move |s| {
                        s.items_center().apply_if(!has_branch, |s| s.hide())
                    }),
                ))
                .style(|s| s.flex_col().flex_grow(1.0f32).min_width(0.0)),
            ))
            .on_click_stop(move |_| {
                if !is_current {
                    cmd.send(WindowCommand::SetWorkspace {
                        workspace: (*ws).clone(),
                    });
                }
                close();
            })
            .style(move |s| {
                let c = config_get.get();
                s.width_full()
                    .padding_horiz(8.0)
                    .padding_vert(6.0)
                    .border_radius(6.0)
                    .items_center()
                    .hover(|s| {
                        s.background(c.color(SparrowColor::PANEL_HOVERED_BACKGROUND))
                            .cursor(CursorStyle::Pointer)
                    })
            })
        };

    stack((
        // Click-outside catcher
        empty()
            .on_click_stop(move |_| close_menu())
            .style(|s| s.absolute().size_full()),
        // Panel
        container(
            scroll(
                stack((
                    menu_row(
                        None,
                        "New Project...",
                        move || {
                            workbench_command
                                .send(SparrowWorkbenchCommand::OpenFolder);
                            close_menu();
                        },
                        config,
                    ),
                    menu_row(
                        Some(SparrowIcons::DIRECTORY),
                        "Open...",
                        move || {
                            workbench_command
                                .send(SparrowWorkbenchCommand::OpenFile);
                            close_menu();
                        },
                        config,
                    ),
                    menu_row(
                        Some(SparrowIcons::SCM),
                        "Clone Repository...",
                        || {},
                        config,
                    ),
                    menu_row(None, "Import Sample...", || {}, config),
                    divider(),
                    section_label("Open Projects"),
                    dyn_stack(
                        move || window_tabs.get(),
                        |(_, tab)| tab.window_tab_id,
                        move |(_, tab)| {
                            build_project_item(
                                tab.workspace.clone(),
                                tab.source_control.branch.get_untracked(),
                                tab.window_tab_id == current_tab_id,
                            )
                        },
                    ),
                    divider(),
                    section_label("Recent Projects"),
                    dyn_stack(
                        move || recent_workspaces.get(),
                        |(ws, _)| ws.path.clone(),
                        move |(ws, branch)| build_project_item(ws, branch, false),
                    ),
                ))
                .style(|s| s.flex_col().width_full()),
            )
            .style(|s| s.max_height(520.0).width_full()),
        )
        .on_click_stop(|_| {}) // clicks inside don't close it
        .style(move |s| {
            let c = config.get();
            let pt = origin.get() - window_origin.get().to_vec2();
            s.absolute()
                .margin_left(pt.x as f32)
                .margin_top(title_height.get() as f32 + 2.0)
                .width(320.0)
                .padding(8.0)
                .background(c.color(SparrowColor::PANEL_BACKGROUND))
                .border(1.0)
                .border_radius(8.0)
                .border_color(c.color(SparrowColor::SPARROW_BORDER))
                .box_shadow_blur(12.0)
                .box_shadow_color(c.color(SparrowColor::SPARROW_DROPDOWN_SHADOW))
        }),
    ))
    .style(move |s| {
        s.absolute()
            .size_full()
            .display(if visible.get() {
                Display::Flex
            } else {
                Display::None
            })
            .z_index(100)
    })
}

/// Menu row with a fixed icon column so all labels line up (like the reference).
fn menu_row(
    icon: Option<&'static str>,
    text: &'static str,
    on_click: impl Fn() + 'static,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    stack((
        container(
            svg(move || icon.map(|i| config.get().ui_svg(i)).unwrap_or_default())
                .style(move |s| {
                    let c = config.get();
                    let size = c.ui.icon_size() as f32;
                    s.size(size, size)
                        .color(c.color(SparrowColor::SPARROW_ICON_ACTIVE))
                }),
        )
        .style(|s| s.width(24.0).items_center()),
        label(move || text.to_string()).style(move |s| {
            s.font_size(13.0)
                .color(config.get().color(SparrowColor::EDITOR_FOREGROUND))
        }),
    ))
    .on_click_stop(move |_| on_click())
    .style(move |s| {
        let c = config.get();
        s.width_full()
            .items_center()
            .padding_horiz(8.0)
            .padding_vert(6.0)
            .border_radius(6.0)
            .hover(|s| {
                s.background(c.color(SparrowColor::PANEL_HOVERED_BACKGROUND))
                    .cursor(CursorStyle::Pointer)
            })
    })
}

/// `/home/me/x` -> `~/x`
fn display_path(path: &Path) -> String {
    let full = path.to_string_lossy().to_string();
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();
    if !home.is_empty() && full.starts_with(&home) {
        format!("~{}", &full[home.len()..])
    } else {
        full
    }
}

/// Current branch from `.git/HEAD`. Empty for detached HEAD, worktrees or non-repos.
fn read_git_branch(root: &Path) -> String {
    std::fs::read_to_string(root.join(".git/HEAD"))
        .ok()
        .and_then(|h| {
            h.trim()
                .strip_prefix("ref: refs/heads/")
                .map(str::to_string)
        })
        .unwrap_or_default()
}
