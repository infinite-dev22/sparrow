use crate::widgets::file_picker::FileDialogOptions;
use super::run_config::{self, Draft, TaskDraft};
use crate::widgets::modal_focus::{ModalChoice, ModalFocus};
use crate::{
    command::InternalCommand,
    config::{color::SparrowColor, icon::SparrowIcons},
    debug::RunDebugMode,
    widgets::clickable_icon,
    window_tab::WindowTabData,
};
use floem::{
    View,
    event::{Event, EventListener},
    file::FileSpec,
    keyboard::{Key, NamedKey},
    reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith},
    views::{Decorators, container, empty, label, stack},
};
use std::rc::Rc;
mod sidebar;
mod task_list;

use crate::widgets::sparrow_scroll;
use sidebar::configuration_sidebar;
use task_list::task_list;

struct Dialog {
    window: Rc<WindowTabData>,
    keyboard: ModalFocus,
    drafts: RwSignal<Vec<Draft>>,
    baseline: RwSignal<Vec<Draft>>,
    selected: RwSignal<usize>,
    fields: [RwSignal<String>; 9],
    load_epoch: RwSignal<usize>,
    name_input: RwSignal<Option<floem::ViewId>>,
    activate: RwSignal<bool>,
    focus: RwSignal<bool>,
    project: RwSignal<bool>,
    templates: RwSignal<Vec<Draft>>,
    baseline_templates: RwSignal<Vec<Draft>>,
    templates_mode: RwSignal<bool>,
    task_selected: RwSignal<usize>,
    error: RwSignal<String>,
}
impl Dialog {
    fn current(&self) -> RwSignal<Vec<Draft>> {
        if self.templates_mode.get_untracked() {
            self.templates
        } else {
            self.drafts
        }
    }
    fn current_tracked(&self) -> RwSignal<Vec<Draft>> {
        let _ = self.templates_mode.get();
        self.current()
    }
    fn choose_config(&self, index: usize) {
        self.capture();
        self.templates_mode.set(false);
        self.selected.set(index);
        self.load();
        self.error.set(String::new());
    }
    fn choose_template(&self, index: usize) {
        self.capture();
        self.templates_mode.set(true);
        self.selected.set(index);
        self.load();
        self.error.set(String::new());
    }
    fn load_task(&self) {
        let task = self.current().with_untracked(|drafts| {
            drafts
                .get(self.selected.get_untracked())
                .and_then(|draft| {
                    draft.tasks.get(self.task_selected.get_untracked())
                })
                .cloned()
        });
        self.fields[6].set(
            task.as_ref()
                .map(|task| task.program.clone())
                .unwrap_or_default(),
        );
        self.fields[7].set(task.map(|task| task.args).unwrap_or_default());
    }
    fn choose_task(&self, index: usize) {
        self.capture();
        self.task_selected.set(index);
        self.load_task();
    }
    fn edit_task(&self, action: i8) {
        self.capture();
        let selected = self.task_selected.get_untracked();
        let mut next = selected;
        self.current().update(|drafts| {
            if let Some(draft) = drafts.get_mut(self.selected.get_untracked()) {
                match action {
                    0 => {
                        draft.tasks.push(TaskDraft::new());
                        next = draft.tasks.len() - 1;
                    }
                    1 if selected < draft.tasks.len() => {
                        draft.tasks.remove(selected);
                        next = selected.min(draft.tasks.len().saturating_sub(1));
                    }
                    -1 if selected > 0 => {
                        draft.tasks.swap(selected, selected - 1);
                        next = selected - 1;
                    }
                    2 if selected + 1 < draft.tasks.len() => {
                        draft.tasks.swap(selected, selected + 1);
                        next = selected + 1;
                    }
                    _ => {}
                }
            }
        });
        self.task_selected.set(next);
        self.load_task();
    }

