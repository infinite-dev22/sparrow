//! Registry completion and SDK/direct checks, augmented by isolated Pub solves.
//! All work runs off the UI thread; workspace edits are previewed and undoable.
mod constraint;
pub(crate) mod solver;
use super::yaml_schema::{self, Part};
use constraint::Constraint;
use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CompletionItem,
    CompletionItemKind, CompletionTextEdit, Diagnostic, DiagnosticSeverity,
    Position, Range, TextEdit, Url, WorkspaceEdit,
};
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_yaml::Value as Yaml;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock, Mutex},
    time::{Duration, Instant},
};

const NAME_TTL: Duration = Duration::from_secs(8 * 60 * 60);
const PACKAGE_TTL: Duration = Duration::from_secs(30 * 60);
#[derive(Clone, Debug, Deserialize, Serialize)]
struct Release {
    version: String,
    pubspec: serde_json::Value,
    #[serde(default, rename = "retracted")]
    retracted: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
struct Package {
    name: String,
    versions: Vec<Release>,
    #[serde(default, rename = "isDiscontinued")]
    discontinued: bool,
    #[serde(default, rename = "replacedBy")]
    replaced_by: Option<String>,
}
#[derive(Clone)]
enum Reply {
    Data(Arc<Vec<u8>>),
    Missing,
    Failed,
}
struct Cached {
    ttl: Duration,
    at: Instant,
    reply: Reply,
}
struct Registry {
    base: String,
    client: reqwest::blocking::Client,
    memory: Mutex<HashMap<String, Cached>>,
    gate: Mutex<()>,
    retry_after: Mutex<Option<Instant>>,
    disk: Option<PathBuf>,
}
static REGISTRY: LazyLock<Registry> = LazyLock::new(|| {
    Registry::new(
        "https://pub.dev".into(),
        sparrow_core::directory::Directory::cache_directory()
            .map(|p| p.join("pub-dev")),
    )
});
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}
impl Registry {
    fn new(base: String, disk: Option<PathBuf>) -> Self {
        Self {
            base,
            client: reqwest::blocking::Client::builder()
                .user_agent("Sparrow IDE pub.dev completion")
                .connect_timeout(Duration::from_secs(3))
                .timeout(Duration::from_secs(6))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("HTTP client"),
            memory: Mutex::new(HashMap::new()),
            gate: Mutex::new(()),
            retry_after: Mutex::new(None),
            disk,
        }
    }
    fn cached(&self, key: &str, ttl: Duration) -> Option<Reply> {
        self.memory
            .lock()
            .unwrap()
            .get(key)
            .filter(|v| v.at.elapsed() < v.ttl.min(ttl))
            .map(|v| v.reply.clone())
    }
    fn get(&self, key: &str, endpoint: &str, ttl: Duration) -> Reply {
        if let Some(reply) = self.cached(key, ttl) {
            return reply;
        }
        // Coalesce identical requests and bound registry traffic. Files are small
        // and this lock is only held on background workers, never the UI thread.
        let _gate = self.gate.lock().unwrap();
        if let Some(reply) = self.cached(key, ttl) {
            return reply;
        }
        let disk = self
            .disk
            .as_ref()
            .map(|dir| dir.join(format!("{key}.json")));
        let stored = disk.as_ref().and_then(|p| {
            let metadata = std::fs::metadata(p).ok()?;
            if metadata.len() > 16 * 1024 * 1024 {
                return None;
            }
            Some((
                std::fs::read(p).ok()?,
                metadata.modified().ok()?.elapsed().ok()?,
            ))
        });
        let reply = if let Some((bytes, age)) = &stored {
            if *age < ttl {
                Reply::Data(Arc::new(bytes.clone()))
            } else {
                self.fetch(endpoint)
            }
        } else {
            self.fetch(endpoint)
        };
        let failed = matches!(reply, Reply::Failed);
        let fetched = stored.as_ref().is_none_or(|(_, age)| *age >= ttl);
        let reply = match reply {
            Reply::Failed => stored
                .map(|(bytes, _)| Reply::Data(Arc::new(bytes)))
                .unwrap_or(Reply::Failed),
            reply => reply,
        };
        if let (Reply::Data(bytes), Some(path), true) =
            (&reply, &disk, fetched && !failed)
        {
            if let Some(dir) = path.parent() {
                if std::fs::create_dir_all(dir).is_ok() {
                    if let Ok(mut tmp) = tempfile::NamedTempFile::new_in(dir) {
                        use std::io::Write;
                        if tmp.write_all(bytes).is_ok() {
                            let _ = tmp.persist(path);
                        }
                    }
                }
            }
        }
        let mut cache = self.memory.lock().unwrap();
        if cache.len() >= 256 {
            cache.clear();
        }
        cache.insert(
            key.into(),
            Cached {
                ttl: if failed { Duration::from_secs(60) } else { ttl },
                at: Instant::now(),
                reply: reply.clone(),
            },
        );
        reply
    }
    fn fetch(&self, endpoint: &str) -> Reply {
        if self.retry_after.lock().unwrap().is_some_and(|until| Instant::now() < until) {
            return Reply::Failed;
        }
        let result = (|| -> Result<Vec<u8>, String> {
            let mut response = self
                .client
                .get(format!("{}{endpoint}", self.base))
                .header("Accept", "application/vnd.pub.v2+json")
                .header("Accept-Encoding", "gzip")
                .send()
                .map_err(|e| e.to_string())?;
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                return Err("404".into());
            }
            if !response.status().is_success() {
                return Err(response.status().to_string());
            }
            let gzip = response
                .headers()
                .get("content-encoding")
                .is_some_and(|v| v == "gzip");
            let mut bytes = vec![];
            (&mut response)
                .take(16 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() > 16 * 1024 * 1024 {
                return Err("oversized registry response".into());
            }
            if gzip {
                let mut decoded = vec![];
                flate2::read::GzDecoder::new(bytes.as_slice())
                    .take(16 * 1024 * 1024 + 1)
                    .read_to_end(&mut decoded)
                    .map_err(|e| e.to_string())?;
                bytes = decoded;
            }
            if bytes.len() > 16 * 1024 * 1024
                || serde_json::from_slice::<serde_json::Value>(&bytes).is_err()
            {
                return Err("invalid registry response".into());
            }
            Ok(bytes)
        })();
        match result {
            Ok(bytes) => {
                *self.retry_after.lock().unwrap() = None;
                Reply::Data(Arc::new(bytes))
            },
            Err(e) if e == "404" => Reply::Missing,
            Err(e) => {
                *self.retry_after.lock().unwrap() = Some(Instant::now() + Duration::from_secs(60));
                tracing::debug!("pub.dev temporarily unavailable; using cached metadata and pausing registry requests for 60s: {e}");
                Reply::Failed
            }
        }
    }
    fn names(&self) -> Vec<String> {
        #[derive(Deserialize)]
        struct Names {
            packages: Vec<String>,
        }
        match self.get("names", "/api/package-name-completion-data", NAME_TTL) {
            Reply::Data(bytes) => serde_json::from_slice::<Names>(&bytes)
                .map(|v| v.packages.into_iter().filter(|n| valid_name(n)).collect())
                .unwrap_or_default(),
            _ => vec![],
        }
    }
    fn package(&self, name: &str) -> Result<Option<Package>, ()> {
        if !valid_name(name) {
            return Err(());
        }
        match self.get(
            &format!("package-{name}"),
            &format!("/api/packages/{name}"),
            PACKAGE_TTL,
        ) {
            Reply::Data(bytes) => serde_json::from_slice::<Package>(&bytes)
                .map(|p| Some(p))
                .map_err(|_| ()),
            Reply::Missing => Ok(None),
            Reply::Failed => Err(()),
        }
    }
    fn known_package(&self, name: &str) -> Option<Package> {
        match self.cached(&format!("package-{name}"), PACKAGE_TTL)? {
            Reply::Data(bytes) => serde_json::from_slice(&bytes).ok(),
            _ => None,
        }
    }
}
fn is_pubspec(path: &Path) -> bool {
    path.file_name().is_some_and(|n| n == "pubspec.yaml")
}
const SECTIONS: [&str; 3] =
    ["dependencies", "dev_dependencies", "dependency_overrides"];
