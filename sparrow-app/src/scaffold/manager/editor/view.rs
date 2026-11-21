//! Template document editor, diagnostics, completion and rendered preview.

use crate::scaffold::manager::stage;
use crate::{config::color::SparrowColor, window_tab::WindowTabData};
use floem::{
    View,
    event::{Event, EventListener, EventPropagation},
    keyboard::{Key, Modifiers, NamedKey},
    reactive::{Scope, SignalGet, SignalUpdate},
    views::{
        Decorators, container, dyn_container, dyn_stack,
        editor::{
            Editor,
            command::CommandExecuted,
            core::cursor::CursorAffinity,
            keypress::{default_key_handler, key::KeyInput},
            text::Document,
            text_document::TextDocument,
        },
        label, stack, text_editor_keys,
    },
};
use std::{cell::RefCell, collections::BTreeMap, path::PathBuf, rc::Rc};
use crate::widgets::sparrow_scroll;
use super::{action_controls::action_controls, analyzer, language};

pub(in crate::scaffold::manager) fn view(window: Rc<WindowTabData>) -> impl View {
    let parent = window.scaffold.clone();
    let state = parent.manager.editor.clone();
    let open = state.open;
    let active = parent.active;
    let draft = state.draft;
    let selected = state.file;
    let sample = state.sample;
    let new_file = state.new_file;
    let rendered = state.rendered;
    let epoch = state.epoch;
    let revision = state.revision;
    let plan = parent.plan;
    let error = parent.error;
    let config = window.common.config;
    let diagnostics = state.diagnostics;
    let local_diagnostics =
        Scope::current().create_rw_signal(Vec::<language::Diagnostic>::new());
    let analyzer = analyzer::Session::new(
        draft
            .get_untracked()
            .map(|d| d.ctx.root)
            .unwrap_or_default(),
    );
    let dart_status = analyzer.status;
    let merged_analyzer = analyzer.clone();
    floem::reactive::create_effect(move |_| {
        let mut all = local_diagnostics.get();
        all.extend(merged_analyzer.diagnostics.get());
        diagnostics.set(all);
    });
    let validation_analyzer = analyzer.clone();
    let suggestions =
        Scope::current().create_rw_signal(Vec::<language::Completion>::new());
    let suggestion_index = Scope::current().create_rw_signal(0usize);
    let popup_point = Scope::current().create_rw_signal((0.0f64, 0.0f64));
    let active_editor = Rc::new(RefCell::new(None::<Editor>));
    let focus = crate::widgets::modal_focus::ModalFocus::default();
    crate::widgets::modal_focus::restore_workbench_on_close(active, window.common.clone());
    floem::reactive::create_effect(move |_| {
        let current = draft.get();
        let sample = sample.get();
        let _ = validation_analyzer.ready.get();
        validation_analyzer.invalidate();
        local_diagnostics.set(vec![]);
        let analysis = validation_analyzer.clone();
        revision.update(|r| *r += 1);
        let token = revision.get_untracked();
        rendered.set(Err("Validating…".into()));
        plan.set(None);
        floem::action::exec_after(
            std::time::Duration::from_millis(150),
            move |_| {
                if revision.get_untracked() == token && open.get_untracked() {
                    local_diagnostics.set(
                        current
                            .as_ref()
                            .map(|d| language::diagnostics(d, &sample))
                            .unwrap_or_default(),
                    );
                    if let Some(d) = current.as_ref() {
                        analysis.update(d, &sample, token);
                    }
                    rendered.set(
                        current
                            .ok_or("No draft".to_string())
                            .and_then(|d| d.validate(&sample)),
                    );
                }
            },
        );
    });
    let file_focus = focus.clone();
    let files =
        sparrow_scroll(
            dyn_stack(
                move || {
                    draft
                        .get()
                        .map(|d| d.files.keys().cloned().collect::<Vec<_>>())
                        .unwrap_or_default()
                },
                |p| p.clone(),
                move |path| {
                    let label_path = path.clone();
                    let click_path = path.clone();
                    let diagnostic_path = path.clone();
                    file_focus.control(
                        label(move || {
                            format!(
                                "{}{}",
                                if diagnostics
                                    .get()
                                    .iter()
                                    .any(|d| d.path == diagnostic_path)
                                {
                                    "● "
                                } else {
                                    ""
                                },
                                label_path.display()
                            )
                        })
                        .on_click_stop(move |_| selected.set(click_path.clone()))
                        .style(move |s| {
                            s.padding(6.0).width_full().apply_if(
                                selected.get() == path,
                                |s| {
                                    s.background(config.get().color(
                                        SparrowColor::PANEL_CURRENT_BACKGROUND,
                                    ))
                                },
                            )
                        }),
                    )
                },
            )
            .style(|s| s.flex_col()),
        )
        .style(|s| s.width(210.0).min_width(140.0).height_full());
    let editor_focus = focus.clone();
    let documents =
        Rc::new(RefCell::new(BTreeMap::<PathBuf, Rc<TextDocument>>::new()));
    let action_bar = action_controls(analyzer.clone(),draft,selected,documents.clone(),active_editor.clone(),plan,error,&focus);
    let editor_scope = Scope::current();
    let text_editor_handle = active_editor.clone();
    let text_analyzer = analyzer.clone();
    let text = dyn_container(
        move || (epoch.get(), selected.get()),
        move |(_, path)| {
            let body = draft
                .get_untracked()
                .and_then(|d| d.files.get(&path).cloned())
                .unwrap_or_default();
            // Keep document undo history when switching file tabs. Documents
            // belong to the editor scope, rather than the disposable file view.
            let cached = documents
                .borrow()
                .get(&path)
                .cloned()
                .filter(|doc| doc.text().to_string() == body);
            let doc = cached.unwrap_or_else(|| {
                let doc = Rc::new(TextDocument::new(editor_scope, body));
                let update_path = path.clone();
                doc.add_on_update(move |event| {
                    let Some(editor) = event.editor else {
                        return;
                    };
                    let text = editor.doc().text().to_string();
                    plan.set(None);
                    draft.update(|d| {
                        if let Some(d) = d {
                            d.files.insert(update_path.clone(), text);
                        }
                    });
                    error.set(String::new());
                });
                documents.borrow_mut().insert(path.clone(), doc.clone());
                doc
            });
            suggestions.set(vec![]);
            let key_path = path.clone();
            let refresh_path = path.clone();
            let style_path = path.clone();
            let text_editor_handle = text_editor_handle.clone();
            let key_analyzer=text_analyzer.clone();
            let refresh_analyzer=text_analyzer.clone();
            let editor = text_editor_keys("",move |editor_signal,keypress,modifiers| {
                let ed = editor_signal.get_untracked();
                let key = match &keypress.key {KeyInput::Keyboard(key,_)=>Some(key),_=>None};
                if matches!(key,Some(Key::Named(NamedKey::Escape))) && (!key_analyzer.actions.list.get_untracked().is_empty() || key_analyzer.actions.preview.get_untracked().is_some()) {
                    key_analyzer.actions.clear();
                    return CommandExecuted::Yes;
                }
                if modifiers.contains(Modifiers::ALT) && matches!(key,Some(Key::Named(NamedKey::Enter))) {
                    plan.set(None);
                    suggestions.set(vec![]);
                    key_analyzer.request_actions(&ed,selected);
                    return CommandExecuted::Yes;
                }
                let explicit = modifiers.contains(Modifiers::CONTROL)
                    && (matches!(key,Some(Key::Named(NamedKey::Space))) || matches!(key,Some(Key::Character(c)) if c.as_str()==" "));
                if explicit {
                    let body=ed.doc().text().to_string();
                    let files=draft.get_untracked().map(|d|d.files).unwrap_or_default();
                    suggestions.set(language::completions(&key_path,&body,ed.cursor.get_untracked().offset(),&files,true));
                    suggestion_index.set(0);
                    key_analyzer.request_completions(&key_path,&ed,selected,suggestions,true);
                    return CommandExecuted::Yes;
                }
                let available=suggestions.get_untracked();
                if !available.is_empty() && !modifiers.intersects(Modifiers::CONTROL|Modifiers::ALT|Modifiers::META) {
                    match key {
                        Some(Key::Named(NamedKey::ArrowDown))=>{
                            suggestion_index.update(|i|*i=(*i+1)%available.len());return CommandExecuted::Yes;
                        }
                        Some(Key::Named(NamedKey::ArrowUp))=>{
                            suggestion_index.update(|i|*i=(*i+available.len()-1)%available.len());return CommandExecuted::Yes;
                        }
                        Some(Key::Named(NamedKey::Enter|NamedKey::Tab))=>{
                            if let Some(choice)=available.get(suggestion_index.get_untracked()) {
                                key_analyzer.accept(&ed,choice,draft,selected,plan,error);
                            }
                            suggestions.set(vec![]);return CommandExecuted::Yes;
                        }
                        Some(Key::Named(NamedKey::Escape))=>{
                            suggestions.set(vec![]);return CommandExecuted::Yes;
                        }
                        _=>{}
                    }
                }
                default_key_handler(editor_signal)(keypress,modifiers)
            }).use_doc(doc).with_editor(move |ed| {
                *text_editor_handle.borrow_mut()=Some(ed.clone());
                let ed=ed.clone();
                let refresh_ed=ed.clone();
                floem::reactive::create_effect(move |_| {
                    let offset=refresh_ed.cursor.get().offset();
                    // Track buffer edits through the draft, even if the cursor did not move.
                    let files=draft.get().map(|d|d.files).unwrap_or_default();
                    let body=refresh_ed.doc().text().to_string();
                    suggestions.set(language::completions(&refresh_path,&body,offset,&files,false));
                    suggestion_index.set(0);
                    refresh_analyzer.request_completions(&refresh_path,&refresh_ed,selected,suggestions,false);
                    let (_,point)=refresh_ed.points_of_offset(offset,CursorAffinity::Forward);
                    let viewport=refresh_ed.viewport.get_untracked();
                    popup_point.set(((point.x-viewport.x0+45.0).clamp(0.0,180.0),(point.y-viewport.y0).clamp(0.0,260.0)));
                });
                let version=Rc::new(std::cell::Cell::new(0u64));
                floem::reactive::create_effect(move |_| {
                    let diags=diagnostics.get();
                    let config=config.get();
                    // Each new document revision and diagnostics refresh invalidates line styles.
                    let files=draft.get();
                    let body=files.and_then(|d|d.files.get(&style_path).cloned()).unwrap_or_default();
                    version.set(version.get()+1);
                    // Styling rebuilds Floem's internal editor effects. Those
                    // implementation signals must not become dependencies of
                    // this diagnostics effect, or the rebuild triggers itself.
                    floem::reactive::untrack(|| {
                        ed.update_styling(Rc::new(language::CodeStyle::new(&style_path,&body,&diags,config,version.get())));
                    });
                });
            })
                .editor_style(move |s| {
                    s.cursor_color(config.get().color(SparrowColor::EDITOR_CARET))
                        .selection_color(
                            config.get().color(SparrowColor::EDITOR_SELECTION),
                        )
                        .current_line_color(
                            config.get().color(SparrowColor::EDITOR_CURRENT_LINE),
                        )
                })
                .style(move |s| {
                    s.size_full()
                        .font_family(config.get().editor.font_family.clone())
                        .font_size(13.0)
                        .background(
                            config.get().color(SparrowColor::EDITOR_BACKGROUND),
                        )
                });
            editor_focus.control(editor)
        },
    )
    .style(|s| s.flex_grow(1.0_f32).min_width(0.0).height_full());
    let popup_handle = active_editor.clone();
    let popup_analyzer = analyzer.clone();
    let popup = sparrow_scroll(
        dyn_stack(
            move || {
                suggestions
                    .get()
                    .into_iter()
                    .enumerate()
                    .collect::<Vec<_>>()
            },
            |(i, c)| (*i, c.label.clone()),
            move |(i, choice)| {
                let handle = popup_handle.clone();
                let analyzer = popup_analyzer.clone();
                let label_text = choice.label.clone();
                label(move || label_text.clone())
                    .on_click_stop(move |_| {
                        if let Some(ed) = handle.borrow().as_ref() {
                            analyzer
                                .accept(ed, &choice, draft, selected, plan, error);
                        }
                        suggestions.set(vec![]);
                    })
                    .style(move |s| {
                        s.height(24.0).padding_horiz(8.0).width_full().apply_if(
                            suggestion_index.get() == i,
                            |s| {
                                s.background(
                                    config.get().color(
                                        SparrowColor::PANEL_CURRENT_BACKGROUND,
                                    ),
                                )
                            },
                        )
                    })
            },
        )
        .style(|s| s.flex_col()),
    )
    .style(move |s| {
        s.absolute()
            .inset_left(popup_point.get().0)
            .inset_top(popup_point.get().1)
            .width(300.0)
            .max_height(150.0)
            .z_index(200)
            .border(1.0)
            .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
            .background(config.get().color(SparrowColor::PANEL_BACKGROUND))
            .apply_if(suggestions.get().is_empty(), |s| s.hide())
    });
    let diagnostic_handle = active_editor.clone();
    let rows_focus = focus.clone();
    let diagnostic_rows = sparrow_scroll(
        dyn_stack(
            move || {
                diagnostics
                    .get()
                    .into_iter()
                    .filter(|d| d.path == selected.get())
                    .collect::<Vec<_>>()
            },
            |d| (d.path.clone(), d.range.start, d.message.clone()),
            move |diagnostic| {
                let handle = diagnostic_handle.clone();
                rows_focus.control(
                    label({
                        let diagnostic = diagnostic.clone();
                        move || {
                            let body = draft
                                .get()
                                .and_then(|d| d.files.get(&diagnostic.path).cloned())
                                .unwrap_or_default();
                            let (line, col) =
                                language::position(&body, diagnostic.range.start);
                            format!(
                                "{}:{line}:{col} · {}",
                                diagnostic.path.display(),
                                diagnostic.message
                            )
                        }
                    })
                    .on_click_stop(move |_| {
                        if let Some(ed) = handle.borrow().as_ref() {
                            ed.cursor.update(|c| {
                                c.set_offset(diagnostic.range.start, false, false)
                            });
                            if let Some(id) = ed.editor_view_id.get_untracked() {
                                id.request_focus();
                            }
                            ed.editor_view_focused.notify();
                        }
                    })
                    .style(move |s| {
                        s.padding(4.0)
                            .width_full()
                            .color(config.get().color(SparrowColor::SPARROW_ERROR))
                    }),
                )
            },
        )
        .style(|s| s.flex_col()),
    )
    .style(move |s| {
        s.width_full().max_height(80.0).apply_if(
            !diagnostics.get().iter().any(|d| d.path == selected.get()),
            |s| s.hide(),
        )
    });
    let code_panel = stack((
        stack((text, popup))
            .style(|s| s.flex_grow(1.0_f32).min_height(160.0).width_full()),
        diagnostic_rows,
    ))
    .style(|s| s.flex_col().flex_grow(1.0_f32).min_width(0.0).height_full());
    let action_preview = analyzer.actions.preview;
    let output = sparrow_scroll(
        dyn_stack(
            move || {
                let mut text = match rendered.get() {
                    Err(e) => e,
                    Ok(files) => files
                        .iter()
                        .map(|(p, s)| format!("── {} ──\n{}\n", p.display(), s))
                        .collect::<Vec<_>>()
                        .join("\n"),
                };
                if let Some(p) = plan.get() {
                    text = format!(
                        "SAVE DIFF · {}\n{}\n",
                        p.title,
                        (0..p.changes.len())
                            .map(|i| p.diff(i))
                            .collect::<Vec<_>>()
                            .join("\n")
                    );
                }
                if let Some(preview) = action_preview.get() { text = preview.diff(); }
                let mut lines = text
                    .lines()
                    .take(2000)
                    .map(str::to_string)
                    .collect::<Vec<_>>();
                if text.lines().count() > 2000 {
                    lines.push("Preview display limited to 2,000 lines.".into());
                }
                lines.into_iter().enumerate().collect::<Vec<_>>()
            },
            |(i, s)| (*i, s.clone()),
            move |(_, text)| {
                let added=text.starts_with('+') && !text.starts_with("+++");
                let removed=text.starts_with('-') && !text.starts_with("---");
                label(move || text.clone()).style(move |s| {
                    s.apply_if(action_preview.get().is_some() && added,|s|s.color(config.get().color(SparrowColor::SOURCE_CONTROL_ADDED)))
                        .apply_if(action_preview.get().is_some() && removed,|s|s.color(config.get().color(SparrowColor::SOURCE_CONTROL_REMOVED)))
                        .height(20.0)
                        .padding_horiz(6.0)
                        .font_family(config.get().editor.font_family.clone())
                        .font_size(12.0)
                })
            },
        )
        .style(|s| s.flex_col()),
    )
    .style(|s| s.width(420.0).min_width(250.0).height_full());
    let add_window = window.clone();
    let remove_window = window.clone();
    let save_window = window.clone();
    let apply_window = window.clone();
    let discard_window = window.clone();
    let save_actions=analyzer.actions.clone();
    let generated_actions=analyzer.actions.clone();
    let body = stack((
        label(move ||format!("Template Editor{}",if draft.get().is_some_and(|d|d.dirty()) {" · Unsaved draft"} else {""}))
            .style(|s|s.font_bold()),
        label(move ||draft.get().map(|d|format!("{} → {}",d.source,d.directory.display())).unwrap_or_default()),
        label(||"Edit metadata in pack.yaml. Ctrl+Space: complete · ↑/↓: choose · Enter/Tab: insert · Esc: dismiss."),
        label(move ||dart_status.get()),
        stack((
            label(||"Sample name:"),focus.input(sample, config).style(|s|s.width(180.0)),
            label(move ||match rendered.get() {Ok(files)=>format!("Pack valid · {} generated files",files.len()),Err(e)=>e})
                .style(move |s|s.flex_grow(1.0_f32).min_width(0.0).apply_if(rendered.get().is_err(),|s|s.color(config.get().color(SparrowColor::SPARROW_ERROR)))) ,
        )).style(|s|s.gap(12.0).items_center()),
        action_bar,
        stack((files,code_panel,output)).style(|s|s.width_full().height(430.0).gap(10.0)),
        stack((
            focus.input(new_file, config).style(|s|s.width(270.0)),
            focus.control(label(||"Add File").on_click_stop(move |_| {
                let mut result = Err("No draft".to_string());
                draft.update(|d| {if let Some(d)=d {result=d.add_file(&new_file.get_untracked());}});
                match result {Ok(path)=>selected.set(path),Err(e)=>add_window.scaffold.error.set(e)}
            })),
            focus.control(label(||"Remove File").on_click_stop(move |_| {
                let mut result = Err("No draft".to_string());
                draft.update(|d| {if let Some(d)=d {result=d.remove_file(&selected.get_untracked());}});
                match result {Ok(())=>selected.set("pack.yaml".into()),Err(e)=>remove_window.scaffold.error.set(e)}
            })),
        )).style(|s|s.gap(16.0).items_center()),
        label(move ||error.get()),
        stack((
            focus.control(label(||"Preview Save").on_click_stop(move |_| {
                save_actions.clear();
                let result = draft.get_untracked().ok_or("No draft".into()).and_then(|d|d.save_plan(&sample.get_untracked()));
                save_window.scaffold.undoing.set(false);
                stage(&save_window,result);
            })),
            focus.control(label(||"Apply Save").disabled(move ||plan.get().is_none()).on_click_stop(move |_| {
                apply_window.scaffold.apply(&apply_window);
                if plan.get_untracked().is_none() {
                    draft.update(|d| {if let Some(d)=d {d.mark_saved();}});
                    apply_window.scaffold.manager.refresh();
                    error.set("Template saved. Undo Last Change is available in the manager.".into());
                }
            })),
            focus.control(label(||"Generated Preview").on_click_stop(move |_| {generated_actions.clear();plan.set(None);} )),
            focus.control(label(||"Back to Manager").on_click_stop(move |_| {plan.set(None);open.set(false);})),
            focus.control(label(||"Discard Draft").on_click_stop(move |_| {
                draft.set(None);plan.set(None);open.set(false);
                discard_window.scaffold.error.set("Draft discarded; saved files are unchanged.".into());
            })),
        )).style(|s|s.gap(18.0)),
    )).style(move |s|s.width(1380.0).max_width_pct(96.0).max_height_pct(96.0).flex_col()
        .gap(10.0).padding(18.0).border(1.0).border_radius(6.0)
        .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
        .background(config.get().color(SparrowColor::PANEL_BACKGROUND))
        .color(config.get().color(SparrowColor::EDITOR_FOREGROUND)));
    let dialog = container(body).style(move |s|s.absolute().size_full().items_center().justify_center().z_index(100)
        .apply_if(!active.get(),|s|s.hide()))
        .on_event_stop(EventListener::PointerDown,|_|{})
        .on_event(EventListener::KeyDown,move |event| {
            if matches!(event,Event::KeyDown(k) if k.key.logical_key==Key::Named(NamedKey::Escape)) {
                plan.set(None);open.set(false);EventPropagation::Stop
            } else {EventPropagation::Continue}
        });
    focus
        .bind_root(dialog, active, config)
        .on_cleanup(move || analyzer.close())
}
