//! Editable run configuration drafts and argument/storage helpers.
use crate::debug::RunDebugConfigs;
use sparrow_rpc::dap_types::{BeforeLaunchTask, RunDebugConfig};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

/// Discover actual entrypoints without assigning Flutter commands to Dart packages.
/// A package with no entrypoint starts with an empty configuration selector.
pub fn discover_defaults(root: &Path) -> Vec<RunDebugConfig> {
    let flutter = super::is_flutter_project(root);
    let candidates: Vec<PathBuf> = if flutter {
        vec![
            root.join("lib/main.dart"),
            root.join("example/lib/main.dart"),
        ]
    } else {
        let package = super::classify_project(root);
        let mut candidates = vec![
            root.join("bin/main.dart"),
            root.join("main.dart"),
            root.join("lib/main.dart"),
        ];
        if let Some(name) = package.name() {
            candidates.insert(0, root.join("bin").join(format!("{name}.dart")));
        }
        candidates
    };
    candidates.into_iter().find(|path| path.is_file()).map(|entry| {
        let cwd = if flutter {
            entry.parent().and_then(Path::parent).unwrap_or(root)
        } else { root };
        let target = entry.strip_prefix(cwd).unwrap_or(&entry).to_string_lossy().replace('\\', "/");
        serde_json::from_value(serde_json::json!({
            "name": entry.file_name().unwrap_or_default().to_string_lossy(), "type": if flutter { "flutter" } else { "dart" },
            "program": if flutter { "flutter" } else { "dart" },
            "args": if flutter { vec!["run", "--target", &target] } else { vec!["run", &target] },
            "cwd": cwd.to_string_lossy()
        })).expect("valid built-in SDK configuration")
    }).into_iter().collect()
}

/// Context menus launch their clicked file, independently of the toolbar selection.
pub fn configuration_for_file(
    path: &Path,
    workspace: Option<&Path>,
) -> Result<RunDebugConfig, String> {
    if !path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("dart"))
    {
        return Err("Only Dart entrypoints can be run or debugged.".into());
    }
    let root = path
        .parent()
        .and_then(|parent| {
            parent
                .ancestors()
                .find(|dir| dir.join("pubspec.yaml").is_file())
        })
        .or(workspace)
        .or(path.parent())
        .ok_or("The file has no parent directory")?;
    let flutter = super::is_flutter_project(root);
    let target = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    serde_json::from_value(serde_json::json!({
        "name":path.file_name().unwrap_or_default().to_string_lossy(),
        "type":if flutter {"flutter"} else {"dart"},
        "program":if flutter {"flutter"} else {"dart"},
        "args":if flutter {vec!["run", "--target", &target]} else {vec!["run", &target]},
        "cwd":root.to_string_lossy()
    })).map_err(|error| error.to_string())
}

pub fn is_flutter_run(config: &RunDebugConfig) -> bool {
    config.config_source != sparrow_rpc::dap_types::ConfigSource::ProjectTask
        && config.debug_command.is_none()
        && is_flutter_target(config)
}

pub fn is_flutter_target(config: &RunDebugConfig) -> bool {
    Path::new(&config.program)
        .file_stem()
        .is_some_and(|stem| stem.eq_ignore_ascii_case("flutter"))
        && config
            .args
            .as_ref()
            .and_then(|args| args.first())
            .is_some_and(|arg| matches!(arg.as_str(), "run" | "attach"))
}

pub fn console_panel(
    mode: crate::debug::RunDebugMode,
) -> crate::panel::kind::PanelKind {
    use crate::{debug::RunDebugMode, panel::kind::PanelKind};
    match mode {
        RunDebugMode::Run => PanelKind::FlutterRun,
        RunDebugMode::Debug => PanelKind::Debug,
    }
}

pub fn launch_directory(root: &Path, configured: Option<&str>) -> PathBuf {
    let Some(configured) = configured else {
        return root.to_path_buf();
    };
    let expanded = configured.replace("${workspace}", &root.to_string_lossy());
    let path = PathBuf::from(expanded);
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}

pub fn new_configuration(root: Option<&Path>) -> RunDebugConfig {
    let flutter = root.is_some_and(super::is_flutter_project);
    serde_json::from_value(serde_json::json!({
        "name": "New configuration", "type": if flutter { "flutter" } else { "dart" },
        "program": if flutter { "flutter" } else { "dart" },
        "args": if flutter { vec!["run", "--target", "lib/main.dart"] } else { vec!["run", "bin/main.dart"] },
        "cwd": root.map(|root| root.to_string_lossy())
    })).expect("valid SDK configuration template")
}

/// Bind a live device only to Flutter run/attach and preserve explicit choices.
pub fn with_selected_device(
    mut config: RunDebugConfig,
    device: Option<&str>,
) -> RunDebugConfig {
    let flutter = Path::new(&config.program)
        .file_stem()
        .is_some_and(|stem| stem.eq_ignore_ascii_case("flutter"));
    if flutter {
        let args = config.args.get_or_insert_with(Vec::new);
        if let Some(device) = device.filter(|_| {
            args.first()
                .is_some_and(|arg| matches!(arg.as_str(), "run" | "attach"))
        }) {
            let end = args
                .iter()
                .position(|arg| arg == "--")
                .unwrap_or(args.len());
            let explicit = args[..end].iter().any(|arg| {
                arg == "-d"
                    || arg == "--device-id"
                    || arg.starts_with("--device-id=")
                    || (arg.starts_with("-d")
                        && arg.len() > 2
                        && !arg.starts_with("--"))
            });
            if !explicit {
                args.splice(end..end, ["-d".into(), device.into()]);
            }
        }
    }
    config
}

