//! Editable run configuration drafts and argument/storage helpers.
use crate::debug::RunDebugConfigs;
use sparrow_rpc::dap_types::{RunDebugConfig, RunDebugProgram};
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

pub fn is_flutter_run(config: &RunDebugConfig) -> bool {
    config.debug_command.is_none() && Path::new(&config.program).file_stem()
        .is_some_and(|stem| stem.eq_ignore_ascii_case("flutter"))
        && config.args.as_ref().and_then(|args| args.first())
            .is_some_and(|arg| matches!(arg.as_str(), "run" | "attach"))
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

#[derive(Clone, PartialEq)]
pub struct Draft {
    pub original: RunDebugConfig,
    pub name: String,
    pub entry: String,
    pub args: String,
    pub attach: String,
    pub flavor: String,
    pub env: String,
    pub before: String,
    pub before_args: String,
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
            args: String::new(),
            attach: join_args(&c.flutter_options.attach_args),
            flavor: String::new(),
            env: String::new(),
            before: c
                .prelaunch
                .as_ref()
                .map(|p| p.program.clone())
                .unwrap_or_default(),
            before_args: c
                .prelaunch
                .as_ref()
                .and_then(|p| p.args.as_ref())
                .map(|a| join_args(a))
                .unwrap_or_default(),
            activate: c.flutter_options.activate_tool_window,
            focus: c.flutter_options.focus_tool_window,
            original: c.clone(),
        };
        let mut args = c.args.unwrap_or_default();
        if d.flutter() {
            if args.first().is_some_and(|a| a == "run" || a == "attach") {
                args.remove(0);
            }
            d.entry = "lib/main.dart".into();
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
        if !self.original.is_dart_flutter() {
            return Err(
                "Run configurations must use the Dart or Flutter SDK executable."
                    .into(),
            );
        }
        if self.name.trim().is_empty() || self.entry.trim().is_empty() {
            return Err("Name and entrypoint/program are required.".into());
        }
        let mut c = self.original.clone();
        c.name = self.name.trim().into();
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
            let mut args =
                vec!["run".into(), "--target".into(), self.entry.trim().into()];
            if !self.flavor.trim().is_empty() {
                args.extend(["--flavor".into(), self.flavor.trim().into()]);
            }
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
        c.prelaunch = if self.before.trim().is_empty() {
            None
        } else {
            Some(RunDebugProgram {
                program: self.before.trim().into(),
                args: Some(split_args(&self.before_args)?),
            })
        };
        Ok(c)
    }
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
pub fn load(root: &Path) -> Option<Vec<RunDebugConfig>> {
    [local_file(root), Some(root.join(".sparrow/run.toml"))]
        .into_iter()
        .flatten()
        .find_map(|path| {
            std::fs::read_to_string(path)
                .ok()
                .and_then(|s| toml::from_str::<RunDebugConfigs>(&s).ok())
                .map(|c| c.configs)
        })
}
pub fn save(
    root: &Path,
    configs: Vec<RunDebugConfig>,
    project: bool,
) -> Result<(), String> {
    let local =
        local_file(root).ok_or("Cannot locate user configuration directory.")?;
    let path = if project {
        root.join(".sparrow/run.toml")
    } else {
        local.clone()
    };
    save_to(&path, configs)?;
    if project && local.is_file() {
        std::fs::remove_file(local).map_err(|e| e.to_string())?;
    }
    Ok(())
}
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
        d.before = "dart".into();
        d.before_args = "run tool/build.dart".into();
        let c = d.build().unwrap();
        assert_eq!(c.env.unwrap()["TOKEN"], "a=b");
        assert_eq!(c.prelaunch.unwrap().program, "dart");
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
    fn cli_sessions_exclude_tests_tools_and_adapter_owned_processes() {
        let mut config: RunDebugConfig = serde_json::from_value(serde_json::json!({"name":"app","program":"/sdk/flutter","args":["run","--target","lib/main.dart"]})).unwrap();
        assert!(is_flutter_run(&config));
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
