//! Project search, match results and replacement preview.

use super::{Hit, Results, pattern, scan};
use crate::{config::color::SparrowColor, window_tab::WindowTabData};
use floem::{
    View,
    event::{Event, EventListener, EventPropagation},
    keyboard::{Key, NamedKey},
    reactive::{Scope, SignalGet, SignalUpdate, SignalWith},
    views::{Decorators, container, dyn_stack, label, scroll, stack},
};
use std::{path::PathBuf, rc::Rc};

pub fn view(window: Rc<WindowTabData>) -> impl View {
    let state = window.project_search.clone();
    let active = state.active;
    crate::widgets::modal_focus::restore_workbench_on_close(active, window.common.clone());
    let query = state.query;
    let replacement = state.replacement;
    let replace = state.replace;
    let mode = state.mode;
    let directory = state.directory;
    let mask = state.mask;
    let case = state.case;
    let word = state.word;
    let regex = state.regex;
    let results = state.results;
    let epoch = state.epoch;
    let busy = state.busy;
    let message = state.message;
    let config = window.common.config;
    let root = window.workspace.path.clone();
    let selected = floem::reactive::create_rw_signal(0usize);
    let lsp = window.common.dart_lsp.clone();
    let progress = window.common.background_tasks.reporter();
    let searching=floem::reactive::create_rw_signal(false);
    let requests = floem::reactive::create_rw_signal(0u64);
    floem::reactive::create_effect(move |_| {
        let open = active.get();
        let query = query.get();
        let dir = directory.get();
        let mask = mask.get();
        let mode = mode.get();
        let case = case.get();
        let word = word.get();
        let regex = regex.get();
        epoch.get();
        if !open {
            return;
        }
        searching.set(true);
        requests.update(|e| *e += 1);
        let request = requests.get_untracked();
        results.set(Results::default());
        selected.set(0);
        let root = root.clone();
        let lsp = lsp.clone();
        let progress = progress.clone();
        let send = floem::ext_event::create_ext_action(Scope::current(), move |r| {
            if requests.get_untracked() == request {
                searching.set(false);
                results.set(r);
            }
        });
        floem::action::exec_after(
            std::time::Duration::from_millis(180),
            move |_| {
                if requests.get_untracked() != request {
                    return;
                }
                if mode >= 4 {
                    if !lsp.is_available() {
                        send(Results {
                            error:
                                "Start Dart Analysis to search classes and symbols."
                                    .into(),
                            ..Default::default()
                        });
                        return;
                    }
                    lsp.request_async(
                        "workspace/symbol",
                        serde_json::json!({"query":query}),
                        move |response| {
                            if let Some(error) = response.get("error") {
                                send(Results {
                                    error: error["message"]
                                        .as_str()
                                        .unwrap_or("Symbol search failed.")
                                        .into(),
                                    ..Default::default()
                                });
                                return;
                            }
                            let hits = response
                                .get("result")
                                .and_then(|v| v.as_array())
                                .into_iter()
                                .flatten()
                                .filter(|item| {
                                    mode != 4
                                        || matches!(
                                            item["kind"].as_u64(),
                                            Some(5 | 10 | 11 | 23)
                                        )
                                })
                                .filter_map(|item| {
                                    let uri = item["location"]["uri"].as_str()?;
                                    let path = lsp_types::Url::parse(uri)
                                        .ok()?
                                        .to_file_path()
                                        .ok()?;
                                    if root
                                        .as_deref()
                                        .is_some_and(|root| !path.starts_with(root))
                                    {
                                        return None;
                                    }
                                    let position = serde_json::from_value::<
                                        lsp_types::Position,
                                    >(
                                        item["location"]["range"]["start"].clone(),
                                    )
                                    .ok();
                                    Some(Hit {
                                        path,
                                        offset: 0,
                                        position,
                                        line: position
                                            .map_or(0, |p| p.line as usize + 1),
                                        text: item["name"]
                                            .as_str()
                                            .unwrap_or_default()
                                            .into(),
                                        action: None,
                                    })
                                })
                                .take(2000)
                                .collect();
                            send(Results {
                                hits,
                                ..Default::default()
                            });
                        },
                    );
                    return;
                }
                let task = progress.begin("Searching project");
                std::thread::spawn(move || {
                    let _task = task;
                    if mode == 2 {
                        let pattern = pattern(&query, case, word, regex);
                        let hits = crate::command::sparrow_internal_commands()
                            .into_values()
                            .filter_map(|command| {
                                let title = command.kind.desc()?.to_string();
                                if pattern.as_ref().is_ok_and(|p| p.is_match(&title))
                                {
                                    Some(Hit {
                                        path: PathBuf::new(),
                                        offset: 0,
                                        position: None,
                                        line: 0,
                                        text: title,
                                        action: Some(command),
                                    })
                                } else {
                                    None
                                }
                            })
                            .collect();
                        send(Results {
                            hits,
                            ..Default::default()
                        });
                    } else {
                        send(
                            root.as_deref()
                                .map(|root| {
                                    let mut result=scan(root,&dir,&mask,&query,mode,case,word,regex);
                                    if mode==3&&!query.is_empty(){if let Ok(pattern)=pattern(&query,case,word,regex){for command in crate::command::sparrow_internal_commands().into_values(){if let Some(title)=command.kind.desc(){if pattern.is_match(title){result.hits.push(Hit{path:PathBuf::new(),offset:0,position:None,line:0,text:title.into(),action:Some(command)});}}}}}
                                    result
                                })
                                .unwrap_or_else(|| Results {
                                    error: "Open a local project to search files."
                                        .into(),
                                    ..Default::default()
                                }),
                        );
                    }
                });
            },
        );
    });
    let focus = crate::widgets::modal_focus::ModalFocus::default();
    let input = focus.input(query, config);
    let input_id = input.id();
    floem::reactive::create_effect(move |_| {
        if active.get() {
            floem::action::exec_after(
                std::time::Duration::from_millis(30),
                move |_| input_id.request_focus(),
            );
        }
    });
    let mut tabs = vec![];
    for (title, value) in [
        ("All", 3),
        ("Classes", 4),
        ("Files", 1),
        ("Symbols", 5),
        ("Actions", 2),
        ("Text", 0),
    ] {
        tabs.push(
            focus.control(
                label(move || title)
                    .on_click_stop(move |_| {
                        mode.set(value);
                        replacement.set(false);
                    })
                    .style(move |s| {
                        s.padding_horiz(10.0).height(26.0).items_center().apply_if(
                            mode.get() == value,
                            |s| {
                                s.background(
                                    config.get().color(
                                        SparrowColor::PANEL_HOVERED_BACKGROUND,
                                    ),
                                )
                            },
                        )
                    }),
            ),
        );
    }
    let commands = window.common.sparrow_command;
    let internal = window.common.internal_command;
    let run = state.clone();
    let w = window.clone();
    let all = state.clone();

    let accept = window.common.internal_command;
    let accept_commands = window.common.sparrow_command;
    focus.set_submit(move || {
        if let Some(hit) =
            results.with_untracked(|r| r.hits.get(selected.get_untracked()).cloned())
        {
            active.set(false);
            if let Some(command) = hit.action {
                accept_commands.send(command);
            } else {
                accept.send(crate::command::InternalCommand::JumpToLocation {
                    location: crate::editor::location::EditorLocation {
                        path: hit.path,
                        position: Some(
                            hit.position.map(crate::editor::location::EditorPosition::Position).unwrap_or(crate::editor::location::EditorPosition::Offset(hit.offset)),
                        ),
                        scroll_offset: None,
                        ignore_unconfirmed: false,
                        same_editor_tab: false,
                    },
                });
            }
        }
    });
    let display_root = window.workspace.path.clone();
    let rows = scroll(
        dyn_stack(
            move || {
                results
                    .get()
                    .hits
                    .into_iter()
                    .enumerate()
                    .collect::<Vec<_>>()
            },
            |(i, hit)| (*i, hit.path.clone(), hit.offset, hit.text.clone()),
            move |(index, hit)| {
                let row = hit.clone();
                let display_path=display_root.as_deref().and_then(|root|hit.path.strip_prefix(root).ok()).unwrap_or(&hit.path).display().to_string();
                label(move || {
                    format!(
                        "{}{}  {}",
                        display_path,
                        if hit.line > 0 {
                            format!(":{}", hit.line)
                        } else {
                            String::new()
                        },
                        hit.text
                    )
                })
                .on_click_stop(move |_| {
                    active.set(false);
                    if let Some(action) = row.action.clone() {
                        commands.send(action);
                    } else {
                        internal
                            .send(crate::command::InternalCommand::JumpToLocation {
                            location: crate::editor::location::EditorLocation {
                                path: row.path.clone(),
                                position: Some(
                                    row.position.map(crate::editor::location::EditorPosition::Position).unwrap_or(crate::editor::location::EditorPosition::Offset(row.offset)),
                                ),
                                scroll_offset: None,
                                ignore_unconfirmed: false,
                                same_editor_tab: false,
                            },
                        });
                    }
                })
                .style(move |s| {
                    s.height(26.0)
                        .width_full()
                        .padding_horiz(8.0)
                        .items_center()
                        .text_ellipsis()
                        .apply_if(selected.get() == index, |s| {
                            s.background(
                                config.get().color(
                                    SparrowColor::SPARROW_TAB_ACTIVE_BACKGROUND,
                                ),
                            )
                        })
                        .hover(|s| {
                            s.background(
                                config
                                    .get()
                                    .color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                            )
                        })
                })
            },
        )
        .style(|s| s.flex_col().width_full()),
    )
    .style(|s| s.height(340.0).width_full())
    .ensure_visible(move || {
        floem::kurbo::Size::new(1.0, 26.0)
            .to_rect()
            .with_origin(floem::kurbo::Point::new(0.0, selected.get() as f64 * 26.0))
    });
    let body=stack((label(move ||if replacement.get(){"Replace in Files"}else{if mode.get()==0 {"Find in Files"}else{"Search Everywhere"}}).style(|s|s.font_bold()),floem::views::stack_from_iter(tabs).style(|s|s.gap(4.0)),input,
 stack((focus.input(replace, config),focus.control(label(||"Replace All").on_click_stop(move |_|run.apply(w.clone())).disabled(move ||busy.get())))).style(move |s|s.width_full().gap(8.0).apply_if(!replacement.get(),|s|s.hide())),
 stack((focus.control(label(move ||format!("{} Match case",if case.get(){"☑"}else{"☐"})).on_click_stop(move |_|case.update(|v|*v=!*v))),focus.control(label(move ||format!("{} Words",if word.get(){"☑"}else{"☐"})).on_click_stop(move |_|word.update(|v|*v=!*v))),focus.control(label(move ||format!("{} Regex",if regex.get(){"☑"}else{"☐"})).on_click_stop(move |_|regex.update(|v|*v=!*v))),focus.control(label(||"Symbols…").on_click_stop(move |_|{active.set(false);commands.send(crate::command::SparrowCommand{kind:crate::command::CommandKind::Workbench(crate::command::SparrowWorkbenchCommand::PaletteWorkspaceSymbol),data:None});})),)).style(|s|s.gap(14.0)),
 stack((label(||"Directory (empty = project)"),focus.input(directory, config))).style(|s|s.width_full().gap(8.0)),stack((label(||"File mask"),focus.input(mask, config))).style(|s|s.width_full().gap(8.0)),
 label(move ||results.with(|r|if searching.get(){"Searching…".into()}else if !r.error.is_empty(){r.error.clone()}else{format!("{} results in {} files{}",r.hits.len(),r.hits.iter().filter(|h|!h.path.as_os_str().is_empty()).map(|h|&h.path).collect::<std::collections::HashSet<_>>().len(),if r.truncated{" · limit reached; narrow your search"}else{""})})),rows,label(move ||message.get()),stack((focus.control(label(||"Replace mode").on_click_stop(move |_|{replacement.update(|v|*v=!*v);mode.set(0);})),focus.control(label(||"Close").on_click_stop(move |_|{if !busy.get_untracked(){all.active.set(false);}})))).style(|s|s.gap(20.0)))).style(move |s| {
        crate::widgets::dialog_surface(
            s.flex_col()
                .width(740.0)
                .max_width_pct(95.0).max_height_pct(95.0)
                .gap(10.0)
                .padding(16.0),
            &config.get(),
        )
    });

    let dialog = container(body)
        .style(move |s| {
            s.absolute()
                .size_full()
                .items_center()
                .justify_center()
                .z_index(100)
                .apply_if(!active.get(), |s| s.hide())
        })
        .on_event_stop(EventListener::PointerDown, |_| {})
        .on_event(EventListener::KeyDown, move |e| {
            if let Event::KeyDown(key) = e {
                if key.key.logical_key == Key::Named(NamedKey::Escape)
                    && !busy.get_untracked()
                {
                    active.set(false);
                    return EventPropagation::Stop;
                }
                let len = results.with_untracked(|r| r.hits.len());
                if len > 0 && key.modifiers.is_empty() {
                    match key.key.logical_key {
                        Key::Named(NamedKey::ArrowDown) => {
                            selected.update(|s| *s = (*s + 1).min(len - 1));
                            return EventPropagation::Stop;
                        }
                        Key::Named(NamedKey::ArrowUp) => {
                            selected.update(|s| *s = s.saturating_sub(1));
                            return EventPropagation::Stop;
                        }
                        _ => {}
                    }
                }
            }
            EventPropagation::Continue
        });
    focus.bind_root(dialog, active, config)
}