pub fn split_args(text: &str) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut started = false;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            } else if ch == '\\'
                && q == '"'
                && chars.peek().is_some_and(|c| matches!(c, '"' | '\\'))
            {
                word.push(chars.next().unwrap());
            } else {
                word.push(ch);
            }
        } else if ch == '\'' || ch == '"' {
            quote = Some(ch);
            started = true;
        } else if ch.is_whitespace() {
            if started {
                args.push(std::mem::take(&mut word));
                started = false;
            }
        } else if ch == '\\'
            && chars
                .peek()
                .is_some_and(|c| c.is_whitespace() || matches!(c, '\'' | '"' | '\\'))
        {
            word.push(chars.next().unwrap());
            started = true;
        } else {
            word.push(ch);
            started = true;
        }
    }
    if quote.is_some() {
        return Err("Close the quoted argument before saving.".into());
    }
    if started {
        args.push(word);
    }
    Ok(args)
}
pub fn join_args(args: &[String]) -> String {
    args.iter()
        .map(|s| {
            if !s.is_empty()
                && !s
                    .chars()
                    .any(|c| c.is_whitespace() || matches!(c, '\'' | '"' | '\\'))
            {
                s.clone()
            } else {
                format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
fn parse_environment(text: &str) -> Result<HashMap<String, String>, String> {
    let mut parts = Vec::new();
    let mut part = String::new();
    let mut quote = None;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if let Some(q) = quote {
            part.push(ch);
            if ch == '\\'
                && q == '"'
                && chars.peek().is_some_and(|c| matches!(c, '"' | '\\'))
            {
                part.push(chars.next().unwrap());
            } else if ch == q {
                quote = None;
            }
        } else if matches!(ch, '\'' | '"') {
            quote = Some(ch);
            part.push(ch);
        } else if ch == ';' {
            parts.push(std::mem::take(&mut part));
        } else {
            part.push(ch);
        }
    }
    if quote.is_some() {
        return Err("Close the quoted environment value.".into());
    }
    parts.push(part);
    let mut env = HashMap::new();
    for pair in parts.into_iter().filter(|p| !p.trim().is_empty()) {
        let (key, raw) = pair
            .split_once('=')
            .ok_or("Use KEY=value entries separated by semicolons.")?;
        let key = key.trim();
        if key.is_empty()
            || key.chars().enumerate().any(|(i, c)| {
                !(c == '_'
                    || c.is_ascii_alphabetic()
                    || (i > 0 && c.is_ascii_digit()))
            })
        {
            return Err("Invalid environment variable name.".into());
        }
        let raw = raw.trim();
        let value = if raw.starts_with(['\'', '"']) {
            let words = split_args(raw)?;
            if words.len() != 1 {
                return Err("Quote the complete environment value.".into());
            }
            words[0].clone()
        } else {
            raw.into()
        };
        env.insert(key.into(), value);
    }
    Ok(env)
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct TaskDraft {
    pub program: String,
    pub args: String,
    pub enabled: bool,
}
impl TaskDraft {
    pub fn new() -> Self {
        Self {
            enabled: true,
            ..Self::default()
        }
    }
    fn build(&self) -> Result<BeforeLaunchTask, String> {
        if self.program.trim().is_empty() {
            return Err(
                "Before-launch tasks require a program. Remove unused tasks with −."
                    .into(),
            );
        }
        Ok(BeforeLaunchTask {
            program: self.program.trim().into(),
            args: Some(split_args(&self.args)?),
            enabled: self.enabled,
        })
    }
}
#[derive(Clone, PartialEq)]
pub struct Draft {
    pub original: RunDebugConfig,
    pub name: String,
    pub entry: String,
    pub dart_entry: bool,
    dart_prefix: Vec<String>,
    flutter_command: String,
    pub args: String,
    pub attach: String,
    pub flavor: String,
    pub env: String,
    pub cwd: String,
    pub tasks: Vec<TaskDraft>,
    pub project: bool,
    pub activate: bool,
    pub focus: bool,
}
impl Draft {
    pub fn flutter(&self) -> bool {
        self.original.ty.as_deref() == Some("flutter")
            || Path::new(&self.original.program)
                .file_stem()
                .is_some_and(|p| p == "flutter")
    }
    pub fn from_config(c: RunDebugConfig) -> Self {
        let mut d = Self {
            name: c.name.clone(),
            entry: c.program.clone(),
            dart_entry: false,
            dart_prefix: Vec::new(),
            flutter_command: "run".into(),
            args: String::new(),
            attach: join_args(&c.flutter_options.attach_args),
            flavor: String::new(),
            env: String::new(),
            cwd: c.cwd.clone().unwrap_or_default(),
            tasks: c
                .prelaunch
                .iter()
                .map(|task| TaskDraft {
                    program: task.program.clone(),
                    args: join_args(task.args.as_deref().unwrap_or_default()),
                    enabled: true,
                })
                .chain(c.before_launch.iter().map(|task| TaskDraft {
                    program: task.program.clone(),
                    args: join_args(task.args.as_deref().unwrap_or_default()),
                    enabled: task.enabled,
                }))
                .collect(),
            project: c.store_in_project,
            activate: c.flutter_options.activate_tool_window,
            focus: c.flutter_options.focus_tool_window,
            original: c.clone(),
        };
        let mut args = c.args.unwrap_or_default();
        if d.flutter() {
            if args.first().is_some_and(|a| matches!(a.as_str(),"run"|"attach"|"test")) {
                d.flutter_command = args.remove(0);
            }
            d.entry = if d.flutter_command=="test" {"test"}else{"lib/main.dart"}.into();
            if d.flutter_command=="test" {if let Some(index)=args.iter().position(|a|!a.starts_with('-')&&(a.is_empty()||a.ends_with(".dart")||a=="test")){d.entry=args.remove(index);}}
            let mut rest = Vec::new();
            let mut iter = args.into_iter();
            while let Some(a) = iter.next() {
                match a.as_str() {
                    "-t" | "--target" => {
                        if let Some(v) = iter.next() {
                            d.entry = v;
                        }
                    }
                    "--flavor" => {
                        if let Some(v) = iter.next() {
                            d.flavor = v;
                        }
                    }
                    _ if a.starts_with("--target=") => d.entry = a[9..].into(),
                    _ if a.starts_with("--flavor=") => d.flavor = a[9..].into(),
                    _ => rest.push(a),
                }
            }
            args = rest;
        }
        if !d.flutter() && c.ty.as_deref() != Some("flutter-test") {
            if let Some(index) = args
                .iter()
                .position(|argument| argument.ends_with(".dart"))
                .or_else(|| {
                    (args.first().is_some_and(|arg| arg == "run")
                        && args.get(1).is_some_and(String::is_empty))
                    .then_some(1)
                })
            {
                d.dart_entry = true;
                d.entry = args[index].clone();
                d.dart_prefix = args[..index].to_vec();
                args = args[index + 1..].to_vec();
            }
        }
        d.args = join_args(&args);
        let mut env = c.env.unwrap_or_default().into_iter().collect::<Vec<_>>();
        env.sort();
        d.env = env
            .into_iter()
            .map(|(k, v)| {
                format!(
                    "{k}={}",
                    if v.contains(';') {
                        format!(
                            "\"{}\"",
                            v.replace('\\', "\\\\").replace('"', "\\\"")
                        )
                    } else {
                        join_args(&[v])
                    }
                )
            })
            .collect::<Vec<_>>()
            .join(";");
        d
    }
    pub fn build(&self) -> Result<RunDebugConfig, String> {
        self.build_inner(false)
    }
    pub fn build_template(&self) -> Result<RunDebugConfig, String> {
        self.build_inner(true)
    }
    fn build_inner(&self, template: bool) -> Result<RunDebugConfig, String> {
        if !self.original.is_dart_flutter() {
            return Err(
                "Run configurations must use the Dart or Flutter SDK executable."
                    .into(),
            );
        }
        if self.name.trim().is_empty()
            || (self.entry.trim().is_empty()
                && !(template && (self.flutter() || self.dart_entry)))
        {
            return Err("Name and entrypoint/program are required.".into());
        }
        let mut c = self.original.clone();
        c.name = self.name.trim().into();
        c.cwd = (!self.cwd.trim().is_empty()).then(|| self.cwd.trim().to_owned());
        let extra = split_args(&self.args)?;
        if self.flutter() {
            if extra.iter().any(|a| {
                matches!(a.as_str(), "--target" | "-t" | "--flavor")
                    || a.starts_with("--target=")
                    || a.starts_with("--flavor=")
            }) {
                return Err(
                    "Set entrypoint and flavor in their dedicated fields.".into()
                );
            }
            let mut args = if self.flutter_command=="test" {vec!["test".into(),self.entry.trim().into()]}else{vec![self.flutter_command.clone(), "--target".into(), self.entry.trim().into()]};
            if !self.flavor.trim().is_empty() {
                args.extend(["--flavor".into(), self.flavor.trim().into()]);
            }
            args.extend(extra);
            c.args = Some(args);
        } else if self.dart_entry {
            let mut args = self.dart_prefix.clone();
            args.push(self.entry.trim().into());
            args.extend(extra);
            c.args = Some(args);
        } else {
            c.program = self.entry.trim().into();
            c.args = Some(extra);
        }
        if !c.is_dart_flutter() {
            return Err("Choose the Dart or Flutter SDK executable.".into());
        }
        let env = parse_environment(&self.env)?;
        c.env = if env.is_empty() { None } else { Some(env) };
        c.flutter_options.attach_args = split_args(&self.attach)?;
        c.flutter_options.activate_tool_window = self.activate;
        c.flutter_options.focus_tool_window = self.focus;
        c.prelaunch = None;
        c.before_launch = self
            .tasks
            .iter()
            .map(TaskDraft::build)
            .collect::<Result<_, _>>()?;
        c.store_in_project = self.project;
        Ok(c)
    }
}
pub fn unique_name(drafts: &[Draft], base: &str) -> String {
    if !drafts.iter().any(|draft| draft.name.trim() == base) {
        return base.into();
    }
    (2..)
        .map(|number| format!("{base} ({number})"))
        .find(|name| !drafts.iter().any(|draft| draft.name.trim() == name))
        .unwrap()
}

pub fn build_drafts(
    drafts: &[Draft],
    root: &Path,
) -> Result<Vec<RunDebugConfig>, String> {
    let mut names = std::collections::HashSet::new();
    drafts
        .iter()
        .map(|draft| {
            let config = draft
                .build()
                .map_err(|error| format!("{}: {error}", draft.name))?;
            if !names.insert(config.name.clone()) {
                return Err(format!(
                    "A configuration named '{}' already exists.",
                    config.name
                ));
            }
            let directory = launch_directory(root, config.cwd.as_deref());
            if !directory.is_dir() {
                return Err(format!(
                    "{}: working directory does not exist: {}",
                    config.name,
                    directory.display()
                ));
            }
            if draft.flutter() && draft.flutter_command=="test" {
                let target=directory.join(draft.entry.trim());
                if !target.exists(){return Err(format!("{}: choose an existing test file or directory.",draft.name));}
            } else if draft.flutter() || draft.dart_entry {
                let target = directory.join(draft.entry.trim());
                if !target.is_file()
                    || !target.extension().is_some_and(|extension| {
                        extension.eq_ignore_ascii_case("dart")
                    })
                {
                    return Err(format!(
                        "{}: choose an existing Dart entrypoint.",
                        config.name
                    ));
                }
            }
            Ok(config)
        })
        .collect()
}

pub fn local_file(root: &Path) -> Option<PathBuf> {
    use sha2::{Digest, Sha256};
    let key = format!("{:x}", Sha256::digest(root.to_string_lossy().as_bytes()));
    Some(
        sparrow_core::directory::Directory::config_directory()?
            .join("run-configurations")
            .join(format!("{key}.toml")),
    )
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
struct LocalConfigs {
    #[serde(default)]
    configuration_order: Vec<String>,
    configs: Vec<RunDebugConfig>,
}
fn read_configs(path: &Path) -> Option<Vec<RunDebugConfig>> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| toml::from_str::<RunDebugConfigs>(&text).ok())
        .map(|data| data.configs)
}
pub fn project_file(root: &Path, name: &str) -> PathBuf {
    use sha2::{Digest, Sha256};
    let slug = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect::<String>();
    let hash = format!("{:x}", Sha256::digest(name.as_bytes()));
    root.join(".run")
        .join(format!("{slug}_{}.sparrow.run.toml", &hash[..8]))
}
fn project_files(root: &Path) -> Vec<PathBuf> {
    let mut paths = std::fs::read_dir(root.join(".run"))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name().is_some_and(|name| {
                name.to_string_lossy().ends_with(".sparrow.run.toml")
            })
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths
}
pub fn load(root: &Path) -> Option<Vec<RunDebugConfig>> {
    load_with_local(root, local_file(root).as_deref())
}
fn load_with_local(
    root: &Path,
    local: Option<&Path>,
) -> Option<Vec<RunDebugConfig>> {
    let mut configs = load_from(&root.join(".sparrow/run.toml"), None);
    for file in project_files(root) {
        if let Some(shared) = read_configs(&file) {
            let configs = configs.get_or_insert_with(Vec::new);
            for mut config in shared {
                config.store_in_project = true;
                if let Some(index) =
                    configs.iter().position(|old| old.name == config.name)
                {
                    configs[index] = config;
                } else {
                    configs.push(config);
                }
            }
        }
    }
    if let Some(private) = local.and_then(read_configs) {
        let configs = configs.get_or_insert_with(Vec::new);
        for mut config in private {
            config.store_in_project = false;
            if let Some(index) =
                configs.iter().position(|old| old.name == config.name)
            {
                configs[index] = config;
            } else {
                configs.push(config);
            }
        }
    }
    if let Some(order) = local
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| toml::from_str::<LocalConfigs>(&text).ok())
        .map(|local| local.configuration_order)
    {
        if let Some(configs) = &mut configs {
            configs.sort_by_key(|config| {
                order
                    .iter()
                    .position(|name| name == &config.name)
                    .unwrap_or(usize::MAX)
            });
        }
    }
    configs
}
fn load_from(project: &Path, local: Option<&Path>) -> Option<Vec<RunDebugConfig>> {
    let shared = read_configs(project);
    let private = local.and_then(read_configs);
    if shared.is_none() && private.is_none() {
        return None;
    }
    let mut configs = shared.unwrap_or_default();
    for config in &mut configs {
        config.store_in_project = true;
    }
    for mut config in private.unwrap_or_default() {
        config.store_in_project = false;
        if let Some(index) = configs
            .iter()
            .position(|existing| existing.name == config.name)
        {
            configs[index] = config;
        } else {
            configs.push(config);
        }
    }
    Some(configs)
}
pub fn templates_file(root: &Path) -> Option<PathBuf> {
    local_file(root).map(|path| path.with_extension("templates.toml"))
}
pub const TEMPLATE_NAMES:&[&str]=&["Flutter","Dart Command Line App","Flutter Test","Dart Test","Flutter Web","Flutter Profile","Flutter Release","Flutter Attach","Dart Remote Debug"];
pub fn default_templates(root: &Path) -> Vec<RunDebugConfig> {
    TEMPLATE_NAMES.iter().enumerate().map(|(i,name)| {
        let mut config=new_configuration(Some(root));config.name=(*name).into();
        config.ty=Some(if matches!(i,1|3|8){if i==3{"dart-test"}else{"dart"}}else if i==2{"flutter-test"}else{"flutter"}.into());
        config.program=if matches!(i,1|3|8){"dart"}else{"flutter"}.into();
        config.args=Some(match i {
            1=>vec!["run","bin/main.dart"],2=>vec!["test","test"],3=>vec!["test"],4=>vec!["run","--target","lib/main.dart","-d","chrome"],5=>vec!["run","--target","lib/main.dart","--profile"],6=>vec!["run","--target","lib/main.dart","--release"],7=>vec!["attach","--target","lib/main.dart"],8=>vec!["attach","http://127.0.0.1:8181/"],_=>vec!["run","--target","lib/main.dart"]
        }.into_iter().map(str::to_string).collect());config.cwd=None;config
    }).collect()
}
pub fn load_templates(root: &Path) -> Vec<RunDebugConfig> {
    let saved = templates_file(root)
        .and_then(|path| read_configs(&path))
        .unwrap_or_default();
    default_templates(root)
        .into_iter()
        .map(|default| {
            saved
                .iter()
                .find(|config| config.ty == default.ty && config.name == default.name && config.is_dart_flutter())
                .cloned()
                .unwrap_or(default)
        })
        .collect()
}
pub fn save_bundle(
    root: &Path,
    configs: Vec<RunDebugConfig>,
    templates: Option<Vec<RunDebugConfig>>,
) -> Result<(), String> {
    let local =
        local_file(root).ok_or("Cannot locate user configuration directory.")?;
    let templates_path =
        templates_file(root).ok_or("Cannot locate templates file")?;
    save_bundle_to(root, local, templates_path, configs, templates)
}
fn save_bundle_to(
    root: &Path,
    local: PathBuf,
    templates_path: PathBuf,
    configs: Vec<RunDebugConfig>,
    templates: Option<Vec<RunDebugConfig>>,
) -> Result<(), String> {
    let mut existing = project_files(root);
    existing.extend([local.clone(), root.join(".sparrow/run.toml")]);
    if templates.is_some() {
        existing.push(templates_path.clone());
    }
    for path in existing.into_iter().filter(|path| path.is_file()) {
        let text =
            std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
        toml::from_str::<RunDebugConfigs>(&text)
            .map_err(|error| format!("{}: {error}", path.display()))?;
    }
    let configuration_order = configs
        .iter()
        .map(|config| config.name.clone())
        .collect::<Vec<_>>();
    let legacy = root.join(".sparrow/run.toml");
    let mut files = Vec::new();
    let mut retained = std::collections::HashSet::new();
    let mut private = Vec::new();
    for config in configs {
        if config.store_in_project {
            let path = project_file(root, &config.name);
            retained.insert(path.clone());
            files.push((
                path,
                Some(
                    toml_edit::ser::to_string_pretty(&RunDebugConfigs {
                        configs: vec![config],
                    })
                    .map_err(|error| error.to_string())?,
                ),
            ));
        } else {
            private.push(config);
        }
    }
    for path in project_files(root) {
        if !retained.contains(&path) {
            files.push((path, None));
        }
    }
    // Retire the old whole-list project file only after preparing the replacement records.
    if legacy.is_file() {
        files.push((legacy, Some("configs = []\n".into())));
    }
    files.push((
        local,
        Some(
            toml_edit::ser::to_string_pretty(&LocalConfigs {
                configuration_order,
                configs: private,
            })
            .map_err(|error| error.to_string())?,
        ),
    ));
    if let Some(configs) = templates {
        files.push((
            templates_path,
            Some(
                toml_edit::ser::to_string_pretty(&RunDebugConfigs { configs })
                    .map_err(|error| error.to_string())?,
            ),
        ));
    }
    crate::persistence::write_changes(files)
}

#[cfg(test)]
pub fn save(
    root: &Path,
    mut configs: Vec<RunDebugConfig>,
    project: bool,
) -> Result<(), String> {
    for config in &mut configs {
        config.store_in_project = project;
    }
    save_bundle(root, configs, None)
}
#[cfg(test)]
fn save_to(path: &Path, configs: Vec<RunDebugConfig>) -> Result<(), String> {
    let text = toml_edit::ser::to_string_pretty(&RunDebugConfigs { configs })
        .map_err(|e| e.to_string())?;
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let temporary = tempfile::NamedTempFile::new_in(path.parent().unwrap())
        .map_err(|e| e.to_string())?;
    std::fs::write(temporary.path(), text).map_err(|e| e.to_string())?;
    temporary.persist(&path).map_err(|e| e.to_string())?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> RunDebugConfig {
        serde_json::from_value(serde_json::json!({"name":"main.dart","type":"flutter","program":"/sdk/flutter","args":["run","-t","lib/main.dart","--flavor","dev","--dart-define","VALUE=hello world"]})).unwrap()
    }
    #[test]
    fn arguments_roundtrip_and_validation() {
        let args = vec![
            "".into(),
            "hello world".into(),
            r"C:\work dir\main.dart".into(),
            "a\"b".into(),
            "$(literal)".into(),
        ];
        assert_eq!(split_args(&join_args(&args)).unwrap(), args);
        assert!(split_args("'unterminated").is_err());
    }
    #[test]
    fn flutter_fields_preserve_executable_and_flags() {
        let d = Draft::from_config(sample());
        let c = d.build().unwrap();
        assert_eq!(c.program, "/sdk/flutter");
        let reloaded = Draft::from_config(c);
        assert_eq!(reloaded.entry, d.entry);
        assert_eq!(reloaded.args, d.args);
        assert_eq!(reloaded.flavor, d.flavor);
    }
    #[test]
    fn storage_preserves_flutter_and_environment_options() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("run.toml");
        let mut c = sample();
        c.flutter_options.attach_args = vec!["--debug-port".into(), "1234".into()];
        c.env = Some(HashMap::from([
            ("LIST".into(), "one;two".into()),
            ("SPACED".into(), "hello world".into()),
        ]));
        let d = Draft::from_config(c);
        let built = d.build().unwrap();
        assert_eq!(built.env.as_ref().unwrap()["LIST"], "one;two");
        save_to(&path, vec![built.clone()]).unwrap();
        let saved: RunDebugConfigs =
            toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(saved.configs[0].env, built.env);
        assert_eq!(saved.configs[0].flutter_options, built.flutter_options);
    }

    #[test]
    fn dart_commands_preserve_sdk_and_quoted_arguments() {
        let original: RunDebugConfig = serde_json::from_value(serde_json::json!({"name":"tool", "program":"/usr/bin/dart", "args":["run", "tool/my script.dart"]})).unwrap();
        let built = Draft::from_config(original.clone()).build().unwrap();
        assert_eq!(built.program, original.program);
        assert_eq!(built.args, original.args);
    }

    #[test]
    fn env_and_prelaunch_are_validated() {
        let mut d = Draft::from_config(sample());
        d.env = "TOKEN=a=b;MODE=dev".into();
        d.tasks.push(TaskDraft {
            program: "dart".into(),
            args: "run tool/build.dart".into(),
            enabled: true,
        });
        let c = d.build().unwrap();
        assert_eq!(c.env.unwrap()["TOKEN"], "a=b");
        assert_eq!(c.before_launch[0].program, "dart");
        d.env = "1INVALID=x".into();
        assert!(d.build().is_err());
    }
}

#[cfg(test)]
mod toolbar_tests {
    use super::*;
    #[test]
    fn defaults_follow_real_dart_and_flutter_entrypoints() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("pubspec.yaml"),
            "name: console_app\nenvironment:\n  sdk: ^3.0.0\n",
        )
        .unwrap();
        assert!(discover_defaults(root.path()).is_empty());
        std::fs::create_dir(root.path().join("bin")).unwrap();
        std::fs::write(root.path().join("bin/console_app.dart"), "void main() {}\n")
            .unwrap();
        let dart = discover_defaults(root.path()).remove(0);
        assert_eq!(dart.ty.as_deref(), Some("dart"));
        assert_eq!(dart.name, "console_app.dart");
        assert_eq!(dart.args.unwrap(), ["run", "bin/console_app.dart"]);
        std::fs::write(
            root.path().join("pubspec.yaml"),
            "name: widgets\ndependencies:\n  flutter:\n    sdk: flutter\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.path().join("example/lib")).unwrap();
        std::fs::write(
            root.path().join("example/lib/main.dart"),
            "void main() {}\n",
        )
        .unwrap();
        let flutter = discover_defaults(root.path()).remove(0);
        assert_eq!(flutter.ty.as_deref(), Some("flutter"));
        assert_eq!(flutter.args.unwrap(), ["run", "--target", "lib/main.dart"]);
        assert_eq!(
            Path::new(flutter.cwd.as_ref().unwrap()),
            root.path().join("example")
        );
    }
    #[test]
    fn launch_cwd_resolves_workspace_and_relative_paths() {
        let root = Path::new("/work/my app");
        assert_eq!(
            launch_directory(root, Some("${workspace}/example")),
            root.join("example")
        );
        assert_eq!(
            launch_directory(root, Some("example")),
            root.join("example")
        );
        assert_eq!(launch_directory(root, None), root);
        assert_eq!(
            launch_directory(root, Some("/elsewhere")),
            Path::new("/elsewhere")
        );
    }
    #[test]
    fn selected_device_is_bound_before_app_arguments_without_overriding_config() {
        let flutter = new_configuration(None);
        assert_eq!(
            with_selected_device(flutter.clone(), Some("linux")),
            flutter
        );
        let mut flutter: RunDebugConfig = serde_json::from_value(serde_json::json!({"name":"dev","type":"flutter","program":"/sdk/flutter","args":["run","--target","lib/main.dart","--","--device-id=app-data"]})).unwrap();
        assert_eq!(
            with_selected_device(flutter.clone(), Some("linux"))
                .args
                .unwrap(),
            [
                "run",
                "--target",
                "lib/main.dart",
                "-d",
                "linux",
                "--",
                "--device-id=app-data"
            ]
        );
        for args in [
            vec!["run", "-d", "chrome"],
            vec!["attach", "--device-id=chrome"],
            vec!["run", "-dchrome"],
            vec!["test", "test/a_test.dart"],
        ] {
            flutter.args = Some(args.into_iter().map(str::to_string).collect());
            assert_eq!(
                with_selected_device(flutter.clone(), Some("linux")),
                flutter
            );
        }
    }
}

