//! Flutter & Dart workspace project bootstrapping and package management.
//!
//! Handles:
//! 1. Project Type Classification (`pubspec.yaml` parsing for Flutter vs pure Dart)
//! 2. Automated Package Resolution Check (`.dart_tool/package_config.json` timestamp comparison)
//! 3. Non-blocking automated `pub get` execution with callback and logging.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::SystemTime,
};

use crate::dart::sdk::{find_dart_binary, find_flutter_binary, find_flutter_sdk};

/// Type of Dart / Flutter project detected in a workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectType {
    /// Flutter application or package.
    Flutter {
        name: String,
        pubspec_path: PathBuf,
        dependencies: Vec<String>,
    },
    /// Pure Dart application or package.
    PureDart {
        name: String,
        pubspec_path: PathBuf,
        dependencies: Vec<String>,
    },
    /// No `pubspec.yaml` found or not a Dart workspace.
    None,
}

impl ProjectType {
    pub fn is_flutter(&self) -> bool {
        matches!(self, ProjectType::Flutter { .. })
    }

    pub fn is_dart(&self) -> bool {
        matches!(
            self,
            ProjectType::Flutter { .. } | ProjectType::PureDart { .. }
        )
    }

    pub fn name(&self) -> Option<&str> {
        match self {
            ProjectType::Flutter { name, .. } => Some(name),
            ProjectType::PureDart { name, .. } => Some(name),
            ProjectType::None => None,
        }
    }

    pub fn pubspec_path(&self) -> Option<&Path> {
        match self {
            ProjectType::Flutter { pubspec_path, .. } => Some(pubspec_path),
            ProjectType::PureDart { pubspec_path, .. } => Some(pubspec_path),
            ProjectType::None => None,
        }
    }
}

/// Classifies a workspace directory by inspecting its `pubspec.yaml`.
pub fn classify_project(workspace_root: &Path) -> ProjectType {
    let pubspec_path = workspace_root.join("pubspec.yaml");
    if !pubspec_path.is_file() {
        return ProjectType::None;
    }

    let Ok(content) = fs::read_to_string(&pubspec_path) else {
        return ProjectType::None;
    };

    parse_pubspec(&content, pubspec_path)
}

/// Checks if the workspace root contains a Flutter project.
pub fn is_flutter_project(workspace_root: &Path) -> bool {
    classify_project(workspace_root).is_flutter()
}

/// Checks if the workspace root contains either a Dart or Flutter project.
pub fn is_dart_or_flutter_project(workspace_root: &Path) -> bool {
    classify_project(workspace_root).is_dart()
}

/// Checks if package resolution (`pub get`) is needed.
///
/// Returns `true` if:
/// - `pubspec.yaml` exists AND
/// - `.dart_tool/package_config.json` does NOT exist OR is older than `pubspec.yaml`.
pub fn is_package_resolution_needed(workspace_root: &Path) -> bool {
    let pubspec_path = workspace_root.join("pubspec.yaml");
    if !pubspec_path.is_file() {
        return false;
    }

    let package_config = workspace_root
        .join(".dart_tool")
        .join("package_config.json");

    if !package_config.is_file() {
        return true;
    }

    let pubspec_mtime = get_mtime(&pubspec_path);
    let config_mtime = get_mtime(&package_config);

    match (pubspec_mtime, config_mtime) {
        (Some(p_time), Some(c_time)) => p_time > c_time,
        _ => true,
    }
}

