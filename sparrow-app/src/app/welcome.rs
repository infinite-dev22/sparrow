mod projects;

use std::{rc::Rc, sync::Arc};

use crate::widgets::{IconButtonStyle, icon_button};
use crate::{
    command::{SparrowWorkbenchCommand as Command, WindowCommand},
    config::{color::SparrowColor, icon::SparrowIcons},
    db::SparrowDb,
    widgets::{
        ButtonKind, button, clickable_icon,
        popup_menu::{Menu, MenuItem, show_context_menu},
        sparrow_scroll,
    },
    window_tab::WindowTabData,
};
use floem::{
    IntoView, View,
    reactive::{SignalGet, SignalUpdate, SignalWith, create_rw_signal, use_context},
    views::editor::{core::register::Clipboard, text::SystemClipboard},
    views::{Decorators, dyn_stack, empty, label, stack, svg},
};
use crate::text_input::{m3_from_config, TextField};

#[derive(Clone, Copy)]
enum WelcomeAction {
    Flutter,
    NewProject,
    Open,
}

impl WelcomeAction {
    fn run(
        self,
        workbench: crate::listener::Listener<Command>,
        window: crate::listener::Listener<WindowCommand>,
        config: crate::widgets::file_picker::Config,
    ) {
        match self {
            Self::Flutter => workbench.send(Command::FlutterNewProject),
            Self::Open => workbench.send(Command::OpenFile),
            Self::NewProject => {
                use crate::widgets::file_picker::{FileDialogOptions, open_file};
                open_file(
                    FileDialogOptions::new()
                        .title("New Folder Project")
                        .select_directories()
                        .start_with_new_folder(),
                    config,
                    move |selected| {
                        if let Some(path) =
                            selected.and_then(|file| file.path.into_iter().next())
                        {
                            window.send(WindowCommand::SetWorkspace {
                                workspace: crate::workspace::SparrowWorkspace {
                                    path: Some(path),
                                    ..Default::default()
                                },
                            });
                        }
                    },
                );
            }
        }
    }
}