#[cfg(test)]
mod flutter_session_tests {
    use super::*;
    #[test]
    fn project_sessions_use_dedicated_panels() {
        use crate::{debug::RunDebugMode, panel::kind::PanelKind};
        assert_eq!(console_panel(RunDebugMode::Run), PanelKind::FlutterRun);
        assert_eq!(console_panel(RunDebugMode::Debug), PanelKind::Debug);
    }

    #[test]
    fn cli_sessions_exclude_tests_tools_and_adapter_owned_processes() {
        let mut config: RunDebugConfig = serde_json::from_value(serde_json::json!({"name":"app","program":"/sdk/flutter","args":["run","--target","lib/main.dart"]})).unwrap();
        assert!(is_flutter_run(&config));
        config.config_source = sparrow_rpc::dap_types::ConfigSource::ProjectTask;
        assert!(!is_flutter_run(&config));
        config.config_source = sparrow_rpc::dap_types::ConfigSource::Palette;
        config.debug_command = Some(vec!["dart".into()]);
        assert!(!is_flutter_run(&config));
        config.debug_command = None;
        config.args = Some(vec!["test".into()]);
        assert!(!is_flutter_run(&config));
        config.args = Some(vec!["attach".into()]);
        assert!(is_flutter_run(&config));
        config.program = "dart".into();
        assert!(!is_flutter_run(&config));
    }
}