type ResolutionCallback = Box<dyn FnOnce(u64, Result<(), String>) + Send + 'static>;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolutionRequest {
    pub id: u64,
    pub joined: bool,
}
struct ResolutionJob {
    id: u64,
    callbacks: Vec<ResolutionCallback>,
}
#[derive(Default)]
struct ResolutionJobs {
    next: u64,
    running: std::collections::HashMap<PathBuf, ResolutionJob>,
}
impl ResolutionJobs {
    fn register(
        &mut self,
        path: PathBuf,
        callback: Option<ResolutionCallback>,
    ) -> ResolutionRequest {
        if let Some(job) = self.running.get_mut(&path) {
            if let Some(callback) = callback {
                job.callbacks.push(callback);
            }
            return ResolutionRequest {
                id: job.id,
                joined: true,
            };
        }
        self.next += 1;
        let id = self.next;
        self.running.insert(
            path,
            ResolutionJob {
                id,
                callbacks: callback.into_iter().collect(),
            },
        );
        ResolutionRequest { id, joined: false }
    }
    fn finish(&mut self, path: &Path, id: u64) -> Vec<ResolutionCallback> {
        if self.running.get(path).is_some_and(|job| job.id == id) {
            self.running.remove(path).unwrap().callbacks
        } else {
            Vec::new()
        }
    }
}
fn resolution_jobs() -> &'static std::sync::Mutex<ResolutionJobs> {
    static JOBS: std::sync::OnceLock<std::sync::Mutex<ResolutionJobs>> =
        std::sync::OnceLock::new();
    JOBS.get_or_init(Default::default)
}
fn complete_resolution(path: &Path, id: u64, result: Result<(), String>) {
    let callbacks = resolution_jobs()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .finish(path, id);
    // Release the coordinator before notifying callers, which may request a retry.
    for callback in callbacks {
        callback(id, result.clone());
    }
}
/// Run pub get asynchronously; concurrent requests for the same workspace share a process.
pub fn run_pub_get(
    workspace_root: &Path,
    on_complete: Option<Box<dyn FnOnce(Result<(), String>) + Send + 'static>>,
) {
    run_pub_get_observed(
        workspace_root,
        on_complete.map(|callback| {
            Box::new(move |_, result| callback(result)) as ResolutionCallback
        }),
    );
}
pub fn run_pub_get_observed(
    workspace_root: &Path,
    on_complete: Option<ResolutionCallback>,
) -> ResolutionRequest {
    let root = workspace_root.canonicalize().unwrap_or_else(|_| {
        if workspace_root.is_absolute() {
            workspace_root.to_path_buf()
        } else {
            std::env::current_dir()
                .unwrap_or_default()
                .join(workspace_root)
        }
    });
    let request = resolution_jobs()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .register(root.clone(), on_complete);
    if request.joined {
        return request;
    }
    let job_root = root.clone();
    let spawned =
        thread::Builder::new()
            .name("DartPubGet".into())
            .spawn(move || {
                let project = classify_project(&job_root);
                let result = if project.is_dart() {
                    execute_pub_get(&job_root, project.is_flutter())
                } else {
                    Err("Not a Dart/Flutter project (no pubspec.yaml)".into())
                };
                complete_resolution(&job_root, request.id, result);
            });
    if let Err(error) = spawned {
        complete_resolution(
            &root,
            request.id,
            Err(format!("Could not start package resolution: {error}")),
        );
    }
    request
}

/// Bootstrap a workspace: inspect SDK, classify project, and trigger `pub get` if required.
pub fn bootstrap_workspace(workspace_root: &Path) {
    bootstrap_workspace_observed(workspace_root, None);
}
pub(crate) fn bootstrap_workspace_observed(
    workspace_root: &Path,
    on_complete: Option<ResolutionCallback>,
) {
    let project_type = classify_project(workspace_root);
    if !project_type.is_dart() {
        return;
    }

    tracing::info!(
        "Bootstrapping workspace at {}: {:?}",
        workspace_root.display(),
        project_type
    );

    if let Some(flutter_sdk) = find_flutter_sdk(Some(workspace_root)) {
        tracing::info!(
            "Detected Flutter SDK at {}: version {:?}, channel {:?}",
            flutter_sdk.root.display(),
            flutter_sdk.version,
            flutter_sdk.channel
        );
    } else if let Some(dart_bin) = find_dart_binary(Some(workspace_root)) {
        tracing::info!("Detected standalone Dart binary at {}", dart_bin.display());
    } else {
        tracing::warn!(
            "No Flutter or Dart SDK discovered for workspace at {}. Run/LSP features may be limited.",
            workspace_root.display()
        );
    }

    if is_package_resolution_needed(workspace_root) {
        tracing::info!(
            "Dependencies outdated or missing for {}; running background pub get...",
            workspace_root.display()
        );
        run_pub_get_observed(
            workspace_root,
            Some(Box::new(move |id, result| {
                match &result {
                    Ok(()) => tracing::info!("pub get completed successfully."),
                    Err(error) => tracing::warn!("pub get failed: {error}"),
                }
                if let Some(callback) = on_complete {
                    callback(id, result);
                }
            })),
        );
    }
}

