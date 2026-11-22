//! Search Everywhere popup and input handling.

use super::{Places, Request, ResultTarget, Row, Tab, search, symbol_rows};
use crate::{
    config::{color::SparrowColor, icon::SparrowIcons},
    project_search::{
        preview,
        view::{open_files, row_style},
    },
    widgets::{
        ButtonKind, Divider, button, dialog_surface, empty_state, labeled_checkbox,
        modal_focus::{
            ModalChoice, ModalFocus, list_destination, restore_workbench_on_close,
        },
        sparrow_scroll, tab_button, tab_strip,
        text_input::text_input,
    },
    window_tab::WindowTabData,
};
use floem::{
    View,
    action::exec_after,
    event::{Event, EventListener, EventPropagation},
    keyboard::{Key, Modifiers, NamedKey},
    reactive::{
        RwSignal, SignalGet, SignalUpdate, SignalWith, batch, create_effect,
        create_memo, create_rw_signal,
    },
    views::{
        Decorators, container, dyn_container, dyn_stack, empty, label, stack,
        stack_from_iter, svg,
    },
};
use std::{rc::Rc, time::Duration};

pub(crate) fn view(window: Rc<WindowTabData>) -> impl View {
    let state = window.search_everywhere;
    let active = state.active;
    let tab = state.tab;
    let query = state.query;
    let places = state.places;
    let preview = state.preview;
    let config = window.common.config;
    let focus = ModalFocus::default();
    let selected = create_rw_signal(0usize);
    let base = create_rw_signal(Vec::<Row>::new());
    let symbols = create_rw_signal(Vec::<Row>::new());
    let message = create_rw_signal(String::new());
    let generation = create_rw_signal(0u64);
    let search_pending = create_rw_signal(false);
    restore_workbench_on_close(active, window.common.clone());

    create_effect({
        let window = window.clone();
        move |_| {
            generation.update(|id| *id += 1);
            let id = generation.get_untracked();
            if !active.get() {
                return;
            }
            let request = Request {
                tab: tab.get(),
                query: query.get(),
                places: places.get(),
                root: window.workspace.path.clone(),
                open: open_files(&window),
                case: state.case.get(),
                words: state.words.get(),
                regex: state.regex.get(),
                mask: if tab.get() == Tab::Text {
                    state.mask.get()
                } else {
                    String::new()
                },
            };
            batch(|| {
                base.set(Vec::new());
                symbols.set(Vec::new());
                selected.set(0);
                message.set(String::new());
                search_pending.set(true);
            });
            let lsp = window.common.dart_lsp.clone();
            let owner = window.scope;
            exec_after(Duration::from_millis(120), move |_| {
                if generation.try_get_untracked() != Some(id) {
                    return;
                }
                let symbol_search =
                    matches!(request.tab, Tab::All | Tab::Classes | Tab::Symbols)
                        && !request.query.trim().is_empty()
                        && !request.query.starts_with('/');
                let symbol_only = matches!(request.tab, Tab::Classes | Tab::Symbols);
                if symbol_search {
                    if lsp.is_available() {
                        let send = floem::ext_event::create_ext_action(
                            owner,
                            move |(rows, error): (Vec<Row>, Option<String>)| {
                                if generation.try_get_untracked() == Some(id)
                                    && active.get_untracked()
                                {
                                    symbols.set(rows);
                                    if symbol_only {
                                        if let Some(error) = error {
                                            message.set(error);
                                        }
                                        search_pending.set(false);
                                    }
                                }
                            },
                        );
                        let mode = request.tab;
                        let scope = request.places;
                        let root = request.root.clone();
                        let open = request.open.clone();
                        lsp.request_async(
                            "workspace/symbol",
                            serde_json::json!({"query":request.query.trim()}),
                            move |response| {
                                let error = response["error"]["message"]
                                    .as_str()
                                    .map(str::to_owned);
                                send((
                                    symbol_rows(
                                        &response,
                                        mode,
                                        scope,
                                        root.as_deref(),
                                        &open,
                                    ),
                                    error,
                                ));
                            },
                        );
                    } else if symbol_only {
                        message.set("Dart Analysis is unavailable.".into());
                        search_pending.set(false);
                    }
                    if symbol_only {
                        return;
                    }
                }
                let send = floem::ext_event::create_ext_action(
                    owner,
                    move |(rows, error)| {
                        if generation.try_get_untracked() == Some(id)
                            && active.get_untracked()
                        {
                            base.set(rows);
                            message.set(error);
                            search_pending.set(false);
                        }
                    },
                );
                std::thread::spawn(move || send(search(&request)));
            });
        }
    });
    let results = create_memo(move |_| {
        let mut rows = base.get();
        rows.extend(symbols.get());
        rows
    });
    let chosen =
        create_memo(move |_| results.with(|rows| rows.get(selected.get()).cloned()));
    let input = focus.input(query, config);
    let input_id = input.id();
    create_effect(move |_| {
        if active.get() {
            exec_after(Duration::from_millis(30), move |_| input_id.request_focus());
        }
    });
    let accept = {
        let window = window.clone();
        move |split| {
            if let Some(row) = chosen.get_untracked() {
                active.set(false);
                match row.target {
                    ResultTarget::Action(command) => {
                        window.common.sparrow_command.send(command)
                    }
                    ResultTarget::Location(hit) if split => {
                        window.main_split.jump_in_right_split(hit.location())
                    }
                    ResultTarget::Location(hit) => {
                        window.main_split.jump_to_location(hit.location(), None)
                    }
                }
            }
        }
    };
    let submit = accept.clone();
    focus.set_submit(move || submit(false));
    let tabs = tab_strip(stack_from_iter(Tab::ALL.map(|(mode, name)| {
        focus.control(tab_button(
            label(move || name),
            move || tab.get() == mode,
            move || {
                tab.set(mode);
                input_id.request_focus();
            },
            config,
        ))
    })));
    let include = focus
        .control(labeled_checkbox(
            move || places.get() == Places::All,
            || "Include non-project items",
            move |on| places.set(if on { Places::All } else { Places::Project }),
            config,
        ))
        .style(move |s| s.apply_if(tab.get() != Tab::All, |s| s.hide()));
    let scope = focus
        .menu(
            button(move || places.get().label(), ButtonKind::Plain, config),
            move || {
                [Places::Project, Places::Open, Places::All]
                    .into_iter()
                    .map(|value| {
                        ModalChoice::new(value.label(), move || places.set(value))
                            .selected(places.get_untracked() == value)
                    })
                    .collect()
            },
        )
        .style(move |s| {
            s.apply_if(matches!(tab.get(), Tab::All | Tab::Actions), |s| s.hide())
        });
    let file_mask = focus
        .control(text_input(state.mask, config).placeholder("All file types"))
        .style(move |s| s.width(104).apply_if(tab.get() != Tab::Text, |s| s.hide()));
    let preview_toggle = focus.icon(
        || SparrowIcons::LAYOUT_PANEL,
        move || preview.update(|on| *on = !*on),
        move || preview.get(),
        || false,
        || "Toggle source preview",
        config,
    );
    let option = |signal: RwSignal<bool>, icon, tip| {
        focus.icon(
            move || icon,
            move || signal.update(|on| *on = !*on),
            move || signal.get(),
            || false,
            move || tip,
            config,
        )
    };
    let text_options = stack((
        option(
            state.case,
            SparrowIcons::SEARCH_CASE_SENSITIVE,
            "Match case",
        ),
        option(state.words, SparrowIcons::SEARCH_WHOLE_WORD, "Whole words"),
        option(
            state.regex,
            SparrowIcons::SEARCH_REGEX,
            "Regular expression",
        ),
    ))
    .style(move |s| {
        s.items_center()
            .gap(4)
            .apply_if(tab.get() != Tab::Text, |s| s.hide())
    });
    let rows = sparrow_scroll(
        dyn_stack(
            move || results.get().into_iter().enumerate(),
            |(index, row)| {
                (
                    *index,
                    row.title.clone(),
                    row.hint.clone(),
                    row.hit()
                        .map(|hit| (hit.path.clone(), hit.offset, hit.position)),
                )
            },
            {
                let accept = accept.clone();
                move |(index, row): (usize, Row)| {
                    let icon_row = row.clone();
                    let accept = accept.clone();
                    stack((
                        svg(move || {
                            let config = config.get();
                            if icon_row.icon == SparrowIcons::FILE
                                && let Some(hit) = icon_row.hit()
                            {
                                config.file_svg(&hit.path).0
                            } else {
                                config.ui_svg(icon_row.icon)
                            }
                        })
                        .style(move |s| {
                            s.size(14, 14)
                                .flex_shrink(0.0_f32)
                                .color(config.get().color(SparrowColor::EDITOR_LINK))
                        }),
                        label(move || row.title.clone()).style(|s| {
                            s.min_width(0)
                                .flex_grow(1.0_f32)
                                .text_ellipsis()
                                .selectable(false)
                        }),
                        label(move || row.hint.clone()).style(move |s| {
                            s.max_width_pct(40.0).text_ellipsis().color(
                                config
                                    .get()
                                    .color(SparrowColor::PANEL_FOREGROUND_DIM),
                            )
                        }),
                    ))
                    .on_click_stop(move |_| selected.set(index))
                    .on_double_click_stop(move |_| {
                        selected.set(index);
                        accept(false);
                    })
                    .style(move |s| {
                        row_style(s, index == selected.get(), &config.get())
                    })
                }
            },
        )
        .style(|s| s.width_full().flex_col()),
    )
    .ensure_visible(move || {
        floem::kurbo::Rect::new(
            0.0,
            selected.get() as f64 * 26.0,
            1.0,
            (selected.get() + 1) as f64 * 26.0,
        )
    })
    .style(move |s| {
        s.width_full()
            .flex_grow(1.0_f32)
            .flex_basis(0)
            .min_height(0)
            .apply_if(results.with(|rows| rows.is_empty()), |s| s.hide())
    });
    let excerpt = dyn_container(
        move || {
            if preview.get() {
                chosen.with(|row| row.as_ref().and_then(Row::hit).cloned())
            } else {
                None
            }
        },
        move |hit| preview::view(window.clone(), hit),
    )
    .style(move |s| {
        s.width_full()
            .min_height(0)
            .flex_grow(1.0_f32)
            .flex_basis(0)
            .apply_if(
                !preview.get()
                    || chosen.with(|row| row.as_ref().and_then(Row::hit).is_none()),
                |s| s.hide(),
            )
    });
    let can_split =
        move || chosen.with(|row| row.as_ref().and_then(Row::hit).is_some());
    let accept_split = accept.clone();
    let empty_query = move || {
        query.with(|text| text.trim().is_empty())
            && tab.get() != Tab::All
            && tab.get() != Tab::Files
    };
    let body = stack((
        stack((
            tabs,
            empty().style(|s| s.flex_grow(1.0_f32)),
            include,
            scope,
            file_mask,
            preview_toggle,
        ))
        .style(|s| {
            s.width_full()
                .items_center()
                .gap(6)
                .flex_wrap(floem::style::FlexWrap::Wrap)
        }),
        Divider::new(config).into_view(),
        stack((
            input.style(|s| s.flex_grow(1.0_f32).flex_basis(0)),
            text_options,
        ))
        .style(|s| s.width_full().items_center().gap(5)),
        stack((
            empty_state(
                move || {
                    let msg = message.get();
                    if !msg.is_empty() {
                        msg
                    } else if search_pending.get() {
                        "Searching…".into()
                    } else {
                        "No results".into()
                    }
                },
                config,
            )
            .style(move |s| {
                s.apply_if(results.with(|rows| !rows.is_empty()), |s| s.hide())
            }),
            rows,
            excerpt,
        ))
        .style(move |s| {
            s.width_full()
                .flex_grow(1.0_f32)
                .min_height(0)
                .flex_col()
                .apply_if(empty_query(), |s| s.hide())
        }),
        stack((
            label(move || {
                chosen.with(|row| {
                    row.as_ref()
                        .and_then(Row::hit)
                        .map(|hit| hit.path.display().to_string())
                        .unwrap_or_default()
                })
            })
            .style(move |s| {
                s.min_width(0)
                    .flex_grow(1.0_f32)
                    .text_ellipsis()
                    .color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
            }),
            focus
                .control_if(
                    button(|| "Open in Right Split", ButtonKind::Plain, config)
                        .on_click_stop(move |_| accept_split(true))
                        .disabled(move || !can_split()),
                    can_split,
                )
                .style(move |s| s.apply_if(!can_split(), |s| s.hide())),
        ))
        .style(move |s| {
            s.items_center()
                .width_full()
                .gap(10)
                .apply_if(empty_query(), |s| s.hide())
        }),
    ))
    .style(move |s| {
        dialog_surface(s, &config.get())
            .width(760)
            .max_width_pct(95.0)
            .apply_if(!empty_query(), |s| s.height(590).max_height_pct(92.0))
            .padding(9)
            .gap(6)
            .min_height(0)
            .flex_col()
    });
    let dialog = container(body)
        .style(|s| {
            s.absolute()
                .size_full()
                .items_start()
                .justify_center()
                .padding_top(32)
        })
        .on_event_stop(EventListener::PointerDown, |_| {})
        .on_event(EventListener::KeyDown, move |event| {
            if let Event::KeyDown(key) = event {
                if key.key.logical_key == Key::Named(NamedKey::Escape) {
                    active.set(false);
                    return EventPropagation::Stop;
                }
                if key.modifiers.contains(Modifiers::SHIFT)
                    && key.key.logical_key == Key::Named(NamedKey::Enter)
                {
                    if can_split() {
                        accept(true);
                    }
                    return EventPropagation::Stop;
                }
                if key.modifiers.is_empty()
                    && matches!(
                        key.key.logical_key,
                        Key::Named(NamedKey::ArrowUp | NamedKey::ArrowDown)
                    )
                    && let Some(index) = list_destination(
                        &key.key.logical_key,
                        selected.get_untracked(),
                        results.with_untracked(Vec::len),
                    )
                {
                    selected.set(index);
                    return EventPropagation::Stop;
                }
            }
            EventPropagation::Continue
        });
    focus
        .bind_root(dialog, active, config)
        .style(|s| s.z_index(100))
}