#[cfg(test)]
mod context_file_launch_tests {
    use super::*;
    #[test]
    fn context_launch_uses_clicked_file_and_its_nearest_flutter_package() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("pubspec.yaml"),
            "name: context_app\ndependencies:\n  flutter:\n    sdk: flutter\n",
        )
        .unwrap();
        let target = root.path().join("lib/alternate entry.dart");
        let unrelated = tempfile::tempdir().unwrap();
        let config =
            configuration_for_file(&target, Some(unrelated.path())).unwrap();
        assert_eq!(config.program, "flutter");
        assert_eq!(
            config.args.unwrap(),
            ["run", "--target", "lib/alternate entry.dart"]
        );
        assert_eq!(config.cwd.as_deref(), root.path().to_str());
    }
    #[test]
    fn dart_context_launch_does_not_become_a_flutter_command() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("pubspec.yaml"),
            "name: cli\nenvironment:\n  sdk: ^3.0.0\n",
        )
        .unwrap();
        let config =
            configuration_for_file(&root.path().join("bin/tool.dart"), None)
                .unwrap();
        assert_eq!(config.program, "dart");
        assert_eq!(config.args.unwrap(), ["run", "bin/tool.dart"]);
        assert!(
            configuration_for_file(
                &root.path().join("android/app.kt"),
                Some(root.path())
            )
            .is_err()
        );
    }
}