fn public_hosted(value: &Yaml) -> bool {
    let default_public = std::env::var("PUB_HOSTED_URL")
        .ok()
        .is_none_or(|url| url.trim_end_matches('/') == "https://pub.dev");
    if value.is_string() || value.is_null() {
        return default_public;
    }
    let Some(map) = value.as_mapping() else {
        return false;
    };
    if ["path", "git", "sdk"]
        .iter()
        .any(|k| map.contains_key(Yaml::String((*k).into())))
    {
        return false;
    }
    match value.get("hosted") {
        None => default_public,
        Some(Yaml::String(s)) => s.trim_end_matches('/') == "https://pub.dev",
        Some(m) if m.is_mapping() => {
            m.get("url")
                .and_then(Yaml::as_str)
                .is_some_and(|s| s.trim_end_matches('/') == "https://pub.dev")
                && m.get("name").is_none()
        }
        _ => false,
    }
}
#[derive(Clone)]
struct Dependency {
    name: String,
    requirement: String,
    path: Vec<Part>,
}
fn dependencies(doc: &Yaml) -> Vec<Dependency> {
    let mut all = BTreeMap::new();
    for section in SECTIONS {
        let Some(map) = doc.get(section).and_then(Yaml::as_mapping) else {
            continue;
        };
        for (name, value) in map {
            let Some(name) = name.as_str().filter(|n| valid_name(n)) else {
                continue;
            };
            // Overrides replace, rather than add to, the main constraint. A
            // local/custom-hosted override must also suppress public lookups.
            all.remove(name);
            if !public_hosted(value) {
                continue;
            }
            let mut path = vec![Part::Key(section.into()), Part::Key(name.into())];
            let requirement = if value.is_mapping() {
                path.push(Part::Key("version".into()));
                value.get("version").and_then(Yaml::as_str).unwrap_or("any")
            } else {
                value.as_str().unwrap_or("any")
            };
            all.insert(
                name.to_owned(),
                Dependency {
                    name: name.into(),
                    requirement: requirement.into(),
                    path,
                },
            );
        }
    }
    all.into_values().collect()
}
#[derive(Clone, Default)]
struct Environment {
    flutter_project: bool,
    dart: Option<Constraint>,
    flutter: Option<Constraint>,
    actual_dart: Option<Version>,
    actual_flutter: Option<Version>,
}
static SDKS: LazyLock<
    Mutex<HashMap<PathBuf, (Instant, Option<Version>, Option<Version>)>>,
