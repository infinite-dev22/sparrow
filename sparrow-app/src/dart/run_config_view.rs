use super::run_config::{self, Draft};
use crate::{
    app::clickable_icon,
    command::InternalCommand,
    config::{color::SparrowColor, icon::SparrowIcons},
    debug::RunDebugMode,
    window_tab::WindowTabData,
};
use floem::{
    View,
    action::open_file,
    event::{Event, EventListener},
    file::FileDialogOptions,
    keyboard::{Key, NamedKey},
    reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith},
    style::CursorStyle,
    views::{
        Decorators, container, dyn_stack, empty, label, scroll, stack, svg,
        text_input,
    },
};
use std::rc::Rc;
struct Dialog {
    window: Rc<WindowTabData>,
    drafts: RwSignal<Vec<Draft>>,
    baseline: RwSignal<Vec<Draft>>,
    selected: RwSignal<usize>,
    fields: [RwSignal<String>; 8],
    activate: RwSignal<bool>,
    focus: RwSignal<bool>,
    project: RwSignal<bool>,
    baseline_project: RwSignal<bool>,
    error: RwSignal<String>,
}
impl Dialog {
    fn load(&self) {
        let Some(d) = self
            .drafts
            .with_untracked(|d| d.get(self.selected.get_untracked()).cloned())
        else {
            return;
        };
        for (field, text) in self.fields.iter().zip([
            d.name,
            d.entry,
            d.args,
            d.attach,
            d.flavor,
            d.env,
            d.before,
            d.before_args,
        ]) {
            field.set(text);
        }
        self.activate.set(d.activate);
        self.focus.set(d.focus);
    }
    fn snapshot(&self) -> Option<Draft> {
        let mut d = self
            .drafts
            .with_untracked(|d| d.get(self.selected.get_untracked()).cloned())?;
        [
            d.name,
            d.entry,
            d.args,
            d.attach,
            d.flavor,
            d.env,
            d.before,
            d.before_args,
        ] = self.fields.map(|f| f.get_untracked());
        d.activate = self.activate.get_untracked();
        d.focus = self.focus.get_untracked();
        Some(d)
    }
    fn capture(&self) {
        if let Some(d) = self.snapshot() {
            self.drafts
                .update(|drafts| drafts[self.selected.get_untracked()] = d);
        }
    }
    fn choose(&self, index: usize) {
        self.capture();
        self.selected.set(index);
        self.load();
        self.error.set(String::new());
    }
    fn dirty(&self) -> bool {
        // Track form edits without mutating the draft list during layout.
        for f in self.fields {
            let _ = f.get();
        }
        let _ = self.activate.get();
        let _ = self.focus.get();
        let mut drafts = self.drafts.get();
        if let Some(d) = self.snapshot() {
            if let Some(slot) = drafts.get_mut(self.selected.get()) {
                *slot = d;
            }
        }
        drafts != self.baseline.get()
            || self.project.get() != self.baseline_project.get()
    }
    fn apply(&self) -> bool {
        self.capture();
        let result = (|| {
            let drafts = self.drafts.get_untracked();
            let configs = drafts
                .iter()
                .map(Draft::build)
                .collect::<Result<Vec<_>, _>>()?;
            let root = self
                .window
                .workspace
                .path
                .as_deref()
                .ok_or("Open a workspace before saving configurations.")?;
            if !self.window.workspace.kind.is_local() {
                return Err(
                    "Configuration editing currently requires a local workspace."
                        .into(),
                );
            }
            run_config::save(root, configs.clone(), self.project.get_untracked())?;
            self.window.run_configs.set(configs);
            self.window
                .active_run_config_idx
                .set(self.selected.get_untracked());
            self.baseline.set(drafts);
            self.baseline_project.set(self.project.get_untracked());
            Ok::<_, String>(())
        })();
        match result {
            Ok(()) => {
                self.error.set(String::new());
                true
            }
            Err(e) => {
                self.error.set(e);
                false
            }
        }
    }
    fn launch(&self, attach: bool) {
        if !self.apply() {
            return;
        }
        if let Some(mut c) = self
            .window
            .run_configs
            .with_untracked(|c| c.get(self.selected.get_untracked()).cloned())
        {
            if attach {
                let mut args = vec!["attach".into()];
                args.extend(c.flutter_options.attach_args.clone());
                if let Some(device) = self
                    .window
                    .flutter_hot_reload
                    .selected_device
                    .get_untracked()
                {
                    args.extend(["-d".into(), device.id]);
                }
                c.args = Some(args);
            }
            self.window.run_config_modal_visible.set(false);
            self.window
                .common
                .internal_command
                .send(InternalCommand::RunAndDebug {
                    mode: RunDebugMode::Run,
                    config: c,
                });
        }
    }
}
pub fn view(window: Rc<WindowTabData>) -> impl View {
    let cx = Scope::current();
    let visible = window.run_config_modal_visible;
    let config = window.common.config;
    let d = Rc::new(Dialog {
        window,
        drafts: cx.create_rw_signal(Vec::new()),
        baseline: cx.create_rw_signal(Vec::new()),
        selected: cx.create_rw_signal(0),
        fields: std::array::from_fn(|_| cx.create_rw_signal(String::new())),
        activate: cx.create_rw_signal(true),
        focus: cx.create_rw_signal(false),
        project: cx.create_rw_signal(false),
        baseline_project: cx.create_rw_signal(false),
        error: cx.create_rw_signal(String::new()),
    });
    cx.create_effect({
        let d = d.clone();
        move |_| {
            if visible.get() {
                let drafts = d
                    .window
                    .run_configs
                    .get_untracked()
                    .into_iter()
                    .map(Draft::from_config)
                    .collect::<Vec<_>>();
                d.selected.set(
                    d.window
                        .active_run_config_idx
                        .get_untracked()
                        .min(drafts.len().saturating_sub(1)),
                );
                d.baseline.set(drafts.clone());
                d.drafts.set(drafts);
                let project =
                    d.window.workspace.path.as_deref().is_some_and(|root| {
                        !run_config::local_file(root).is_some_and(|p| p.is_file())
                            && root.join(".sparrow/run.toml").is_file()
                    });
                d.project.set(project);
                d.baseline_project.set(project);
                d.error.set(String::new());
                d.load();
            }
        }
    });
    let add = d.clone();
    let remove = d.clone();
    let duplicate = d.clone();
    let sidebar = stack((
        stack((
            clickable_icon(
                || SparrowIcons::ADD,
                move || {
                    add.capture();
                    if let Some(mut draft) = add.drafts.with_untracked(|v| {
                        v.get(add.selected.get_untracked()).cloned()
                    }) {
                        draft.name = "New configuration".into();
                        draft.original.dap_id = Default::default();
                        if draft.flutter() {
                            draft.entry = "lib/main.dart".into();
                        }
                        draft.args.clear();
                        draft.attach.clear();
                        draft.flavor.clear();
                        draft.env.clear();
                        draft.before.clear();
                        draft.before_args.clear();
                        let index = add.drafts.with_untracked(Vec::len);
                        add.drafts.update(|v| v.push(draft));
                        add.selected.set(index);
                        add.load();
                    }
                },
                || false,
                || false,
                || "Add Configuration",
                config,
            ),
            clickable_icon(
                || SparrowIcons::WINDOW_MINIMIZE,
                move || {
                    remove.capture();
                    if remove.drafts.with_untracked(Vec::len) > 1 {
                        let index = remove.selected.get_untracked();
                        remove.drafts.update(|v| {
                            v.remove(index);
                        });
                        remove.selected.set(index.saturating_sub(1));
                        remove.load();
                    }
                },
                || false,
                {
                    let drafts = d.drafts;
                    move || drafts.with(|v| v.len() <= 1)
                },
                || "Remove Configuration",
                config,
            ),
            clickable_icon(
                || SparrowIcons::FILE,
                move || {
                    duplicate.capture();
                    if let Some(mut draft) = duplicate.drafts.with_untracked(|v| {
                        v.get(duplicate.selected.get_untracked()).cloned()
                    }) {
                        draft.name.push_str(" (copy)");
                        draft.original.dap_id = Default::default();
                        let index = duplicate.drafts.with_untracked(Vec::len);
                        duplicate.drafts.update(|v| v.push(draft));
                        duplicate.selected.set(index);
                        duplicate.load();
                    }
                },
                || false,
                || false,
                || "Duplicate Configuration",
                config,
            ),
        ))
        .style(|s| s.height(26.0).items_center()),
        label({
            let drafts = d.drafts;
            move || {
                if drafts.with(|v| v.iter().all(Draft::flutter)) {
                    "⌄  Flutter"
                } else {
                    "⌄  Run configurations"
                }
            }
        })
        .style(|s| s.padding_left(10.0).height(24.0).font_bold()),
        scroll(
            dyn_stack(
                {
                    let d = d.clone();
                    move || {
                        d.drafts.get().into_iter().enumerate().collect::<Vec<_>>()
                    }
                },
                |(i, _)| *i,
                {
                    let d = d.clone();
                    move |(i, draft)| {
                        let select = d.clone();
                        let active = d.selected;
                        let name = d.fields[0];
                        stack((
                            svg(|| include_str!("../../../icons/dart-flutter/flutter_logo.svg").to_string())
                                .style(|s| s.size(16.0, 16.0)),
                            label(move || {
                                if active.get() == i {
                                    name.get()
                                } else {
                                    draft.name.clone()
                                }
                            }),
                        ))
                        .on_click_stop(move |_| select.choose(i))
                        .style(move |s| {
                            s.height(22.0)
                                .width_full()
                                .padding_left(36.0)
                                .gap(4.0)
                                .items_center()
                                .border_radius(4.0)
                                .apply_if(active.get() == i, |s| {
                                    s.background(config.get().color(
                                        SparrowColor::SPARROW_TAB_ACTIVE_BACKGROUND,
                                    ))
                                })
                                .hover(|s| s.cursor(CursorStyle::Pointer))
                        })
                    }
                },
            )
            .style(|s| s.flex_col().min_width_full()),
        )
        .style(|s| s.flex_grow(1.0_f32).min_height(0.0)),
    ))
    .style(move |s| {
        s.width(210.0)
            .height_full()
            .flex_col()
            .flex_shrink(0.0_f32)
            .padding_horiz(10.0)
            .border_right(1.0)
            .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
    });
    let form_row =
        move |title: &'static str, field: RwSignal<String>, help: &'static str| {
            stack((
                stack((
                    label(move || title).style(move |s| {
                        s.width(if title == "Name:" { 64.0 } else { 154.0 })
                            .flex_shrink(0.0_f32)
                    }),
                    text_input(field).style(move |s| {
                        s.height(24.0)
                            .min_width(0.0)
                            .flex_grow(1.0_f32)
                            .padding_horiz(8.0)
                            .border(1.0)
                            .border_radius(3.0)
                            .border_color(
                                config.get().color(SparrowColor::SPARROW_BORDER),
                            )
                            .background(
                                config.get().color(SparrowColor::EDITOR_BACKGROUND),
                            )
                    }),
                ))
                .style(|s| s.items_center().width_full()),
                label(move || help).style(move |s| {
                    s.margin_left(154.0)
                        .margin_top(6.0)
                        .font_size(12.0)
                        .color(config.get().color(SparrowColor::EDITOR_DIM))
                        .apply_if(help.is_empty(), |s| s.hide())
                }),
            ))
            .style(move |s| {
                s.flex_col()
                    .margin_bottom(if title == "Name:" { 0.0 } else { 15.0 })
                    .width_full()
            })
        };
    let entry = d.fields[1];
    let before = d.fields[6];
    let before_args = d.fields[7];
    let editing_task = cx.create_rw_signal(false);
    let selected_task = d.selected;
    cx.create_effect(move |_| {
        let _ = selected_task.get();
        if visible.get() {
            editing_task.set(!before.get_untracked().is_empty());
        }
    });
    let apply = d.clone();
    let ok = d.clone();
    let run = d.clone();
    let attach = d.clone();
    let dirty = d.clone();
    let activate = d.activate;
    let focus = d.focus;
    let project = d.project;
    let error = d.error;
    let checkbox = move |value: RwSignal<bool>, text: &'static str| {
        stack((
            label(move || if value.get() { "✓" } else { "" }).style(move |s| {
                s.size(16.0, 16.0)
                    .items_center()
                    .justify_center()
                    .border(1.0)
                    .border_radius(3.0)
                    .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
                    .apply_if(value.get(), |s| {
                        s.background(
                            config.get().color(
                                SparrowColor::SPARROW_BUTTON_PRIMARY_BACKGROUND,
                            ),
                        )
                    })
            }),
            label(move || text),
        ))
        .on_click_stop(move |_| value.update(|v| *v = !*v))
        .style(|s| {
            s.items_center()
                .gap(8.0)
                .padding_vert(4.0)
                .flex_shrink(0.0_f32)
        })
    };
    let button = move |text: &'static str| {
        label(move || text).style(move |s| {
            s.min_width(72.0)
                .height(24.0)
                .padding_horiz(14.0)
                .items_center()
                .justify_center()
                .border(1.0)
                .border_radius(4.0)
                .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
                .hover(|s| {
                    s.cursor(CursorStyle::Pointer).background(
                        config.get().color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                    )
                })
        })
    };
    let dialog = container(container(stack((
        stack((empty().style(|s|s.width(26.0)),label(||"Run/Debug Configurations").style(|s|s.flex_grow(1.0_f32).items_center().justify_center().font_bold()),clickable_icon(||SparrowIcons::CLOSE,move ||visible.set(false),||false,||false,||"Close",config)))
            .style(move |s|s.height(36.0).width_full().items_center().flex_shrink(0.0_f32).background(floem::peniko::Color::from_rgb8(40,40,40))),
        stack((sidebar,scroll(stack((
            stack((form_row("Name:",d.fields[0],"").style(|s|s.flex_grow(1.0_f32).flex_basis(0.0).min_width(0.0)),checkbox(project,"Store as project file"))).style(|s|s.items_center().gap(16.0).margin_bottom(16.0)),
            stack((form_row("Dart entrypoint:",entry,"The entry-point for the application (e.g. lib/main.dart).").style(|s|s.flex_grow(1.0_f32).flex_basis(0.0).min_width(0.0)),clickable_icon(||SparrowIcons::DIRECTORY_CLOSED,move ||open_file(FileDialogOptions::new().title("Choose Dart entrypoint"),move |file|{if let Some(file)=file{if let Some(path)=file.path.first(){entry.set(path.display().to_string());}}}),||false,||false,||"Browse",config))).style(|s|s.items_start()),
            form_row("Additional run args:",d.fields[2],"Additional arguments to 'flutter run'."),
            form_row("Additional attach args:",d.fields[3],"Additional arguments to 'flutter attach'."),
            form_row("Build flavor:",d.fields[4],"Optional Gradle product flavor or Xcode custom scheme."),
            form_row("Environment variables:",d.fields[5],"Additional environment variables. Format: KEY=value; KEY2=value."),
            label(||"⌄  Before launch").style(|s|s.margin_top(6.0).margin_bottom(10.0)),
            stack((
                stack((
                    clickable_icon(||SparrowIcons::ADD,move ||editing_task.set(true),||false,||false,||"Add before-launch task",config),
                    clickable_icon(||SparrowIcons::REMOVE,move ||{before.set(String::new());before_args.set(String::new());editing_task.set(false);},||false,move ||!editing_task.get(),||"Remove before-launch task",config),
                )).style(move |s|s.height(26.0).width_full().items_center().border_bottom(1.0).border_color(config.get().color(SparrowColor::SPARROW_BORDER))),
                label(||"There are no tasks to run before launch").style(move |s|s.width_full().flex_grow(1.0_f32).items_center().justify_center().color(config.get().color(SparrowColor::EDITOR_DIM)).apply_if(editing_task.get(),|s|s.hide())),
                stack((form_row("Program:",before,""),form_row("Arguments:",before_args,""))).style(move |s|s.flex_col().padding(8.0).width_full().apply_if(!editing_task.get(),|s|s.hide())),
            )).style(move |s|s.flex_col().height(134.0).margin_left(24.0).border(1.0).border_color(config.get().color(SparrowColor::SPARROW_BORDER))),
            stack((checkbox(activate,"Activate tool window"),checkbox(focus,"Focus tool window"))).style(|s|s.gap(12.0).margin_top(4.0)),
            label(move ||error.get()).style(move |s|s.color(config.get().color(SparrowColor::SPARROW_ERROR)).selectable(true)),
        )).style(|s|s.flex_col().padding(16.0).width_full())).style(|s|s.flex_grow(1.0_f32).flex_basis(0.0).min_width(0.0).height_full())))
            .style(|s|s.width_full().flex_grow(1.0_f32).flex_basis(0.0).min_height(0.0)),
        stack((empty().style(|s|s.flex_grow(1.0_f32)),stack((button("▷ Run").on_click_stop(move |_|run.launch(false)),button("▾").style(|s|s.min_width(24.0).padding_horiz(4.0)).popout_menu(move ||{let attach=attach.clone();floem::menu::Menu::new("").entry(floem::menu::MenuItem::new("Attach to Flutter App").action(move ||attach.launch(true)))}))).style(|s|s.gap(0.0)),button("OK").on_click_stop(move |_|{if ok.apply(){visible.set(false);}}).style(move |s|s.background(config.get().color(SparrowColor::SPARROW_BUTTON_PRIMARY_BACKGROUND))),button("Cancel").on_click_stop(move |_|visible.set(false)),button("Apply").on_click_stop(move |_|{apply.apply();}).disabled(move ||!dirty.dirty())))
            .style(move |s|s.height(46.0).width_full().padding_horiz(16.0).gap(12.0).items_center().flex_shrink(0.0_f32).border_top(1.0).border_color(config.get().color(SparrowColor::SPARROW_BORDER))),
    )).style(|s|s.flex_col().size_full())).style(move |s|s.font_family(config.get().ui.font_family.clone()).font_size(13.0).width(916.0).height(684.0).max_width_pct(96.0).max_height_pct(92.0).flex_col().color(config.get().color(SparrowColor::EDITOR_FOREGROUND)).background(config.get().color(SparrowColor::PANEL_BACKGROUND)).border_radius(8.0)))
    .on_event_stop(EventListener::PointerDown,|_|{})
    .on_event(EventListener::KeyDown,move |event|{if let Event::KeyDown(key)=event{if key.key.logical_key==Key::Named(NamedKey::Escape){visible.set(false);return floem::event::EventPropagation::Stop;}}floem::event::EventPropagation::Continue})
    .style(move |s|s.absolute().size_full().padding_bottom(116.0).items_center().justify_center().z_index(200).background(floem::peniko::Color::from_rgba8(0,0,0,70)).apply_if(!visible.get(),|s|s.hide()));
    let id = dialog.id();
    cx.create_effect(move |_| {
        if visible.get() {
            id.request_focus();
        }
    });
    dialog
}
