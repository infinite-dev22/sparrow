//! Multi-tiered Flutter & Dart SDK discovery pipeline.
//!
//! Hierarchical evaluation:
//! 1. Active Environment Variables (`FLUTTER_ROOT`, `FLUTTER_HOME`, `DART_SDK`, `DART_HOME`)
//! 2. Project-Local Version Managers (FVM in workspace or global, asdf, mise, pub-cache)
//! 3. Platform-Specific Standard Installation Paths (Linux, macOS, Windows)
//! 4. System PATH Fallback (`which flutter` / `where flutter.bat`, `which dart`)

use std::{
    env,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// Information about a discovered Flutter SDK.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlutterSdk {
    /// Full path to the `flutter` binary.
    pub flutter_bin: PathBuf,
    /// Full path to the bundled `dart` binary inside this Flutter SDK.
    pub dart_bin: PathBuf,
    /// Root directory of the Flutter SDK installation.
    pub root: PathBuf,
    /// Flutter version string (e.g. "3.24.0"), if queryable.
    pub version: Option<String>,
    /// Release channel (e.g. "stable", "beta", "master"), if queryable.
    pub channel: Option<String>,
}

/// Information about a discovered Dart SDK (standalone or bundled).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartSdk {
    /// Full path to the `dart` binary.
    pub dart_bin: PathBuf,
    /// Root directory of the Dart SDK, if known.
    pub root: Option<PathBuf>,
    /// Dart SDK version string (e.g. "3.5.0"), if queryable.
    pub version: Option<String>,
    /// Whether this Dart SDK comes from a Flutter installation.
    pub is_from_flutter: bool,
}

/// Find the optimal Flutter SDK for a given workspace root (or globally).
pub fn find_flutter_sdk(workspace_root: Option<&Path>) -> Option<FlutterSdk> {
    // 1. Environment variables
    if let Some(sdk) = check_flutter_env_vars() {
        return Some(sdk);
    }

    // 2. Project-local version managers (FVM, asdf, mise)
    if let Some(sdk) = check_flutter_version_managers(workspace_root) {
        return Some(sdk);
    }

    // 3. Platform-specific standard installation paths
    if let Some(sdk) = check_flutter_standard_paths() {
        return Some(sdk);
    }

    // 4. System PATH fallback
    if let Some(sdk) = check_flutter_from_path() {
        return Some(sdk);
    }

    None
}

/// Find the optimal Dart SDK (standalone or from Flutter) for a given workspace root.
pub fn find_dart_sdk(workspace_root: Option<&Path>) -> Option<DartSdk> {
    // If a Flutter SDK is found, prefer its bundled Dart SDK for Flutter consistency
    if let Some(flutter_sdk) = find_flutter_sdk(workspace_root) {
        let version = query_dart_version(&flutter_sdk.dart_bin);
        return Some(DartSdk {
            dart_bin: flutter_sdk.dart_bin.clone(),
            root: flutter_sdk.root.join("bin").join("cache").join("dart-sdk").into(),
            version,
            is_from_flutter: true,
        });
    }

    // 1. Environment variables (DART_SDK, DART_HOME)
    if let Some(sdk) = check_dart_env_vars() {
        return Some(sdk);
    }

    // 2. Version managers (asdf, mise)
    if let Some(sdk) = check_dart_version_managers(workspace_root) {
        return Some(sdk);
    }

    // 3. Platform-specific standard paths
    if let Some(sdk) = check_dart_standard_paths() {
        return Some(sdk);
    }

    // 4. System PATH fallback
    if let Some(sdk) = check_dart_from_path() {
        return Some(sdk);
    }

    None
}

/// Find the `dart` executable path for a workspace.
pub fn find_dart_binary(workspace_root: Option<&Path>) -> Option<PathBuf> {
    find_dart_sdk(workspace_root).map(|sdk| sdk.dart_bin)
}

/// Find the `flutter` executable path for a workspace.
pub fn find_flutter_binary(workspace_root: Option<&Path>) -> Option<PathBuf> {
    find_flutter_sdk(workspace_root).map(|sdk| sdk.flutter_bin)
}

/// Checks if Dart is available in the current environment or workspace.
pub fn is_dart_available(workspace_root: Option<&Path>) -> bool {
    find_dart_binary(workspace_root)
        .map(|bin| verify_executable(&bin, &["--version"]))
        .unwrap_or(false)
}