pub(super) fn view(tab: Rc<WindowTabData>) -> impl View {
    let config = tab.common.config;
    let workbench = tab.common.workbench_command;
    let window = tab.common.window_common.window_command;
    let db = use_context::<Arc<SparrowDb>>();
    let projects = create_rw_signal(
        db.as_ref()
            .and_then(|db| db.recent_workspaces().ok())
            .unwrap_or_default(),
    );
    let project_styles = projects::Projects::new();
    let group_form = project_styles.group_form(config);
    let sort_styles = project_styles.clone();
    let query = create_rw_signal(String::new());
    let actions = [
        ("New Flutter Project", WelcomeAction::Flutter),
        ("New Project", WelcomeAction::NewProject),
        ("Open", WelcomeAction::Open),
    ];
    let toolbar = stack((
        TextField::new(query)
            .colors(move || m3_from_config(config))
            .placeholder("Search recent projects")
            .leading_icon(svg(move || config.get().ui_svg(SparrowIcons::SEARCH)))
            .into_view()
            .style(|s| {
                s.width(floem::unit::PxPctAuto::Auto)
                    .flex_grow(1.0_f32)
                    .flex_basis(0)
                    .min_width(180.0)
            }),
        floem::views::stack_from_iter(actions.iter().map(|(title, command)| {
            let title = *title;
            let command = *command;
            button(move || title, ButtonKind::Outlined, config)
                .on_click_stop(move |_| command.run(workbench, window, config))
                .style(|s| s.height(30).padding_horiz(10))
        }))
        .style(|s| {
            s.items_center()
                .gap(6)
                .flex_wrap(floem::taffy::style::FlexWrap::Wrap)
        }),
    ))
    .style(|s| {
        s.width_full()
            .items_center()
            .gap(12)
            .flex_wrap(floem::taffy::style::FlexWrap::Wrap)
    });
    let rows = dyn_stack(
        move || {
            let q = query.get().trim().to_lowercase();
            let mut rows = projects.with(|projects| projects.iter().filter(|ws| {
                ws.path.as_ref().is_some_and(|path| path.to_string_lossy().to_lowercase().contains(&q))
            }).cloned().map(|ws| (sort_styles.group(&ws.to_string()), ws)).collect::<Vec<_>>());
            rows.sort_by(|(left, _), (right, _)| left.cmp(right));
            rows.into_iter().map(|(_, ws)| ws).collect::<Vec<_>>()
        },
        |ws| ws.to_string(),
        move |workspace| {
            let name = workspace.display().unwrap_or_default();
            let path = workspace.path.as_ref().unwrap().clone();
            let identity = workspace.to_string();
            let group_key = identity.clone();
            let menu_styles = project_styles.clone();
            let row_styles = project_styles.clone();
            let initials = name.chars().filter(|c| c.is_alphanumeric()).take(2).collect::<String>().to_uppercase();
            let display_path = path.display().to_string();
            let open = workspace.clone();
            let menu_workspace = workspace.clone();
            let db = db.clone();
            let exists = workspace.kind.is_remote() || path.is_dir();
            stack((
                project_styles.badge(identity.clone(), initials, config),
                stack((
                    label(move || name.clone()).style(|s| s.font_bold().max_width_pct(100.0).text_ellipsis()),
                    label(move || {
                        match row_styles.group(&group_key) {
                            Some(group) => format!("{group}  ·  {display_path}"),
                            None => display_path.clone(),
                        }
                    }).style(move |s| {
                        s.max_width_pct(100.0).text_ellipsis()
                            .color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
                    }),
                )).style(|s| s.flex_col().flex_grow(1.0_f32).min_width(0).gap(4)),
                clickable_icon(
                    || SparrowIcons::WINDOW_MORE, move || {
                        let open = menu_workspace.clone();
                        let reveal = path.clone();
                        let copy = path.clone();
                        let remove = menu_workspace.clone();
                        let db = db.clone();
                        let menu = Menu::new("Recent project")
                                .entry(MenuItem::new("Open Project").enabled(exists).action(move || {
                                    window.send(WindowCommand::SetWorkspace {workspace: open.clone()});
                                }))
                                .entry(MenuItem::new("Show in Files").enabled(!menu_workspace.kind.is_remote() && exists)
                                    .action(move || { let _ = open::that(&reveal); }))
                                .entry(MenuItem::new("Copy Path").action(move || {
                                    SystemClipboard::new().put_string(copy.to_string_lossy());
                                }))
                                ;
                        let menu = menu_styles.actions(menu, identity.clone(), config);
                        show_context_menu(menu.separator()
                                .entry(MenuItem::new("Remove from Recent Projects").action(move || {
                                    if let Some(db) = &db
                                        && let Err(error) = db.remove_recent_workspace(&remove) {
                                        tracing::error!(%error, "Could not remove recent project");
                                        return;
                                    }
                                    projects.update(|list| list.retain(|ws| ws.kind != remove.kind || ws.path != remove.path));
                                })),
                            None,
                        );
                    }, || false, || false, || "Recent project actions", config,
                ),
            ))
            .on_click_stop(move |_| {
                if exists { window.send(WindowCommand::SetWorkspace { workspace: open.clone() }); }
            })
            .style(move |s| s.width_full().min_height(56).padding(8).gap(12)
                .border_radius(8).items_center()
                .hover(|s| s.background(config.get().color(SparrowColor::PANEL_HOVERED_BACKGROUND))))
        },
    ).style(|s| s.width_full().flex_col().gap(4));
    let initial_actions = floem::views::stack_from_iter(
        actions
            .iter()
            .zip([
                SparrowIcons::FLUTTER,
                SparrowIcons::ADD,
                SparrowIcons::DIRECTORY,
            ])
            .map(|((title, command), icon)| {
                let title = *title;
                let command = *command;
                stack((
                    svg(move || config.get().ui_svg(icon)).style(move |s| {
                        s.size(24, 24)
                            .color(config.get().color(SparrowColor::EDITOR_LINK))
                    }),
                    label(move || title),
                ))
                .on_click_stop(move |_| command.run(workbench, window, config))
                .style(move |s| {
                    s.width(140)
                        .height(82)
                        .gap(8)
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .border_radius(10)
                        .background(
                            config
                                .get()
                                .color(SparrowColor::SPARROW_WINDOW_BACKGROUND),
                        )
                        .hover(|s| {
                            s.background(
                                config
                                    .get()
                                    .color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                            )
                        })
                })
            }),
    )
    .style(|s| {
        s.items_center()
            .justify_center()
            .gap(10)
            .flex_wrap(floem::taffy::style::FlexWrap::Wrap)
    });
    stack((
        stack((
            stack((
                svg(|| {
                    include_str!("../../../icons/sparrow/sparrow_logo.svg")
                        .to_owned()
                })
                .style(|s| s.size(34, 34)),
                stack((
                    label(|| "Sparrow").style(|s| s.font_size(18.0).font_bold()),
                    label(|| sparrow_core::meta::VERSION).style(move |s| {
                        s.color(
                            config.get().color(SparrowColor::PANEL_FOREGROUND_DIM),
                        )
                    }),
                ))
                .style(move |s| {
                    s.flex_shrink(0.0_f32)
                        .height_full()
                        .flex_col()
                        .items_start()
                }),
            ))
            .style(move |s| {
                s.flex_shrink(0.0_f32).width_full().flex_row().items_start()
            }),
            empty().style(|s| s.flex_grow(1.0_f32)),
            icon_button(
                || SparrowIcons::SETTINGS,
                move || "Settings",
                config,
                IconButtonStyle::default,
            )
            .on_click_stop(move |_| workbench.send(Command::OpenSettings))
            .style(|s| s.height(30).padding_horiz(12)),
        ))
        .style(move |s| {
            s.width(200)
                .flex_shrink(0.0_f32)
                .height_full()
                .flex_col()
                .items_start()
                .padding(20)
                .gap(8)
                .background(
                    config.get().color(SparrowColor::SPARROW_WINDOW_BACKGROUND),
                )
        }),
        stack((
            stack((
                toolbar,
                group_form,
                sparrow_scroll(rows).style(|s| {
                    s.flex_basis(0)
                        .flex_grow(1.0_f32)
                        .min_height(0)
                        .width_full()
                }),
            ))
            .style(move |s| {
                s.size_full()
                    .flex_col()
                    .gap(18)
                    .apply_if(projects.with(Vec::is_empty), |s| s.hide())
            }),
            stack((
                label(|| "Welcome to Sparrow")
                    .style(|s| s.font_size(26.0).font_bold()),
                label(|| "Open a folder or start a Flutter project.").style(
                    move |s| {
                        s.color(
                            config.get().color(SparrowColor::PANEL_FOREGROUND_DIM),
                        )
                    },
                ),
                initial_actions,
                button(|| "Clone Repository", ButtonKind::Plain, config)
                    .on_click_stop(move |_| workbench.send(Command::CloneRepository))
                    .style(|s| s.height(28).padding_horiz(12)),
            ))
            .style(move |s| {
                s.size_full()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(16)
                    .apply_if(!projects.with(Vec::is_empty), |s| s.hide())
            }),
        ))
        .style(move |s| {
            s.height_full()
                .flex_grow(1.0_f32)
                .flex_basis(0)
                .min_width(0)
                .padding(22)
                .background(config.get().color(SparrowColor::PANEL_BACKGROUND))
        }),
    ))
    .style(|s| s.size_full().flex_basis(0).flex_grow(1.0_f32).min_height(0))
}

pub(super) fn title(tab: Rc<WindowTabData>) -> impl View {
    let config = tab.common.config;
    let command = tab.common.window_common.window_command;
    stack((
        label(|| "Welcome to Sparrow").style(|s| s.height_full().flex_grow(1.0_f32).items_center().font_bold())
            .on_event_cont(floem::event::EventListener::PointerDown, |event| {
                if matches!(event, floem::event::Event::PointerDown(pointer) if pointer.button.is_primary()) {
                    floem::action::drag_window();
                }
            }),
        clickable_icon(|| SparrowIcons::WINDOW_CLOSE, move || command.send(WindowCommand::CloseWindow),
            || false, || false, || "Close Sparrow", config),
    )).style(move |s| {
        let config = config.get();
        s.width_full().height(36).flex_shrink(0.0_f32).items_center().padding_horiz(12)
            .background(config.color(SparrowColor::SPARROW_WINDOW_BACKGROUND))
            .border_bottom(1).border_color(config.color(SparrowColor::SPARROW_BORDER))
    })
}