#[cfg(test)]
mod dialog_validation_tests {
    use super::*;
    #[test]
    fn empty_lists_unique_names_and_invalid_directories_are_handled() {
        let root = tempfile::tempdir().unwrap();
        assert!(build_drafts(&[], root.path()).unwrap().is_empty());
        std::fs::create_dir(root.path().join("bin")).unwrap();
        std::fs::write(root.path().join("bin/main.dart"), "void main() {}").unwrap();
        let mut draft = Draft::from_config(new_configuration(None));
        draft.name = "Tool".into();
        assert_eq!(unique_name(&[draft.clone()], "Tool"), "Tool (2)");
        assert!(
            build_drafts(&[draft.clone(), draft.clone()], root.path())
                .unwrap_err()
                .contains("already exists")
        );
        draft.cwd = "missing".into();
        assert!(
            build_drafts(&[draft], root.path())
                .unwrap_err()
                .contains("working directory")
        );
    }
    #[test]
    fn flutter_target_is_validated_against_edited_working_directory() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("example/lib")).unwrap();
        std::fs::write(root.path().join("example/lib/main.dart"), "void main() {} ")
            .unwrap();
        let mut draft = Draft::from_config(serde_json::from_value(serde_json::json!({"name":"Example", "type":"flutter", "program":"flutter", "args":["run","-t","lib/main.dart"]})).unwrap());
        draft.cwd = "${workspace}/example".into();
        let configs = build_drafts(&[draft.clone()], root.path()).unwrap();
        assert_eq!(configs[0].cwd.as_deref(), Some("${workspace}/example"));
        draft.entry = "lib/missing.dart".into();
        assert!(build_drafts(&[draft], root.path()).is_err());
    }
}