/// Checks if Flutter is available in the current environment or workspace.
pub fn is_flutter_available(workspace_root: Option<&Path>) -> bool {
    find_flutter_binary(workspace_root)
        .map(|bin| verify_executable(&bin, &["--version"]))
        .unwrap_or(false)
}

// ─────────────────────────────────────────────────────────────────────
//  Tier 1: Environment Variables
// ─────────────────────────────────────────────────────────────────────

fn check_flutter_env_vars() -> Option<FlutterSdk> {
    for var_name in &["FLUTTER_ROOT", "FLUTTER_HOME"] {
        if let Ok(val) = env::var(var_name) {
            let root = PathBuf::from(val.trim());
            if let Some(sdk) = build_flutter_sdk_from_root(&root) {
                return Some(sdk);
            }
        }
    }
    None
}

fn check_dart_env_vars() -> Option<DartSdk> {
    for var_name in &["DART_SDK", "DART_HOME"] {
        if let Ok(val) = env::var(var_name) {
            let root = PathBuf::from(val.trim());
            let bin = resolve_binary(&root.join("bin"), "dart");
            if let Some(dart_bin) = bin {
                let version = query_dart_version(&dart_bin);
                return Some(DartSdk {
                    dart_bin,
                    root: Some(root),
                    version,
                    is_from_flutter: false,
                });
            }
        }
    }
    None
}

// ─────────────────────────────────────────────────────────────────────
//  Tier 2: Version Managers (FVM, asdf, mise, pub-cache)
// ─────────────────────────────────────────────────────────────────────

fn check_flutter_version_managers(workspace_root: Option<&Path>) -> Option<FlutterSdk> {
    // 2a. Workspace-local FVM: `.fvm/flutter_sdk`
    if let Some(root) = workspace_root {
        let local_fvm_sdk = root.join(".fvm").join("flutter_sdk");
        if let Some(sdk) = build_flutter_sdk_from_root(&local_fvm_sdk) {
            return Some(sdk);
        }
    }

    // 2b. Global FVM default: `~/.fvm/default`
    if let Some(home) = home_directory() {
        let global_fvm_default = home.join(".fvm").join("default");
        if let Some(sdk) = build_flutter_sdk_from_root(&global_fvm_default) {
            return Some(sdk);
        }
    }

    // 2c. asdf / mise shims
    if let Some(home) = home_directory() {
        let candidates = [
            home.join(".asdf").join("shims"),
            home.join(".local").join("share").join("mise").join("shims"),
            home.join(".mise").join("shims"),
        ];
        for dir in candidates {
            if let Some(flutter_bin) = resolve_binary(&dir, "flutter") {
                if verify_executable(&flutter_bin, &["--version"]) {
                    let root = flutter_bin
                        .parent()
                        .and_then(|p| p.parent())
                        .unwrap_or(&dir)
                        .to_path_buf();
                    let dart_bin = resolve_dart_from_flutter_root(&root)
                        .unwrap_or_else(|| resolve_binary(&dir, "dart").unwrap_or_else(|| flutter_bin.clone()));
                    let (version, channel) = query_flutter_info(&flutter_bin);
                    return Some(FlutterSdk {
                        flutter_bin,
                        dart_bin,
                        root,
                        version,
                        channel,
                    });
                }
            }
        }
    }

    None
}

fn check_dart_version_managers(_workspace_root: Option<&Path>) -> Option<DartSdk> {
    if let Some(home) = home_directory() {
        let candidates = [
            home.join(".asdf").join("shims"),
            home.join(".local").join("share").join("mise").join("shims"),
            home.join(".pub-cache").join("bin"),
        ];
        for dir in candidates {
            if let Some(dart_bin) = resolve_binary(&dir, "dart") {
                if verify_executable(&dart_bin, &["--version"]) {
                    let version = query_dart_version(&dart_bin);
                    let root = dart_bin.parent().and_then(|p| p.parent()).map(|p| p.to_path_buf());
                    return Some(DartSdk {
                        dart_bin,
                        root,
                        version,
                        is_from_flutter: false,
                    });
                }
            }
        }
    }
    None
}

// ─────────────────────────────────────────────────────────────────────
//  Tier 3: Platform-Specific Standard Installation Paths
// ─────────────────────────────────────────────────────────────────────

