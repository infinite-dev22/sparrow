mod view;

pub use view::view;

use crate::window_tab::WindowTabData;
use floem::reactive::{RwSignal, Scope, SignalGet, SignalUpdate};
use std::{path::PathBuf, rc::Rc};
const PLATFORMS: [&str; 6] = ["android", "ios", "web", "linux", "macos", "windows"];
const TEMPLATES: [&str; 5] = ["app", "package", "plugin", "module", "package_ffi"];
#[derive(Clone)]
pub struct ProjectWizard {
    pub active: RwSignal<bool>,
    name: RwSignal<String>,
    parent: RwSignal<String>,
    template: RwSignal<String>,
    platforms: RwSignal<Vec<String>>,
    empty: RwSignal<bool>,
    busy: RwSignal<bool>,
    message: RwSignal<String>,
}
pub fn arguments(
    name: &str,
    parent: &str,
    template: &str,
    platforms: &[String],
    empty: bool,
) -> Result<(PathBuf, Vec<String>), String> {
    if !regex::Regex::new(r"^[a-z][a-z0-9_]*$")
        .unwrap()
        .is_match(name)
    {
        return Err("Use a lowercase Dart package name, such as my_app.".into());
    }
    if !TEMPLATES.contains(&template) {
        return Err("Choose a project type.".into());
    }
    let parent = PathBuf::from(parent);
    if !parent.is_absolute() || !parent.is_dir() {
        return Err("Choose an existing absolute parent directory.".into());
    }
    let destination = parent.join(name);
    if destination.exists() {
        return Err("The destination already exists. Choose another name.".into());
    }
    let mut args = vec![
        "create".into(),
        "--project-name".into(),
        name.into(),
        "--template".into(),
        template.into(),
    ];
    if matches!(template, "app" | "plugin") {
        if platforms.is_empty() {
            return Err("Select at least one target platform.".into());
        }
        if platforms.iter().any(|p| !PLATFORMS.contains(&p.as_str())) {
            return Err("Choose supported platform targets.".into());
        }
        args.extend(["--platforms".into(), platforms.join(",")]);
    }
    if empty && template == "app" {
        args.push("--empty".into());
    }
    args.push(destination.display().to_string());
    Ok((destination, args))
}
impl ProjectWizard {
    pub fn is_busy(&self) -> bool { self.busy.get_untracked() }

    pub fn new(cx: Scope, parent: String) -> Self {
        Self {
            active: cx.create_rw_signal(false),
            name: cx.create_rw_signal("my_app".into()),
            parent: cx.create_rw_signal(parent),
            template: cx.create_rw_signal("app".into()),
            platforms: cx
                .create_rw_signal(PLATFORMS.iter().map(|s| s.to_string()).collect()),
            empty: cx.create_rw_signal(false),
            busy: cx.create_rw_signal(false),
            message: cx.create_rw_signal(String::new()),
        }
    }
    fn create(&self, window: Rc<WindowTabData>) {
        if self.busy.get_untracked() {
            return;
        }
        let (destination, args) = match arguments(
            &self.name.get_untracked(),
            &self.parent.get_untracked(),
            &self.template.get_untracked(),
            &self.platforms.get_untracked(),
            self.empty.get_untracked(),
        ) {
            Ok(v) => v,
            Err(e) => {
                self.message.set(e);
                return;
            }
        };
        let task = window.common.background_tasks.begin("Creating Flutter project", None);
        self.busy.set(true);
        self.message.set("Creating Flutter project…".into());
        let this = self.clone();
        let program =
            crate::dart::sdk::find_flutter_binary(window.workspace.path.as_deref());
        let worker_destination = destination.clone();
        let send = floem::ext_event::create_ext_action(
            window.scope,
            move |result: Result<(), String>| {
                this.busy.set(false);
                match result {
                    Ok(()) => {
                        this.active.set(false);
                        let mut workspace = (*window.workspace).clone();
                        workspace.path = Some(destination.clone());
                        workspace.kind =
                            crate::workspace::SparrowWorkspaceType::Local;
                        window.common.window_common.window_command.send(
                            crate::command::WindowCommand::NewWorkspaceTab {
                                workspace,
                                end: false,
                            },
                        );
                    }
                    Err(e) => this.message.set(e),
                }
            },
        );
        std::thread::spawn(move || {
            let _task = task;
            let result = program
                .ok_or_else(|| "Flutter SDK not found.".to_string())
                .and_then(|program| {
                    std::fs::create_dir(&worker_destination)
                        .map_err(|e| format!("Cannot create destination: {e}"))?;
                    let output = std::process::Command::new(program)
                        .args(args)
                        .output()
                        .map_err(|e| e.to_string())?;
                    if output.status.success() {
                        Ok(())
                    } else {
                        Err(format!(
                            "{}\n{}",
                            String::from_utf8_lossy(&output.stdout),
                            String::from_utf8_lossy(&output.stderr)
                        ))
                    }
                });
            send(result);
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_destinations_and_templates() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().to_str().unwrap();
        assert!(
            arguments("../oops", parent, "app", &["linux".into()], false).is_err()
        );
        assert!(
            !arguments("demo", parent, "package_ffi", &["web".into()], false)
                .unwrap()
                .1
                .contains(&"--platforms".into())
        );
        let (_, args) =
            arguments("demo", parent, "app", &["linux".into()], true).unwrap();
        assert!(args.contains(&"--empty".into()));
        std::fs::create_dir(root.path().join("demo")).unwrap();
        assert!(arguments("demo", parent, "app", &["linux".into()], false).is_err());
    }
}