#[cfg(test)]
mod dart_form_tests {
    use super::*;
    #[test]
    fn dart_script_fields_keep_vm_options_sdk_and_program_arguments_separate() {
        let original: RunDebugConfig = serde_json::from_value(serde_json::json!({"name":"tool", "program":"/sdk/dart", "args":["run","--enable-asserts","bin/my tool.dart","hello world"]})).unwrap();
        let mut draft = Draft::from_config(original.clone());
        assert!(draft.dart_entry);
        assert_eq!(draft.entry, "bin/my tool.dart");
        assert_eq!(draft.args, "\"hello world\"");
        assert_eq!(draft.build().unwrap().args, original.args);
        draft.entry = "bin/other.dart".into();
        draft.args = "--verbose".into();
        let built = draft.build().unwrap();
        assert_eq!(built.program, "/sdk/dart");
        assert_eq!(
            built.args.unwrap(),
            ["run", "--enable-asserts", "bin/other.dart", "--verbose"]
        );
    }
    #[test]
    fn non_script_dart_commands_remain_editable_without_rewriting_them() {
        let original: RunDebugConfig = serde_json::from_value(serde_json::json!({"name":"tests", "program":"dart", "args":["test","--reporter","expanded"]})).unwrap();
        let draft = Draft::from_config(original.clone());
        assert!(!draft.dart_entry);
        let built = draft.build().unwrap();
        assert_eq!(built.program, original.program);
        assert_eq!(built.args, original.args);
    }
}