// ─────────────────────────────────────────────────────────────────────
//  Internal Helpers
// ─────────────────────────────────────────────────────────────────────

fn execute_pub_get(workspace_root: &Path, is_flutter: bool) -> Result<(), String> {
    let (program, args) = if is_flutter {
        if let Some(flutter_bin) = find_flutter_binary(Some(workspace_root)) {
            (
                flutter_bin.to_string_lossy().to_string(),
                vec!["pub", "get"],
            )
        } else if let Some(dart_bin) = find_dart_binary(Some(workspace_root)) {
            (dart_bin.to_string_lossy().to_string(), vec!["pub", "get"])
        } else {
            ("flutter".to_string(), vec!["pub", "get"])
        }
    } else {
        if let Some(dart_bin) = find_dart_binary(Some(workspace_root)) {
            (dart_bin.to_string_lossy().to_string(), vec!["pub", "get"])
        } else if let Some(flutter_bin) = find_flutter_binary(Some(workspace_root)) {
            (
                flutter_bin.to_string_lossy().to_string(),
                vec!["pub", "get"],
            )
        } else {
            ("dart".to_string(), vec!["pub", "get"])
        }
    };

    let output = Command::new(&program)
        .arg("--suppress-analytics")
        .args(&args)
        .stdin(Stdio::null())
        .current_dir(workspace_root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("failed to spawn `{program} pub get`: {e}"))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(pub_get_failure(&program, &output))
    }
}

fn pub_get_failure(program: &str, output: &std::process::Output) -> String {
    let messages = [
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    ]
    .into_iter()
    .filter(|text| !text.trim().is_empty())
    .map(|text| text.trim().to_string())
    .collect::<Vec<_>>()
    .join("\n");
    format!(
        "`{program} pub get` failed ({}): {}",
        output.status,
        if messages.is_empty() {
            "The resolver returned no output."
        } else {
            &messages
        }
    )
}

