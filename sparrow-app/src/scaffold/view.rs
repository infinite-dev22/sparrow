//! Scaffold generator dialog and transaction preview.

use super::{catalog, engine, manager};
use crate::{
    config::color::SparrowColor,
    widgets::popup_menu::{Menu, MenuItem, PopupMenuExt},
    window_tab::WindowTabData,
};
use floem::{
    View,
    event::{Event, EventListener, EventPropagation},
    keyboard::{Key, NamedKey},
    reactive::{SignalGet, SignalUpdate},
    views::{Decorators, container, dyn_stack, label, stack},
};
use std::{path::Path, rc::Rc};
use crate::widgets::sparrow_scroll;

pub fn view(window: Rc<WindowTabData>) -> impl View {
    let mode = window.scaffold.manager_mode;
    floem::views::dyn_container(move || mode.get(), move |manager| -> Box<dyn View> {
        if manager { Box::new(manager::view(window.clone())) }
        else { Box::new(scaffold_view(window.clone())) }
    }).style(|s| s.absolute().size_full())
}
fn scaffold_view(window: Rc<WindowTabData>) -> impl View {
    let state = window.scaffold.clone();
    let config = window.common.config;
    let active = state.active;
    crate::widgets::modal_focus::restore_workbench_on_close(active, window.common.clone());
    let name = state.name;
    let template = state.template;
    let error = state.error;
    let base = state.base;
    let plan = state.plan;
    let selected = state.selected;
    let undoing = state.undoing;
    let busy = state.busy;
    let export_mode = state.export_mode;
    let picker = state.clone();
    let picker_window = window.clone();
    let auto_register = state.auto_register;
    let registration_plan = state.plan;
    let focus = crate::widgets::modal_focus::ModalFocus::default();
    let preview = state.clone();
    let w = window.clone();
    focus.set_submit(move || preview.preview(&w));
    let input = focus
        .input(name, config)
        .disabled(move || undoing.get() || busy.get() || export_mode.get());
    let id = input.id();
    let initial_focus = focus.clone();
    floem::reactive::create_effect(move |_| {
        if active.get() {
            let exporting = export_mode.get() || undoing.get();
            let focus = initial_focus.clone();
            floem::action::exec_after(
                std::time::Duration::from_millis(30),
                move |_| {
                    if exporting {
                        focus.focus_first();
                    } else {
                        id.request_focus();
                    }
                },
            );
        }
    });
    let file_focus = focus.clone();
    let list = sparrow_scroll(
        dyn_stack(
            move || {
                plan.get()
                    .map(|p| {
                        p.changes
                            .iter()
                            .enumerate()
                            .map(|(i, c)| {
                                (
                                    i,
                                    c.path
                                        .strip_prefix(&p.root)
                                        .unwrap_or(&c.path)
                                        .display()
                                        .to_string(),
                                    if c.after.is_none() {
                                        "Delete"
                                    } else if c.before.is_none() {
                                        "Create"
                                    } else {
                                        "Edit"
                                    },
                                )
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            },
            |(i, path, _)| (*i, path.clone()),
            move |(i, path, kind)| {
                let filename = Path::new(&path)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                file_focus.control(
                    label(move || format!("{kind}  {filename}"))
                        .on_click_stop(move |_| selected.set(i))
                        .style(move |s| {
                            s.width_full().padding(6.0).apply_if(
                                selected.get() == i,
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
    .style(|s| s.width(260.0).height_full());
    let diff=sparrow_scroll(dyn_stack(
        move ||plan.get().map(|p|p.diff(selected.get())).unwrap_or_else(|| if export_mode.get() {"Select a bundled pack and Preview Export. Every exported file will appear here before anything is written.".into()} else {"Enter a name and select Preview Changes. Every affected file will appear here before anything is written.".into()}).lines().enumerate().map(|(i,line)|(i,line.to_string())).collect::<Vec<_>>(),
        |(i,line)|(*i,line.clone()),
        move |(_,line)| {
            let added=line.starts_with('+');let removed=line.starts_with('-');
            label(move ||line.clone()).style(move |s|s.min_width_full().height(20.0).padding_horiz(8.0).font_family(config.get().editor.font_family.clone()).font_size(12.0)
                .apply_if(added,|s|s.background(floem::peniko::Color::from_rgba8(70,160,80,35)))
                .apply_if(removed,|s|s.background(floem::peniko::Color::from_rgba8(200,60,60,35))))
        },
    ).style(|s|s.flex_col())).style(|s|s.flex_grow(1.0_f32).min_width(0.0).height_full());
    let preview = state.clone();
    let w = window.clone();
    let apply = state.clone();
    let aw = window.clone();
    let undo = state.clone();
    let history = state.history;
    let export = state.clone();
    let export_window = window.clone();
    let pub_state = state.clone();
    let pub_window = window.clone();
    let body = stack((
        label(move || {
            if undoing.get() {
                "Undo Last Scaffold".into()
            } else if export_mode.get() {
                "Export Bundled Template".into()
            } else {
                format!(
                    "New {}",
                    template.get().map(|t| t.title).unwrap_or_default()
                )
            }
        })
        .style(|s| s.font_bold()),
        label(move || base.get().display().to_string()).style(|s| s.width_full()),
        focus.control(
            label(move || {
                format!(
                    "{} ▾",
                    template
                        .get()
                        .map(|t| t.title)
                        .unwrap_or_else(|| "Choose a pack".into())
                )
            })
            .ide_popout_menu(move || {
                let mut menu = Menu::new("Scaffold packs");
                let Some(root) = picker_window.workspace.path.as_deref() else {
                    return menu;
                };
                let exporting = picker.export_mode.get_untracked();
                let entries = if exporting {
                    catalog::bundled_entries()
                } else {
                    engine::context(root, &picker.base.get_untracked())
                        .map(|ctx| catalog(&ctx.root))
                        .unwrap_or_default()
                };
                for entry in entries {
                    let state = picker.clone();
                    let base = state.base.get_untracked();
                    menu = menu.entry(
                        MenuItem::new(format!(
                            "{} · {}",
                            entry.title, entry.provenance
                        ))
                        .action(move || {
                            state.open(base.clone(), entry.clone());
                            state.export_mode.set(exporting);
                        }),
                    );
                }
                menu
            })
            .disabled(move || busy.get() || undoing.get())
                .style(move |s| s.apply_if(export_mode.get() || undoing.get(), |s| s.hide())),
        ),
        focus.control(label(|| "Manage templates…").on_click_stop({
            let window = window.clone();
            move |_| window.scaffold.open_manager(&window)
        })),
        input.style(move |s| s.apply_if(export_mode.get(), |s| s.hide())),
        focus.control(
            crate::widgets::labeled_checkbox(
                move || auto_register.get(),
                || "Automatically register router and providers",
                move |checked| {
                    auto_register.set(checked);
                    registration_plan.set(None);
                },
                config,
            )
            .disabled(move || busy.get())
            .style(move |s| {
                s.apply_if(
                    undoing.get()
                        || export_mode.get()
                        || !template.get().is_some_and(|t| t.registration),
                    |s| s.hide(),
                )
            }),
        ),
        sparrow_scroll(label(move || {
            plan.get().map(|p| p.notes.join("\n")).unwrap_or_default()
        }))
        .style(|s| s.width_full().max_height(100.0)),
        stack((list, diff)).style(move |s| {
            s.width_full()
                .height(340.0)
                .border(1.0)
                .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
        }),
        label(move || error.get()).style(|s| s.width_full()),
        stack((
            focus.control(
                label(move || {
                    if export_mode.get() {
                        "Preview Export"
                    } else {
                        "Preview Changes"
                    }
                })
                .on_click_stop(move |_| preview.preview(&w))
                .disabled(move || busy.get()),
            ),
            focus.control(
                label(move || {
                    if undoing.get() {
                        "Undo Scaffold"
                    } else {
                        "Apply Changes"
                    }
                })
                .on_click_stop(move |_| apply.apply(&aw))
                .disabled(move || plan.get().is_none() || busy.get()),
            ),
            focus.control(
                label(|| "Undo Last Scaffold…")
                    .on_click_stop(move |_| undo.open_undo())
                    .disabled(move || history.get().is_empty()),
            ),
            focus.control(label(|| "Pub Get").on_click_stop(move |_| {
                let Some(root) = pub_window.workspace.path.as_deref() else {
                    return;
                };
                match engine::context(root, &pub_state.base.get_untracked()) {
                    Ok(ctx) => {
                        if let Some(dart) =
                            crate::dart::sdk::find_dart_binary(Some(&ctx.root))
                        {
                            pub_window.common.internal_command.send(
                                crate::command::InternalCommand::RunProjectTask {
                                    name: format!("Pub Get · {}", ctx.package),
                                    program: dart.display().to_string(),
                                    args: vec!["pub".into(), "get".into()],
                                    cwd: Some(ctx.root.clone()),
                                },
                            );
                        } else {
                            pub_state.error.set(
                                "Configure a Dart or Flutter SDK first.".into(),
                            );
                        }
                    }
                    Err(e) => pub_state.error.set(e),
                }
            })).style(move |s| s.apply_if(export_mode.get(), |s| s.hide())),
            focus.control(
                label(|| "Close").on_click_stop(move |_| active.set(false)),
            ),
        ))
        .style(|s| s.gap(16.0)),
        focus.control(
            label(|| "Export bundled default to project…")
                .on_click_stop(move |_| export.export(&export_window))
                .disabled(move || busy.get() || undoing.get())
                .style(move |s| s.apply_if(export_mode.get() || undoing.get(), |s| s.hide())),
        ),
    ))
    .style(move |s| {
        crate::widgets::dialog_surface(
            s.width(1020.0)
                .max_width_pct(95.0)
                .max_height_pct(95.0)
                .flex_col()
                .gap(12.0)
                .padding(18.0),
            &config.get(),
        )
    });
    let dialog=container(body).style(move |s|s.absolute().size_full().items_center().justify_center().z_index(100).apply_if(!active.get(),|s|s.hide())).on_event_stop(EventListener::PointerDown, |_|{}).on_event(EventListener::KeyDown,move |e|{if matches!(e,Event::KeyDown(k) if k.key.logical_key==Key::Named(NamedKey::Escape)){active.set(false);EventPropagation::Stop}else{EventPropagation::Continue}});
    focus.bind_root(dialog, active, config)
}