#[cfg(test)]
mod independent_storage_tests {
    use super::*;
    fn config(name: &str, shared: bool) -> RunDebugConfig {
        let mut config = new_configuration(None);
        config.name = name.into();
        config.store_in_project = shared;
        config
    }
    #[test]
    fn templates_allow_blank_entrypoints_but_launch_configurations_require_them() {
        let root = tempfile::tempdir().unwrap();
        for template in default_templates(root.path()).into_iter().take(2) {
            let mut draft = Draft::from_config(template);
            draft.entry.clear();
            let saved = draft.build_template().unwrap();
            let inherited = Draft::from_config(saved);
            assert!(inherited.entry.is_empty());
            assert!(inherited.build().is_err());
        }
    }
    #[test]
    fn mixed_storage_survives_rename_move_delete_and_reopen() {
        let root = tempfile::tempdir().unwrap();
        let private = root.path().join("private/run.toml");
        let templates = root.path().join("private/templates.toml");
        save_bundle_to(
            root.path(),
            private.clone(),
            templates.clone(),
            vec![config("shared", true), config("private", false)],
            None,
        )
        .unwrap();
        assert!(project_file(root.path(), "shared").is_file());
        assert!(!project_file(root.path(), "private").exists());
        let loaded = load_with_local(root.path(), Some(&private)).unwrap();
        assert_eq!(loaded.len(), 2);
        assert!(
            loaded
                .iter()
                .find(|config| config.name == "shared")
                .unwrap()
                .store_in_project
        );
        assert!(
            !loaded
                .iter()
                .find(|config| config.name == "private")
                .unwrap()
                .store_in_project
        );
        save_bundle_to(
            root.path(),
            private.clone(),
            templates.clone(),
            vec![config("renamed", true), config("private", true)],
            None,
        )
        .unwrap();
        assert!(!project_file(root.path(), "shared").exists());
        assert!(project_file(root.path(), "renamed").is_file());
        assert!(project_file(root.path(), "private").is_file());
        assert_eq!(
            load_with_local(root.path(), Some(&private)).unwrap().len(),
            2
        );
        save_bundle_to(
            root.path(),
            private.clone(),
            templates,
            vec![config("renamed", false)],
            None,
        )
        .unwrap();
        assert!(project_files(root.path()).is_empty());
        let loaded = load_with_local(root.path(), Some(&private)).unwrap();
        assert_eq!(loaded.len(), 1);
        assert!(!loaded[0].store_in_project);
    }
    #[test]
    fn malformed_existing_storage_is_preserved() {
        let root = tempfile::tempdir().unwrap();
        let private = root.path().join("local.toml");
        std::fs::write(&private, "[invalid").unwrap();
        assert!(
            save_bundle_to(
                root.path(),
                private.clone(),
                root.path().join("templates.toml"),
                vec![config("new", true)],
                None
            )
            .is_err()
        );
        assert_eq!(std::fs::read_to_string(private).unwrap(), "[invalid");
        assert!(!root.path().join(".run").exists());
    }
    #[test]
    fn private_legacy_override_merges_with_other_project_configs() {
        let root = tempfile::tempdir().unwrap();
        let shared = root.path().join("project.toml");
        let private = root.path().join("local.toml");
        save_to(&shared, vec![config("same", true), config("shared", true)])
            .unwrap();
        save_to(&private, vec![config("same", false)]).unwrap();
        let loaded = load_from(&shared, Some(&private)).unwrap();
        assert_eq!(loaded.len(), 2);
        assert!(!loaded[0].store_in_project);
        assert!(loaded[1].store_in_project);
    }
    #[test]
    fn templates_are_separate_defaults_and_do_not_modify_existing_configurations() {
        let root = tempfile::tempdir().unwrap();
        let private = root.path().join("local.toml");
        let templates = root.path().join("templates.toml");
        let original = config("existing", false);
        let mut defaults = default_templates(root.path());
        defaults[0].env = Some(HashMap::from([("MODE".into(), "template".into())]));
        defaults[0].before_launch.push(BeforeLaunchTask {
            program: "dart".into(),
            args: Some(vec!["run".into(), "tool/build.dart".into()]),
            enabled: true,
        });
        save_bundle_to(
            root.path(),
            private.clone(),
            templates.clone(),
            vec![original],
            Some(defaults.clone()),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(read_configs(&templates).unwrap()).unwrap(),
            serde_json::to_value(&defaults).unwrap()
        );
        assert!(
            load_with_local(root.path(), Some(&private)).unwrap()[0]
                .env
                .is_none()
        );
        let inherited = Draft::from_config(defaults[0].clone()).build().unwrap();
        assert_eq!(inherited.env.unwrap()["MODE"], "template");
        assert_eq!(inherited.before_launch.len(), 1);
    }
    #[test]
    fn ordered_task_drafts_preserve_flags_quoting_and_reject_empty_programs() {
        let mut draft = Draft::from_config(config("task", false));
        draft.tasks = vec![
            TaskDraft {
                program: "dart".into(),
                args: "run 'tool/a b.dart'".into(),
                enabled: false,
            },
            TaskDraft {
                program: "dart".into(),
                args: "run tool/c.dart".into(),
                enabled: true,
            },
        ];
        let config = draft.build().unwrap();
        assert!(config.prelaunch.is_none());
        assert!(!config.before_launch[0].enabled);
        assert_eq!(
            config.before_launch[0].args.as_ref().unwrap()[1],
            "tool/a b.dart"
        );
        assert_eq!(
            Draft::from_config(config.clone())
                .build()
                .unwrap()
                .before_launch,
            config.before_launch
        );
        draft.tasks.push(TaskDraft::new());
        assert!(draft.build().is_err());
    }
}

#[cfg(test)] mod preset_tests {use super::*;
 #[test]fn expanded_presets_round_trip_without_changing_the_sdk_command(){let root=tempfile::tempdir().unwrap();for c in default_templates(root.path()){let restored=Draft::from_config(c.clone()).build().unwrap();assert_eq!(restored.args,c.args,"{}",c.name);assert_eq!(restored.program,c.program);assert!(restored.is_dart_flutter());}}
}