fn check_flutter_standard_paths() -> Option<FlutterSdk> {
    let mut candidate_roots = Vec::new();

    if let Some(home) = home_directory() {
        candidate_roots.push(home.join("flutter"));
        candidate_roots.push(home.join("development").join("flutter"));
        candidate_roots.push(home.join("Developer").join("flutter"));
        candidate_roots.push(home.join(".local").join("share").join("flutter"));
        candidate_roots.push(home.join("lib").join("flutter"));
        candidate_roots.push(home.join("src").join("flutter"));
        candidate_roots.push(home.join("scoop").join("apps").join("flutter").join("current"));
    }

    // Linux & Unix paths
    candidate_roots.push(PathBuf::from("/opt/flutter"));
    candidate_roots.push(PathBuf::from("/usr/local/flutter"));
    candidate_roots.push(PathBuf::from("/snap/flutter/current/usr"));
    candidate_roots.push(PathBuf::from("/snap/flutter/current"));

    // macOS Homebrew paths
    candidate_roots.push(PathBuf::from("/opt/homebrew/Caskroom/flutter/current/flutter"));
    candidate_roots.push(PathBuf::from("/usr/local/Caskroom/flutter/current/flutter"));

    // Windows paths
    if cfg!(windows) {
        candidate_roots.push(PathBuf::from(r"C:\src\flutter"));
        candidate_roots.push(PathBuf::from(r"C:\flutter"));
        candidate_roots.push(PathBuf::from(r"C:\tools\flutter"));
        if let Ok(appdata) = env::var("LOCALAPPDATA") {
            candidate_roots.push(PathBuf::from(appdata).join("Flutter"));
        }
        if let Ok(prog_files) = env::var("ProgramFiles") {
            candidate_roots.push(PathBuf::from(prog_files).join("Flutter"));
        }
        if let Ok(choco) = env::var("ChocolateyInstall") {
            candidate_roots.push(PathBuf::from(choco).join("lib").join("flutter"));
        }
    }

    for root in candidate_roots {
        if let Some(sdk) = build_flutter_sdk_from_root(&root) {
            return Some(sdk);
        }
    }

    None
}

fn check_dart_standard_paths() -> Option<DartSdk> {
    let mut candidate_roots = Vec::new();

    if let Some(home) = home_directory() {
        candidate_roots.push(home.join("dart-sdk"));
        candidate_roots.push(home.join(".dart-sdk"));
        candidate_roots.push(home.join("scoop").join("apps").join("dart-sdk").join("current"));
    }

    // Unix / macOS paths
    candidate_roots.push(PathBuf::from("/usr/lib/dart"));
    candidate_roots.push(PathBuf::from("/usr/local/dart-sdk"));
    candidate_roots.push(PathBuf::from("/opt/dart-sdk"));
    candidate_roots.push(PathBuf::from("/opt/homebrew/opt/dart/libexec"));
    candidate_roots.push(PathBuf::from("/usr/local/opt/dart/libexec"));

    // Windows paths
    if cfg!(windows) {
        candidate_roots.push(PathBuf::from(r"C:\tools\dart-sdk"));
        candidate_roots.push(PathBuf::from(r"C:\Program Files\Dart\dart-sdk"));
        if let Ok(choco) = env::var("ChocolateyInstall") {
            candidate_roots.push(PathBuf::from(choco).join("lib").join("dart-sdk"));
        }
    }

    for root in candidate_roots {
        let bin = resolve_binary(&root.join("bin"), "dart");
        if let Some(dart_bin) = bin {
            if verify_executable(&dart_bin, &["--version"]) {
                let version = query_dart_version(&dart_bin);
                return Some(DartSdk {
                    dart_bin,
                    root: Some(root),
                    version,
                    is_from_flutter: false,
                });
            }
        }
    }

    None
}

// ─────────────────────────────────────────────────────────────────────
//  Tier 4: System PATH Fallback
// ─────────────────────────────────────────────────────────────────────

