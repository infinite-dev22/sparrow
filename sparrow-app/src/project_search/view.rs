//! Dedicated Find in Files and Replace in Files dialogs.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    rc::Rc,
    time::Duration,
};

use floem::{
    View,
    action::exec_after,
    event::{Event, EventListener, EventPropagation},
    keyboard::{Key, NamedKey},
    reactive::{
        SignalGet, SignalUpdate, SignalWith, create_effect, create_rw_signal,
    },
    views::{
        Decorators, dyn_container, dyn_stack, empty, label, stack, stack_from_iter,
    },
};

use super::{FileScope, Results, preview, scan};
use crate::{
    config::{color::SparrowColor, icon::SparrowIcons},
    editor_tab::EditorTabChild,
    panel::kind::PanelKind,
    widgets::{
        ButtonKind, Divider, button, empty_state,
        floating_search::{floating_search, search_field},
        labeled_checkbox,
        modal_focus::{ModalChoice, ModalFocus, restore_workbench_on_close},
        search_match_options::search_match_options,
        search_result_row::{search_result_bounds, search_result_row},
        sparrow_scroll, tab_button,
        text_input::text_input,
    },
    window_tab::WindowTabData,
};

pub fn view(window: Rc<WindowTabData>) -> impl View {
    let state = window.project_search.clone();
    let active = state.active;
    restore_workbench_on_close(active, window.common.clone());
    let config = window.common.config;
    let root = window.workspace.path.clone();
    let focus = ModalFocus::default();
    let selected = create_rw_signal(0usize);
    let searching = create_rw_signal(false);
    let requests = create_rw_signal(0u64);
    let results = state.results;
    let replacement = state.replacement;
    let query = state.query;
    let busy = state.busy;
    let scope = state.scope;
    let directory = state.directory;
    let module = state.module;
    let open_only = state.open_only;
    let mask = state.mask;
    let mask_enabled = state.mask_enabled;
    let (case_sensitive, whole_words, regex_search) =
        (state.case, state.word, state.regex);

    create_effect({
        let state = state.clone();
        let window = window.clone();
        let root = root.clone();
        move |_| {
            requests.update(|id| *id += 1);
            let request = requests.get_untracked();
            if !active.get() {
                return;
            }
            let query = query.get();
            let dir = match scope.get() {
                FileScope::Directory => directory.get(),
                FileScope::Module => module.get().to_string_lossy().into_owned(),
                _ => String::new(),
            };
            let mask = if mask_enabled.get() {
                mask.get()
            } else {
                String::new()
            };
            let case = state.case.get();
            let word = state.word.get();
            let regex = state.regex.get();
            let open_paths = (scope.get() == FileScope::Scope && open_only.get())
                .then(|| open_files(&window));
            state.epoch.get();
            results.set(Results::default());
            selected.set(0);
            searching.set(!query.is_empty());
            if query.is_empty() {
                return;
            }
            let root = root.clone();
            let progress = window.common.background_tasks.reporter();
            let owner = window.scope;
            exec_after(Duration::from_millis(160), move |_| {
                if requests.try_get_untracked() != Some(request) {
                    return;
                }
                let send =
                    floem::ext_event::create_ext_action(owner, move |result| {
                        if requests.try_get_untracked() == Some(request)
                            && active.get_untracked()
                        {
                            searching.set(false);
                            results.set(result);
                        }
                    });
                std::thread::spawn(move || {
                    let _task = progress.begin("Searching project files");
                    send(root.map_or_else(
                        || Results {
                            error: "Open a project to search its files.".into(),
                            ..Default::default()
                        },
                        |root| {
                            scan(
                                &root,
                                &dir,
                                &mask,
                                &query,
                                open_paths.as_ref(),
                                case,
                                word,
                                regex,
                            )
                        },
                    ));
                });
            });
        }
    });

    focus.set_list_navigation(selected, move || {
        results.with_untracked(|r| r.hits.len())
    });
    let input = focus.input_with_placeholder(query, config, "Search");
    let input_id = input.id();
    create_effect(move |_| {
        if active.get() {
            exec_after(Duration::from_millis(30), move |_| {
                if active.get_untracked() {
                    input_id.request_focus();
                }
            });
        }
    });
    let open = {
        let window = window.clone();
        move || {
            if searching.get_untracked() || busy.get_untracked() {
                return;
            }
            if let Some(hit) = results
                .with_untracked(|r| r.hits.get(selected.get_untracked()).cloned())
            {
                active.set(false);
                window.main_split.jump_to_location(hit.location(), None);
            }
        }
    };
    focus.set_submit(open.clone());

    let search = search_field(
        input,
        search_match_options(&focus, state.case, state.word, state.regex, config),
        config,
        SparrowIcons::SEARCH,
    );
    let replacements = search_field(
        focus.control(text_input(state.replace, config).placeholder("Replace with")),
        empty(),
        config,
        SparrowIcons::SEARCH_REPLACE,
    )
    .style(move |s| s.apply_if(!replacement.get(), |s| s.hide()));

    let mask_options = stack((
        focus.control(labeled_checkbox(
            move || mask_enabled.get(),
            || "File mask",
            move |checked| mask_enabled.set(checked),
            config,
        )),
        focus
            .control_if(text_input(mask, config).placeholder("*.dart"), move || {
                mask_enabled.get_untracked()
            })
            .disabled(move || !mask_enabled.get())
            .style(|s| s.width(92)),
        focus
            .menu_if(
                button(|| "⌄", ButtonKind::Plain, config),
                move || mask_enabled.get_untracked(),
                move || {
                    [
                        "*.dart", "*.rs", "*.java", "*.kt", "*.swift", "*.toml",
                        "*.yaml", "*.json", "*.xml",
                    ]
                    .into_iter()
                    .map(|pattern| {
                        ModalChoice::new(pattern, move || mask.set(pattern.into()))
                            .selected(mask.get_untracked() == pattern)
                    })
                    .collect()
                },
            )
            .disabled(move || !mask_enabled.get()),
    ))
    .style(|s| s.items_center().gap(4));

    let tabs = stack_from_iter(FileScope::TABS.map(|(kind, title)| {
        focus.control(tab_button(
            label(move || title),
            move || scope.get() == kind,
            move || scope.set(kind),
            config,
        ))
    }))
    .style(|s| s.items_center().flex_wrap(floem::style::FlexWrap::Wrap));
    let directory_picker = focus.clone();
    let root_for_picker = root.clone();
    let directory_controls = stack((
        focus
            .control(text_input(directory, config).placeholder("Project root"))
            .style(|s| s.flex_grow(1.0_f32).flex_basis(0)),
        focus.control(
            button(|| "Browse…", ButtonKind::Plain, config).on_click_stop(
                move |_| {
                    let mut options = crate::widgets::file_picker::FileDialogOptions::new()
                        .title("Search directory")
                        .select_directories();
                    if let Some(root) = &root_for_picker {
                        options = options.force_starting_directory(root.clone());
                    }
                    directory_picker.open_file(options, config, move |file| {
                        if let Some(path) =
                            file.and_then(|file| file.path.into_iter().next())
                        {
                            directory.set(path.display().to_string());
                        }
                    });
                },
            ),
        ),
    ))
    .style(move |s| {
        s.min_width(0)
            .flex_grow(1.0_f32)
            .flex_basis(0)
            .gap(4)
            .apply_if(scope.get() != FileScope::Directory, |s| s.hide())
    });
    let module_menu = focus
        .menu(
            button(
                move || {
                    if module.with(|p| p.as_os_str().is_empty()) {
                        "Root module".into()
                    } else {
                        module.with(|p| p.display().to_string())
                    }
                },
                ButtonKind::Outlined,
                config,
            ),
            move || {
                let mut choices = vec![ModalChoice::new("Root module", move || {
                    module.set(PathBuf::new())
                })];
                if let Some(root) = &root {
                    for path in modules(root) {
                        let name = path.display().to_string();
                        let selected = module.get_untracked() == path;
                        choices.push(
                            ModalChoice::new(name, move || module.set(path.clone()))
                                .selected(selected),
                        );
                    }
                }
                choices
            },
        )
        .style(move |s| s.apply_if(scope.get() != FileScope::Module, |s| s.hide()));
    let scope_menu = focus
        .menu(
            button(
                move || {
                    if open_only.get() {
                        "Open Files"
                    } else {
                        "Project Files"
                    }
                },
                ButtonKind::Outlined,
                config,
            ),
            move || {
                [("Project Files", false), ("Open Files", true)]
                    .into_iter()
                    .map(|(label, value)| {
                        ModalChoice::new(label, move || open_only.set(value))
                            .selected(open_only.get_untracked() == value)
                    })
                    .collect()
            },
        )
        .style(move |s| s.apply_if(scope.get() != FileScope::Scope, |s| s.hide()));

    let bounds_root = window.workspace.path.clone();
    let rows = sparrow_scroll(
        dyn_stack(
            move || results.with(|r| r.hits.clone()).into_iter().enumerate(),
            |(index, hit)| (*index, hit.path.clone(), hit.offset, hit.text.clone()),
            {
                let open = open.clone();
                move |(index, hit): (usize, super::Hit)| {
                    let path = hit
                        .path
                        .strip_prefix(
                            bounds_root.as_deref().unwrap_or(Path::new("")),
                        )
                        .unwrap_or(&hit.path);
                    let source = format!("{}:{}", path.display(), hit.line);
                    let open = open.clone();
                    let (snippet, highlights) = hit.snippet();
                    search_result_row(
                        crate::widgets::focus_text::focus_text(
                            move || snippet.clone(),
                            move || highlights.clone(),
                            move || config.get().color(SparrowColor::EDITOR_FOCUS),
                        )
                        .style(|s| {
                            s.flex_grow(1.0_f32)
                                .flex_basis(0)
                                .min_width(0)
                                .text_ellipsis()
                                .selectable(false)
                        }),
                        move || source.clone(),
                        48.0,
                        move || selected.get() == index,
                        move || {
                            selected.set(index);
                            input_id.request_focus();
                        },
                        open,
                        config,
                    )
                }
            },
        )
        .style(|s| s.width_full().flex_col()),
    )
    .ensure_visible(move || search_result_bounds(selected.get()))
    .style(move |s| {
        s.width_full()
            .min_height(0)
            .flex_basis(0)
            .flex_grow(1.0_f32)
            .apply_if(results.with(|r| r.hits.is_empty()), |s| s.hide())
    });
    let preview_window = window.clone();
    let excerpt = dyn_container(
        move || results.with(|r| r.hits.get(selected.get()).cloned()),
        move |hit| preview::view(preview_window.clone(), hit),
    )
    .style(move |s| {
        s.width_full()
            .flex_basis(0)
            .flex_grow(1.2_f32)
            .min_height(0)
            .apply_if(results.with(|r| r.hits.is_empty()), |s| s.hide())
    });

    let enabled = move || {
        !busy.get() && !searching.get() && results.with(|r| !r.hits.is_empty())
    };
    let apply_one = state.clone();
    let apply_all = state.clone();
    let one_window = window.clone();
    let all_window = window.clone();
    let transfer = window.clone();
    let replace_buttons = stack((
        focus.control_if(
            button(|| "Replace All", ButtonKind::Outlined, config)
                .on_click_stop(move |_| apply_all.apply(all_window.clone(), None))
                .disabled(move || !enabled() || results.with(|r| r.truncated)),
            move || enabled() && !results.with(|r| r.truncated),
        ),
        focus.control_if(
            button(|| "Replace", ButtonKind::Primary, config)
                .on_click_stop(move |_| {
                    apply_one
                        .apply(one_window.clone(), Some(selected.get_untracked()))
                })
                .disabled(move || !enabled()),
            enabled,
        ),
    ))
    .style(move |s| s.gap(6).apply_if(!replacement.get(), |s| s.hide()));
    let footer = stack((
        label(move || state.message.get())
            .style(|s| s.flex_grow(1.0_f32).min_width(0).text_ellipsis()),
        focus.control_if(
            button(|| "Open in Find Window", ButtonKind::Outlined, config)
                .on_click_stop(move |_| {
                    if enabled() {
                        let hits = results.with_untracked(|r| r.hits.clone());
                        floem::reactive::batch(|| {
                            active.set(false);
                            transfer.show_panel(PanelKind::Search);
                            transfer.global_search.publish_results(
                                query.get_untracked(),
                                &hits,
                                case_sensitive.get_untracked(),
                                whole_words.get_untracked(),
                                regex_search.get_untracked(),
                            );
                        });
                    }
                })
                .disabled(move || !enabled()),
            enabled,
        ),
        replace_buttons,
        focus.control(button(|| "Close", ButtonKind::Plain, config).on_click_stop(
            move |_| {
                if !busy.get_untracked() {
                    active.set(false);
                }
            },
        )),
    ))
    .style(|s| {
        s.width_full()
            .items_center()
            .gap(8)
            .flex_wrap(floem::style::FlexWrap::Wrap)
    });

    let body = stack((
        stack((
            label(move || {
                if replacement.get() {
                    "Replace in Files"
                } else {
                    "Find in Files"
                }
            })
            .style(|s| s.font_bold()),
            label(move || {
                results.with(|r| {
                    if r.hits.is_empty() {
                        return String::new();
                    }
                    let (hits, files) = (r.hits.len(), r.files.len());
                    let suffix = if r.truncated { "+" } else { "" };
                    let match_label = if hits == 1 { "match" } else { "matches" };
                    let file_label = if files == 1 { "file" } else { "files" };
                    format!("{hits}{suffix} {match_label} in {files} {file_label}")
                })
            })
            .style(move |s| {
                s.flex_grow(1.0_f32)
                    .color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
            }),
            mask_options,
        ))
        .style(|s| {
            s.items_center()
                .gap(10)
                .flex_wrap(floem::style::FlexWrap::Wrap)
        }),
        Divider::new(config).into_view(),
        search,
        replacements,
        stack((tabs, directory_controls, module_menu, scope_menu)).style(|s| {
            s.items_center()
                .min_width(0)
                .width_full()
                .gap(8)
                .flex_wrap(floem::style::FlexWrap::Wrap)
        }),
        stack((
            empty_state(
                move || {
                    if searching.get() {
                        return "Searching…".into();
                    }
                    if query.with(String::is_empty) {
                        return "Type a search query to find text in files.".into();
                    }
                    results.with(|r| {
                        if r.error.is_empty() {
                            "No matching text found.".into()
                        } else {
                            r.error.clone()
                        }
                    })
                },
                config,
            )
            .style(move |s| {
                s.apply_if(results.with(|r| !r.hits.is_empty()), |s| s.hide())
            }),
            rows,
            excerpt,
        ))
        .style(|s| s.flex_col().min_height(0).width_full().flex_grow(1.0_f32)),
        Divider::new(config).into_view(),
        footer,
    ))
    .style(|s| s.padding(12).flex_col().gap(8));
    let dialog = floating_search(
        body,
        active,
        config,
        None,
        move || !busy.get_untracked(),
        |s| s.width(728).height(590).max_height_pct(89.0),
    )
    .on_event(EventListener::KeyDown, move |event| {
        if let Event::KeyDown(key) = event {
            if key.key.logical_key == Key::Named(NamedKey::Escape)
                && !busy.get_untracked()
            {
                active.set(false);
                return EventPropagation::Stop;
            }
        }
        EventPropagation::Continue
    });
    focus
        .bind_root(dialog, active, config)
        .style(|s| s.z_index(100))
}

