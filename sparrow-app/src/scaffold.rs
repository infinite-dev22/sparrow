//! Dependency-aware scaffolds, previewed and applied as one reversible plan.
pub mod engine;
mod feature;
mod registration;
#[cfg(test)]
mod tests;
use crate::{config::color::SparrowColor, window_tab::WindowTabData};
use floem::{
    View,
    event::{Event, EventListener, EventPropagation},
    keyboard::{Key, NamedKey},
    reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith},
    views::{Decorators, container, dyn_stack, label, scroll, stack},
};
use std::{
    path::{Path, PathBuf},
    rc::Rc,
};
#[derive(Clone, Debug)]
pub struct Template {
    pub title: String,
    pub suffix: String,
    pub package: Option<String>,
    pub body: String,
    pub pack: Option<engine::Pack>,
    pub manifest: Option<PathBuf>,
}
fn template(
    title: &str,
    suffix: &str,
    package: Option<&str>,
    body: &str,
) -> Template {
    Template {
        title: title.into(),
        suffix: suffix.into(),
        package: package.map(Into::into),
        body: body.into(),
        pack: None,
        manifest: None,
    }
}
pub fn catalog(root: &Path) -> Vec<Template> {
    let mut templates = vec![
        template(
            "Dart Class",
            "",
            None,
            "class {{name}} {\n  const {{name}}();\n}\n",
        ),
        template(
            "Stateless Widget",
            "",
            None,
            "import 'package:flutter/material.dart';\n\nclass {{name}} extends StatelessWidget {\n  const {{name}}({super.key});\n  @override\n  Widget build(BuildContext context) => const SizedBox.shrink();\n}\n",
        ),
        template(
            "Stateful Widget / setState",
            "",
            None,
            "import 'package:flutter/material.dart';\n\nclass {{name}} extends StatefulWidget {\n  const {{name}}({super.key});\n  @override\n  State<{{name}}> createState() => _{{name}}State();\n}\nclass _{{name}}State extends State<{{name}}> {\n  int count = 0;\n  @override\n  Widget build(BuildContext context) => TextButton(onPressed: () => setState(() => count++), child: Text('$count'));\n}\n",
        ),
        template(
            "ChangeNotifier",
            "_notifier",
            None,
            "import 'package:flutter/foundation.dart';\n\nclass {{name}} extends ChangeNotifier {\n  int _count = 0;\n  int get count => _count;\n  void increment() { _count++; notifyListeners(); }\n}\n",
        ),
        template(
            "ValueNotifier",
            "_notifier",
            None,
            "import 'package:flutter/foundation.dart';\n\nclass {{name}} extends ValueNotifier<int> {\n  {{name}}() : super(0);\n  void increment() => value++;\n}\n",
        ),
        template(
            "InheritedWidget",
            "_scope",
            None,
            "import 'package:flutter/widgets.dart';\n\nclass {{name}} extends InheritedWidget {\n  const {{name}}({super.key, required super.child, required this.value});\n  final int value;\n  static {{name}} of(BuildContext context) {\n    final scope = context.dependOnInheritedWidgetOfExactType<{{name}}>();\n    assert(scope != null, 'No {{name}} found');\n    return scope!;\n  }\n  @override\n  bool updateShouldNotify({{name}} oldWidget) => value != oldWidget.value;\n}\n",
        ),
        template(
            "Streams / StreamBuilder",
            "_controller",
            None,
            "import 'dart:async';\n\nclass {{name}} {\n  final _controller = StreamController<int>.broadcast();\n  int _count = 0;\n  Stream<int> get stream => _controller.stream;\n  void increment() => _controller.add(++_count);\n  Future<void> dispose() => _controller.close();\n}\n",
        ),
        template(
            "BLoC",
            "_bloc",
            Some("bloc"),
            "import 'package:bloc/bloc.dart';\n\nsealed class {{name}}Event {}\nfinal class {{name}}Increment extends {{name}}Event {}\n\nclass {{name}} extends Bloc<{{name}}Event, int> {\n  {{name}}() : super(0) {\n    on<{{name}}Increment>((event, emit) => emit(state + 1));\n  }\n}\n",
        ),
        template(
            "Cubit",
            "_cubit",
            Some("bloc"),
            "import 'package:bloc/bloc.dart';\n\nclass {{name}} extends Cubit<int> {\n  {{name}}() : super(0);\n  void increment() => emit(state + 1);\n}\n",
        ),
        template(
            "Riverpod Notifier",
            "_notifier",
            Some("flutter_riverpod"),
            "import 'package:flutter_riverpod/flutter_riverpod.dart';\n\nclass {{name}} extends Notifier<int> {\n  @override\n  int build() => 0;\n  void increment() => state++;\n}\nfinal {{variable}}Provider = NotifierProvider<{{name}}, int>({{name}}.new);\n",
        ),
        template(
            "Riverpod AsyncNotifier",
            "_notifier",
            Some("flutter_riverpod"),
            "import 'package:flutter_riverpod/flutter_riverpod.dart';\n\nclass {{name}} extends AsyncNotifier<int> {\n  @override\n  Future<int> build() async => 0;\n  Future<void> refresh() async {\n    state = const AsyncLoading();\n    state = await AsyncValue.guard(build);\n  }\n}\nfinal {{variable}}Provider = AsyncNotifierProvider<{{name}}, int>({{name}}.new);\n",
        ),
        template(
            "Provider",
            "_provider",
            Some("provider"),
            "import 'package:flutter/foundation.dart';\nimport 'package:provider/provider.dart';\n\nclass {{name}} extends ChangeNotifier {\n  int count = 0;\n  void increment() { count++; notifyListeners(); }\n}\nfinal {{variable}}Provider = ChangeNotifierProvider<{{name}}>(create: (_) => {{name}}());\n",
        ),
        template(
            "GetX Controller",
            "_controller",
            Some("get"),
            "import 'package:get/get.dart';\n\nclass {{name}} extends GetxController {\n  final count = 0.obs;\n  void increment() => count.value++;\n}\n",
        ),
        template(
            "MobX Store (no code generation)",
            "_store",
            Some("mobx"),
            "import 'package:mobx/mobx.dart';\n\nclass {{name}} {\n  final count = Observable(0);\n  void increment() => runInAction(() => count.value++);\n}\n",
        ),
        template(
            "Redux Store",
            "_store",
            Some("redux"),
            "import 'package:redux/redux.dart';\n\nenum {{name}}Action { increment }\nint {{variable}}Reducer(int state, dynamic action) => action == {{name}}Action.increment ? state + 1 : state;\nStore<int> create{{name}}Store() => Store<int>({{variable}}Reducer, initialState: 0);\n",
        ),
        template(
            "Signals",
            "_model",
            Some("signals"),
            "import 'package:signals/signals.dart';\n\nclass {{name}} {\n  final count = signal(0);\n  void increment() => count.value++;\n  void dispose() => count.dispose();\n}\n",
        ),
        template(
            "RxDart",
            "_controller",
            Some("rxdart"),
            "import 'package:rxdart/rxdart.dart';\n\nclass {{name}} {\n  final count = BehaviorSubject<int>.seeded(0);\n  void increment() => count.add(count.value + 1);\n  Future<void> dispose() => count.close();\n}\n",
        ),
        template(
            "Flutter Hooks",
            "",
            Some("flutter_hooks"),
            "import 'package:flutter/material.dart';\nimport 'package:flutter_hooks/flutter_hooks.dart';\n\nclass {{name}} extends HookWidget {\n  const {{name}}({super.key});\n  @override\n  Widget build(BuildContext context) {\n    final count = useState(0);\n    return TextButton(onPressed: () => count.value++, child: Text('${count.value}'));\n  }\n}\n",
        ),
        template(
            "Scoped Model",
            "_model",
            Some("scoped_model"),
            "import 'package:scoped_model/scoped_model.dart';\n\nclass {{name}} extends Model {\n  int count = 0;\n  void increment() { count++; notifyListeners(); }\n}\n",
        ),
        template(
            "Stacked ViewModel",
            "_view_model",
            Some("stacked"),
            "import 'package:stacked/stacked.dart';\n\nclass {{name}} extends BaseViewModel {\n  int count = 0;\n  void increment() { count++; notifyListeners(); }\n}\n",
        ),
        template(
            "GetIt Service",
            "_service",
            Some("get_it"),
            "import 'package:get_it/get_it.dart';\n\nclass {{name}} {\n  int count = 0;\n  void increment() => count++;\n}\nvoid register{{name}}() => GetIt.I.registerLazySingleton<{{name}}>({{name}}.new);\n",
        ),
    ];
    if let Ok(entries) = std::fs::read_dir(root.join(".sparrow/templates")) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file()
                && !path.is_symlink()
                && path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| s.ends_with(".dart.tmpl"))
            {
                if let Ok(body) = std::fs::read_to_string(&path) {
                    templates.push(template(
                        &format!(
                            "Custom: {}",
                            path.file_name()
                                .unwrap()
                                .to_string_lossy()
                                .trim_end_matches(".dart.tmpl")
                        ),
                        "",
                        None,
                        &body,
                    ));
                }
            }
        }
    }
    let pack = feature::pack();
    let mut feature = template(&pack.title, "", None, "");
    feature.pack = Some(pack);
    templates.insert(3, feature);
    // Repo-local packs are declarative only. Invalid manifests remain visible
    // and report their validation errors when previewed.
    if let Ok(entries) = std::fs::read_dir(root.join(".sparrow/scaffolds")) {
        let mut paths = entries
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                (path.is_dir()
                    && !path.is_symlink()
                    && path.join("pack.yaml").is_file())
                .then(|| path.join("pack.yaml"))
            })
            .collect::<Vec<_>>();
        paths.sort();
        for path in paths {
            let title =
                engine::load_pack(&path)
                    .map(|p| p.title)
                    .unwrap_or_else(|_| {
                        format!(
                            "Pack: {}",
                            path.parent()
                                .unwrap()
                                .file_name()
                                .unwrap()
                                .to_string_lossy()
                        )
                    });
            let mut custom = template(&title, "", None, "");
            custom.manifest = Some(path);
            templates.push(custom);
        }
    }
    templates
}
fn render(template: &Template, name: &str) -> Result<(String, String), String> {
    if name.is_empty()
        || !name.as_bytes()[0].is_ascii_uppercase()
        || !name.chars().all(|c| c.is_ascii_alphanumeric())
    {
        return Err(
            "Use a PascalCase Dart class name, for example CounterCubit.".into(),
        );
    }
    let mut file = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            file.push('_');
        }
        file.push(c.to_ascii_lowercase());
    }
    if !file.ends_with(&template.suffix) {
        file.push_str(&template.suffix);
    }
    file.push_str(".dart");
    let variable = format!("{}{}", name[..1].to_ascii_lowercase(), &name[1..]);
    Ok((
        file,
        template
            .body
            .replace("{{name}}", name)
            .replace("{{variable}}", &variable),
    ))
}
fn missing_dependency(root: &Path, template: &Template) -> Option<String> {
    let package = template.package.as_ref()?;
    let text =
        std::fs::read_to_string(root.join("pubspec.yaml")).unwrap_or_default();
    let declared = serde_yaml::from_str::<serde_yaml::Value>(&text)
        .ok()
        .is_some_and(|doc| {
            ["dependencies", "dev_dependencies"].iter().any(|key| {
                doc.get(*key)
                    .and_then(|v| v.get(package.as_str()))
                    .is_some()
            })
        });
    (!declared).then(|| package.clone())
}
#[derive(Clone)]
pub struct State {
    pub active: RwSignal<bool>,
    base: RwSignal<PathBuf>,
    template: RwSignal<Option<Template>>,
    name: RwSignal<String>,
    error: RwSignal<String>,
    plan: RwSignal<Option<engine::Plan>>,
    planned_name: RwSignal<String>,
    selected: RwSignal<usize>,
    history: RwSignal<Vec<engine::Applied>>,
    undoing: RwSignal<bool>,
    busy: RwSignal<bool>,
    generation: RwSignal<u64>,
    auto_register: RwSignal<bool>,
}
impl State {
    pub fn new(cx: Scope) -> Self {
        Self {
            active: cx.create_rw_signal(false),
            base: cx.create_rw_signal(PathBuf::new()),
            template: cx.create_rw_signal(None),
            name: cx.create_rw_signal(String::new()),
            error: cx.create_rw_signal(String::new()),
            plan: cx.create_rw_signal(None),
            planned_name: cx.create_rw_signal(String::new()),
            selected: cx.create_rw_signal(0),
            history: cx.create_rw_signal(Vec::new()),
            undoing: cx.create_rw_signal(false),
            busy: cx.create_rw_signal(false),
            generation: cx.create_rw_signal(0),
            auto_register: cx.create_rw_signal(true),
        }
    }
    pub fn open(&self, base: PathBuf, template: Template) {
        self.generation.update(|g| *g += 1);
        self.busy.set(false);
        self.base.set(base);
        self.template.set(Some(template));
        self.name.set(String::new());
        self.error.set(String::new());
        self.plan.set(None);
        self.selected.set(0);
        self.undoing.set(false);
        self.active.set(true);
    }
    pub fn open_feature(&self, window: &WindowTabData) {
        if let Some(root) = window.workspace.path.clone() {
            let base = window
                .main_split
                .active_editor
                .get_untracked()
                .and_then(|e| {
                    e.doc().content.with_untracked(|c| {
                        c.path().and_then(|p| p.parent()).map(Path::to_path_buf)
                    })
                })
                .unwrap_or(root);
            let pack = feature::pack();
            let mut t = template(&pack.title, "", None, "");
            t.pack = Some(pack);
            self.open(base, t);
        }
    }
    pub fn open_undo(&self) {
        self.generation.update(|g| *g += 1);
        self.busy.set(false);
        self.undoing.set(true);
        self.selected.set(0);
        self.error.set(String::new());
        self.plan
            .set(self.history.get_untracked().last().map(|a| a.undo_plan()));
        self.active.set(true);
        if self.plan.get_untracked().is_none() {
            self.error
                .set("No scaffold to undo in this window session.".into());
        }
    }
    fn preview(&self, window: &WindowTabData) {
        if self.busy.get_untracked() {
            return;
        }
        self.error.set(String::new());
        self.plan.set(None);
        self.selected.set(0);
        if self.undoing.get_untracked() {
            self.open_undo();
            return;
        }
        let mut registration_context = None;
        let result = (|| {
            let t = self.template.get_untracked().ok_or("Choose a scaffold")?;
            let root = window.workspace.path.as_deref().ok_or("Open a project")?;
            let ctx = engine::context(root, &self.base.get_untracked())?;
            let name = self.name.get_untracked();
            let pack = if let Some(ref path) = t.manifest {
                Some(engine::load_pack(&path)?)
            } else {
                t.pack.clone()
            };
            if let Some(pack) = pack {
                let plan = engine::plan_pack(&ctx, &pack, &name)?;
                if pack.id == "sparrow.feature.mvvm"
                    && self.auto_register.get_untracked()
                {
                    if window.main_split.docs.with_untracked(|docs| {
                        docs.iter().any(|(path, doc)| {
                            path.starts_with(&ctx.root)
                                && crate::dart::is_dart_path(path)
                                && !doc.is_pristine()
                        })
                    }) {
                        return Err("Save modified Dart files before analyzer-backed registration.".into());
                    }
                    registration_context = Some(ctx);
                }
                return Ok(plan);
            }
            if let Some(package) = missing_dependency(&ctx.root, &t) {
                return Err(format!(
                    "Requires {package}. Add the required package first, then preview again."
                ));
            }
            let (file, body) = render(&t, &name)?;
            engine::single_plan(
                &ctx,
                &self.base.get_untracked(),
                file,
                body,
                t.title,
            )
        })();
        match result {
            Ok(plan) => {
                if let Some(ctx) = registration_context {
                    self.busy.set(true);
                    self.error.set("Resolving router/provider registrations with the Dart analyzer… First use may download the bundled analyzer's pinned dependencies.".into());
                    self.generation.update(|g| *g += 1);
                    let ticket = self.generation.get_untracked();
                    let state = self.clone();
                    let name = self.name.get_untracked();
                    let planned_name = name.clone();
                    let send = floem::ext_event::create_ext_action(
                        window.scope,
                        move |result: Result<engine::Plan, String>| {
                            if state.generation.get_untracked() != ticket
                                || !state.active.get_untracked()
                            {
                                return;
                            }
                            state.busy.set(false);
                            state.error.set(String::new());
                            match result {
                                Ok(plan) => {
                                    state.planned_name.set(planned_name);
                                    state.plan.set(Some(plan));
                                }
                                Err(error) => state.error.set(error),
                            }
                        },
                    );
                    std::thread::spawn(move || {
                        send(registration::augment(plan, &ctx, &name))
                    });
                } else {
                    self.planned_name.set(self.name.get_untracked());
                    self.plan.set(Some(plan));
                }
            }
            Err(error) => self.error.set(error),
        }
    }
    fn apply(&self, window: &WindowTabData) {
        if self.busy.get_untracked() {
            return;
        }
        let Some(plan) = self.plan.get_untracked() else {
            self.error
                .set("Preview the scaffold before applying.".into());
            return;
        };
        if !self.undoing.get_untracked()
            && self.planned_name.get_untracked() != self.name.get_untracked()
        {
            self.error
                .set("The name changed. Preview again before applying.".into());
            return;
        }
        if plan.changes.is_empty() {
            self.error.set("No changes to apply.".into());
            return;
        }
        if window.main_split.docs.with_untracked(|docs| {
            plan.changes
                .iter()
                .any(|c| docs.get(&c.path).is_some_and(|d| !d.is_pristine()))
        }) {
            self.error.set(
                "Save modified files in this plan before applying or undoing."
                    .into(),
            );
            return;
        }
        let undoing = self.undoing.get_untracked();
        let result = if undoing {
            self.history
                .get_untracked()
                .last()
                .ok_or_else(|| "No scaffold to undo".to_string())
                .and_then(|a| a.undo())
        } else {
            plan.apply()
                .map(|applied| self.history.update(|h| h.push(applied)))
        };
        match result {
            Err(error) => self.error.set(error),
            Ok(()) => {
                if undoing {
                    self.history.update(|h| {
                        h.pop();
                    });
                }
                for change in &plan.changes {
                    if let Some(after) = &change.after {
                        window.main_split.docs.with_untracked(|docs| {
                            if let Some(doc) = docs.get(&change.path) {
                                doc.reload(
                                    lapce_xi_rope::Rope::from(after.clone()),
                                    true,
                                );
                            }
                        });
                    } else {
                        let editors = window
                            .main_split
                            .editors
                            .with_editors_untracked(|items| {
                                items
                                    .values()
                                    .filter(|e| {
                                        e.doc().content.with_untracked(|c| {
                                            c.path() == Some(&change.path)
                                        })
                                    })
                                    .cloned()
                                    .collect::<Vec<_>>()
                            });
                        for editor in editors {
                            if let Some(tab) = editor.editor_tab_id.get_untracked() {
                                window.main_split.editor_tab_child_close(
                                    tab,
                                    crate::editor_tab::EditorTabChild::Editor(
                                        editor.id(),
                                    ),
                                    true,
                                );
                            }
                        }
                        window.main_split.docs.update(|docs| {
                            docs.remove(&change.path);
                        });
                        window.common.dart_lsp.send_close(change.path.clone());
                    }
                }
                window.file_explorer.reload();
                self.plan.set(None);
                self.error.set(format!("{} {} file(s). {}",if undoing{"Restored"}else{"Applied"},plan.changes.len(),if undoing{""}else{"Undo Last Scaffold is available here and in the command palette. Run Pub Get if dependencies changed."}));
            }
        }
    }
}
pub fn view(window: Rc<WindowTabData>) -> impl View {
    let state = window.scaffold.clone();
    let config = window.common.config;
    let active = state.active;
    crate::modal_focus::restore_workbench_on_close(active, window.common.clone());
    let name = state.name;
    let template = state.template;
    let error = state.error;
    let base = state.base;
    let plan = state.plan;
    let selected = state.selected;
    let undoing = state.undoing;
    let busy = state.busy;
    let auto_register = state.auto_register;
    let registration_plan = state.plan;
    let focus = crate::modal_focus::ModalFocus::default();
    let preview = state.clone();
    let w = window.clone();
    focus.set_submit(move || preview.preview(&w));
    let input = focus
        .input(name)
        .disabled(move || undoing.get() || busy.get())
        .style(move |s| {
            s.width_full()
                .height(28.0)
                .padding_horiz(6.0)
                .border(1.0)
                .border_color(config.get().color(SparrowColor::EDITOR_FOCUS))
        });
    let id = input.id();
    floem::reactive::create_effect(move |_| {
        if active.get() {
            floem::action::exec_after(
                std::time::Duration::from_millis(30),
                move |_| id.request_focus(),
            );
        }
    });
    let file_focus = focus.clone();
    let list = scroll(
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
    let diff=scroll(dyn_stack(
        move ||plan.get().map(|p|p.diff(selected.get())).unwrap_or_else(||"Enter a name and select Preview Changes. Every affected file will appear here before anything is written.".into()).lines().enumerate().map(|(i,line)|(i,line.to_string())).collect::<Vec<_>>(),
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
    let dependency = state.clone();
    let dw = window.clone();
    let pub_state = state.clone();
    let pub_window = window.clone();
    let body=stack((
        label(move ||if undoing.get(){"Undo Last Scaffold".into()}else{format!("New {}",template.get().map(|t|t.title).unwrap_or_default())}).style(|s|s.font_bold()),
        label(move ||base.get().display().to_string()).style(|s|s.width_full()),
        input,
        focus.control(stack((crate::settings::checkbox(move ||auto_register.get(),config),label(||"Automatically register router and providers"))).style(|s|s.gap(8.0)).on_click_stop(move |_|{auto_register.update(|value|*value=!*value);registration_plan.set(None);}).disabled(move ||busy.get()).style(move |s|s.apply_if(undoing.get()||!template.get().and_then(|t|t.pack).is_some_and(|p|p.id=="sparrow.feature.mvvm"),|s|s.hide()))),
        scroll(label(move ||plan.get().map(|p|p.notes.join("\n")).unwrap_or_default())).style(|s|s.width_full().max_height(100.0)),
        stack((list,diff)).style(move |s|s.width_full().height(340.0).border(1.0).border_color(config.get().color(SparrowColor::SPARROW_BORDER))),
        label(move ||error.get()).style(|s|s.width_full()),
        stack((
            focus.control(label(||"Preview Changes").on_click_stop(move |_|preview.preview(&w)).disabled(move ||busy.get())),
            focus.control(label(move ||if undoing.get(){"Undo Scaffold"}else{"Apply Changes"}).on_click_stop(move |_|apply.apply(&aw)).disabled(move ||plan.get().is_none()||busy.get())),
            focus.control(label(||"Undo Last Scaffold…").on_click_stop(move |_|undo.open_undo()).disabled(move ||history.get().is_empty())),
            focus.control(label(||"Pub Get").on_click_stop(move |_|{
                let Some(root)=pub_window.workspace.path.as_deref() else{return;};
                match engine::context(root,&pub_state.base.get_untracked()) {
                    Ok(ctx)=> { if let Some(dart)=crate::dart::sdk::find_dart_binary(Some(&ctx.root)) {
                        pub_window.common.internal_command.send(crate::command::InternalCommand::NewTerminal{profile:Some(sparrow_rpc::terminal::TerminalProfile{name:format!("Pub Get · {}",ctx.package),command:Some(dart.display().to_string()),arguments:Some(vec!["pub".into(),"get".into()]),workdir:lsp_types::Url::from_directory_path(&ctx.root).ok(),..Default::default()})});
                    } else {pub_state.error.set("Configure a Dart or Flutter SDK first.".into());}},
                    Err(e)=>pub_state.error.set(e),
                }
            })),
            focus.control(label(||"Close").on_click_stop(move |_|active.set(false))),
        )).style(|s|s.gap(16.0)),
        focus.control(label(||"Add required package…").on_click_stop(move |_|{
            let Some(t)=dependency.template.get_untracked() else{return;};let Some(package)=t.package else{return;};let Some(root)=dw.workspace.path.as_deref() else{return;};
            let ctx=match engine::context(root,&dependency.base.get_untracked()){Ok(ctx)=>ctx,Err(e)=>{dependency.error.set(e);return;}};
            if let Some(flutter)=crate::dart::sdk::find_flutter_binary(Some(&ctx.root)){
                dw.common.internal_command.send(crate::command::InternalCommand::NewTerminal{profile:Some(sparrow_rpc::terminal::TerminalProfile{name:format!("Add {package}"),command:Some(flutter.display().to_string()),arguments:Some(vec!["pub".into(),"add".into(),package]),workdir:lsp_types::Url::from_directory_path(&ctx.root).ok(),..Default::default()})});dependency.error.set("Package installation started in Terminal. Preview after it finishes.".into());
            }else{dependency.error.set("Configure a Flutter SDK before adding packages.".into());}
        }).style(move |s|s.apply_if(template.get().and_then(|t|t.package).is_none()||undoing.get(),|s|s.hide()))),
    )).style(move |s|s.width(1020.0).max_width_pct(95.0).max_height_pct(95.0).flex_col().gap(12.0).padding(18.0).border_radius(6.0).border(1.0).border_color(config.get().color(SparrowColor::SPARROW_BORDER)).box_shadow_blur(16.0).box_shadow_color(config.get().color(SparrowColor::SPARROW_DROPDOWN_SHADOW)).background(config.get().color(SparrowColor::PANEL_BACKGROUND)).color(config.get().color(SparrowColor::EDITOR_FOREGROUND)));
    let dialog=container(body).style(move |s|s.absolute().size_full().items_center().justify_center().z_index(100).apply_if(!active.get(),|s|s.hide())).on_event_stop(EventListener::PointerDown, |_|{}).on_event(EventListener::KeyDown,move |e|{if matches!(e,Event::KeyDown(k) if k.key.logical_key==Key::Named(NamedKey::Escape)){active.set(false);EventPropagation::Stop}else{EventPropagation::Continue}});
    focus.bind_root(dialog, active, config)
}
#[cfg(test)]
mod template_tests {
    use super::*;
    #[test]
    fn catalog_names_are_valid_and_substitutions_are_complete() {
        for t in catalog(Path::new("/nonexistent"))
            .into_iter()
            .filter(|t| t.pack.is_none() && t.manifest.is_none())
        {
            let (file, body) = render(&t, "Counter").unwrap();
            assert!(file.ends_with(".dart"));
            assert!(!body.contains("{{"), "{}", t.title);
            assert!(body.contains("Counter"));
        }
        assert!(
            render(&catalog(Path::new("/nonexistent"))[0], "../../evil").is_err()
        );
    }
    #[test]
    fn dependencies_checked_in_yaml_sections() {
        let d = tempfile::tempdir().unwrap();
        let t = template("Cubit", "", Some("bloc"), "");
        std::fs::write(
            d.path().join("pubspec.yaml"),
            "name: test\ndependencies:\n  bloc: ^9.0.0\n",
        )
        .unwrap();
        assert!(missing_dependency(d.path(), &t).is_none());
        std::fs::write(d.path().join("pubspec.yaml"), "name: bloc\n").unwrap();
        assert_eq!(missing_dependency(d.path(), &t), Some("bloc".into()));
    }
}

#[cfg(test)]
mod sdk_validation {
    use super::*;
    #[test]
    #[ignore = "requires an isolated Flutter project supplied through SPARROW_SCAFFOLD_EXPORT_ROOT"]
    fn export_templates_for_installed_sdk_analysis() {
        let root = PathBuf::from(
            std::env::var_os("SPARROW_SCAFFOLD_EXPORT_ROOT").expect("fixture root"),
        );
        assert!(root.join("pubspec.yaml").is_file());
        let mut count = 0;
        for (i, template) in catalog(&root).iter().enumerate() {
            if template.pack.is_some()
                || template.manifest.is_some()
                || missing_dependency(&root, template).is_some()
            {
                continue;
            }
            let (file, body) = render(template, "Counter").unwrap();
            let dir = root.join("lib/generated").join(format!("t{i}"));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(file), body).unwrap();
            count += 1;
        }
        assert!(count >= 7);
        println!("Exported {count} templates for analysis");
    }
}
