//! Dependency-aware, extensible Dart and Flutter file templates.
use crate::{config::color::SparrowColor, window_tab::WindowTabData};
use floem::{
    View,
    event::{Event, EventListener, EventPropagation},
    keyboard::{Key, NamedKey},
    reactive::{RwSignal, Scope, SignalGet, SignalUpdate},
    views::{Decorators, container, label, scroll, stack},
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
}
impl State {
    pub fn new(cx: Scope) -> Self {
        Self {
            active: cx.create_rw_signal(false),
            base: cx.create_rw_signal(PathBuf::new()),
            template: cx.create_rw_signal(None),
            name: cx.create_rw_signal(String::new()),
            error: cx.create_rw_signal(String::new()),
        }
    }
    pub fn open(&self, base: PathBuf, template: Template) {
        self.base.set(base);
        self.template.set(Some(template));
        self.name.set(String::new());
        self.error.set(String::new());
        self.active.set(true);
    }
    fn create(&self, window: &WindowTabData) {
        let Some(template) = self.template.get_untracked() else {
            return;
        };
        let Some(root) = window.workspace.path.as_deref() else {
            return;
        };
        if let Some(package) = missing_dependency(root, &template) {
            self.error.set(format!("Requires {package}. Run flutter pub add {package}, then create again."));
            return;
        }
        let result = render(&template, &self.name.get_untracked()).and_then(
            |(file, body)| {
                let base = self
                    .base
                    .get_untracked()
                    .canonicalize()
                    .map_err(|e| e.to_string())?;
                if !base.starts_with(root.canonicalize().map_err(|e| e.to_string())?)
                {
                    return Err("Choose a directory inside the project.".into());
                }
                let path = base.join(file);
                use std::io::Write;
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                    .map_err(|e| e.to_string())?;
                if let Err(e) = file.write_all(body.as_bytes()) {
                    drop(file);
                    let _ = std::fs::remove_file(&path);
                    return Err(e.to_string());
                }
                Ok(path)
            },
        );
        match result {
            Ok(path) => {
                self.active.set(false);
                window.file_explorer.reload();
                window
                    .common
                    .internal_command
                    .send(crate::command::InternalCommand::OpenFile { path });
            }
            Err(e) => self.error.set(e),
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
    let focus = crate::modal_focus::ModalFocus::default();
    let submit = state.clone();
    let w = window.clone();
    focus.set_submit(move || submit.create(&w));
    let input = focus.input(name).style(move |s| {
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
    let dependency = state.clone();
    let dependency_window = window.clone();
    let create = state.clone();
    let w = window.clone();
    let cancel = state.clone();
    let close = state.clone();
    let body = stack((
        label(move || {
            format!(
                "New {}",
                template.get().map(|t| t.title).unwrap_or_default()
            )
        })
        .style(|s| s.font_bold()),
        label(move || base.get().display().to_string()).style(|s| s.width_full()),
        label(|| "Class name"),
        input,
        label(move || {
            template
                .get()
                .and_then(|t| t.package)
                .map(|p| format!("Package: {p}"))
                .unwrap_or_else(|| "Built-in Flutter / Dart APIs".into())
        }),
        scroll(label(move || {
            template
                .get()
                .and_then(|t| render(&t, &name.get()).ok())
                .map(|(p, b)| format!("{p}\n\n{b}"))
                .unwrap_or_default()
        }))
        .style(|s| s.width_full().max_height(250.0)),
        focus.control(label(||"Add required package…").on_click_stop(move |_|{
            let Some(t)=dependency.template.get_untracked() else{return;};let Some(package)=t.package else{return;};let Some(root)=dependency_window.workspace.path.as_deref() else{return;};
            if let Some(flutter)=crate::dart::sdk::find_flutter_binary(Some(root)){
                dependency_window.common.internal_command.send(crate::command::InternalCommand::NewTerminal{profile:Some(sparrow_rpc::terminal::TerminalProfile{name:format!("Add {package}"),command:Some(flutter.display().to_string()),arguments:Some(vec!["pub".into(),"add".into(),package]),workdir:lsp_types::Url::from_directory_path(root).ok(),..Default::default()})});dependency.error.set("Package installation started in Terminal. Create the file after it finishes.".into());
            }else{dependency.error.set("Configure a Flutter SDK before adding packages.".into());}
        }).style(move |s|s.apply_if(template.get().and_then(|t|t.package).is_none(),|s|s.hide()))),
        label(move || error.get()).style(|s| s.width_full()),
        stack((
            focus.control(
                label(|| "Create").on_click_stop(move |_| create.create(&w)),
            ),
            focus.control(
                label(|| "Cancel").on_click_stop(move |_| cancel.active.set(false)),
            ),
        ))
        .style(|s| s.gap(20.0)),
    ))
    .style(move |s| {
        s.width(540.0)
            .max_width_pct(95.0)
            .flex_col()
            .gap(12.0)
            .padding(18.0)
            .border_radius(6.0)
            .border(1.0).border_color(config.get().color(SparrowColor::SPARROW_BORDER))
            .box_shadow_blur(16.0).box_shadow_color(config.get().color(SparrowColor::SPARROW_DROPDOWN_SHADOW))
            .background(config.get().color(SparrowColor::PANEL_BACKGROUND))
            .color(config.get().color(SparrowColor::EDITOR_FOREGROUND))
    });
    let dialog=container(body).style(move |s|s.absolute().size_full().items_center().justify_center().z_index(100).apply_if(!active.get(),|s|s.hide())).on_event_stop(EventListener::PointerDown, |_|{}).on_event(EventListener::KeyDown,move |e|{if matches!(e,Event::KeyDown(k) if k.key.logical_key==Key::Named(NamedKey::Escape)){close.active.set(false);EventPropagation::Stop}else{EventPropagation::Continue}});
    focus.bind_root(dialog, active, config)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_names_are_valid_and_substitutions_are_complete() {
        for t in catalog(Path::new("/nonexistent")) {
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
            if missing_dependency(&root, template).is_some() {
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
