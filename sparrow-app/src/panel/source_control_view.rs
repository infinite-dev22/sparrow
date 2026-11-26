use std::rc::Rc;

use floem::{
    IntoView, View,
    event::{Event, EventListener},
    reactive::{SignalGet, SignalUpdate, SignalWith, create_memo, create_rw_signal},
    views::{Decorators, container, dyn_container, label, stack},
};
use sparrow_core::buffer::rope_text::RopeText;

use super::{kind::PanelKind, position::PanelPosition};
use crate::{
    config::{color::SparrowColor, icon::SparrowIcons},
    editor::view::editor_view,
    widgets::{CheckboxState, group_checkbox, labeled_checkbox},
    window_tab::{Focus, WindowTabData},
};

mod file_list;

use crate::widgets::sparrow_scroll;
use file_list::file_diffs_view;

pub fn source_control_panel(
    window: Rc<WindowTabData>,
    _position: PanelPosition,
) -> impl View {
    let source = window.source_control.clone();
    let config = window.common.config;
    let focus = source.common.focus;
    let editor = create_rw_signal(source.editor.clone());
    let doc = source.editor.doc_signal();
    let viewport = source.editor.viewport();
    let origin = source.editor.window_origin();
    let debug = create_memo(move |_| None);
    let active = move |tracked| {
        if tracked {
            focus.get() == Focus::Panel(PanelKind::SourceControl)
        } else {
            focus.get_untracked() == Focus::Panel(PanelKind::SourceControl)
        }
    };
    let message_width = create_rw_signal(400.0f64);
    let empty_message =
        create_memo(move |_| doc.get().buffer.with(|b| b.is_empty()));
    let commit = source.clone();
    let push = source.clone();
    let amend_source = source.clone();
    let amend = source.amend;
    let busy = source.busy;
    let status = source.status;
    let untracked = source.untracked;
    let files = source.file_diffs;
    let tracked_open = create_rw_signal(true);
    let new_open = create_rw_signal(true);
    let refresh_source = source.clone();
    let commit_action = source.commit_action;
    let lease = source.pending_amend_push;
    let pending_source = source.clone();
    let ai_window = window.clone();
    let ai_model = create_rw_signal(config.get_untracked().ai_commit.model.clone());
    let ai_models = create_rw_signal(Vec::<String>::new());
    let ai_context = create_rw_signal(None::<crate::ai_commit::PromptContext>);
    let ai_open = create_rw_signal(false);
    let ai_loading = create_rw_signal(false);
    let ai_generating = create_rw_signal(false);
    let ai_suggestion = create_rw_signal(String::new());
    let ai_status = create_rw_signal(String::new());
    let ai_update_window = window.clone();
    let model_picker = dyn_container(
        move || ai_models.get(),
        move |models| {
            if models.is_empty() {
                label(|| "No local models loaded. Refresh models or enter one in Settings.")
                    .into_any()
            } else {
                let selected_model = ai_model;
                let update_window = ai_update_window.clone();
                let picker_status = ai_status;
                crate::widgets::dropdown_with_label(
                    selected_model,
                    models,
                    config,
                    |model| model.clone(),
                    move |model| {
                        if crate::config::SparrowConfig::update_file(
                            "ai-commit",
                            "model",
                            model.clone().into(),
                        )
                        .is_some()
                        {
                            selected_model.set(model);
                            update_window.reload_config();
                        } else {
                            picker_status.set(
                                "Unable to save the selected local model.".into(),
                            );
                        }
                    },
                )
                .into_any()
            }
        },
    );
    let refresh_models_window = ai_window.clone();
    let refresh_models = crate::widgets::button(
        || "Refresh local models",
        crate::widgets::ButtonKind::Outlined,
        config,
    )
    .on_click_stop(move |_| {
        let endpoint = refresh_models_window
            .common
            .config
            .get_untracked()
            .ai_commit
            .base_url
            .clone();
        ai_status.set("Checking the local Ollama model list…".into());
        let send = floem::ext_event::create_ext_action(
            refresh_models_window.scope,
            move |result: Result<Vec<String>, String>| match result {
                Ok(models) => {
                    let count = models.len();
                    ai_models.set(models);
                    ai_status.set(format!("Found {count} local models."));
                }
                Err(error) => ai_status.set(error),
            },
        );
        std::thread::spawn(move || send(crate::ai_commit::list_models(&endpoint)));
    });
    let open_ai_settings_window = ai_window.clone();
    let open_ai_settings = crate::widgets::button(
        || "Settings",
        crate::widgets::ButtonKind::Plain,
        config,
    )
    .on_click_stop(move |_| {
        open_ai_settings_window
            .settings_search_query
            .set("ai-commit".into());
        open_ai_settings_window.settings_modal_visible.set(true);
    });
    let preview_settings = config;
    let ai_preview = sparrow_scroll(label(move || {
        let Some(context) = ai_context.get() else {
            return if ai_loading.get() {
                "Building a preview from selected files…".into()
            } else {
                String::new()
            };
        };
        let ai_config = preview_settings.get().ai_commit.clone();
        let [system, user] = crate::ai_commit::prompt_messages(
            &context,
            &ai_config.style,
            &ai_config.language,
        );
        format!("SYSTEM PROMPT\n{system}\n\nUSER PROMPT\n{user}")
    }))
    .style(|s| s.flex_grow(1.0_f32).min_height(80.0).width_full());
    let start_ai_source = source.clone();
    let start_ai_window = ai_window.clone();
    let ai_button = crate::widgets::button(
        || "AI ✦",
        crate::widgets::ButtonKind::Outlined,
        config,
    )
    .on_click_stop(move |_| {
        if ai_loading.get_untracked()
            || ai_generating.get_untracked()
            || busy.get_untracked()
        {
            return;
        }
        let ai_config = config.get_untracked().ai_commit.clone();
        if !ai_config.enabled {
            ai_status.set("AI commit messages are disabled in Settings.".into());
            return;
        }
        let Some(root) = start_ai_window
            .workspace
            .path
            .clone()
            .filter(|_| start_ai_window.workspace.kind.is_local())
        else {
            ai_status.set("AI commit messages require a local project.".into());
            return;
        };
        let selected = start_ai_source.file_diffs.with_untracked(|items| {
            items
                .values()
                .filter(|(_, checked)| *checked)
                .map(|(diff, _)| diff.clone())
                .collect::<Vec<_>>()
        });
        if selected.is_empty() {
            ai_status.set("Select files to include before building the preview.".into());
            return;
        }
        ai_open.set(true);
        ai_context.set(None);
        ai_suggestion.set(String::new());
        ai_loading.set(true);
        ai_status.set("Building a redacted prompt preview. Nothing has been sent.".into());
        let send = floem::ext_event::create_ext_action(
            start_ai_window.scope,
            move |result: Result<crate::ai_commit::PromptContext, String>| {
                ai_loading.set(false);
                match result {
                    Ok(context) => {
                        ai_status.set(format!(
                            "Preview ready for {} selected path(s) ({} diff characters{}). {} suspected secret value(s) redacted; nothing has been sent.",
                            context.paths.len(),
                            context.diff.chars().count(),
                            if context.truncated { ", truncated" } else { "" },
                            context.redacted_values
                        ));
                        ai_context.set(Some(context));
                    }
                    Err(error) => ai_status.set(error),
                }
            },
        );
        std::thread::spawn(move || {
            send(crate::ai_commit::build_context(
                &root,
                &selected,
                ai_config.max_diff_chars,
                &ai_config.exclude_globs,
            ));
        });
    })
    .disabled(move || {
        busy.get()
            || ai_loading.get()
            || ai_generating.get()
            || !config.get().ai_commit.enabled
    })
    .style(|s| s.padding_horiz(10.0).height(26.0));

    let generate_window = ai_window.clone();
    let generate_button = crate::widgets::button(
        || "Generate on local Ollama",
        crate::widgets::ButtonKind::Primary,
        config,
    )
    .on_click_stop(move |_| {
        if ai_generating.get_untracked() || ai_loading.get_untracked() {
            return;
        }
        let Some(context) = ai_context.get_untracked() else {
            ai_status.set("Build and review the prompt preview first.".into());
            return;
        };
        let ai_config = config.get_untracked().ai_commit.clone();
        let model = ai_model.get_untracked();
        if model.trim().is_empty() {
            ai_status.set("Choose an installed local model before generating.".into());
            return;
        }
        ai_generating.set(true);
        ai_status.set("Generating locally. Your selected diff is sent only to Ollama on this device.".into());
        let send = floem::ext_event::create_ext_action(
            generate_window.scope,
            move |result: Result<String, String>| {
                ai_generating.set(false);
                match result {
                    Ok(message) => {
                        ai_suggestion.set(message);
                        ai_status.set("Review and edit the suggestion before applying it. No commit was made.".into());
                    }
                    Err(error) => ai_status.set(error),
                }
            },
        );
        std::thread::spawn(move || {
            send(crate::ai_commit::generate_message(
                &ai_config.base_url,
                &model,
                &ai_config.style,
                &ai_config.language,
                &context,
            ));
        });
    })
    .disabled(move || {
        ai_generating.get()
            || ai_loading.get()
            || ai_context.get().is_none()
            || ai_model.get().trim().is_empty()
    });

    let replace_source = source.clone();
    let apply_replace = crate::widgets::button(
        || "Replace message",
        crate::widgets::ButtonKind::Outlined,
        config,
    )
    .on_click_stop(move |_| {
        let message = ai_suggestion.get_untracked();
        if message.trim().is_empty() {
            return;
        }
        replace_source
            .editor
            .doc()
            .reload(lapce_xi_rope::Rope::from(message), false);
        ai_status
            .set("Suggestion applied. Review the message before committing.".into());
        ai_open.set(false);
    })
    .style(move |s| s.apply_if(ai_suggestion.get().trim().is_empty(), |s| s.hide()));
    let append_source = source.clone();
    let apply_append = crate::widgets::button(
        || "Append message",
        crate::widgets::ButtonKind::Outlined,
        config,
    )
    .on_click_stop(move |_| {
        let suggestion = ai_suggestion.get_untracked();
        if suggestion.trim().is_empty() {
            return;
        }
        let existing = append_source
            .editor
            .doc()
            .buffer
            .with_untracked(|buffer| buffer.to_string());
        let message = if existing.trim().is_empty() {
            suggestion
        } else {
            format!("{}\n\n{}", existing.trim_end(), suggestion)
        };
        append_source
            .editor
            .doc()
            .reload(lapce_xi_rope::Rope::from(message), false);
        ai_status.set(
            "Suggestion appended. Review the message before committing.".into(),
        );
        ai_open.set(false);
    })
    .style(move |s| s.apply_if(ai_suggestion.get().trim().is_empty(), |s| s.hide()));
    let ai_panel = stack((
        stack((
            label(|| "Local AI commit message · exact redacted prompt preview")
                .style(|s| s.flex_grow(1.0_f32).min_width(0.0).text_ellipsis()),
            refresh_models,
            model_picker,
            open_ai_settings,
            generate_button,
            crate::widgets::button(
                || "Close",
                crate::widgets::ButtonKind::Plain,
                config,
            )
            .on_click_stop(move |_| {
                ai_open.set(false);
                if ai_generating.get_untracked() {
                    ai_status.set(
                        "Local inference continues until completion or the 90-second timeout.".into(),
                    );
                }
            }),
        ))
        .style(|s| s.width_full().items_center().gap(6.0)),
        ai_preview,
        label(|| "Local inference may take up to 90 seconds. Close hides this preview but does not interrupt an active request.")
            .style(move |s| s.font_size(10.0).color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))),
        crate::widgets::TextField::new(ai_suggestion)
            .colors(move || crate::widgets::m3_from_config(config))
            .placeholder("Generated subject — edit before applying")
            .into_view()
            .style(move |s| {
                s.apply_if(ai_suggestion.get().trim().is_empty(), |s| s.hide())
            }),
        stack((apply_replace, apply_append)).style(|s| s.gap(6.0)),
        label(move || ai_status.get()).style(move |s| {
            s.width_full()
                .max_height(42.0)
                .font_size(11.0)
                .color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
        }),
    ))
    .style(move |s| {
        s.flex_col()
            .width_full()
            .max_height(300.0)
            .gap(6.0)
            .padding(8.0)
            .border(1.0)
            .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
            .apply_if(!ai_open.get(), |s| s.hide())
    });
    let group = move |unversioned: bool, open: floem::reactive::RwSignal<bool>| {
        let selection = create_memo(move |_| {
            untracked.with(|untracked| {
                files.with(|items| {
                    items
                        .values()
                        .filter(|(diff, _)| {
                            untracked.contains(diff.path()) == unversioned
                        })
                        .fold((0, 0), |(total, selected), (_, checked)| {
                            (total + 1, selected + usize::from(*checked))
                        })
                })
            })
        });
        let count = move || selection.with(|(total, _)| *total);
        stack((
            stack((
                crate::widgets::tree_chevron(config, move || open.get(), || true)
                    .on_click_stop(move |_| open.update(|v| *v = !*v))
                    .style(|s| s.margin_right(6.0)),
                group_checkbox(
                    move || {
                        selection.with(|&(total, selected)| {
                            CheckboxState::from_selection(total, selected)
                        })
                    },
                    move |selected| {
                        untracked.with_untracked(|untracked| {
                            files.update(|items| {
                                for (diff, checked) in items.values_mut() {
                                    if untracked.contains(diff.path()) == unversioned
                                    {
                                        *checked = selected;
                                    }
                                }
                            });
                        });
                    },
                    config,
                ),
                label(move || {
                    format!(
                        "{}  {} files",
                        if unversioned {
                            "Unversioned Files"
                        } else {
                            "Changes"
                        },
                        count()
                    )
                })
                .style(|s| s.margin_left(6.0)),
            ))
            .style(move |s| {
                s.height(24.0)
                    .width_full()
                    .items_center()
                    .padding_horiz(6.0)
                    .background(
                        config
                            .get()
                            .color(SparrowColor::PANEL_HOVERED_BACKGROUND)
                            .with_alpha(0.3),
                    )
            }),
            file_diffs_view(source.clone(), unversioned).style(move |s| {
                s.min_height(0.0)
                    .height((count() * 24) as f32)
                    .apply_if(!open.get(), |s| s.hide())
            }),
        ))
        .style(move |s| {
            s.width_full()
                .flex_col()
                .apply_if(count() == 0, |s| s.hide())
        })
    };
    let commit_button = crate::widgets::button(
        move || {
            if busy.get() {
                "Committing…"
            } else if amend.get() {
                "Amend Commit"
            } else {
                "Commit"
            }
        },
        crate::widgets::ButtonKind::Outlined,
        config,
    )
    .on_click_stop(move |_| commit.commit())
    .disabled(move || busy.get())
    .style(|s| s.padding_horiz(12.0).height(26.0));
    let commit_button_id = commit_button.id();
    commit_action.set(Some(commit_button_id));
    let push_button = crate::widgets::button(
        || "Commit and Push…",
        crate::widgets::ButtonKind::Outlined,
        config,
    )
    .on_click_stop(move |_| push.commit_with_options(true))
    .disabled(move || busy.get())
    .style(|s| s.padding_horiz(12.0).height(26.0));
    let pending_window = window.clone();
    let pending_push_button = crate::widgets::button(
        move || {
            lease.with(|pending| {
                pending
                    .as_ref()
                    .map(|pending| format!("{}…", pending.label()))
                    .unwrap_or_default()
            })
        },
        crate::widgets::ButtonKind::Outlined,
        config,
    )
    .on_click_stop(move |_| {
        let Some(pending) = lease.get_untracked() else {
            return;
        };
        let label = pending.label();
        let source = pending_source.clone();
        let alert_active = pending_window.alert_data.active;
        pending_window.show_alert(
            label.into(),
            pending.confirmation(),
            vec![crate::widgets::alert::AlertButton {
                text: label.into(),
                action: Rc::new(move || {
                    alert_active.set(false);
                    source.push_pending_amend(pending.clone());
                }),
            }],
        );
    })
    .disabled(move || busy.get())
    .style(move |s| {
        s.padding_horiz(12.0)
            .height(26.0)
            .margin_left(6.0)
            .margin_bottom(4.0)
            .apply_if(lease.with(Option::is_none), |s| s.hide())
    });
    let message_view = stack((
        editor_view(editor.get_untracked(), debug, active)
            .style(move |s| s.width(message_width.get().max(40.0) as f32)),
        label(|| "Commit Message").style(move |s| {
            s.absolute()
                .color(config.get().color(SparrowColor::EDITOR_DIM))
                .apply_if(!empty_message.get(), |s| s.hide())
        }),
    ))
    .style(|s| s.absolute().min_size_pct(100.0, 100.0).padding(8.0));
    let message_id = message_view.id();
    let message_view = message_view
        .keyboard_navigable()
        .on_event_cont(EventListener::FocusGained, move |_| {
            focus.set(Focus::Panel(PanelKind::SourceControl));
        })
        .on_event_stop(EventListener::PointerDown, move |event| {
            focus.set(Focus::Panel(PanelKind::SourceControl));
            message_id.request_focus();
            if let Event::PointerDown(pointer) = event.clone().offset((8.0, 8.0)) {
                message_id.request_active();
                editor.get_untracked().pointer_down(&pointer);
            }
        })
        .on_event_stop(EventListener::PointerMove, move |event| {
            if let Event::PointerMove(pointer) = event.clone().offset((8.0, 8.0)) {
                editor.get_untracked().pointer_move(&pointer);
            }
        })
        .on_event_stop(EventListener::PointerUp, move |event| {
            if let Event::PointerUp(pointer) = event.clone().offset((8.0, 8.0)) {
                editor.get_untracked().pointer_up(&pointer);
            }
        });
    stack((
        super::chrome::header(
            window.clone(),
            PanelKind::SourceControl,
            "Commit",
            floem::views::empty(),
        ),
        stack((
            crate::widgets::clickable_icon(
                || SparrowIcons::IMAGE_LOADING,
                {
                    let source = refresh_source.clone();
                    move || source.refresh()
                },
                || false,
                || false,
                || "Refresh changes",
                config,
            ),
            crate::widgets::action_link(
                || "Show Git Log",
                move || window.show_panel(PanelKind::GitHistory),
                config,
            ),
        ))
        .style(|s| s.height(28.0).gap(8.0).items_center().padding_horiz(6.0)),
        sparrow_scroll(
            stack((group(false, tracked_open), group(true, new_open)))
                .style(|s| s.flex_col().width_full()),
        )
        .style(|s| s.flex_grow(1.0_f32).min_height(40.0).width_full()),
        labeled_checkbox(
            move || amend.get(),
            || "Amend last commit",
            move |checked| amend_source.set_amend(checked),
            config,
        )
        .style(move |s| {
            s.height(30.0)
                .padding_horiz(8.0)
                .items_center()
                .border_top(1.0)
                .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
        }),
        container(
            sparrow_scroll(message_view)
                .on_move(move |point| origin.set(point + (8.0, 8.0)))
                .on_scroll(move |rect| viewport.set(rect))
                .style(|s| s.absolute().size_full()),
        )
        .on_resize(move |rect| message_width.set((rect.width() - 16.0).max(40.0)))
        .style(move |s| {
            s.height_pct(36.0)
                .min_height(100.0)
                .width_full()
                .border(1.0)
                .border_radius(2.0)
                .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
        }),
        ai_panel,
        label(move || status.get()).style(move |s| {
            s.width_full()
                .max_height(72.0)
                .padding_horiz(8.0)
                .font_size(11.0)
                .apply_if(status.with(|s| s.is_empty()), |s| s.hide())
        }),
        pending_push_button,
        stack((ai_button, commit_button, push_button))
            .style(|s| s.height(36.0).gap(6.0).items_center().padding_horiz(6.0)),
    ))
    .style(|s| s.absolute().size_full().flex_col().font_size(13.0))
    .on_cleanup(move || {
        if commit_action.get_untracked() == Some(commit_button_id) {
            commit_action.set(None);
        }
    })
}