> = LazyLock::new(Default::default);
fn environment(doc: &Yaml, root: &Path) -> Environment {
    let mut env = Environment {
        flutter_project: doc
            .get("dependencies")
            .and_then(|d| d.get("flutter"))
            .and_then(|v| v.get("sdk"))
            .and_then(Yaml::as_str)
            == Some("flutter")
            || doc
                .get("environment")
                .and_then(|v| v.get("flutter"))
                .is_some(),
        dart: doc
            .get("environment")
            .and_then(|e| e.get("sdk"))
            .and_then(Yaml::as_str)
            .and_then(Constraint::parse),
        flutter: doc
            .get("environment")
            .and_then(|e| e.get("flutter"))
            .and_then(Yaml::as_str)
            .and_then(Constraint::parse),
        ..Default::default()
    };
    env.dart = env.dart.map(Constraint::dart_sdk);
    env.flutter = env.flutter.map(Constraint::flutter_sdk);
    let stored = SDKS
        .lock()
        .unwrap()
        .get(root)
        .filter(|(t, _, _)| t.elapsed() < Duration::from_secs(120))
        .cloned();
    let (_, dart, flutter) = stored.unwrap_or_else(|| {
        let dart = crate::dart::sdk::find_dart_sdk(Some(root))
            .and_then(|s| s.version)
            .and_then(|v| Version::parse(v.trim()).ok());
        let flutter = crate::dart::sdk::find_flutter_sdk(Some(root))
            .and_then(|s| s.version)
            .and_then(|v| Version::parse(v.trim()).ok());
        let data = (Instant::now(), dart, flutter);
        SDKS.lock().unwrap().insert(root.into(), data.clone());
        data
    });
    env.actual_dart = dart;
    env.actual_flutter = flutter;
    env
}
fn release_version(release: &Release) -> Option<Version> {
    Version::parse(&release.version).ok()
}
fn usable(release: &Release, doc: &Yaml, env: &Environment) -> Result<(), String> {
    if release.retracted {
        return Err("release is retracted".into());
    }
    if env.actual_dart.as_ref().is_some_and(|v| v.major >= 3)
        && release
            .pubspec
            .get("environment")
            .and_then(|v| v.get("sdk"))
            .and_then(serde_json::Value::as_str)
            .is_none()
    {
        return Err("package has no null-safe Dart SDK constraint".into());
    }
    if !env.flutter_project
        && release
            .pubspec
            .get("dependencies")
            .and_then(|v| v.get("flutter"))
            .and_then(|v| v.get("sdk"))
            .and_then(serde_json::Value::as_str)
            == Some("flutter")
    {
        return Err("requires a Flutter SDK dependency in this project".into());
    }
    for (key, declared, actual) in [
        ("sdk", &env.dart, &env.actual_dart),
        ("flutter", &env.flutter, &env.actual_flutter),
    ] {
        if let Some(required) = release
            .pubspec
            .get("environment")
            .and_then(|e| e.get(key))
            .and_then(serde_json::Value::as_str)
        {
            if let Some(constraint) = Constraint::parse(required) {
                if key == "sdk"
                    && actual.as_ref().is_some_and(|v| v.major >= 3)
                    && !constraint.null_safe()
                {
                    return Err(format!(
                        "requires a null-safe Dart SDK lower bound (package declares {required})"
                    ));
                }
                let constraint = if key == "sdk" {
                    constraint.dart_sdk()
                } else {
                    constraint.flutter_sdk()
                };
                if declared.as_ref().is_some_and(|v| !v.overlaps(&constraint)) {
                    return Err(format!(
                        "requires {} {required}, outside the declared SDK range",
                        if key == "sdk" { "Dart" } else { "Flutter" }
                    ));
                }
                if actual.as_ref().is_some_and(|v| !constraint.contains(v)) {
                    return Err(format!(
                        "requires {} {required}; selected SDK is {}",
                        if key == "sdk" { "Dart" } else { "Flutter" },
                        actual.as_ref().unwrap()
                    ));
                }
            }
        }
    }
    if let Some(required) = release
        .pubspec
        .get("dependencies")
        .and_then(serde_json::Value::as_object)
    {
        for dep in dependencies(doc) {
            let Some(other) = required.get(&dep.name).and_then(|v| {
                v.as_str()
                    .or_else(|| v.get("version").and_then(serde_json::Value::as_str))
            }) else {
                continue;
            };
            // Explicit overrides intentionally bypass dependency constraints.
            if doc
                .get("dependency_overrides")
                .and_then(|v| v.get(&dep.name))
                .is_some()
            {
                continue;
            }
            if let (Some(a), Some(b)) = (
                Constraint::parse(&dep.requirement),
                Constraint::parse(other),
            ) {
                if !a.overlaps(&b) {
                    return Err(format!(
                        "requires {} {other}, conflicting with {}",
                        dep.name, dep.requirement
                    ));
                }
            }
        }
    }
    Ok(())
}
fn sorted_releases(package: &Package) -> Vec<&Release> {
    let mut releases: Vec<_> = package
        .versions
        .iter()
        .filter(|r| !r.retracted && release_version(r).is_some())
        .collect();
    releases.sort_by(|a, b| release_version(b).cmp(&release_version(a)));
    releases
}
fn best<'a>(
    package: &'a Package,
    doc: &Yaml,
    env: &Environment,
) -> Option<&'a Release> {
    sorted_releases(package).into_iter().find(|r| {
        release_version(r).is_some_and(|v| v.pre.is_empty())
            && usable(r, doc, env).is_ok()
    })
}