fn check_flutter_from_path() -> Option<FlutterSdk> {
    let flutter_bin = find_in_path("flutter")?;
    // Resolve symlinks if any
    let real_flutter_bin = std::fs::canonicalize(&flutter_bin).unwrap_or(flutter_bin);

    // If binary is in `<root>/bin/flutter`, root is `<root>`
    let root = real_flutter_bin
        .parent()
        .and_then(|p| {
            if p.file_name().and_then(|n| n.to_str()) == Some("bin") {
                p.parent()
            } else {
                Some(p)
            }
        })
        .unwrap_or_else(|| Path::new("/"))
        .to_path_buf();

    let dart_bin = resolve_dart_from_flutter_root(&root)
        .or_else(|| find_in_path("dart"))
        .unwrap_or_else(|| real_flutter_bin.clone());

    let (version, channel) = query_flutter_info(&real_flutter_bin);

    Some(FlutterSdk {
        flutter_bin: real_flutter_bin,
        dart_bin,
        root,
        version,
        channel,
    })
}

fn check_dart_from_path() -> Option<DartSdk> {
    let dart_bin = find_in_path("dart")?;
    let real_dart_bin = std::fs::canonicalize(&dart_bin).unwrap_or(dart_bin);

    let is_from_flutter = real_dart_bin.to_string_lossy().contains("flutter")
        || real_dart_bin.to_string_lossy().contains("cache");

    let root = real_dart_bin
        .parent()
        .and_then(|p| {
            if p.file_name().and_then(|n| n.to_str()) == Some("bin") {
                p.parent()
            } else {
                Some(p)
            }
        })
        .map(|p| p.to_path_buf());

    let version = query_dart_version(&real_dart_bin);

    Some(DartSdk {
        dart_bin: real_dart_bin,
        root,
        version,
        is_from_flutter,
    })
}

// ─────────────────────────────────────────────────────────────────────
//  Helper Functions
// ─────────────────────────────────────────────────────────────────────

/// Builds a `FlutterSdk` struct if the given directory contains a valid Flutter installation.
pub fn build_flutter_sdk_from_root(root: &Path) -> Option<FlutterSdk> {
    let bin_dir = root.join("bin");
    let flutter_bin = resolve_binary(&bin_dir, "flutter")?;
    let dart_bin = resolve_dart_from_flutter_root(root)
        .or_else(|| resolve_binary(&bin_dir, "dart"))?;

    let (version, channel) = query_flutter_info(&flutter_bin);

    Some(FlutterSdk {
        flutter_bin,
        dart_bin,
        root: root.to_path_buf(),
        version,
        channel,
    })
}

/// Locate bundled Dart inside a Flutter SDK root (`bin/cache/dart-sdk/bin/dart`).
fn resolve_dart_from_flutter_root(root: &Path) -> Option<PathBuf> {
    let cache_dart_dir = root.join("bin").join("cache").join("dart-sdk").join("bin");
    resolve_binary(&cache_dart_dir, "dart")
}

/// Resolves an executable binary name in a directory, checking Windows extensions if needed.
fn resolve_binary(dir: &Path, name: &str) -> Option<PathBuf> {
    if !dir.is_dir() {
        return None;
    }

    let candidates = if cfg!(windows) {
        vec![
            format!("{name}.bat"),
            format!("{name}.exe"),
            format!("{name}.cmd"),
            name.to_string(),
        ]
    } else {
        vec![name.to_string()]
    };

    for candidate in candidates {
        let path = dir.join(&candidate);
        if path.is_file() {
            return Some(path);
        }
    }

    None
}

/// Search for a binary name in system `PATH`.
pub fn find_in_path(binary_name: &str) -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    for dir in env::split_paths(&path_var) {
        if let Some(bin) = resolve_binary(&dir, binary_name) {
            return Some(bin);
        }
    }
    None
}

/// Get the current user's home directory.
fn home_directory() -> Option<PathBuf> {
    directories::BaseDirs::new()
        .map(|b| b.home_dir().to_path_buf())
        .or_else(|| env::var("HOME").ok().map(PathBuf::from))
        .or_else(|| env::var("USERPROFILE").ok().map(PathBuf::from))
}

/// Quickly check if an executable can run with given arguments.
fn verify_executable(bin: &Path, args: &[&str]) -> bool {
    Command::new(bin)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Query Dart SDK version string from `dart --version`.
/// Metadata is optional: a locked or updating SDK must not block tool launches.
fn version_output(program: &Path, args: &[&str]) -> Option<std::process::Output> {
    let mut child = Command::new(program).args(args).stdin(Stdio::null())
        .stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().ok()?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().ok(),
            Ok(None) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(20)),
            _ => { let _ = child.kill(); let _ = child.wait(); return None; }
        }
    }
}