fn get_mtime(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Parse `pubspec.yaml` content into `ProjectType`.
fn parse_pubspec(content: &str, pubspec_path: PathBuf) -> ProjectType {
    let mut project_name = String::new();
    let mut is_flutter = false;
    let mut dependencies = Vec::new();

    let mut in_dependencies = false;
    let mut in_dev_dependencies = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }

        // Top level keys (no leading spaces)
        if !line.starts_with(' ') && !line.starts_with('\t') {
            in_dependencies = trimmed.starts_with("dependencies:");
            in_dev_dependencies = trimmed.starts_with("dev_dependencies:");

            if trimmed.starts_with("flutter:") {
                is_flutter = true;
            }

            if let Some(rest) = trimmed.strip_prefix("name:") {
                project_name = rest.trim().trim_matches(['\'', '"']).to_string();
            }
            continue;
        }

        // Nested lines
        if in_dependencies || in_dev_dependencies {
            if let Some((dep_name, _)) = trimmed.split_once(':') {
                let dep = dep_name.trim().trim_matches(['\'', '"']).to_string();
                if dep == "flutter"
                    || dep == "flutter_test"
                    || dep == "flutter_localizations"
                {
                    is_flutter = true;
                }
                if !dependencies.contains(&dep) {
                    dependencies.push(dep);
                }
            }
        }
    }

    if project_name.is_empty() {
        project_name = pubspec_path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("unnamed")
            .to_string();
    }

    if is_flutter {
        ProjectType::Flutter {
            name: project_name,
            pubspec_path,
            dependencies,
        }
    } else {
        ProjectType::PureDart {
            name: project_name,
            pubspec_path,
            dependencies,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_classify_flutter_project() {
        let dir = tempdir().unwrap();
        let pubspec = dir.path().join("pubspec.yaml");
        let content = r#"
name: my_flutter_app
description: A new Flutter project.
environment:
  sdk: '>=3.0.0 <4.0.0'

dependencies:
  flutter:
    sdk: flutter
  cupertino_icons: ^1.0.6
"#;
        fs::write(&pubspec, content).unwrap();

        let pt = classify_project(dir.path());
        assert!(pt.is_flutter());
        assert_eq!(pt.name(), Some("my_flutter_app"));
    }

    #[test]
    fn test_classify_pure_dart_project() {
        let dir = tempdir().unwrap();
        let pubspec = dir.path().join("pubspec.yaml");
        let content = r#"
name: my_dart_cli
description: A pure Dart CLI.
dependencies:
  args: ^2.4.2
  path: ^1.9.0
"#;
        fs::write(&pubspec, content).unwrap();

        let pt = classify_project(dir.path());
        assert!(!pt.is_flutter());
        assert!(pt.is_dart());
        assert_eq!(pt.name(), Some("my_dart_cli"));
    }

    #[test]
    fn test_classify_flutter_with_flutter_section() {
        let dir = tempdir().unwrap();
        let pubspec = dir.path().join("pubspec.yaml");
        let content = r#"
name: custom_flutter_app
flutter:
  uses-material-design: true
"#;
        fs::write(&pubspec, content).unwrap();

        let pt = classify_project(dir.path());
        assert!(pt.is_flutter());
        assert!(is_flutter_project(dir.path()));
        assert!(is_dart_or_flutter_project(dir.path()));
        assert_eq!(pt.name(), Some("custom_flutter_app"));
        assert_eq!(pt.pubspec_path(), Some(pubspec.as_path()));
    }

    #[test]
    fn test_classify_flutter_with_dev_dependency() {
        let dir = tempdir().unwrap();
        let pubspec = dir.path().join("pubspec.yaml");
        let content = r#"
name: plugin_test
dev_dependencies:
  flutter_test:
    sdk: flutter
"#;
        fs::write(&pubspec, content).unwrap();

        let pt = classify_project(dir.path());
        assert!(pt.is_flutter());
    }

    #[test]
    fn test_classify_non_dart_project() {
        let dir = tempdir().unwrap();
        let pt = classify_project(dir.path());
        assert_eq!(pt, ProjectType::None);
        assert!(!pt.is_flutter());
        assert!(!pt.is_dart());
        assert_eq!(pt.name(), None);
        assert_eq!(pt.pubspec_path(), None);
        assert!(!is_package_resolution_needed(dir.path()));
    }

    #[test]
    fn test_package_resolution_needed_when_missing() {
        let dir = tempdir().unwrap();
        let pubspec = dir.path().join("pubspec.yaml");
        fs::write(&pubspec, "name: test_app\n").unwrap();

        assert!(is_package_resolution_needed(dir.path()));
    }

    #[test]
    fn test_package_resolution_not_needed_when_fresh() {
        let dir = tempdir().unwrap();
        let pubspec = dir.path().join("pubspec.yaml");
        fs::write(&pubspec, "name: test_app\n").unwrap();

        let dart_tool = dir.path().join(".dart_tool");
        fs::create_dir_all(&dart_tool).unwrap();
        let pkg_cfg = dart_tool.join("package_config.json");
        // Ensure timestamp is later
        std::thread::sleep(std::time::Duration::from_millis(50));
        fs::write(&pkg_cfg, "{}").unwrap();

        assert!(!is_package_resolution_needed(dir.path()));
    }
}

#[cfg(test)]
mod live_pub_tests {
    use super::*;
    #[test]
    #[ignore = "requires SPARROW_TEST_DART_SDK; resolves a real local path package"]
    fn resolves_real_local_dependency_without_home_writes() {
        let sdk = std::env::var("SPARROW_TEST_DART_SDK")
            .expect("set SPARROW_TEST_DART_SDK");
        let root = tempfile::Builder::new()
            .prefix("sparrow-pub-")
            .tempdir()
            .unwrap();
        fs::create_dir(root.path().join(".sparrow")).unwrap();
        fs::create_dir(root.path().join("local")).unwrap();
        fs::write(
            root.path().join(".sparrow/settings.toml"),
            format!("[core]\ndart-sdk-path = {:?}\n", sdk),
        )
        .unwrap();
        fs::write(
            root.path().join("local/pubspec.yaml"),
            "name: local_probe\nenvironment:\n  sdk: '>=3.0.0 <4.0.0'\n",
        )
        .unwrap();
        fs::write(root.path().join("pubspec.yaml"),"name: pub_probe\nenvironment:\n  sdk: '>=3.0.0 <4.0.0'\ndependencies:\n  local_probe:\n    path: local\n").unwrap();
        execute_pub_get(root.path(), false).unwrap();
        let config: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(root.path().join(".dart_tool/package_config.json"))
                .unwrap(),
        )
        .unwrap();
        assert!(
            config["packages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|package| package["name"] == "local_probe")
        );
        assert!(root.path().join("pubspec.lock").is_file());
    }
}

#[cfg(all(test, unix))]
mod failure_output_tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    #[test]
    fn retains_solver_stdout_stderr_and_failure_status() {
        let output = std::process::Output {
            status: std::process::ExitStatus::from_raw(65 << 8),
            stdout: b"Version solving failed.\n".to_vec(),
            stderr: b"Dependency local_probe does not exist.\n".to_vec(),
        };
        let message = pub_get_failure("dart", &output);
        assert!(message.contains("65"));
        assert!(message.contains("Version solving failed."));
        assert!(message.contains("local_probe does not exist."));
    }
}