    fn load(&self) {
        let Some(d) = self
            .current()
            .with_untracked(|d| d.get(self.selected.get_untracked()).cloned())
        else {
            for field in self.fields {
                field.set(String::new());
            }
            self.load_epoch.update(|epoch| *epoch += 1);
            let keyboard = self.keyboard.clone();
            floem::action::exec_after(
                std::time::Duration::from_millis(30),
                move |_| keyboard.focus_first(),
            );
            return;
        };
        for (field, text) in self.fields.iter().zip([
            d.name,
            d.entry,
            d.args,
            d.attach,
            d.flavor,
            d.env,
            String::new(),
            String::new(),
            d.cwd,
        ]) {
            field.set(text);
        }
        self.task_selected.set(0);
        self.load_task();
        self.project.set(d.project);
        self.activate.set(d.activate);
        self.focus.set(d.focus);
        self.load_epoch.update(|epoch| *epoch += 1);
    }
    fn snapshot(&self) -> Option<Draft> {
        let mut d = self
            .current()
            .with_untracked(|d| d.get(self.selected.get_untracked()).cloned())?;
        d.name = self.fields[0].get_untracked();
        d.entry = self.fields[1].get_untracked();
        d.args = self.fields[2].get_untracked();
        d.attach = self.fields[3].get_untracked();
        d.flavor = self.fields[4].get_untracked();
        d.env = self.fields[5].get_untracked();
        d.cwd = self.fields[8].get_untracked();
        if let Some(task) = d.tasks.get_mut(self.task_selected.get_untracked()) {
            task.program = self.fields[6].get_untracked();
            task.args = self.fields[7].get_untracked();
        }
        d.project = self.project.get_untracked();
        d.activate = self.activate.get_untracked();
        d.focus = self.focus.get_untracked();
        Some(d)
    }
    fn capture(&self) {
        if let Some(d) = self.snapshot() {
            self.current().update(|drafts| {
                if let Some(slot) = drafts.get_mut(self.selected.get_untracked()) {
                    *slot = d;
                }
            });
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
        let _ = self.project.get();
        let mut drafts = self.drafts.get();
        let mut templates = self.templates.get();
        if let Some(draft) = self.snapshot() {
            let current = if self.templates_mode.get() {
                &mut templates
            } else {
                &mut drafts
            };
            if let Some(slot) = current.get_mut(self.selected.get()) {
                *slot = draft;
            }
        }
        drafts != self.baseline.get() || templates != self.baseline_templates.get()
    }

    fn apply(&self) -> bool {
        self.capture();
        let result = (|| {
            let drafts = self.drafts.get_untracked();
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
            let configs = run_config::build_drafts(&drafts, root)?;
            let template_drafts = self.templates.get_untracked();
            let templates = template_drafts
                .iter()
                .map(Draft::build_template)
                .collect::<Result<Vec<_>, _>>()?;
            run_config::save_bundle(root, configs.clone(), Some(templates))?;
            self.window.run_configs.set(configs);
            self.window.active_run_config_idx.set(
                if self.templates_mode.get_untracked() {
                    self.window
                        .active_run_config_idx
                        .get_untracked()
                        .min(drafts.len().saturating_sub(1))
                } else {
                    self.selected.get_untracked()
                },
            );
            self.baseline.set(drafts);
            self.baseline_templates.set(template_drafts);
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
    fn remove(&self) {
        if self.templates_mode.get_untracked() {
            return;
        }
        self.capture();
        if !self.drafts.with_untracked(Vec::is_empty) {
            let index = self.selected.get_untracked();
            self.drafts.update(|v| {
                v.remove(index);
            });
            self.selected.set(index.saturating_sub(1));
            self.load();
        }
    }
    fn focus_name(&self) {
        let name = self.name_input;
        let visible = self.window.run_config_modal_visible;
        floem::action::exec_after(std::time::Duration::from_millis(30), move |_| {
            if visible.get_untracked() {
                if let Some(id) = name.get_untracked() {
                    id.request_focus();
                }
            }
        });
    }
    fn duplicate(&self) {
        if self.templates_mode.get_untracked() {
            return;
        }
        self.capture();
        if let Some(mut draft) = self
            .drafts
            .with_untracked(|v| v.get(self.selected.get_untracked()).cloned())
        {
            draft.name = self.drafts.with_untracked(|drafts| {
                run_config::unique_name(drafts, &format!("{} (copy)", draft.name))
            });
            draft.original.dap_id = Default::default();
            let index = self.drafts.with_untracked(Vec::len);
            self.drafts.update(|v| v.push(draft));
            self.selected.set(index);
            self.load();
            self.focus_name();
        }
    }
    fn add(&self, index: usize) {
        self.capture();
        let config = match self
            .templates
            .with_untracked(|templates| templates[index].build_template())
        {
            Ok(config) => config,
            Err(error) => {
                self.error.set(format!(
                    "Fix the template before creating a configuration: {error}"
                ));
                return;
            }
        };
        self.templates_mode.set(false);
        let mut draft = Draft::from_config(config);
        draft.original.dap_id = Default::default();
        draft.name = self
            .drafts
            .with_untracked(|drafts| run_config::unique_name(drafts, &draft.name));
        let index = self.drafts.with_untracked(Vec::len);
        self.drafts.update(|drafts| drafts.push(draft));
        self.selected.set(index);
        self.error.set(String::new());
        self.load();
        self.focus_name();
    }

    fn launch(&self, mode: RunDebugMode, attach: bool) {
        if self.templates_mode.get_untracked() {
            return;
        }
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
                .send(InternalCommand::RunAndDebug { mode, config: c });
        }
    }
}
pub fn view(window: Rc<WindowTabData>) -> impl View {
    let cx = Scope::current();
    let visible = window.run_config_modal_visible;
    let config = window.common.config;
    let keyboard = ModalFocus::default();
    let d = Rc::new(Dialog {
        window,
        keyboard: keyboard.clone(),
        drafts: cx.create_rw_signal(Vec::new()),
        baseline: cx.create_rw_signal(Vec::new()),
        selected: cx.create_rw_signal(0),
        fields: std::array::from_fn(|_| cx.create_rw_signal(String::new())),
        load_epoch: cx.create_rw_signal(0),
        name_input: cx.create_rw_signal(None),
        activate: cx.create_rw_signal(true),
        focus: cx.create_rw_signal(false),
        project: cx.create_rw_signal(false),
        templates: cx.create_rw_signal(Vec::new()),
        baseline_templates: cx.create_rw_signal(Vec::new()),
        templates_mode: cx.create_rw_signal(false),
        task_selected: cx.create_rw_signal(0),
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
                d.templates_mode.set(false);
                let templates = run_config::load_templates(
                    d.window
                        .workspace
                        .path
                        .as_deref()
                        .unwrap_or(std::path::Path::new(".")),
                )
                .into_iter()
                .map(Draft::from_config)
                .collect::<Vec<_>>();
                d.templates.set(templates.clone());
                d.baseline_templates.set(templates);
                d.error.set(String::new());
                d.load();
            }
        }
    });
    keyboard.set_submit({
        let d = d.clone();
        move || {
            if d.apply() {
                visible.set(false);
            }
        }
    });
    let templates_mode = d.templates_mode;
    let is_flutter = cx.create_memo({
        let d = d.clone();
        move |_| {
            d.current_tracked().with(|drafts| {
                drafts.get(d.selected.get()).is_some_and(Draft::flutter)
            })
        }
    });
    let is_dart_entry = cx.create_memo({
        let d = d.clone();
        move |_| {
            d.current_tracked().with(|drafts| {
                drafts
                    .get(d.selected.get())
                    .is_some_and(|draft| draft.dart_entry)
            })
        }
    });
    let has_selected = cx.create_memo({
        let d = d.clone();
        move |_| !d.current_tracked().with(Vec::is_empty)
    });
    let sidebar = configuration_sidebar(d.clone(), has_selected, cx);
    let name_input = d.name_input;
    let task_program_focus = cx.create_rw_signal(None::<floem::ViewId>);
    let entry_focus = cx.create_rw_signal(None::<floem::ViewId>);
    let cwd_focus = cx.create_rw_signal(None::<floem::ViewId>);
    let form_keyboard = keyboard.clone();
    let form_row = move |title: &'static str,
                         field: RwSignal<String>,
                         help: &'static str| {
        let input = form_keyboard.input(field, config);
        match title {
            "Name:" => name_input.set(Some(input.id())),
            "Dart entrypoint:" => entry_focus.set(Some(input.id())),
            "Working directory:" => cwd_focus.set(Some(input.id())),
            "Program:" => task_program_focus.set(Some(input.id())),
            _ => {}
        }
        stack((
                stack((
                    label(move || {
                        if title == "Dart entrypoint:" && !is_flutter.get() {
                            if is_dart_entry.get() { "Dart file:" } else { "Dart SDK executable:" }
                        } else if title == "Additional run args:" && is_dart_entry.get() { "Program arguments:" } else { title }
                    })
                    .style(move |s| {
                        s.width(if title == "Name:" { 64.0 } else { 154.0 })
                            .flex_shrink(0.0_f32)
                    }),
                    input.style(|s| s.min_width(0.0).flex_grow(1.0_f32)),
                ))
                .style(|s| s.items_center().width_full()),
                label(move || {
                    if !is_flutter.get() && title == "Dart entrypoint:" {
                        if is_dart_entry.get() { "The Dart file to run. The configured Dart SDK remains the executable." } else { "Dart SDK executable or its full path." }
                    } else if !is_flutter.get() && title == "Additional run args:" {
                        if is_dart_entry.get() { "Arguments passed to the Dart application." } else { "Arguments for Dart, for example: run bin/main.dart." }
                    } else {
                        help
                    }
                })
                .style(move |s| {
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
    let task_count = cx.create_memo({
        let d = d.clone();
        move |_| {
            d.current_tracked().with(|drafts| {
                drafts
                    .get(d.selected.get())
                    .map(|draft| draft.tasks.len())
                    .unwrap_or(0)
            })
        }
    });
    let editing_task = cx.create_memo(move |_| task_count.get() > 0);
    let selected_task = d.task_selected;
    let tasks = task_list(d.clone());
    let add_task = d.clone();
    let remove_task = d.clone();
    let up_task = d.clone();
    let down_task = d.clone();
    let apply = d.clone();
    let ok = d.clone();
    let run = d.clone();
    let attach = d.clone();
    let dirty = d.clone();
    let activate = d.activate;
    let focus = d.focus;
    let project = d.project;
    let error = d.error;
    let checkbox_keyboard = keyboard.clone();
    let checkbox = move |value: RwSignal<bool>, text: &'static str| {
        checkbox_keyboard.control(
            crate::widgets::labeled_checkbox(
                move || value.get(),
                move || text,
                move |checked| value.set(checked),
                config,
            )
            .style(|s| s.padding_vert(4.0).flex_shrink(0.0_f32)),
        )
    };
    let button_keyboard = keyboard.clone();
    let button_draft = d.clone();
    let button = move |text: &'static str| {
        let draft = button_draft.clone();
        button_keyboard.control_if(
            crate::widgets::button(
                move || text,
                if text == "OK" {
                    crate::widgets::ButtonKind::Primary
                } else {
                    crate::widgets::ButtonKind::Outlined
                },
                config,
            )
            .style(|s| s.min_width(72.0).height(24.0).padding_horiz(14.0)),
            move || match text {
                "Apply" => draft.dirty(),
                "▷ Run" | "▾" => {
                    has_selected.get_untracked() && !templates_mode.get_untracked()
                }
                _ => true,
            },
        )
    };
    let launch_options = keyboard.menu_if(
        crate::widgets::button(|| "▾", crate::widgets::ButtonKind::Outlined, config)
            .style(|s| s.size(24.0, 24.0))
            .disabled(move || !has_selected.get() || templates_mode.get()),
        move || has_selected.get_untracked() && !templates_mode.get_untracked(),
        move || {
            let attach = attach.clone();
            let debug = attach.clone();
            vec![
                ModalChoice::new("Debug", move || {
                    debug.launch(RunDebugMode::Debug, false)
                }),
                ModalChoice::new("Attach to Flutter App", move || {
                    attach.launch(RunDebugMode::Run, true)
                })
                .enabled(is_flutter.get_untracked()),
            ]
        },
    );
    let browse_root = d.window.workspace.path.clone().unwrap_or_default();
    let cwd = d.fields[8];
    let entry_root = browse_root.clone();
    let entry_browser = keyboard.clone();
    let browse_entry = keyboard.icon(
        || SparrowIcons::DIRECTORY,
        move || {
            let dart_file =
                is_flutter.get_untracked() || is_dart_entry.get_untracked();
            let directory = run_config::launch_directory(
                &entry_root,
                Some(&cwd.get_untracked()),
            );
            let mut options = FileDialogOptions::new()
                .title(if dart_file {
                    "Choose Dart entrypoint"
                } else {
                    "Choose Dart SDK executable"
                })
                .force_starting_directory(&directory);
            if dart_file {
                options = options.allowed_types(vec![FileSpec {
                    name: "Dart source",
                    extensions: &["dart"],
                }]);
            }
            entry_browser.open_file(options, config, move |file| {
                if !visible.get_untracked() {
                    return;
                }
                if let Some(path) = file.and_then(|file| file.path.first().cloned())
                {
                    let path = if dart_file {
                        path.strip_prefix(&directory).unwrap_or(&path)
                    } else {
                        &path
                    };
                    entry.set(path.to_string_lossy().into_owned());
                }
                if let Some(id) = entry_focus.get_untracked() {
                    id.request_focus();
                }
            });
        },
        || false,
        || false,
        || "Browse Dart entrypoint",
        config,
    );
    let directory_browser = keyboard.clone();
    let browse_cwd = keyboard.icon(
        || SparrowIcons::DIRECTORY,
        move || {
            directory_browser.open_file(
                FileDialogOptions::new()
                    .title("Choose working directory")
                    .select_directories()
                    .force_starting_directory(run_config::launch_directory(
                        &browse_root,
                        Some(&cwd.get_untracked()),
                    )),
                config,
                move |file| {
                    if !visible.get_untracked() {
                        return;
                    }
                    if let Some(path) =
                        file.and_then(|file| file.path.first().cloned())
                    {
                        cwd.set(path.to_string_lossy().into_owned());
                    }
                    if let Some(id) = cwd_focus.get_untracked() {
                        id.request_focus();
                    }
                },
            );
        },
        || false,
        || false,
        || "Browse working directory",
        config,
    );
    let dialog = container(container(stack((
        stack((empty().style(|s|s.width(26.0)),label(||"Run/Debug Configurations").style(|s|s.flex_grow(1.0_f32).items_center().justify_center().font_bold()),clickable_icon(||SparrowIcons::CLOSE,move ||visible.set(false),||false,||false,||"Close",config)))
            .style(move |s|s.height(36.0).width_full().items_center().flex_shrink(0.0_f32).background(floem::peniko::Color::from_rgb8(40,40,40))),
        stack((sidebar,label(||"Add a Dart or Flutter configuration to get started.").style(move |s|s.flex_grow(1.0_f32).items_center().justify_center().apply_if(has_selected.get(),|s|s.hide())),sparrow_scroll(stack((
            stack((form_row("Name:",d.fields[0],"").style(move |s|s.flex_grow(1.0_f32).flex_basis(0.0).min_width(0.0).apply_if(templates_mode.get(),|s|s.hide())),label(||"Defaults for new configurations").style(move |s|s.flex_grow(1.0_f32).font_bold().apply_if(!templates_mode.get(),|s|s.hide())),checkbox(project,"Store as project file"))).style(|s|s.items_center().gap(16.0).margin_bottom(16.0)),
            stack((form_row("Dart entrypoint:",entry,"The entry-point for the application (e.g. lib/main.dart).").style(|s|s.flex_grow(1.0_f32).flex_basis(0.0).min_width(0.0)),browse_entry)).style(|s|s.items_start()),
            stack((form_row("Working directory:", d.fields[8], "Leave empty to use the project directory. ${workspace} is supported.").style(|s|s.flex_grow(1.0_f32).flex_basis(0.0).min_width(0.0)), browse_cwd)).style(|s|s.items_start()),
            form_row("Additional run args:",d.fields[2],"Additional arguments to 'flutter run'."),
            form_row("Additional attach args:",d.fields[3],"Additional arguments to 'flutter attach'.").style(move |s|s.apply_if(!is_flutter.get(),|s|s.hide())),
            form_row("Build flavor:",d.fields[4],"Optional Gradle product flavor or Xcode custom scheme.").style(move |s|s.apply_if(!is_flutter.get(),|s|s.hide())),
            form_row("Environment variables:",d.fields[5],"Additional environment variables. Format: KEY=value; KEY2=value."),
            label(||"⌄  Before launch").style(|s|s.margin_top(6.0).margin_bottom(10.0)),
            stack((
                stack((
                    keyboard.icon(||SparrowIcons::ADD,move ||{add_task.edit_task(0);floem::action::exec_after(std::time::Duration::from_millis(30),move |_|{if visible.get_untracked(){if let Some(id)=task_program_focus.get_untracked(){id.request_focus();}}});},||false,||false,||"Add before-launch task",config),
                    keyboard.menu(label(||"Task type ▾"),{let presets=d.clone();move ||{
                        let browser=if cfg!(target_os="macos"){("open", "https://localhost:7357")}else if cfg!(windows){("rundll32", "url.dll,FileProtocolHandler https://localhost:7357")}else{("xdg-open", "http://localhost:7357")};
                        let mut choices=[("Launch Web Browser",browser.0,browser.1),("External Tool","",""),("Run Gradle Task",if cfg!(windows){"android\\gradlew.bat"}else{"./android/gradlew"},"assembleDebug"),("Build APK Artifact","flutter","build apk"),("Dart Code Generation","dart","run build_runner build --delete-conflicting-outputs")].into_iter().map(|(title,program,args)|{let d=presets.clone();ModalChoice::new(title,move ||{d.edit_task(0);d.fields[6].set(program.into());d.fields[7].set(args.into());d.capture();})}).collect::<Vec<_>>();
                        presets.capture();
                        for draft in presets.drafts.get_untracked(){if let Ok(mut config)=draft.build(){if let Some(root)=presets.window.workspace.path.as_deref(){config.cwd=Some(run_config::launch_directory(root,config.cwd.as_deref()).display().to_string());}let name=config.name.clone();let args=run_config::join_args(&["--run-config-task".into(),serde_json::to_string(&config).unwrap()]);let d=presets.clone();choices.push(ModalChoice::new(format!("Run configuration: {name}"),move ||{d.edit_task(0);d.fields[6].set("${sparrow}".into());d.fields[7].set(args.clone());d.capture();}));}}
                        choices
                    }}),
                    keyboard.icon(||SparrowIcons::REMOVE,move ||remove_task.edit_task(1),||false,move ||!editing_task.get(),||"Remove selected task",config),
                    keyboard.icon(||SparrowIcons::FILE_PICKER_UP,move ||up_task.edit_task(-1),||false,move ||selected_task.get()==0 || !editing_task.get(),||"Move task up",config),
                    keyboard.icon(||SparrowIcons::FOLD_DOWN,move ||down_task.edit_task(2),||false,move ||selected_task.get()+1>=task_count.get(),||"Move task down",config),
                )).style(move |s|s.height(26.0).width_full().items_center().border_bottom(1.0).border_color(config.get().color(SparrowColor::SPARROW_BORDER))),
                label(||"There are no tasks to run before launch").style(move |s|s.width_full().flex_grow(1.0_f32).items_center().justify_center().color(config.get().color(SparrowColor::EDITOR_DIM)).apply_if(editing_task.get(),|s|s.hide())),
                sparrow_scroll(tasks).style(move |s|s.height(72.0).width_full().apply_if(!editing_task.get(),|s|s.hide())),
                stack((form_row("Program:",before,""),form_row("Arguments:",before_args,""))).style(move |s|s.flex_col().padding(8.0).width_full().apply_if(!editing_task.get(),|s|s.hide())),
            )).style(move |s|s.flex_col().min_height(134.0).margin_left(24.0).border(1.0).border_color(config.get().color(SparrowColor::SPARROW_BORDER))),
            stack((checkbox(activate,"Activate tool window"),checkbox(focus,"Focus tool window"))).style(|s|s.gap(12.0).margin_top(4.0)),
            label(move ||error.get()).style(move |s|s.color(config.get().color(SparrowColor::SPARROW_ERROR)).selectable(true)),
        )).style(|s|s.flex_col().padding(16.0).width_full())).style(move |s|s.flex_grow(1.0_f32).flex_basis(0.0).min_width(0.0).height_full().apply_if(!has_selected.get(),|s|s.hide()))))
            .style(|s|s.width_full().flex_grow(1.0_f32).flex_basis(0.0).min_height(0.0)),
        stack((empty().style(|s|s.flex_grow(1.0_f32)),stack((button("▷ Run").disabled(move || !has_selected.get() || templates_mode.get()).on_click_stop(move |_|run.launch(RunDebugMode::Run, false)),launch_options)).style(|s|s.gap(0.0)),button("OK").on_click_stop(move |_|{if ok.apply(){visible.set(false);}}),button("Cancel").on_click_stop(move |_|visible.set(false)),button("Apply").on_click_stop(move |_|{apply.apply();}).disabled(move ||!dirty.dirty())))
            .style(move |s|s.height(46.0).width_full().padding_horiz(16.0).gap(12.0).items_center().flex_shrink(0.0_f32).border_top(1.0).border_color(config.get().color(SparrowColor::SPARROW_BORDER))),
    )).style(|s|s.flex_col().size_full())).style(move |s|s.font_family(config.get().ui.font_family.clone()).font_size(13.0).width(916.0).height(732.0).max_width_pct(96.0).max_height_pct(92.0).flex_col().color(config.get().color(SparrowColor::EDITOR_FOREGROUND)).background(config.get().color(SparrowColor::PANEL_BACKGROUND)).border_radius(8.0)))
    .on_event_stop(EventListener::PointerDown,|_|{})
    .on_event(EventListener::KeyDown,move |event|{if let Event::KeyDown(key)=event{if key.key.logical_key==Key::Named(NamedKey::Escape){visible.set(false);return floem::event::EventPropagation::Stop;}}floem::event::EventPropagation::Continue})
    .style(move |s|s.absolute().size_full().padding_bottom(100.0).items_center().justify_center().z_index(200).background(floem::peniko::Color::from_rgba8(0,0,0,70)).apply_if(!visible.get(),|s|s.hide()));
    let dialog = keyboard.bind_root(dialog, visible, config);
    let id = dialog.id();
    cx.create_effect(move |_| {
        if visible.get() {
            // Focus only after the hidden dialog has completed its style/layout pass.
            floem::action::exec_after(
                std::time::Duration::from_millis(30),
                move |_| {
                    if visible.get_untracked() {
                        if has_selected.get_untracked() {
                            name_input.get_untracked().unwrap_or(id).request_focus();
                        } else {
                            id.request_focus();
                        }
                    }
                },
            );
        }
    });
    dialog
}
