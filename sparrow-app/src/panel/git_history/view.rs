//! Git log, committed file tree and revision inspection.

use super::{Branch, ChangedFile, Details, Log, files, load};
use crate::panel::{kind::PanelKind, reference_tools::output};
use crate::widgets::text_input;
use crate::{
    config::{color::SparrowColor, icon::SparrowIcons},
    widgets::clickable_icon,
    widgets::popup_menu::{Menu, MenuItem, PopupMenuExt},
    window_tab::WindowTabData,
};
use floem::{
    View,
    event::{Event, EventListener, EventPropagation},
    keyboard::{Key, NamedKey},
    reactive::{
        Scope, SignalGet, SignalUpdate, SignalWith, create_effect, create_memo,
        create_rw_signal,
    },
    views::{Decorators, dyn_stack, label, scroll, stack, svg},
};
use std::{
    path::{Path, PathBuf},
    rc::Rc,
};

#[derive(Clone, Debug, PartialEq)]
struct FileRow {
    path: PathBuf,
    level: usize,
    directory: bool,
    count: usize,
    status: String,
}
fn file_tree(
    files: &[ChangedFile],
    closed: &std::collections::BTreeSet<PathBuf>,
) -> Vec<FileRow> {
    fn append(
        prefix: &Path,
        level: usize,
        files: &[ChangedFile],
        closed: &std::collections::BTreeSet<PathBuf>,
        rows: &mut Vec<FileRow>,
    ) {
        let mut directories = std::collections::BTreeSet::new();
        for file in files {
            if let Ok(relative) = file.path.strip_prefix(prefix) {
                if relative.components().count() > 1 {
                    if let Some(first) = relative.components().next() {
                        directories.insert(prefix.join(first.as_os_str()));
                    }
                }
            }
        }
        for path in directories {
            let count = files
                .iter()
                .filter(|file| file.path.starts_with(&path))
                .count();
            rows.push(FileRow {
                path: path.clone(),
                level,
                directory: true,
                count,
                status: String::new(),
            });
            if !closed.contains(&path) {
                append(&path, level + 1, files, closed, rows);
            }
        }
        for file in files
            .iter()
            .filter(|file| file.path.parent() == Some(prefix))
        {
            rows.push(FileRow {
                path: file.path.clone(),
                level,
                directory: false,
                count: 0,
                status: file.status.clone(),
            });
        }
    }
    let mut rows = Vec::new();
    append(Path::new(""), 0, files, closed, &mut rows);
    rows
}
pub fn panel(window: Rc<WindowTabData>) -> impl View {
    let config = window.common.config;
    let root = window
        .workspace
        .path
        .clone()
        .filter(|_| window.workspace.kind.is_local());
    let git_commands = window.common.internal_command;
    let state = create_rw_signal(Log::default());
    let refresh = create_rw_signal(0u64);
    let limit = create_rw_signal(500usize);
    let epoch = create_rw_signal(0u64);
    let query = create_rw_signal(String::new());
    let branch = create_rw_signal(String::new());
    let branch_query = create_rw_signal(String::new());
    let user = create_rw_signal(String::new());
    let since = create_rw_signal(String::new());
    let path = create_rw_signal(String::new());
    let closed = create_rw_signal(std::collections::BTreeSet::<PathBuf>::new());
    let selected_file = create_rw_signal(None::<PathBuf>);
    let selected = create_rw_signal(String::new());
    let detail = create_rw_signal(Details::default());
    let git_updates = window.source_control.file_diffs;
    let current_branch = window.source_control.branch;
    let local_open = create_rw_signal(true);
    let remote_open = create_rw_signal(true);
    create_effect(move |_| {
        refresh.get();
        let limit = limit.get();
        git_updates.get();
        current_branch.get();
        let query = query.get();
        let branch = branch.get();
        let user = user.get();
        let since = since.get();
        let path = path.get();
        let root = root.clone();
        epoch.update(|value| *value += 1);
        let request = epoch.get_untracked();
        let send =
            floem::ext_event::create_ext_action(Scope::current(), move |log| {
                if epoch.get_untracked() == request {
                    state.set(log);
                }
            });
        std::thread::spawn(move || {
            send(match root {
                Some(root) => {
                    load(&root, &branch, &user, &since, &path, &query, limit)
                }
                None => Log {
                    error: "Open a local Git project to view history.".into(),
                    ..Default::default()
                },
            })
        });
    });
    create_effect(move |_| {
        let hash = selected.get();
        if hash.is_empty() {
            detail.set(Details::default());
            return;
        }
        detail.set(Details {
            message: "Loading commit…".into(),
            ..Default::default()
        });
        let root = state.with_untracked(|state| state.root.clone());
        let parent = state.with_untracked(|state| {
            state
                .commits
                .iter()
                .find(|commit| commit.hash == hash)
                .and_then(|commit| commit.parents.first().cloned())
        });
        let send = floem::ext_event::create_ext_action(
            Scope::current(),
            move |(hash, result)| {
                if selected.get_untracked() == hash {
                    detail.set(result);
                }
            },
        );
        std::thread::spawn(move || {
            let message = output(
                Path::new("git"),
                &[
                    "show",
                    "-s",
                    "--format=%B%n%H%n%an <%ae>%n%ad",
                    "--date=local",
                    "--end-of-options",
                    &hash,
                    "--",
                ],
                Some(&root),
            );
            let changed = if let Some(parent) = parent {
                output(
                    Path::new("git"),
                    &[
                        "diff",
                        "--no-renames",
                        "--name-status",
                        "-z",
                        &parent,
                        &hash,
                        "--",
                    ],
                    Some(&root),
                )
            } else {
                output(
                    Path::new("git"),
                    &[
                        "diff-tree",
                        "--root",
                        "--no-commit-id",
                        "--no-renames",
                        "--name-status",
                        "-r",
                        "-z",
                        "--end-of-options",
                        &hash,
                        "--",
                    ],
                    Some(&root),
                )
            };
            send((
                hash,
                match (message, changed) {
                    (Ok(message), Ok(changed)) => Details {
                        message,
                        files: files(&changed),
                        error: String::new(),
                    },
                    (Err(error), _) | (_, Err(error)) => Details {
                        error,
                        ..Default::default()
                    },
                },
            ));
        });
    });
    let filtered = create_memo(move |_| state.with(|state| state.commits.clone()));
    create_effect(move |_| {
        let commits = filtered.get();
        let hash = selected.get_untracked();
        if !commits.iter().any(|commit| commit.hash == hash) {
            selected.set(
                commits
                    .first()
                    .map(|commit| commit.hash.clone())
                    .unwrap_or_default(),
            );
        }
    });
    let split = window.main_split.clone();
    let checkout = window.common.proxy.clone();
    let branches = move |remote: bool| {
        dyn_stack(
            move || {
                let q = branch_query.get().to_lowercase();
                state.with(|state| {
                    state
                        .branches
                        .iter()
                        .filter(|item| {
                            item.remote == remote
                                && item.name.to_lowercase().contains(&q)
                        })
                        .cloned()
                        .collect::<Vec<_>>()
                })
            },
            |item| item.reference.clone(),
            {
                let checkout = checkout.clone();
                move |item: Branch| {
                    let select = item.reference.clone();
                    let ref_for_menu = item.reference.clone();
                    let checkout = checkout.clone();
                    let name = item.name.clone();
                    label(move || {
                        format!("{} {}", if item.current { "◆" } else { "⑂" }, name)
                    })
                    .on_click_stop(move |_| branch.set(select.clone()))
                    .ide_context_menu(move || {
                        let reference = ref_for_menu.clone();
                        let checkout = checkout.clone();
                        Menu::new("")
                            .entry(MenuItem::new("Filter log by branch").action({
                                let reference = reference.clone();
                                move || branch.set(reference.clone())
                            }))
                            .entry(
                                MenuItem::new("Checkout branch")
                                    .enabled(!remote)
                                    .action(move || {
                                        checkout.git_checkout(reference.clone())
                                    }),
                            )
                    })
                    .style(move |s| {
                        s.height(24.0)
                            .items_center()
                            .padding_left(28.0)
                            .text_ellipsis()
                            .width_full()
                            .apply_if(
                                branch.with(|selected| *selected == item.reference),
                                |s| {
                                    s.background(config.get().color(
                                        SparrowColor::PANEL_HOVERED_BACKGROUND,
                                    ))
                                },
                            )
                    })
                }
            },
        )
        .style(move |s| {
            s.flex_col().width_full().apply_if(
                if remote {
                    !remote_open.get()
                } else {
                    !local_open.get()
                },
                |s| s.hide(),
            )
        })
    };
    let log_rows = scroll(
        dyn_stack(
            move || filtered.get(),
            |commit| {
                (
                    commit.hash.clone(),
                    commit.graph.clone(),
                    commit.refs.clone(),
                )
            },
            move |commit| {
                let select = commit.hash.clone();
                let check = commit.hash.clone();
                let copy = commit.hash.clone();
                let message = commit.subject.clone();
                stack((
                    svg(move || commit.graph.clone())
                        .style(|s| s.width(70.0).height(24.0).flex_shrink(0.0_f32)),
                    label(move || commit.subject.clone()).style(|s| {
                        s.flex_basis(0.0)
                            .flex_grow(1.0_f32)
                            .min_width(0.0)
                            .text_ellipsis()
                    }),
                    label(move || commit.refs.clone()).style(move |s| {
                        s.max_width_pct(28.0)
                            .text_ellipsis()
                            .color(config.get().color(SparrowColor::EDITOR_LINK))
                    }),
                    label(move || commit.author.clone())
                        .style(|s| s.width(130.0).min_width(0.0).text_ellipsis()),
                    label(move || commit.date.clone())
                        .style(|s| s.width(145.0).text_ellipsis()),
                ))
                .on_click_stop(move |_| selected.set(select.clone()))
                .ide_context_menu(move || {
                    let hash = copy.clone();
                    let message = message.clone();
                    Menu::new("")
                        .entry(crate::git_actions::menu(
                            git_commands,
                            Some(copy.clone()),
                        ))
                        .entry(
                            MenuItem::new("Copy Revision Number")
                                .action(move || crate::menus::copy_text(&hash)),
                        )
                        .entry(
                            MenuItem::new("Copy Commit Message")
                                .action(move || crate::menus::copy_text(&message)),
                        )
                })
                .style(move |s| {
                    s.height(24.0)
                        .width_full()
                        .items_center()
                        .gap(6.0)
                        .padding_right(8.0)
                        .apply_if(selected.get() == check, |s| {
                            s.background(
                                config.get().color(
                                    SparrowColor::SPARROW_TAB_ACTIVE_BACKGROUND,
                                ),
                            )
                        })
                })
            },
        )
        .style(|s| s.flex_col().width_full()),
    )
    .style(|s| s.flex_grow(1.0_f32).min_height(0.0).width_full());
    let log_id = log_rows.id();
    let log_rows = log_rows
        .keyboard_navigable()
        .on_event_cont(EventListener::PointerDown, move |_| log_id.request_focus())
        .on_event(EventListener::KeyDown, move |event| {
            if let Event::KeyDown(key) = event {
                if matches!(
                    key.key.logical_key,
                    Key::Named(NamedKey::ArrowUp | NamedKey::ArrowDown)
                ) {
                    let rows = filtered.get_untracked();
                    let at = rows
                        .iter()
                        .position(|row| row.hash == selected.get_untracked())
                        .unwrap_or(0);
                    let next =
                        if key.key.logical_key == Key::Named(NamedKey::ArrowUp) {
                            at.saturating_sub(1)
                        } else {
                            (at + 1).min(rows.len().saturating_sub(1))
                        };
                    if let Some(row) = rows.get(next) {
                        selected.set(row.hash.clone());
                    }
                    return EventPropagation::Stop;
                }
            }
            EventPropagation::Continue
        });
    stack((crate::panel::chrome::header(window,PanelKind::GitHistory,"Git",label(||"Log").style(move |s|s.height(24.0).padding_horiz(10.0).items_center().border_radius(3.0).background(config.get().color(SparrowColor::PANEL_HOVERED_BACKGROUND)))),
        stack((
            stack((text_input(branch_query, config).placeholder("Branch or tag"),
                label(||"HEAD (Current Branch)").on_click_stop(move |_|branch.set("HEAD".into())).style(|s|s.height(26.0).items_center().padding_left(18.0)),
                label(||"All branches").on_click_stop(move |_|branch.set(String::new())).style(|s|s.height(24.0).items_center().padding_left(18.0)),
                scroll(stack((label(move ||if local_open.get(){"⌄  Local"}else{"›  Local"}).on_click_stop(move |_|local_open.update(|value|*value=!*value)).style(|s|s.height(24.0).items_center().padding_left(8.0)),branches(false),label(move ||if remote_open.get(){"⌄  Remote"}else{"›  Remote"}).on_click_stop(move |_|remote_open.update(|value|*value=!*value)).style(|s|s.height(24.0).items_center().padding_left(8.0)),branches(true))).style(|s|s.flex_col().width_full())).style(|s|s.flex_grow(1.0_f32).width_full().min_height(0.0))
            )).style(move |s|s.width_pct(27.0).height_full().padding(6.0).flex_col().border_right(1.0).border_color(config.get().color(SparrowColor::SPARROW_BORDER))),
            stack((stack((text_input(query, config).placeholder("Text or hash").style(move |s|s.min_width(80.0).width_pct(32.0)),
                text_input(user, config).placeholder("User").style(move |s|s.width(100.0)),
                crate::widgets::menu_button(move || if since.with(|s| s.is_empty()) { "Date".to_string() } else { since.get() }, move || {
                    let mut menu=Menu::new("");for (label,value) in [("All time",""),("Last 24 hours","24 hours ago"),("Last 7 days","7 days ago"),("Last 30 days","30 days ago")]{menu=menu.entry(MenuItem::new(label).action(move ||since.set(value.into())));}menu
                }, config).style(|s| s.max_width(130.0)),
                text_input(path, config).placeholder("Paths").style(move |s|s.width(95.0)),
                label(||"Load more").on_click_stop(move |_|limit.update(|n|*n+=500)).style(|s|s.padding_horiz(5.0)),
                crate::widgets::menu_button(|| "Git Actions", move || crate::git_actions::menu(git_commands, None), config),
                clickable_icon(||SparrowIcons::IMAGE_LOADING,move ||refresh.update(|n|*n+=1),||false,||false,||"Refresh Git Log",config)
            )).style(|s|s.height(30.0).width_full().gap(6.0).padding_horiz(6.0).items_center()),
                label(move ||state.with(|s|s.error.clone())).style(move |s|s.width_full().padding(12.0).apply_if(state.with(|s|s.error.is_empty()),|s|s.hide())),
                label(||"No commits match these filters.").style(move |s|s.padding(16.0).apply_if(!filtered.with(|r|r.is_empty()),|s|s.hide())),log_rows
            )).style(|s|s.width_pct(48.0).height_full().flex_col()),
            stack((scroll(dyn_stack(move ||detail.with(|d|file_tree(&d.files,&closed.get())),move |file|(selected.get_untracked(),file.path.clone(),file.directory),move |file|{
                let target=file.path.clone();let folder_toggle=file.path.clone();let selection=file.path.clone();let highlight=file.path.clone();let directory=file.directory;let level=file.level;let count=file.count;let revision=selected.get_untracked();let split=split.clone();let root=state.with_untracked(|s|s.root.clone());let folder=file.path.parent().unwrap_or(Path::new("")).to_string_lossy().to_string();
                stack((label(move ||if directory{if closed.with(|set|set.contains(&folder_toggle)){"›"}else{"⌄"}}else{""}).style(|s|s.width(10.0)),
                    svg({let path=file.path.clone();move ||if directory{config.get().folder_svg(&path).0}else{config.get().file_svg(&path).0}}).style(|s|s.size(14.0,14.0)),label(move ||if directory{format!("{}  {count} files",file.path.file_name().unwrap_or_default().to_string_lossy())}else{file.path.file_name().unwrap_or_default().to_string_lossy().to_string()}).style(|s|s.text_ellipsis()),label(move ||if directory{String::new()}else{folder.clone()}).style(move |s|s.min_width(0.0).text_ellipsis().color(config.get().color(SparrowColor::EDITOR_DIM)))))
                    .on_click_stop(move |_|{selected_file.set(Some(selection.clone()));if directory{closed.update(|set|{if !set.remove(&selection){set.insert(selection.clone());}});}})
                    .on_double_click_stop(move |_|{if !directory{split.open_revision_diff(root.join(&target),revision.clone(),root.clone());}})
                    .style(move |s|s.height(24.0).width_full().items_center().gap(6.0).padding_left((level*14+6)as f32).padding_right(6.0).apply_if(selected_file.with(|selected|selected.as_ref()==Some(&highlight)),|s|s.background(config.get().color(SparrowColor::PANEL_HOVERED_BACKGROUND))).color(config.get().color(match file.status.as_str(){"A"=>SparrowColor::SOURCE_CONTROL_ADDED,"D"=>SparrowColor::SOURCE_CONTROL_REMOVED,_=>if directory{SparrowColor::PANEL_FOREGROUND}else{SparrowColor::SOURCE_CONTROL_MODIFIED}})))
            }).style(|s|s.flex_col().width_full())).style(move |s|s.width_full().height_pct(50.0).min_height(40.0).border_bottom(1.0).border_color(config.get().color(SparrowColor::SPARROW_BORDER))),
                scroll(label(move ||detail.with(|d|if d.error.is_empty(){d.message.clone()}else{d.error.clone()})).style(|s|s.width_full().padding(12.0))).style(|s|s.width_full().flex_grow(1.0_f32).min_height(0.0))
            )).style(move |s|s.width_pct(25.0).height_full().flex_col().border_left(1.0).border_color(config.get().color(SparrowColor::SPARROW_BORDER)))
        )).style(|s|s.width_full().flex_grow(1.0_f32).min_height(0.0))
    )).style(|s|s.absolute().size_full().flex_col().font_size(13.0))
}