fn query_dart_version(dart_bin: &Path) -> Option<String> {
    let output = version_output(dart_bin, &["--version"])?;

    let text = if !output.stdout.is_empty() {
        String::from_utf8_lossy(&output.stdout).to_string()
    } else {
        String::from_utf8_lossy(&output.stderr).to_string()
    };

    // Output is typically: "Dart SDK version: 3.5.0 (stable) (Tue Jul 30 02:17:59 2024 +0000) on 'linux_x64'"
    if let Some(start) = text.find("version: ") {
        let rest = &text[start + "version: ".len()..];
        let version = rest.split_whitespace().next().unwrap_or("").trim();
        if !version.is_empty() {
            return Some(version.to_string());
        }
    }
    None
}

/// Query Flutter version and channel from `flutter --version --machine` or `flutter --version`.
fn query_flutter_info(flutter_bin: &Path) -> (Option<String>, Option<String>) {
    let output = version_output(flutter_bin, &["--version", "--machine"]);

    if let Some(out) = output {
        if out.status.success() {
            if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&out.stdout) {
                let version = json.get("frameworkVersion")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let channel = json.get("channel")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                return (version, channel);
            }
        }
    }

    (None, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use tempfile::tempdir;

    #[test]
    fn test_build_flutter_sdk_from_root() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        let bin_dir = root.join("bin");
        let cache_dart_dir = bin_dir.join("cache").join("dart-sdk").join("bin");
        fs::create_dir_all(&cache_dart_dir).unwrap();

        let flutter_file = if cfg!(windows) { "flutter.bat" } else { "flutter" };
        let dart_file = if cfg!(windows) { "dart.exe" } else { "dart" };

        File::create(bin_dir.join(flutter_file)).unwrap();
        File::create(cache_dart_dir.join(dart_file)).unwrap();

        let sdk = build_flutter_sdk_from_root(root);
        assert!(sdk.is_some());
        let sdk = sdk.unwrap();
        assert_eq!(sdk.root, root);
        assert!(sdk.flutter_bin.ends_with(flutter_file));
        assert!(sdk.dart_bin.ends_with(dart_file));
    }

    #[test]
    fn test_build_flutter_sdk_missing_binaries() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let sdk = build_flutter_sdk_from_root(root);
        assert!(sdk.is_none());
    }

    #[test]
    fn test_fvm_workspace_discovery() {
        let dir = tempdir().unwrap();
        let ws = dir.path();
        let fvm_sdk = ws.join(".fvm").join("flutter_sdk");
        let bin_dir = fvm_sdk.join("bin");
        let cache_dart_dir = bin_dir.join("cache").join("dart-sdk").join("bin");
        fs::create_dir_all(&cache_dart_dir).unwrap();

        let flutter_file = if cfg!(windows) { "flutter.bat" } else { "flutter" };
        let dart_file = if cfg!(windows) { "dart.exe" } else { "dart" };

        File::create(bin_dir.join(flutter_file)).unwrap();
        File::create(cache_dart_dir.join(dart_file)).unwrap();

        let sdk = check_flutter_version_managers(Some(ws));
        assert!(sdk.is_some());
        assert_eq!(sdk.unwrap().root, fvm_sdk);
    }

    #[test]
    fn test_query_dart_version_parser() {
        let sample = "Dart SDK version: 3.5.2 (stable) (Tue Aug 27 10:11:12 2024 +0000) on 'linux_x64'";
        let start = sample.find("version: ").unwrap();
        let rest = &sample[start + "version: ".len()..];
        let version = rest.split_whitespace().next().unwrap();
        assert_eq!(version, "3.5.2");
    }

    #[test]
    fn test_resolve_binary() {
        let dir = tempdir().unwrap();
        let bin_name = if cfg!(windows) { "test_bin.bat" } else { "test_bin" };
        let file_path = dir.path().join(bin_name);
        File::create(&file_path).unwrap();

        let resolved = resolve_binary(dir.path(), "test_bin");
        assert!(resolved.is_some());
        assert_eq!(resolved.unwrap(), file_path);

        let missing = resolve_binary(dir.path(), "non_existent");
        assert!(missing.is_none());
    }
}
