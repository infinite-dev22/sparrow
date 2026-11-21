//! Template library, operations, and transaction preview.

use super::{
    catalog, create, duplicate, editable, editor, export, import, remove, stage,
    toggle,
};
use crate::{
    command::InternalCommand, config::color::SparrowColor, window_tab::WindowTabData,
};
use floem::{
    View,
    event::{Event, EventListener, EventPropagation},
    keyboard::{Key, NamedKey},
    reactive::{SignalGet, SignalUpdate},
    views::{Decorators, container, dyn_stack, label, scroll, stack},
};
use std::rc::Rc;

pub(in crate::scaffold) fn view(window: Rc<WindowTabData>) -> impl View {
    let editing = window.scaffold.manager.editor.open;
    floem::views::dyn_container(move || editing.get(), move |editing| -> Box<dyn View> {
        if editing { Box::new(editor::view(window.clone())) }
        else { Box::new(manager_view(window.clone())) }
    }).style(|s|s.absolute().size_full())
}
fn manager_view(window: Rc<WindowTabData>) -> impl View {
    let state = window.scaffold.clone();
    let manager = state.manager.clone();
    let config = window.common.config;
    let active = state.active;
    crate::widgets::modal_focus::restore_workbench_on_close(active, window.common.clone());
    let entries = manager.entries;
    let selected = manager.selected;
    let query = manager.query;
    let new_id = manager.id;
    let global = manager.global;
    let root = manager.root;
    let plan = state.plan;
    let error = state.error;
    let undoing = state.undoing;
    let focus = crate::widgets::modal_focus::ModalFocus::default();
    // Never apply actions to an entry hidden by the filter.
    floem::reactive::create_effect(move |_| {
        let q = query.get().to_lowercase();
        let all = entries.get();
        let matches = |e: &catalog::Entry| {
            format!("{} {} {}", e.title, e.id, e.provenance)
                .to_lowercase()
                .contains(&q)
        };
        let current = selected.get();
        if !current.and_then(|i| all.get(i)).is_some_and(matches) {
            let next = all.iter().position(matches);
            if next != current {
                selected.set(next);
                plan.set(None);
            }
        }
    });
    let list_focus = focus.clone();
    let list =
        scroll(
            dyn_stack(
                move || {
                    let q = query.get().to_lowercase();
                    entries
                        .get()
                        .into_iter()
                        .enumerate()
                        .filter(|(_, e)| {
                            format!("{} {} {}", e.title, e.id, e.provenance)
                                .to_lowercase()
                                .contains(&q)
                        })
                        .collect::<Vec<_>>()
                },
                |(i, e)| (*i, e.id.clone(), e.provenance.clone()),
                move |(i, e)| {
                    let status = if e.problem().is_some() {
                        "Invalid"
                    } else if e.is_disabled() {
                        "Disabled"
                    } else {
                        "Enabled"
                    };
                    list_focus.control(
                        label(move || {
                            format!(
                                "{} · {}\n{}\n{}",
                                e.title, status, e.id, e.provenance
                            )
                        })
                        .on_click_stop(move |_| {
                            selected.set(Some(i));
                            plan.set(None);
                            error.set(String::new());
                            undoing.set(false);
                        })
                        .style(move |s| {
                            s.width_full()
                                .padding(8.0)
                                .border_bottom(1.0)
                                .border_color(
                                    config.get().color(SparrowColor::SPARROW_BORDER),
                                )
                                .apply_if(selected.get() == Some(i), |s| {
                                    s.background(config.get().color(
                                        SparrowColor::PANEL_CURRENT_BACKGROUND,
                                    ))
                                })
                        }),
                    )
                },
            )
            .style(|s| s.flex_col()),
        )
        .style(|s| s.width(360.0).height_full());
    let detail = label(move || {
        let Some(e) = selected.get().and_then(|i| entries.get().get(i).cloned()) else { return "Select a template pack.".into(); };
        let mut text = format!("{}\n{}\n{}",e.title,e.id,e.provenance);
        if let Some(problem) = e.problem() { text.push_str(&format!("\nError: {problem}")); }
        else if let Ok(pack) = e.load() {
            text.push_str(&format!("\n{}\n{} output file(s) · {:?} · v{}",
                pack.description,pack.files.len(),pack.target,pack.version));
        }
        if e.is_disabled() { text.push_str("\nDisabled: hidden from New From Pack."); }
        text.push_str("\nPriority: Project > User > Bundled. Higher-priority packs with the same ID override this entry.");
        text
    }).style(|s|s.width_full().padding(8.0));
    let preview = scroll(dyn_stack(
        move || {
            let text = plan.get().map(|p| {
                let mut text = format!("{}\n{}\n",p.title,p.notes.join("\n"));
                for i in 0..p.changes.len() { text.push_str(&p.diff(i)); }
                if p.changes.is_empty() { text.push_str("No changes."); }
                text
            }).unwrap_or_else(|| "Choose an action to preview its changes. Apply writes the preview; Undo restores the last applied transaction.\nEdit opens pack files in the editor; save them, then Refresh to validate.".into());
            text.lines().enumerate().map(|(i,l)|(i,l.to_string())).collect::<Vec<_>>()
        }, |(i,l)|(*i,l.clone()),
        move |(_,line)| {
            let added = line.starts_with('+');
            let removed = line.starts_with('-');
            label(move ||line.clone()).style(move |s|s.height(20.0).padding_horiz(8.0)
                .font_family(config.get().editor.font_family.clone()).font_size(12.0)
                .apply_if(added,|s|s.background(floem::peniko::Color::from_rgba8(70,160,80,35)))
                .apply_if(removed,|s|s.background(floem::peniko::Color::from_rgba8(200,60,60,35))))
        }
    ).style(|s|s.flex_col())).style(|s|s.flex_grow(1.0_f32).min_width(0.0).width_full());
    let edit_window = window.clone();
    let built_in_window = window.clone();
    let draft = manager.editor.draft;
    let export_window = window.clone();
    let toggle_window = window.clone();
    let remove_window = window.clone();
    let create_window = window.clone();
    let import_window = window.clone();
    let duplicate_window = window.clone();
    let import_focus = focus.clone();
    let apply_window = window.clone();
    let undo_window = window.clone();
    let refresh_window = window.clone();
    let source_window = window.clone();
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
    let body = stack((
        label(||"Scaffolding Template Manager").style(|s|s.font_bold()),
        label(move ||root.get().display().to_string()),
        label(||"Filter templates"),
        input,
        stack((
            focus.control(label(move || if global.get() {"Destination: User ▾"} else {"Destination: Project ▾"})
                .on_click_stop(move |_| { global.update(|g|*g=!*g); plan.set(None); })),
            label(||"New pack ID:"),
            focus.input(new_id, config).style(|s|s.width(260.0)),
            focus.control(label(||"Create…").on_click_stop(move |_| {
                let m = &create_window.scaffold.manager;
                undoing.set(false);
                stage(&create_window,m.destination().and_then(|d|create(&d,&new_id.get_untracked())));
            })),
            focus.control(label(||"Import…").on_click_stop(move |_| {
                // Capture the destination at picker launch, and ignore callbacks from a closed dialog.
                let destination = import_window.scaffold.manager.destination();
                let generation = import_window.scaffold.generation.get_untracked();
                let window = import_window.clone();
                import_focus.open_file(
                    floem::file::FileDialogOptions::new().title("Import scaffold pack folder").select_directories(),
                    move |file| {
                        if !active.get_untracked() || !window.scaffold.manager_mode.get_untracked()
                            || window.scaffold.generation.get_untracked() != generation { return; }
                        if let Some(source) = file.and_then(|f|f.path.first().cloned()) {
                            undoing.set(false);
                            stage(&window,destination.clone().and_then(|d|import(&source,&d)));
                        }
                    },
                );
            })),
            focus.control(label(||"Refresh").on_click_stop(move |_| {
                refresh_window.scaffold.manager.refresh(); plan.set(None);
            })),
        )).style(|s|s.items_center().gap(14.0)),
        stack((list,stack((detail,preview)).style(|s|s.flex_col().flex_grow(1.0_f32).min_width(0.0).height_full())))
            .style(|s|s.height(420.0).width_full().gap(12.0)),
        stack((
            focus.control(label(move || if draft.get().is_some_and(|d|d.dirty()) {"Resume Editor…"} else {"Template Editor…"})
                .on_click_stop(move |_| {
                    if let Err(e) = editor::open(&built_in_window) { error.set(e); }
                })),
            focus.control(label(||"Edit Manifest").on_click_stop(move |_| {
                let m = &edit_window.scaffold.manager;
                let result = m.entry().ok_or("Select a template".to_string())
                    .and_then(|e|editable(&e,&root.get_untracked()));
                match result {
                    Ok(path) => { active.set(false); edit_window.common.internal_command.send(InternalCommand::OpenFile {path}); }
                    Err(e) => error.set(e),
                }
            })),
            focus.control(label(||"Edit Sources").on_click_stop(move |_| {
                let m = &source_window.scaffold.manager;
                let result = (|| {
                    let e = m.entry().ok_or("Select a template")?;
                    let manifest = editable(&e,&root.get_untracked())?;
                    let pack = e.load()?;
                    let sources = pack.files.iter().filter_map(|f|f.source.as_ref())
                        .map(|p| manifest.parent().unwrap().join(p)).collect::<Vec<_>>();
                    Ok::<_,String>((manifest,sources))
                })();
                match result {
                    Ok((manifest,sources)) => {
                        active.set(false);
                        for path in if sources.is_empty() {vec![manifest]} else {sources} {
                            source_window.common.internal_command.send(InternalCommand::OpenFile {path});
                        }
                    }
                    Err(e) => error.set(e),
                }
            })),
            focus.control(label(||"Enable / Disable…").on_click_stop(move |_| {
                let m = &toggle_window.scaffold.manager;
                undoing.set(false);
                stage(&toggle_window,(||toggle(&m.entry().ok_or("Select a template")?,&root.get_untracked(),&m.destination()?))());
            })),
            focus.control(label(||"Remove…").on_click_stop(move |_| {
                let m = &remove_window.scaffold.manager;
                undoing.set(false);
                stage(&remove_window,m.entry().ok_or("Select a template".to_string()).and_then(|e|remove(&e,&root.get_untracked())));
            })),
            focus.control(label(||"Duplicate…").on_click_stop(move |_| {
                let m = &duplicate_window.scaffold.manager;
                undoing.set(false);
                stage(&duplicate_window,(||duplicate(&m.entry().ok_or("Select a template")?,&m.destination()?,&new_id.get_untracked()))());
            })),
            focus.control(label(||"Export…").on_click_stop(move |_| {
                let m = &export_window.scaffold.manager;
                undoing.set(false);
                stage(&export_window,(||export(&m.entry().ok_or("Select a template")?,&m.destination()?))());
            })),
        )).style(|s|s.gap(18.0)),
        label(move ||error.get()),
        stack((
            focus.control(label(move ||if undoing.get() {"Apply Undo"} else {"Apply Preview"})
                .disabled(move ||plan.get().is_none())
                .on_click_stop(move |_| {
                    apply_window.scaffold.apply(&apply_window);
                    apply_window.scaffold.manager.refresh();
                    if plan.get_untracked().is_none() {
                        undoing.set(false);
                        error.set("Template changes applied. Undo is available in this manager and the command palette.".into());
                    }
                })),
            focus.control(label(||"Undo Last Change…").on_click_stop(move |_| {
                let s = &undo_window.scaffold;
                match s.history.get_untracked().last() {
                    Some(applied) => { undoing.set(true); plan.set(Some(applied.undo_plan())); error.set(String::new()); }
                    None => error.set("No scaffold or template change to undo in this window session.".into()),
                }
            })),
            focus.control(label(||"Close").on_click_stop(move |_|active.set(false))),
        )).style(|s|s.gap(18.0)),
    )).style(move |s|s.width(1120.0).max_width_pct(95.0).max_height_pct(95.0)
        .flex_col().gap(10.0).padding(18.0).border_radius(6.0).border(1.0)
        .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
        .background(config.get().color(SparrowColor::PANEL_BACKGROUND))
        .color(config.get().color(SparrowColor::EDITOR_FOREGROUND)));
    let dialog = container(body).style(move |s|s.absolute().size_full().items_center().justify_center().z_index(100)
        .apply_if(!active.get(),|s|s.hide()))
        .on_event_stop(EventListener::PointerDown, |_|{})
        .on_event(EventListener::KeyDown,move |event| {
            if matches!(event,Event::KeyDown(k) if k.key.logical_key==Key::Named(NamedKey::Escape)) {
                active.set(false); EventPropagation::Stop
            } else {EventPropagation::Continue}
        });
    focus.bind_root(dialog, active, config)
}