#[cfg(all(test, unix))]
mod coordination_tests {
    use super::*;
    use std::{
        os::unix::fs::{PermissionsExt, symlink},
        sync::mpsc,
        time::Duration,
    };
    #[test]
    fn concurrent_alias_requests_share_one_process_and_failure_can_be_retried() {
        let root = tempfile::Builder::new()
            .prefix("pub-coordination-")
            .tempdir()
            .unwrap();
        let project = root.path().join("project");
        let sdk = root.path().join("sdk");
        fs::create_dir_all(project.join(".sparrow")).unwrap();
        fs::create_dir_all(sdk.join("bin")).unwrap();
        fs::write(project.join("pubspec.yaml"), "name: coordination_probe\n")
            .unwrap();
        fs::write(
            project.join(".sparrow/settings.toml"),
            format!("[core]\ndart-sdk-path = {:?}\n", sdk.to_string_lossy()),
        )
        .unwrap();
        let executable = sdk.join("bin/dart");
        fs::write(&executable,r#"#!/bin/sh
if [ "$1" = "--version" ]; then echo 'Dart SDK version: 3.0.0'; exit 0; fi
base=$(dirname "$0")
printf 'run\n' >> "$base/calls"
i=0
while [ ! -e "$base/release" ]; do i=$((i+1)); if [ "$i" -gt 500 ]; then exit 9; fi; sleep 0.01; done
if [ -e "$base/fail" ]; then echo 'solver failure' >&2; exit 23; fi
exit 0
"#).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(sdk.join("bin/fail"), "").unwrap();
        let alias = root.path().join("alias");
        symlink(&project, &alias).unwrap();
        let (tx, rx) = mpsc::channel();
        let first_tx = tx.clone();
        let first = run_pub_get_observed(
            &project,
            Some(Box::new(move |id, result| {
                first_tx.send((id, result)).unwrap()
            })),
        );
        let joined = run_pub_get_observed(
            &alias,
            Some(Box::new(move |id, result| tx.send((id, result)).unwrap())),
        );
        assert!(!first.joined);
        assert!(joined.joined);
        assert_eq!(first.id, joined.id);
        fs::write(sdk.join("bin/release"), "").unwrap();
        for _ in 0..2 {
            let (id, result) = rx.recv_timeout(Duration::from_secs(10)).unwrap();
            assert_eq!(id, first.id);
            assert!(result.unwrap_err().contains("solver failure"));
        }
        assert_eq!(
            fs::read_to_string(sdk.join("bin/calls"))
                .unwrap()
                .lines()
                .count(),
            1
        );
        fs::remove_file(sdk.join("bin/fail")).unwrap();
        let (tx, rx) = mpsc::channel();
        let retry = run_pub_get_observed(
            &project,
            Some(Box::new(move |id, result| tx.send((id, result)).unwrap())),
        );
        assert!(!retry.joined);
        assert_ne!(retry.id, first.id);
        assert!(rx.recv_timeout(Duration::from_secs(10)).unwrap().1.is_ok());
        assert_eq!(
            fs::read_to_string(sdk.join("bin/calls"))
                .unwrap()
                .lines()
                .count(),
            2
        );
    }
    #[test]
    fn late_completion_cannot_remove_a_new_request() {
        let mut jobs = ResolutionJobs::default();
        let path = PathBuf::from("project");
        let old = jobs.register(path.clone(), None);
        jobs.finish(&path, old.id);
        let new = jobs.register(path.clone(), None);
        assert_ne!(old.id, new.id);
        jobs.finish(&path, old.id);
        assert_eq!(jobs.running[&path].id, new.id);
        let other = jobs.register(PathBuf::from("other"), None);
        assert!(!other.joined);
    }
}