pub(crate) fn open_files(window: &WindowTabData) -> HashSet<PathBuf> {
    window.main_split.editor_tabs.with(|tabs| {
        tabs.values()
            .flat_map(|tab| {
                tab.with(|tab| {
                    tab.children
                        .iter()
                        .filter_map(|(_, _, child)| {
                            let EditorTabChild::Editor(id) = child else {
                                return None;
                            };
                            let editor =
                                window.main_split.editors.editor_untracked(*id)?;
                            editor
                                .doc()
                                .content
                                .with_untracked(|content| content.path().cloned())
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect()
    })
}

fn modules(root: &Path) -> Vec<PathBuf> {
    let mut choices = Vec::new();
    for parent in [
        root.to_path_buf(),
        root.join("android"),
        root.join("packages"),
        root.join("crates"),
    ] {
        for entry in std::fs::read_dir(parent).into_iter().flatten().flatten() {
            let path = entry.path();
            if [
                "pubspec.yaml",
                "Cargo.toml",
                "build.gradle",
                "build.gradle.kts",
                "package.json",
            ]
            .into_iter()
            .any(|manifest| path.join(manifest).is_file())
                && let Ok(relative) = path.strip_prefix(root)
            {
                choices.push(relative.to_path_buf());
            }
        }
    }
    choices.sort();
    choices.dedup();
    choices
}
