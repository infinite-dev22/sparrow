//! Real Pub solving on a bounded, isolated manifest graph. No source workspace
//! files are written and every proposed repair is solved again before exposure.
use super::{constraint::Constraint, is_pubspec};
use crate::doc::yaml_schema::{self, Part};
use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, Diagnostic, DiagnosticSeverity,
    Range, TextEdit, Url, WorkspaceEdit,
};
use serde::{Deserialize, Serialize};
use serde_yaml::Value as Yaml;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    path::{Component, Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, LazyLock, Mutex},
    time::{Duration, Instant},
};

const LIMIT: usize = 128;
const TIMEOUT: Duration = Duration::from_secs(90);
#[derive(Clone, Serialize, Deserialize)]
struct Input {
    path: PathBuf,
    hash: Option<String>,
}
fn hash(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}
fn read(path: &Path) -> Result<Option<String>, String> {
    match fs::metadata(path) {
        Ok(m) if m.len() > 2 * 1024 * 1024 => {
            Err(format!("Manifest is too large: {}", path.display()))
        }
        Ok(_) => fs::read_to_string(path)
            .map(Some)
            .map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}
impl Input {
    fn unchanged(&self) -> bool {
        read(&self.path).is_ok_and(|text| text.as_deref().map(hash) == self.hash)
    }
}
struct Snapshot {
    temp: tempfile::TempDir,
    root: PathBuf,
    manifests: BTreeMap<PathBuf, String>,
    targets: BTreeSet<PathBuf>,
    inputs: Vec<Input>,
}
fn mirror(base: &Path, path: &Path) -> PathBuf {
    let mut out = base.join("tree");
    for c in path.components() {
        match c {
            Component::Normal(v) => out.push(v),
            Component::Prefix(p) => {
                out.push(p.as_os_str().to_string_lossy().replace([':', '\\'], "_"))
            }
            _ => {}
        }
    }
    out
}
fn workspace_members(root: &Path, doc: &Yaml) -> Result<Vec<PathBuf>, String> {
    let Some(members) = doc.get("workspace").and_then(Yaml::as_sequence) else {
        return Ok(vec![]);
    };
    let mut out = vec![];
    for member in members {
        let member = member
            .as_str()
            .ok_or("Workspace entries must be directory paths")?;
        if member.contains(['*', '?', '[']) {
            let matcher = globset::GlobBuilder::new(member)
                .literal_separator(true)
                .build()
                .map_err(|e| e.to_string())?
                .compile_matcher();
            let mut pending = vec![(root.to_path_buf(), 0)];
            let max_depth = if member.contains("**") {
                12
            } else {
                member.split('/').count()
            };
            let mut visited = 0;
            while let Some((dir, depth)) = pending.pop() {
                visited += 1;
                if visited > 16_384 || out.len() > LIMIT {
                    return Err("Workspace is too large to check safely".into());
                }
                if depth >= max_depth {
                    continue;
                }
                for entry in fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten()
                {
                    if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                        continue;
                    }
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    if name.starts_with('.')
                        || matches!(
                            name.as_ref(),
                            "build" | "target" | "node_modules"
                        )
                    {
                        continue;
                    }
                    let path = entry.path();
                    if matcher.is_match(path.strip_prefix(root).unwrap())
                        && path.join("pubspec.yaml").is_file()
                    {
                        out.push(path.clone());
                    }
                    pending.push((path, depth + 1));
                }
            }
        } else {
            out.push(root.join(member));
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}
impl Snapshot {
    fn new(selected: &Path, source: &str) -> Result<Self, String> {
        let selected = selected.canonicalize().map_err(|e| e.to_string())?;
        let selected_dir = selected
            .parent()
            .ok_or("Pubspec has no package directory")?;
        let doc: Yaml = serde_yaml::from_str(source).map_err(|e| e.to_string())?;
        let overridden_resolution =
            read(&selected_dir.join("pubspec_overrides.yaml"))?
                .and_then(|s| serde_yaml::from_str::<Yaml>(&s).ok())
                .and_then(|v| {
                    v.get("resolution")
                        .and_then(Yaml::as_str)
                        .map(str::to_owned)
                });
        let root = if overridden_resolution
            .as_deref()
            .or_else(|| doc.get("resolution").and_then(Yaml::as_str))
            == Some("workspace")
        {
            selected_dir
                .ancestors()
                .skip(1)
                .take(16)
                .find(|dir| {
                    read(&dir.join("pubspec.yaml"))
                        .ok()
                        .flatten()
                        .and_then(|s| serde_yaml::from_str::<Yaml>(&s).ok())
                        .is_some_and(|v| v.get("workspace").is_some())
                })
                .ok_or("Cannot find the parent Pub workspace manifest")?
                .to_path_buf()
        } else {
            selected_dir.into()
        };
        let mut snapshot = Self {
            temp: tempfile::tempdir().map_err(|e| e.to_string())?,
            root: root.clone(),
            manifests: BTreeMap::new(),
            targets: BTreeSet::new(),
            inputs: vec![],
        };
        let mut pending = vec![(root, true)];
        let mut total = 0;
        while let Some((dir, target)) = pending.pop() {
            let dir = dir.canonicalize().map_err(|e| {
                format!("Cannot read package {}: {e}", dir.display())
            })?;
            let path = dir.join("pubspec.yaml");
            if target {
                snapshot.targets.insert(path.clone());
            }
            if snapshot.manifests.contains_key(&path) {
                continue;
            }
            if snapshot.manifests.len() >= LIMIT {
                return Err("Package graph exceeds 128 local manifests".into());
            }
            let on_disk = read(&path)?;
            let text = if path == selected {
                source.to_string()
            } else {
                on_disk
                    .clone()
                    .ok_or_else(|| format!("Missing manifest: {}", path.display()))?
            };
            total += text.len();
            if total > 8 * 1024 * 1024 {
                return Err("Package manifest graph exceeds 8 MiB".into());
            }
            if path != selected {
                snapshot.inputs.push(Input {
                    path: path.clone(),
                    hash: on_disk.as_deref().map(hash),
                });
            }
            let value: Yaml = serde_yaml::from_str(&text)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            for member in workspace_members(&dir, &value)? {
                pending.push((member, target));
            }
            snapshot.manifests.insert(path.clone(), text.clone());
            let override_path = dir.join("pubspec_overrides.yaml");
            let overrides = read(&override_path)?;
            total += overrides.as_ref().map_or(0, String::len);
            if total > 8 * 1024 * 1024 {
                return Err("Package manifest graph exceeds 8 MiB".into());
            }
            snapshot.inputs.push(Input {
                path: override_path.clone(),
                hash: overrides.as_deref().map(hash),
            });
            for input in std::iter::once(text.as_str()).chain(overrides.as_deref()) {
                let value: Yaml =
                    serde_yaml::from_str(input).map_err(|e| e.to_string())?;
                for section in
                    ["dependencies", "dev_dependencies", "dependency_overrides"]
                {
                    if let Some(deps) = value.get(section).and_then(Yaml::as_mapping)
                    {
                        for dep in deps.values() {
                            if let Some(path) =
                                dep.get("path").and_then(Yaml::as_str)
                            {
                                pending.push((dir.join(path), false));
                            }
                        }
                    }
                }
                if override_path.is_file() {
                    for member in workspace_members(&dir, &value)? {
                        pending.push((member, target));
                    }
                }
            }
        }
        if !snapshot.manifests.contains_key(&selected) {
            return Err(
                "The edited package is not a member of the parent Pub workspace"
                    .into(),
            );
        }
        let originals = snapshot.manifests.clone();
        for (path, text) in originals {
            snapshot.write_manifest(&path, &text)?;
            let overrides = path.with_file_name("pubspec_overrides.yaml");
            if let Some(text) = read(&overrides)? {
                snapshot.write_manifest(&overrides, &text)?;
            }
        }
        for directory in snapshot.manifests.keys().filter_map(|p| p.parent()) {
            let settings = directory.join(".sparrow/settings.toml");
            snapshot.inputs.push(Input {
                hash: read(&settings)?.as_deref().map(hash),
                path: settings,
            });
        }
        if let Some(settings) = crate::config::SparrowConfig::settings_file() {
            snapshot.inputs.push(Input {
                hash: read(&settings)?.as_deref().map(hash),
                path: settings,
            });
        }
        let lock = snapshot.root.join("pubspec.lock");
        let text = read(&lock)?;
        snapshot.inputs.push(Input {
            path: lock.clone(),
            hash: text.as_deref().map(hash),
        });
        if let Some(text) = text {
            let path = mirror(snapshot.temp.path(), &lock);
            fs::write(path, text).map_err(|e| e.to_string())?;
        }
        Ok(snapshot)
    }
    fn write_manifest(&self, path: &Path, text: &str) -> Result<(), String> {
        let mut value: Yaml =
            serde_yaml::from_str(text).map_err(|e| e.to_string())?;
        let base = path.parent().unwrap();
        for section in ["dependencies", "dev_dependencies", "dependency_overrides"] {
            if let Some(deps) = value.get_mut(section).and_then(Yaml::as_mapping_mut)
            {
                for dep in deps.values_mut() {
                    if let Some(p) = dep.get_mut("path") {
                        if let Some(s) = p.as_str() {
                            let original = base
                                .join(s)
                                .canonicalize()
                                .map_err(|e| e.to_string())?;
                            *p = Yaml::String(
                                mirror(self.temp.path(), &original)
                                    .to_string_lossy()
                                    .into_owned(),
                            );
                        }
                    }
                }
            }
        }
        if value.get("workspace").is_some() {
            let members = workspace_members(base, &value)?;
            let root = mirror(self.temp.path(), base);
            let mut paths = vec![];
            for p in members {
                let p = p.canonicalize().map_err(|e| e.to_string())?;
                let stage = mirror(self.temp.path(), &p);
                let relative = stage
                    .strip_prefix(&root)
                    .map_err(|_| "Workspace members must be inside their root")?;
                paths.push(Yaml::String(
                    relative.to_string_lossy().replace('\\', "/"),
                ));
            }
            value["workspace"] = Yaml::Sequence(paths);
        }
        let target = mirror(self.temp.path(), path);
        fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
        fs::write(
            target,
            serde_yaml::to_string(&value).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())
    }
    fn current(&self) -> bool {
        self.inputs.iter().all(Input::unchanged)
    }
    fn stage_root(&self) -> PathBuf {
        mirror(self.temp.path(), &self.root)
    }
}
#[derive(Clone)]
struct Sdk {
    dart: PathBuf,
    flutter: Option<PathBuf>,
    cache: Option<PathBuf>,
}
impl Sdk {
    fn find(root: &Path) -> Result<Self, String> {
        if crate::dart::is_flutter_project(root) {
            if let Some(sdk) = crate::dart::sdk::find_flutter_sdk(Some(root)) {
                return Ok(Self {
                    dart: sdk.dart_bin,
                    flutter: Some(sdk.root),
                    cache: None,
                });
            }
        }
        let sdk = crate::dart::sdk::find_dart_sdk(Some(root)).ok_or(
            "Select a Dart/Flutter SDK to check transitive dependencies with Pub",
        )?;
        let flutter = if sdk.is_from_flutter {
            crate::dart::sdk::find_flutter_sdk(Some(root)).map(|sdk| sdk.root)
        } else {
            None
        };
        Ok(Self {
            dart: sdk.dart_bin,
            flutter,
            cache: None,
        })
    }
    fn command(&self, root: &Path, args: &[&str]) -> Command {
        let mut c = Command::new(&self.dart);
        c.arg("--suppress-analytics")
            .arg("pub")
            .args(args)
            .current_dir(root)
            .env("DART_SUPPRESS_ANALYTICS", "true")
            .env("FLUTTER_SUPPRESS_ANALYTICS", "true")
            .env("NO_COLOR", "1")
            .env("TERM", "dumb");
        if let Some(cache) = &self.cache {
            c.env("PUB_CACHE", cache);
        }
        if let Some(root) = &self.flutter {
            c.env("FLUTTER_ROOT", root);
        }
        c
    }
}
struct Output {
    success: bool,
    text: String,
}
fn stop(child: &mut Child) {
    #[cfg(unix)]
    {
        let _ = Command::new("kill")
            .args(["-KILL", "--", &format!("-{}", child.id())])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}
fn run(
    mut command: Command,
    deadline: Instant,
    cancelled: &impl Fn() -> bool,
) -> Result<Output, String> {
    let stdout = tempfile::NamedTempFile::new().map_err(|e| e.to_string())?;
    let stderr = tempfile::NamedTempFile::new().map_err(|e| e.to_string())?;
    command
        .stdin(Stdio::null())
        .stdout(stdout.reopen().map_err(|e| e.to_string())?)
        .stderr(stderr.reopen().map_err(|e| e.to_string())?);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    if cancelled() {
        return Err("Check superseded by newer edits".into());
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("Could not launch Pub: {e}"))?;
    loop {
        if cancelled() || Instant::now() > deadline {
            stop(&mut child);
            return Err(if cancelled() {
                "Check superseded by newer edits"
            } else {
                "Pub timed out; check connectivity and retry"
            }
            .into());
        }
        if [&stdout, &stderr].iter().any(|f| {
            f.as_file()
                .metadata()
                .is_ok_and(|m| m.len() > 2 * 1024 * 1024)
        }) {
            stop(&mut child);
            return Err("Pub output exceeded 2 MiB".into());
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                if [&stdout, &stderr].iter().any(|f| {
                    f.as_file()
                        .metadata()
                        .is_ok_and(|m| m.len() > 2 * 1024 * 1024)
                }) {
                    return Err("Pub output exceeded 2 MiB".into());
                }
                let mut text =
                    fs::read_to_string(stdout.path()).map_err(|e| e.to_string())?;
                text.push('\n');
                text.push_str(
                    &fs::read_to_string(stderr.path()).map_err(|e| e.to_string())?,
                );
                static ANSI: LazyLock<regex::Regex> = LazyLock::new(|| {
                    regex::Regex::new(r"\x1b\[[0-9;]*[a-zA-Z]").unwrap()
                });
                return Ok(Output {
                    success: status.success(),
                    text: ANSI.replace_all(&text, "").into_owned(),
                });
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(30)),
            Err(e) => {
                stop(&mut child);
                return Err(e.to_string());
            }
        }
    }
}
#[derive(Clone)]
struct Solution {
    at: Instant,
    source: String,
    inputs: Vec<Input>,
    sdk: PathBuf,
    conflict: bool,
    diagnostics: Vec<Diagnostic>,
    actions: Vec<CodeActionOrCommand>,
}
static RESULTS: LazyLock<Mutex<HashMap<PathBuf, Solution>>> =
    LazyLock::new(Default::default);
static GATES: LazyLock<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> =
    LazyLock::new(Default::default);
fn diagnostic(
    range: Range,
    message: String,
    severity: DiagnosticSeverity,
) -> Diagnostic {
    Diagnostic {
        range,
        message,
        severity: Some(severity),
        source: Some(crate::doc::yaml::SOURCE.into()),
        ..Default::default()
    }
}
fn conflict_text(output: &str, snapshot: &Snapshot) -> String {
    let mut text = output.replace(
        &snapshot
            .temp
            .path()
            .join("tree")
            .to_string_lossy()
            .to_string(),
        "",
    );
    if let Some(i) = text.find("Because ") {
        text = text[i..].into();
    }
    text = text.trim().chars().take(10000).collect();
    text
}
fn proposed_edits(
    path: &Path,
    text: &str,
    rows: &serde_json::Value,
) -> Vec<TextEdit> {
    let Ok(doc) = serde_yaml::from_str::<Yaml>(text) else {
        return vec![];
    };
    let mut edits = vec![];
    for section in ["dependencies", "dev_dependencies"] {
        let Some(map) = doc.get(section).and_then(Yaml::as_mapping) else {
            continue;
        };
        for (name, value) in map {
            let Some(name) = name.as_str() else { continue };
            if doc
                .get("dependency_overrides")
                .and_then(|v| v.get(name))
                .is_some()
            {
                continue;
            }
            let Some(version) = rows["packages"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|p| p["package"] == name)
                .and_then(|p| p["resolvable"]["version"].as_str())
            else {
                continue;
            };
            let Ok(parsed) = semver::Version::parse(version) else {
                continue;
            };
            let mut parts = vec![Part::Key(section.into()), Part::Key(name.into())];
            let old = if value.is_mapping() {
                parts.push(Part::Key("version".into()));
                value.get("version").and_then(Yaml::as_str)
            } else {
                value.as_str()
            };
            let Some(old) = old else { continue };
            if !parsed.pre.is_empty() && !old.contains('-') {
                continue;
            }
            if Constraint::parse(old).is_some_and(|c| c.contains(&parsed)) {
                continue;
            }
            if let Some(range) = yaml_schema::scalar_range(text, parts, false) {
                edits.push(TextEdit {
                    range,
                    new_text: format!("^{version}"),
                });
            }
        }
    }
    let _ = path;
    edits
}
fn relax(text: &str) -> Result<String, String> {
    let mut doc: Yaml = serde_yaml::from_str(text).map_err(|e| e.to_string())?;
    for section in ["dependencies", "dev_dependencies"] {
        if let Some(map) = doc.get_mut(section).and_then(Yaml::as_mapping_mut) {
            for value in map.values_mut() {
                if value.is_string() {
                    *value = Yaml::String("any".into());
                } else if let Some(version) = value.get_mut("version") {
                    if version.is_string() {
                        *version = Yaml::String("any".into());
                    }
                }
            }
        }
    }
    serde_yaml::to_string(&doc).map_err(|e| e.to_string())
}
fn parse_rows(text: &str) -> Option<serde_json::Value> {
    for (i, _) in text.match_indices('{').take(32) {
        let mut stream = serde_json::Deserializer::from_str(&text[i..])
            .into_iter::<serde_json::Value>();
        if let Some(Ok(value)) = stream.next() {
            if value["packages"].is_array() {
                return Some(value);
            }
        }
    }
    None
}
fn solve(
    snapshot: &Snapshot,
    sdk: &Sdk,
    source: &str,
    cancelled: &impl Fn() -> bool,
) -> Result<Solution, String> {
    let deadline = Instant::now() + TIMEOUT;
    let root = snapshot.stage_root();
    let original = run(
        sdk.command(&root, &["upgrade", "--dry-run", "--no-example"]),
        deadline,
        cancelled,
    )?;
    let mut result = Solution {
        at: Instant::now(),
        source: source.into(),
        inputs: snapshot.inputs.clone(),
        sdk: sdk.dart.clone(),
        conflict: false,
        diagnostics: vec![],
        actions: vec![],
    };
    if original.success {
        return Ok(result);
    }
    let range = serde_yaml::from_str::<Yaml>(source)
        .ok()
        .and_then(|doc| {
            for section in ["dependencies", "dev_dependencies"] {
                if let Some(deps) = doc.get(section).and_then(Yaml::as_mapping) {
                    for name in deps.keys().filter_map(Yaml::as_str) {
                        if original.text.contains(&format!("depends on {name} ")) {
                            if let Some(range) = yaml_schema::scalar_range(
                                source,
                                vec![
                                    Part::Key(section.into()),
                                    Part::Key(name.into()),
                                ],
                                true,
                            ) {
                                return Some(range);
                            }
                        }
                    }
                }
            }
            None
        })
        .unwrap_or_default();
    let explanation = conflict_text(&original.text, snapshot);
    if !original
        .text
        .to_lowercase()
        .contains("version solving failed")
    {
        result.diagnostics.push(diagnostic(
            range,
            format!("Pub compatibility check could not complete:\n{explanation}"),
            DiagnosticSeverity::INFORMATION,
        ));
        return Ok(result);
    }
    result.conflict = true;
    // Let Pub itself find a coordinated set with direct constraints relaxed.
    // Only workspace manifests are repair targets; external path packages remain
    // immutable and still constrain the solve.
    for path in &snapshot.targets {
        if let Some(text) = snapshot.manifests.get(path) {
            snapshot.write_manifest(path, &relax(text)?)?;
        }
    }
    let outdated = run(
        sdk.command(&root, &["outdated", "--json", "--show-all"]),
        deadline,
        cancelled,
    );
    let mut changes = BTreeMap::<Url, Vec<TextEdit>>::new();
    let mut proposed = BTreeMap::new();
    if let Ok(output) = outdated {
        if let Some(rows) = parse_rows(&output.text) {
            for path in &snapshot.targets {
                let Some(before) = snapshot.manifests.get(path) else {
                    continue;
                };
                let edits = proposed_edits(path, before, &rows);
                if edits.is_empty() {
                    continue;
                }
                let after = crate::code_action_preview::edited(before, &edits)?;
                if let Ok(uri) = Url::from_file_path(path) {
                    changes.insert(uri, edits);
                    proposed.insert(path.clone(), after);
                }
            }
        }
    }
    // Restore every unchanged constraint before validating the actual proposed
    // edits. A successful relaxed solve alone is never treated as verification.
    for (path, before) in &snapshot.manifests {
        snapshot.write_manifest(path, proposed.get(path).unwrap_or(before))?;
    }
    let mut message =
        format!("Pub found a transitive dependency conflict:\n{explanation}");
    if !changes.is_empty() {
        let verified = run(
            sdk.command(&root, &["upgrade", "--dry-run", "--no-example"]),
            deadline,
            cancelled,
        )?;
        if verified.success {
            let count: usize = changes.values().map(Vec::len).sum();
            message.push_str(&format!("\n\nPub verified a compatible set with {count} constraint change(s). Use Alt+Enter to review the diff."));
            result
                .actions
                .push(CodeActionOrCommand::CodeAction(CodeAction {
                    title: format!(
                        "Use Pub-verified compatible versions ({count} constraints)"
                    ),
                    kind: Some(CodeActionKind::QUICKFIX),
                    is_preferred: Some(true),
                    edit: Some(WorkspaceEdit {
                        changes: Some(changes.into_iter().collect()),
                        ..Default::default()
                    }),
                    data: Some(
                        serde_json::json!({"sparrowPubInputs":result.inputs}),
                    ),
                    ..Default::default()
                }));
        }
    }
    if result.actions.is_empty() {
        message.push_str("\n\nNo version-only repair was verified. Review the dependency/SDK requirements in Pub's explanation; no project files were changed.");
    }
    result
        .diagnostics
        .push(diagnostic(range, message, DiagnosticSeverity::ERROR));
    Ok(result)
}
pub(super) fn diagnostics(
    path: &Path,
    source: &str,
    cancelled: &impl Fn() -> bool,
) -> Vec<Diagnostic> {
    if !is_pubspec(path) || cancelled() {
        return vec![];
    }
    let Ok(path) = path.canonicalize() else {
        return vec![];
    };
    let quiet = Instant::now() + Duration::from_millis(750);
    while Instant::now() < quiet {
        if cancelled() {
            return vec![];
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    let gate = GATES
        .lock()
        .unwrap()
        .entry(path.clone())
        .or_default()
        .clone();
    let _guard = loop {
        if cancelled() {
            return vec![];
        }
        if let Ok(guard) = gate.try_lock() {
            break guard;
        }
        std::thread::sleep(Duration::from_millis(30));
    };
    let outcome: Result<Solution, String> = (|| {
        let sdk = Sdk::find(path.parent().unwrap())?;
        if let Some(cached) = RESULTS
            .lock()
            .unwrap()
            .get(&path)
            .filter(|v| {
                v.source == source
                    && v.sdk == sdk.dart
                    && v.at.elapsed() < Duration::from_secs(120)
                    && v.inputs.iter().all(Input::unchanged)
            })
            .cloned()
        {
            return Ok(cached);
        }
        let snapshot = Snapshot::new(&path, source)?;
        if cancelled() {
            return Err("Check superseded by newer edits".into());
        }
        let result = solve(&snapshot, &sdk, source, cancelled)?;
        if !snapshot.current() {
            return Err("A local dependency changed during the Pub check; edit or reload pubspec.yaml to check again".into());
        }
        Ok(result)
    })();
    if cancelled() {
        return vec![];
    }
    match outcome {
        Ok(result) => {
            let diagnostics = result.diagnostics.clone();
            let mut cache = RESULTS.lock().unwrap();
            if cache.len() >= 32 {
                cache.clear();
            }
            cache.insert(path, result);
            diagnostics
        }
        Err(error) => vec![diagnostic(
            Range::default(),
            format!("Pub compatibility check unavailable: {error}"),
            DiagnosticSeverity::INFORMATION,
        )],
    }
}
pub(super) fn code_actions(
    path: &Path,
    source: &str,
) -> Option<Vec<CodeActionOrCommand>> {
    let path = path.canonicalize().ok()?;
    let cache = RESULTS.lock().unwrap();
    let result = cache.get(&path).filter(|r| {
        r.source == source
            && r.at.elapsed() < Duration::from_secs(120)
            && r.inputs.iter().all(Input::unchanged)
    })?;
    result.conflict.then(|| result.actions.clone())
}
#[cfg(test)]
fn validate_action(action: &CodeActionOrCommand) -> Result<(), String> {
    validate_action_buffers(action, |_| None)
}
pub(crate) fn validate_action_buffers(
    action: &CodeActionOrCommand,
    buffer: impl Fn(&Path) -> Option<String>,
) -> Result<(), String> {
    let CodeActionOrCommand::CodeAction(action) = action else {
        return Ok(());
    };
    let Some(inputs) = action.data.as_ref().and_then(|d| d.get("sparrowPubInputs"))
    else {
        return Ok(());
    };
    let inputs: Vec<Input> = serde_json::from_value(inputs.clone())
        .map_err(|_| "Invalid Pub snapshot metadata")?;
    if !inputs.iter().all(|input| {
        input.unchanged()
            && buffer(&input.path).is_none_or(|text| Some(hash(&text)) == input.hash)
    }) {
        return Err("A dependency manifest, override or lockfile changed since Pub verified this fix. Refresh the check before applying it; no edits were made.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    #[test]
    fn snapshot_preserves_local_graph_overrides_and_the_original_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("app");
        let shared = dir.path().join("shared");
        let source = "name: app\nenvironment: {sdk: ^3.0.0}\ndependencies:\n  shared: {path: ../shared, version: ^1.0.0}\n";
        write(&root.join("pubspec.yaml"), source);
        write(
            &shared.join("pubspec.yaml"),
            "name: shared\nversion: 1.0.0\nenvironment: {sdk: ^3.0.0}\n",
        );
        write(&root.join("pubspec.lock"), "packages: {}\n");
        let unsaved = source.replace("^1.0.0", "^2.0.0");
        let snapshot = Snapshot::new(&root.join("pubspec.yaml"), &unsaved).unwrap();
        let staged = read(&mirror(snapshot.temp.path(), &root.join("pubspec.yaml")))
            .unwrap()
            .unwrap();
        assert!(staged.contains("^2.0.0"));
        assert!(staged.contains(snapshot.temp.path().to_str().unwrap()));
        assert_eq!(read(&root.join("pubspec.yaml")).unwrap().unwrap(), source);
        assert_eq!(snapshot.manifests.len(), 2);
        assert_eq!(snapshot.targets.len(), 1);
        assert!(snapshot.current());
        write(
            &shared.join("pubspec_overrides.yaml"),
            "dependency_overrides: {}\n",
        );
        assert!(!snapshot.current());
        assert!(!root.join(".dart_tool").exists());
    }
    #[test]
    fn workspace_members_and_snapshots_keep_sibling_relationships() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            &root.join("pubspec.yaml"),
            "name: mono\nenvironment: {sdk: ^3.6.0}\nworkspace: [packages/*]\n",
        );
        let source =
            "name: member\nenvironment: {sdk: ^3.6.0}\nresolution: workspace\n";
        write(&root.join("packages/member/pubspec.yaml"), source);
        let snapshot =
            Snapshot::new(&root.join("packages/member/pubspec.yaml"), source)
                .unwrap();
        assert_eq!(snapshot.root, root.canonicalize().unwrap());
        assert_eq!(snapshot.targets.len(), 2);
        let staged = read(&snapshot.stage_root().join("pubspec.yaml"))
            .unwrap()
            .unwrap();
        assert!(staged.contains("packages/member"));
    }
    #[test]
    fn structured_pub_versions_produce_minimal_coordinated_edits_and_stale_guards() {
        let text = "name: app\ndependencies:\n  one: ^1.0.0 # keep\n  two: {version: '^1.0.0', hosted: https://private.example}\n";
        let rows = serde_json::json!({"packages":[{"package":"one","resolvable":{"version":"2.0.0"}},{"package":"two","resolvable":{"version":"3.0.0"}}]});
        let edits = proposed_edits(Path::new("/app/pubspec.yaml"), text, &rows);
        assert_eq!(edits.len(), 2);
        let changed = crate::code_action_preview::edited(text, &edits).unwrap();
        assert!(changed.contains("one: ^2.0.0 # keep"));
        assert!(changed.contains("version: ^3.0.0, hosted:"));
        let dir = tempfile::tempdir().unwrap();
        let dep = dir.path().join("pubspec.yaml");
        write(&dep, "name: dep\n");
        let action = CodeActionOrCommand::CodeAction(CodeAction {
            data: Some(
                serde_json::json!({"sparrowPubInputs":[{"path":dep,"hash":hash("name: dep\n")}]}),
            ),
            ..Default::default()
        });
        assert!(validate_action(&action).is_ok());
        assert!(
            validate_action_buffers(&action, |_| Some(
                "name: unsaved_changes\n".into()
            ))
            .is_err()
        );
        write(&dep, "name: changed\n");
        assert!(validate_action(&action).is_err());
        assert!(parse_rows("log\n{\"packages\":[]}\n").is_some());
    }
    #[test]
    #[ignore = "requires a Dart SDK; local hosted registry, no Internet"]
    fn real_pub_explains_transitive_conflicts_and_verifies_the_repair() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
            sync::atomic::{AtomicBool, Ordering},
        };
        let registry = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = registry.local_addr().unwrap();
        registry.set_nonblocking(true).unwrap();
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        let server = std::thread::spawn(move || {
            while !stop.load(Ordering::Acquire) {
                let Ok((mut client, _)) = registry.accept() else {
                    std::thread::sleep(Duration::from_millis(10));
                    continue;
                };
                client
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut buf = [0u8; 8192];
                let len = client.read(&mut buf).unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..len]);
                let route = request.split_whitespace().nth(1).unwrap_or("");
                let archive = route
                    .strip_prefix("/archive/")
                    .and_then(|v| v.strip_suffix(".tar.gz"))
                    .and_then(|v| v.rsplit_once('-'));
                let parts: Vec<_> = route
                    .trim_start_matches("/api/packages/")
                    .split('/')
                    .collect();
                let name = archive.map(|(name, _)| name).unwrap_or(parts[0]);
                let mut versions = vec![];
                if ["alpha", "beta", "shared"].contains(&name) {
                    for major in if name == "alpha" || name == "shared" {
                        vec![1, 2]
                    } else {
                        vec![1]
                    } {
                        let dependencies = if name == "shared" {
                            serde_json::json!({})
                        } else {
                            serde_json::json!({"shared":{"hosted":format!("http://{addr}"),"version":if name=="alpha"&&major==1{"^1.0.0"}else{"^2.0.0"}}})
                        };
                        let version = format!("{major}.0.0");
                        versions.push(serde_json::json!({"version":version,"archive_url":format!("http://{addr}/archive/{name}-{version}.tar.gz"),"pubspec":{"name":name,"version":version,"environment":{"sdk":"^3.0.0"},"dependencies":dependencies}}));
                    }
                }
                let version =
                    archive.map(|(_, v)| v).or_else(|| parts.get(2).copied());
                let selected = version
                    .and_then(|v| versions.iter().find(|r| r["version"] == v));
                let (status, body, content_type) = if archive.is_some()
                    && selected.is_some()
                {
                    let yaml = serde_yaml::to_string(&selected.unwrap()["pubspec"])
                        .unwrap();
                    let mut tar = tar::Builder::new(Vec::new());
                    let mut header = tar::Header::new_gnu();
                    header.set_mode(0o644);
                    header.set_size(yaml.len() as u64);
                    header.set_cksum();
                    tar.append_data(&mut header, "pubspec.yaml", yaml.as_bytes())
                        .unwrap();
                    let bytes = tar.into_inner().unwrap();
                    let mut gzip = flate2::write::GzEncoder::new(
                        Vec::new(),
                        flate2::Compression::default(),
                    );
                    gzip.write_all(&bytes).unwrap();
                    ("200 OK", gzip.finish().unwrap(), "application/gzip")
                } else if let Some(selected) = selected {
                    (
                        "200 OK",
                        serde_json::to_vec(selected).unwrap(),
                        "application/vnd.pub.v2+json",
                    )
                } else if !versions.is_empty() && parts.len() == 1 {
                    ("200 OK",serde_json::to_vec(&serde_json::json!({"name":name,"latest":versions.last(),"versions":versions})).unwrap(),"application/vnd.pub.v2+json")
                } else {
                    ("404 Not Found", b"{}".to_vec(), "application/json")
                };
                let _ = write!(
                    client,
                    "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = client.write_all(&body);
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let source = format!(
            "name: app\nenvironment: {{sdk: ^3.0.0}}\ndependencies:\n  alpha: {{hosted: 'http://{addr}', version: ^1.0.0}}\n  beta: {{hosted: 'http://{addr}', version: ^1.0.0}}\n"
        );
        write(&dir.path().join("pubspec.yaml"), &source);
        let snapshot =
            Snapshot::new(&dir.path().join("pubspec.yaml"), &source).unwrap();
        let mut sdk = Sdk::find(dir.path()).unwrap();
        sdk.cache = Some(dir.path().join("pub-cache"));
        let result = solve(&snapshot, &sdk, &source, &|| false);
        stopped.store(true, Ordering::Release);
        server.join().unwrap();
        let result = result.unwrap();
        assert!(result.conflict, "{:?}", result.diagnostics);
        assert!(result.diagnostics[0].message.contains("shared"));
        assert_eq!(result.actions.len(), 1, "{}", result.diagnostics[0].message);
        let CodeActionOrCommand::CodeAction(action) = &result.actions[0] else {
            panic!()
        };
        let files =
            crate::code_action_preview::edit_files(action.edit.as_ref().unwrap())
                .unwrap();
        assert_eq!(files.len(), 1);
        let fixed =
            crate::code_action_preview::edited(&source, &files[0].edits).unwrap();
        assert!(fixed.contains("version: ^2.0.0"));
        assert_eq!(
            read(&dir.path().join("pubspec.yaml")).unwrap().unwrap(),
            source
        );
        assert!(!dir.path().join("pubspec.lock").exists());
        assert!(!dir.path().join(".dart_tool").exists());
    }
}

#[cfg(test)]
mod process_tests {
    use super::*;
    #[test]
    fn fixture_child() {
        let Ok(mode) = std::env::var("SPARROW_PUB_SOLVER_TEST_CHILD") else {
            return;
        };
        if mode == "output" {
            use std::io::Write;
            std::io::stdout()
                .write_all(&vec![b'x'; 3 * 1024 * 1024])
                .unwrap();
        } else {
            std::thread::sleep(Duration::from_secs(20));
        }
    }
    fn child(mode: &str) -> Command {
        let mut c = Command::new(std::env::current_exe().unwrap());
        c.args([
            "--exact",
            "doc::pubdev::solver::process_tests::fixture_child",
            "--nocapture",
        ])
        .env("SPARROW_PUB_SOLVER_TEST_CHILD", mode);
        c
    }
    #[test]
    fn cancelled_children_are_reaped_and_output_is_bounded() {
        let start = Instant::now();
        let error = run(
            child("sleep"),
            Instant::now() + Duration::from_secs(3),
            &|| start.elapsed() > Duration::from_millis(100),
        )
        .err()
        .unwrap();
        assert!(error.contains("superseded"));
        assert!(start.elapsed() < Duration::from_secs(3));
        let error = run(
            child("output"),
            Instant::now() + Duration::from_secs(3),
            &|| false,
        )
        .err()
        .unwrap();
        assert!(error.contains("output exceeded"));
    }
}