/// Execute a captured configuration as an ordered before-launch task, without a GUI.
/// A snapshot retains its working directory, environment and own prerequisites.
pub fn execute_task_snapshot(json:&str)->Result<i32,String>{
    let mut config:RunDebugConfig=serde_json::from_str(json).map_err(|e|e.to_string())?;
    if !config.is_dart_flutter(){return Err("Configuration tasks require a Dart or Flutter target.".into());}
    if std::path::Path::new(&config.program).file_stem().is_some_and(|p|p=="dart")&&config.args.as_ref().and_then(|a|a.first()).is_some_and(|a|a=="attach"){return Err("Remote debugging configurations cannot run as before-launch commands.".into());}
    let cwd=config.cwd.as_deref().map(PathBuf::from).unwrap_or(std::env::current_dir().map_err(|e|e.to_string())?);
    let execute=|program:&str,args:Option<&Vec<String>>|->Result<i32,String>{
        let binary=match program {"dart"=>super::sdk::find_dart_binary(Some(&cwd)),"flutter"=>super::sdk::find_flutter_binary(Some(&cwd)),"${sparrow}"=>std::env::current_exe().ok(),_=>None}.unwrap_or_else(||PathBuf::from(program));
        let mut command=std::process::Command::new(binary);command.current_dir(&cwd);if let Some(args)=args{command.args(args);}if let Some(env)=&config.env{command.envs(env);}
        command.status().map(|s|s.code().unwrap_or(1)).map_err(|e|e.to_string())
    };
    if let Some(before)=&config.prelaunch {let code=execute(&before.program,before.args.as_ref())?;if code!=0{return Ok(code);}}
    for before in &config.before_launch {if before.enabled{let code=execute(&before.program,before.args.as_ref())?;if code!=0{return Ok(code);}}}
    // Apply user-supplied SDK paths exactly as ordinary run configurations do.
    config.debug_command=None;
    execute(&config.program,config.args.as_ref())
}

pub fn task_title(program:&str,args:&str)->String {
    if program=="${sparrow}" {if let Ok(args)=split_args(args){if args.first().is_some_and(|a|a=="--run-config-task"){if let Some(config)=args.get(1).and_then(|json|serde_json::from_str::<RunDebugConfig>(json).ok()){return format!("Run configuration: {}",config.name);}}}}
    format!("{program} {args}")
}

/// Keep exact entrypoint arguments aligned with a file move; similarly named files
/// in other directories are left intact.
pub(crate) fn rebind_file(config:&mut RunDebugConfig,root:&Path,old:&Path,new:&Path)->bool{
    let cwd=launch_directory(root,config.cwd.as_deref());let mut changed=false;
    let map=|value:&str|->Option<String>{let (prefix,path)=value.strip_prefix("--target=").map(|p|("--target=",p)).unwrap_or(("",value));let candidate=PathBuf::from(path);let candidate=if candidate.is_absolute(){candidate}else{cwd.join(candidate)};if candidate==old{Some(format!("{prefix}{}",new.strip_prefix(&cwd).unwrap_or(new).to_string_lossy().replace('\\',"/")))}else{None}};
    if let Some(value)=map(&config.program){config.program=value;changed=true;}
    if let Some(args)=&mut config.args{for arg in args{if let Some(value)=map(arg){*arg=value;changed=true;}}}
    changed
}
/// Configuration files are refactoring inputs even when comment searching is off.
pub(crate) fn refactored_configuration_files(root:&Path,old:&Path,new:&Path)->Result<Vec<(PathBuf,String,String)>,String>{
    let mut paths=project_files(root);paths.push(root.join(".sparrow/run.toml"));if let Some(path)=local_file(root){paths.push(path);}let mut result=Vec::new();
    for path in paths{if !path.is_file(){continue;}if path.is_symlink(){return Err(format!("Refusing to refactor linked configuration {}",path.display()));}let text=std::fs::read_to_string(&path).map_err(|e|e.to_string())?;let Ok(parsed)=toml::from_str::<RunDebugConfigs>(&text)else{continue;};let Ok(mut document)=text.parse::<toml_edit::Document>()else{continue;};let Some(configs)=document.get_mut("configs")else{continue;};let mut changed=false;
    for(index,mut config)in parsed.configs.into_iter().enumerate(){let Some(table)=configs.get_mut(index)else{continue;};if rebind_file(&mut config,root,old,new){table["program"]=toml_edit::value(config.program);if let Some(args)=config.args{let mut array=toml_edit::Array::new();for arg in args{array.push(arg);}table["args"]=toml_edit::value(array);}changed=true;}}
    if changed{result.push((path,text,document.to_string()));}}
    Ok(result)
}

#[cfg(test)]mod refactoring_tests {use super::*;
 #[test]fn moves_only_the_exact_configuration_target_and_preserves_toml_comments(){let root=tempfile::tempdir().unwrap();let old=root.path().join("lib/old.dart");let new=root.path().join("lib/new.dart");let mut config=new_configuration(Some(root.path()));config.program="dart".into();config.ty=Some("dart".into());config.args=Some(vec!["run".into(),"lib/old.dart".into(),"other/old.dart".into()]);assert!(rebind_file(&mut config,root.path(),&old,&new));assert_eq!(config.args.as_ref().unwrap()[1],"lib/new.dart");assert_eq!(config.args.as_ref().unwrap()[2],"other/old.dart");config.args.as_mut().unwrap()[1]="lib/old.dart".into();std::fs::create_dir(root.path().join(".run")).unwrap();let path=root.path().join(".run/check.sparrow.run.toml");let body=format!("# Keep this comment\n{}",toml_edit::ser::to_string(&RunDebugConfigs{configs:vec![config]}).unwrap());std::fs::write(&path,&body).unwrap();let files=refactored_configuration_files(root.path(),&old,&new).unwrap();assert_eq!(files.len(),1);assert!(files[0].2.starts_with("# Keep this comment"));assert!(files[0].2.contains("lib/new.dart"));assert!(files[0].2.contains("other/old.dart"));assert_eq!(std::fs::read_to_string(path).unwrap(),body);}
}