#[derive(Clone)]
enum RequestKind {
    Names {
        existing: HashSet<String>,
        colon: bool,
    },
    Versions {
        name: String,
    },
}
#[derive(Clone)]
pub(crate) struct CompletionRequest {
    pub start: usize,
    pub input: String,
    kind: RequestKind,
    range: Range,
    source: String,
    root: PathBuf,
}
fn byte_position(text: &str, offset: usize) -> Position {
    let prefix = &text[..offset];
    Position::new(
        prefix.bytes().filter(|b| *b == b'\n').count() as u32,
        prefix
            .rsplit('\n')
            .next()
            .unwrap_or("")
            .encode_utf16()
            .count() as u32,
    )
}
pub(crate) fn completion_request(
    path: &Path,
    text: &str,
    offset: usize,
) -> Option<CompletionRequest> {
    if !is_pubspec(path) {
        return None;
    }
    let (start, end, document, context, key) =
        yaml_schema::completion_context(path, text, offset)?;
    let section = match context.first()? {
        Part::Key(s) if SECTIONS.contains(&s.as_str()) => s,
        _ => return None,
    };
    let mut range = Range::new(byte_position(text, start), byte_position(text, end));
    if key
        && start > 0
        && end < text.len()
        && matches!(text.as_bytes()[start - 1], b'\'' | b'"')
        && text.as_bytes()[start - 1] == text.as_bytes()[end]
    {
        range =
            Range::new(byte_position(text, start - 1), byte_position(text, end + 1));
    }
    let kind = if key && context.len() == 1 {
        if !public_hosted(&Yaml::Null) {
            return None;
        }
        let existing = document
            .get(section)
            .and_then(Yaml::as_mapping)
            .into_iter()
            .flat_map(|m| m.keys())
            .filter_map(Yaml::as_str)
            .map(str::to_owned)
            .collect();
        RequestKind::Names {
            existing,
            colon: text[end..]
                .trim_start_matches(['\'', '"', ' ', '\t'])
                .starts_with(':'),
        }
    } else if !key && (context.len() == 2 || context.len() == 3) {
        let Part::Key(name) = &context[1] else {
            return None;
        };
        if context.len() == 3 && !matches!(&context[2],Part::Key(k)if k=="version") {
            return None;
        }
        let original = serde_yaml::from_str::<Yaml>(text).ok();
        if original
            .as_ref()
            .and_then(|d| d.get(section))
            .and_then(|d| d.get(name))
            .is_some_and(|v| !public_hosted(v))
        {
            return None;
        }
        if let Some(value_range) =
            yaml_schema::scalar_range(text, context.clone(), false)
        {
            range = value_range;
        }
        RequestKind::Versions { name: name.clone() }
    } else {
        return None;
    };
    Some(CompletionRequest {
        start,
        input: text[start..offset].into(),
        kind,
        range,
        source: text.into(),
        root: path.parent()?.into(),
    })
}
impl CompletionRequest {
    pub(crate) fn complete(self) -> Vec<CompletionItem> {
        self.with_registry(&REGISTRY)
    }
    fn with_registry(self, registry: &Registry) -> Vec<CompletionItem> {
        let mut options: Vec<(String, String, String)> = vec![];
        match &self.kind {
            RequestKind::Names { existing, colon } => {
                let input = self.input.to_lowercase();
                let mut names = registry.names();
                names.retain(|n| n.contains(&input) && !existing.contains(n));
                names.sort_by_key(|n| (!n.starts_with(&input), n != &input));
                // The documented index intentionally excludes some packages.
                // An exact-name lookup still lets users add those packages.
                if input.len() >= 2
                    && valid_name(&input)
                    && !existing.contains(&input)
                    && !names.iter().any(|n| n == &input)
                    && registry.package(&input).ok().flatten().is_some()
                {
                    names.insert(0, input);
                }
                for name in names.into_iter().take(40) {
                    options.push((
                        name.clone(),
                        if *colon { name } else { format!("{name}: ") },
                        "pub.dev package — select a version after the colon".into(),
                    ));
                }
            }
            RequestKind::Versions { name } => {
                let Some(package) = registry.package(&name).ok().flatten() else {
                    return vec![];
                };
                let doc =
                    serde_yaml::from_str::<Yaml>(&self.source).unwrap_or(Yaml::Null);
                let env = environment(&doc, &self.root);
                // Offer stable releases first; prereleases are explicit opt-in.
                let prefix = self.input.trim_start_matches(['^', '\'', '"']);
                for release in sorted_releases(&package) {
                    let version = release_version(release).unwrap();
                    if !version.pre.is_empty() && !prefix.contains('-') {
                        continue;
                    }
                    if !release.version.starts_with(prefix)
                        || usable(release, &doc, &env).is_err()
                    {
                        continue;
                    }
                    options.push((release.version.clone(),format!("^{}",release.version),format!("{name} — SDK/direct checks passed; Pub resolves transitive dependencies")));
                    if options.len() >= 40 {
                        break;
                    }
                }
            }
        }
        options
            .into_iter()
            .enumerate()
            .map(|(i, (label, new_text, detail))| CompletionItem {
                label,
                sort_text: Some(format!("{i:04}")),
                detail: Some(detail),
                kind: Some(match &self.kind {
                    RequestKind::Names { .. } => CompletionItemKind::MODULE,
                    _ => CompletionItemKind::VALUE,
                }),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                    range: self.range,
                    new_text,
                })),
                data: Some(serde_json::json!({"sparrowYaml":true})),
                ..Default::default()
            })
            .collect()
    }
}
struct Report {
    range: Range,
    message: String,
    severity: DiagnosticSeverity,
    fix: Option<(String, TextEdit)>,
}
fn reports(
    doc: &Yaml,
    text: &str,
    env: &Environment,
    lookup: impl FnMut(&str) -> Result<Option<Package>, ()>,
) -> Vec<Report> {
    reports_at(doc, text, env, None, lookup)
}
fn reports_at(
    doc: &Yaml,
    text: &str,
    env: &Environment,
    line: Option<u32>,
    mut lookup: impl FnMut(&str) -> Result<Option<Package>, ()>,
) -> Vec<Report> {
    let mut result = vec![];
    for dep in dependencies(doc).into_iter().take(80) {
        let mut name_path = dep.path.clone();
        if name_path.len() == 3 {
            name_path.pop();
        }
        let name_range = yaml_schema::scalar_range(text, name_path, true);
        let value_range = yaml_schema::scalar_range(text, dep.path.clone(), false);
        let Some(range) = value_range.or(name_range) else {
            continue;
        };
        if line.is_some_and(|line| range.start.line != line) {
            continue;
        }
        let package = match lookup(&dep.name) {
            Ok(Some(p)) => p,
            Ok(None) => {
                result.push(Report {
                    range,
                    message: format!(
                        "Package '{}' was not found on pub.dev.",
                        dep.name
                    ),
                    severity: DiagnosticSeverity::WARNING,
                    fix: None,
                });
                continue;
            }
            Err(()) => continue,
        };
        let recommended = best(&package, doc, env);
        let fix = recommended.filter(|_| value_range.is_some()).map(|r| {
            (
                format!(
                    "Use {} ^{} (SDK/direct checks passed)",
                    dep.name, r.version
                ),
                TextEdit {
                    range,
                    new_text: format!("^{}", r.version),
                },
            )
        });
        let Some(requirement) = Constraint::parse(&dep.requirement) else {
            result.push(Report {
                range,
                message: format!(
                    "Invalid Pub version constraint '{}' for {}.",
                    dep.requirement, dep.name
                ),
                severity: DiagnosticSeverity::ERROR,
                fix,
            });
            continue;
        };
        let matching: Vec<_> = package
            .versions
            .iter()
            .filter(|r| release_version(r).is_some_and(|v| requirement.contains(&v)))
            .collect();
        let usable_matching = matching.iter().any(|r| usable(r, doc, env).is_ok());
        if !usable_matching {
            let reason = matching
                .iter()
                .find_map(|r| usable(r, doc, env).err())
                .unwrap_or_else(|| {
                    "no published version satisfies this constraint".into()
                });
            result.push(Report {
                range,
                message: format!("{} {}: {reason}.", dep.name, dep.requirement),
                severity: DiagnosticSeverity::WARNING,
                fix,
            });
        } else if package.discontinued {
            result.push(Report {
                range,
                message: format!(
                    "{} is discontinued on pub.dev{}.",
                    dep.name,
                    package
                        .replaced_by
                        .as_ref()
                        .map(|n| format!("; suggested replacement: {n}"))
                        .unwrap_or_default()
                ),
                severity: DiagnosticSeverity::WARNING,
                fix: None,
            });
        } else if let Some(release) = recommended {
            if !requirement.contains(&release_version(release).unwrap()) {
                result.push(Report{range,message:format!("{} {} is available outside this constraint (SDK/direct checks passed; run Pub to resolve transitive dependencies).",dep.name,release.version),severity:DiagnosticSeverity::HINT,fix});
            }
        }
    }
    result
}
pub(crate) fn diagnostics(
    path: &Path,
    text: &str,
    cancelled: impl Fn() -> bool,
) -> Vec<Diagnostic> {
    if !is_pubspec(path) {
        return vec![];
    }
    let Ok(doc) = serde_yaml::from_str::<Yaml>(text) else {
        return vec![];
    };
    if cancelled() { return vec![]; }
    let mut diagnostics = if dependencies(&doc).is_empty() { vec![] } else {
        let env = environment(&doc, path.parent().unwrap_or(Path::new(".")));
        reports(&doc, text, &env, |name| {
            if cancelled() { Err(()) } else { REGISTRY.package(name) }
        }).into_iter().map(|r| Diagnostic {
            range: r.range, message: r.message, severity: Some(r.severity),
            source: Some(super::yaml::SOURCE.into()), ..Default::default()
        }).collect::<Vec<_>>()
    };
    diagnostics.extend(solver::diagnostics(path, text, &cancelled));
    diagnostics
}
pub(crate) fn code_actions(
    path: &Path,
    text: &str,
    line: u32,
) -> Vec<CodeActionOrCommand> {
    if !is_pubspec(path) {
        return vec![];
    }
    if let Some(actions) = solver::code_actions(path, text) { return actions; }
    let Ok(doc) = serde_yaml::from_str::<Yaml>(text) else {
        return vec![];
    };
    let mut env = Environment {
        flutter_project: doc
            .get("dependencies")
            .and_then(|d| d.get("flutter"))
            .and_then(|v| v.get("sdk"))
            .and_then(Yaml::as_str)
            == Some("flutter")
            || doc
                .get("environment")
                .and_then(|v| v.get("flutter"))
                .is_some(),
        dart: doc
            .get("environment")
            .and_then(|e| e.get("sdk"))
            .and_then(Yaml::as_str)
            .and_then(Constraint::parse),
        flutter: doc
            .get("environment")
            .and_then(|e| e.get("flutter"))
            .and_then(Yaml::as_str)
            .and_then(Constraint::parse),
        ..Default::default()
    };
    env.dart = env.dart.map(Constraint::dart_sdk);
    env.flutter = env.flutter.map(Constraint::flutter_sdk);
    if let Some((_, dart, flutter)) = SDKS
        .lock()
        .unwrap()
        .get(path.parent().unwrap_or(Path::new(".")))
    {
        env.actual_dart = dart.clone();
        env.actual_flutter = flutter.clone();
    }
    let Ok(uri) = Url::from_file_path(path) else {
        return vec![];
    };
    reports_at(&doc, text, &env, Some(line), |name| {
        REGISTRY.known_package(name).map(Some).ok_or(())
    })
    .into_iter()
    .filter(|r| r.range.start.line == line)
    .filter_map(|r| {
        let (title, edit) = r.fix?;
        Some(CodeActionOrCommand::CodeAction(CodeAction {
            title,
            kind: Some(CodeActionKind::QUICKFIX),
            edit: Some(WorkspaceEdit {
                changes: Some(HashMap::from([(uri.clone(), vec![edit])])),
                ..Default::default()
            }),
            ..Default::default()
        }))
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn package() -> Package {
        serde_json::from_value(json!({"name":"demo","versions":[
        {"version":"1.0.0","pubspec":{"environment":{"sdk":">=2.0.0 <3.0.0"}}},
        {"version":"2.0.0","pubspec":{"environment":{"sdk":"^3.0.0"},"dependencies":{"shared":"^2.0.0"}}},
        {"version":"2.1.0","retracted":true,"pubspec":{"environment":{"sdk":"^3.0.0"}}},
        {"version":"3.0.0","pubspec":{"environment":{"sdk":"^4.0.0"}}},
        {"version":"4.0.0-dev.1","pubspec":{"environment":{"sdk":"^3.0.0"}}}
    ]})).unwrap()
    }
    fn doc(text: &str) -> Yaml {
        serde_yaml::from_str(text).unwrap()
    }
    fn env() -> Environment {
        Environment {
            dart: Constraint::parse("^3.0.0"),
            actual_dart: Some(Version::new(3, 5, 0)),
            ..Default::default()
        }
    }
    #[test]
    fn sdk_direct_conflicts_retractions_and_prereleases() {
        let package = package();
        let manifest = doc("dependencies: {demo: ^1.0.0, shared: ^2.0.0}");
        assert_eq!(best(&package, &manifest, &env()).unwrap().version, "2.0.0");
        assert!(
            usable(&package.versions[0], &manifest, &env())
                .unwrap_err()
                .contains("Dart")
        );
        assert!(
            usable(&package.versions[2], &manifest, &env())
                .unwrap_err()
                .contains("retracted")
        );
        let conflicting = doc("dependencies: {shared: ^1.0.0}");
        assert!(
            usable(&package.versions[1], &conflicting, &env())
                .unwrap_err()
                .contains("shared")
        );
        let override_doc = doc(
            "dependencies: {shared: ^1.0.0}\ndependency_overrides: {shared: ^1.0.0}",
        );
        assert!(usable(&package.versions[1], &override_doc, &env()).is_ok());
    }
    #[test]
    fn requests_understand_yaml_and_skip_other_dependency_sources() {
        let path = Path::new("/project/pubspec.yaml");
        for source in [
            "dependencies:\n  ht|",
            "dev_dependencies: {ht|: any}",
            "dependencies:\n  http: ^|",
            "dependency_overrides:\n  http:\n    version: ^1.|\n",
        ] {
            let offset = source.find('|').unwrap();
            let text = source.replace('|', "");
            assert!(
                completion_request(path, &text, offset).is_some(),
                "{source}"
            );
        }
        for source in [
            "description: ht|",
            "dependencies:\n  http:\n    path: ../http|",
            "dependencies:\n  http:\n    hosted: https://private.example\n    version: ^1.|",
            "dependencies:\n  http:\n    sdk: flutter\n    version: ^1.|",
        ] {
            let offset = source.find('|').unwrap();
            let text = source.replace('|', "");
            assert!(
                completion_request(path, &text, offset).is_none(),
                "{source}"
            );
        }
        let manifest = doc(
            "dependencies: {demo: ^1.0.0, local: {path: ../local}}\ndependency_overrides: {demo: {git: https://example.com/repo}}\n",
        );
        assert!(dependencies(&manifest).is_empty());
        let text = "dependencies:\n  http: \n  other: any\n";
        let offset = text.find("http: ").unwrap() + "http: ".len();
        let request = completion_request(path, text, offset).unwrap();
        assert_eq!(
            request.range,
            Range::new(Position::new(1, 8), Position::new(1, 8))
        );
        assert!(!valid_name("../private"));
        assert!(!valid_name("http?key=secret"));
    }
    #[test]
    fn compatibility_fixes_are_small_and_network_failures_are_not_errors() {
        let text = "name: app\nenvironment: {sdk: ^3.0.0}\ndependencies:\n  demo: ^99.0.0 # retain é🙂\n  shared: ^2.0.0\n";
        let reports = reports(&doc(text), text, &env(), |name| {
            if name == "demo" {
                Ok(Some(package()))
            } else {
                Err(())
            }
        });
        assert_eq!(reports.len(), 1);
        let (_, edit) = reports[0].fix.clone().unwrap();
        let fixed = crate::code_action_preview::edited(text, &[edit]).unwrap();
        assert!(fixed.contains("demo: ^2.0.0 # retain é🙂"));
        assert!(reports_empty(&fixed));
        assert!(super::reports(&doc(text), text, &env(), |_| Err(())).is_empty());
        let missing = super::reports(&doc(text), text, &env(), |_| Ok(None));
        assert!(missing[0].message.contains("not found"));
        fn reports_empty(text: &str) -> bool {
            super::reports(&doc(text), text, &env(), |name| {
                if name == "demo" {
                    Ok(Some(package()))
                } else {
                    Err(())
                }
            })
            .is_empty()
        }
    }
    #[test]
    fn registry_failure_pauses_other_requests_and_preserves_stale_metadata() {
        use std::{io::Write, net::TcpListener};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = listener.try_clone().unwrap();
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = server.accept().unwrap();
            let mut request = [0; 4096];
            stream.read(&mut request).unwrap();
            stream.write_all(b"HTTP/1.1 503 Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        });
        let cache = tempfile::tempdir().unwrap();
        std::fs::write(cache.path().join("old.json"), b"{\"name\":\"old\"}").unwrap();
        let registry = Registry::new(format!("http://{address}"), Some(cache.path().to_path_buf()));
        assert!(matches!(registry.get("old", "/old", Duration::ZERO), Reply::Data(_)));
        worker.join().unwrap();
        assert!(matches!(registry.get("other", "/other", PACKAGE_TTL), Reply::Failed));
        listener.set_nonblocking(true).unwrap();
        assert_eq!(listener.accept().unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
        assert!(cache.path().join("old.json").is_file());
    }
    #[test]
    fn documented_gzip_names_api_is_cached_in_memory_and_on_disk() {
        use std::{
            io::Write,
            net::TcpListener,
            sync::atomic::{AtomicUsize, Ordering},
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = calls.clone();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            let len = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..len]);
            assert!(request.contains("/api/package-name-completion-data"));
            assert!(
                request
                    .to_ascii_lowercase()
                    .contains("accept-encoding: gzip")
            );
            counted.fetch_add(1, Ordering::SeqCst);
            let mut encoder = flate2::write::GzEncoder::new(
                vec![],
                flate2::Compression::default(),
            );
            encoder
                .write_all(br#"{"packages":["http","provider","flutter_bloc"]}"#)
                .unwrap();
            let bytes = encoder.finish().unwrap();
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",bytes.len()).unwrap();
            stream.write_all(&bytes).unwrap();
        });
        let dir = tempfile::tempdir().unwrap();
        let registry =
            Registry::new(format!("http://{address}"), Some(dir.path().into()));
        assert_eq!(registry.names().len(), 3);
        assert_eq!(registry.names().len(), 3);
        server.join().unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let offline =
            Registry::new(format!("http://{address}"), Some(dir.path().into()));
        assert_eq!(offline.names().len(), 3);
        let cache_path = dir.path().join("names.json");
        let old_time =
            std::time::SystemTime::now() - NAME_TTL - Duration::from_secs(60);
        std::fs::File::open(&cache_path)
            .unwrap()
            .set_modified(old_time)
            .unwrap();
        let stale =
            Registry::new(format!("http://{address}"), Some(dir.path().into()));
        assert_eq!(stale.names().len(), 3);
        assert_eq!(
            std::fs::metadata(&cache_path).unwrap().modified().unwrap(),
            old_time
        );
        let source = "dependencies:\n  ht";
        let request = completion_request(
            Path::new("/project/pubspec.yaml"),
            source,
            source.len(),
        )
        .unwrap();
        let items = request.with_registry(&offline);
        assert_eq!(items[0].label, "http");
        let CompletionTextEdit::Edit(edit) = items[0].text_edit.as_ref().unwrap()
        else {
            panic!()
        };
        assert_eq!(
            crate::code_action_preview::edited(source, &[edit.clone()]).unwrap(),
            "dependencies:\n  http: "
        );
    }
    #[test]
    #[ignore = "live pub.dev API contract check; requires network"]
    fn live_pubdev_contract() {
        let dir = tempfile::tempdir().unwrap();
        let registry =
            Registry::new("https://pub.dev".into(), Some(dir.path().into()));
        let names = registry.names();
        assert!(names.iter().any(|n| n == "http"));
        let package = registry.package("http").unwrap().unwrap();
        assert_eq!(package.name, "http");
        assert!(!package.versions.is_empty());
        assert!(
            package
                .versions
                .iter()
                .any(|r| release_version(r).is_some())
        );
        assert!(registry.known_package("http").is_some());
        for suffix in ["", "^"] {
            let text = format!(
                "name: app\nenvironment: {{sdk: ^3.0.0}}\ndependencies:\n  http: {suffix}"
            );
            let request = completion_request(
                &dir.path().join("pubspec.yaml"),
                &text,
                text.len(),
            )
            .unwrap();
            let items = request.with_registry(&registry);
            assert!(!items.is_empty(), "no versions for {suffix:?}");
            let CompletionTextEdit::Edit(edit) =
                items[0].text_edit.as_ref().unwrap()
            else {
                panic!()
            };
            let fixed =
                crate::code_action_preview::edited(&text, &[edit.clone()]).unwrap();
            assert!(fixed.contains("http: ^"));
        }
    }
}
