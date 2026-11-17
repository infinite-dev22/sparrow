//! Deterministic, declarative scaffold plans. No template hooks are executed.
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pack {
    pub schema: u32,
    pub id: String,
    pub version: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub target: Target,
    #[serde(default)]
    pub category: Category,
    #[serde(default)]
    pub suffix: Option<String>,
    #[serde(default)]
    pub compatible_with: BTreeMap<String, String>,
    #[serde(default)]
    pub registration: Option<Registration>,
    #[serde(default)]
    pub disabled: bool,
    pub files: Vec<PackFile>,
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
    #[serde(skip)]
    pub loaded_inputs: Vec<(PathBuf, Option<String>)>,
}
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    Sibling,
    #[default]
    Package,
}
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Widget,
    State,
    Feature,
    #[default]
    Other,
}
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Registration {
    MvvmChangeNotifier,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackFile {
    pub path: String,
    #[serde(default)]
    pub content: String,
    pub source: Option<String>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dependency {
    pub package: String,
    pub version: Option<String>,
    pub sdk: Option<String>,
    #[serde(default)]
    pub dev: bool,
}
#[derive(Clone, Debug)]
pub struct Context {
    pub root: PathBuf,
    pub package: String,
    pub layout: String,
    pub feature_root: String,
    pub signals: String,
    pub registration: RegistrationSettings,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RegistrationSettings {
    pub router: Option<String>,
    pub providers: Option<String>,
}
#[derive(Clone, Debug)]
pub struct Change {
    pub path: PathBuf,
    pub before: Option<String>,
    pub after: Option<String>,
}
#[derive(Clone, Debug)]
pub struct Plan {
    pub root: PathBuf,
    pub title: String,
    pub changes: Vec<Change>,
    pub notes: Vec<String>,
    inputs: Vec<(PathBuf, Option<String>)>,
}
#[derive(Clone, Debug)]
pub struct Applied {
    pub plan: Plan,
    created_dirs: Vec<PathBuf>,
}
pub(crate) fn read(path: &Path) -> Result<Option<String>, String> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    if file.metadata().map_err(|e| e.to_string())?.len() > 2 * 1024 * 1024 {
        return Err(format!("{} exceeds 2 MB", path.display()));
    }
    let mut bytes = Vec::new();
    file.take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err(format!("{} exceeds 2 MB", path.display()));
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|e| format!("{}: {e}", path.display()))
}
fn relative(path: &str) -> Result<PathBuf, String> {
    let path = Path::new(path);
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(format!(
            "Expected a relative path without traversal: {}",
            path.display()
        ));
    }
    Ok(path.into())
}
/// Reject symlinks in every component, including dangling links and destinations.
fn checked_path(root: &Path, path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(root).map_err(|e| e.to_string())?;
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || root.canonicalize().map_err(|e| e.to_string())? != root
    {
        return Err("The target package directory changed. Preview again.".into());
    }
    let rel = path
        .strip_prefix(root)
        .map_err(|_| "Scaffold path escapes the target package")?;
    if rel.as_os_str().is_empty()
        || rel.components().any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("Invalid scaffold destination".into());
    }
    let mut current = root.to_path_buf();
    for component in rel.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(m) if m.file_type().is_symlink() => {
                return Err(format!(
                    "Symlinks are not scaffold destinations: {}",
                    current.display()
                ));
            }
            Ok(_) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}
pub fn context(workspace: &Path, selected: &Path) -> Result<Context, String> {
    let workspace = workspace.canonicalize().map_err(|e| e.to_string())?;
    let selected = selected.canonicalize().map_err(|e| e.to_string())?;
    if !selected.starts_with(&workspace) {
        return Err("Choose a folder inside the workspace".into());
    }
    let mut root = if selected.is_file() {
        selected.parent().unwrap().to_path_buf()
    } else {
        selected
    };
    loop {
        if root.join("pubspec.yaml").is_file() {
            break;
        }
        if root == workspace || !root.pop() {
            return Err("No pubspec.yaml found for the selected folder".into());
        }
    }
    let pubspec: serde_yaml::Value =
        serde_yaml::from_str(&read(&root.join("pubspec.yaml"))?.unwrap_or_default())
            .map_err(|e| e.to_string())?;
    let package = pubspec
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or("pubspec.yaml needs a package name")?
        .to_string();
    if !package
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return Err("Invalid Dart package name".into());
    }
    let config = root.join(".sparrow/scaffold.yaml");
    #[derive(Default, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Conventions {
        layout: Option<String>,
        #[serde(default)]
        registration: RegistrationSettings,
    }
    let pinned: Conventions = match read(&config)? {
        Some(s) => serde_yaml::from_str(&s)
            .map_err(|e| format!("{}: {e}", config.display()))?,
        None => Conventions::default(),
    };
    let (detected, feature_root) = if root.join("lib/features").is_dir() {
        ("feature-first", "lib/features")
    } else if root.join("lib/feature").is_dir() {
        ("feature-first", "lib/feature")
    } else if ["lib/presentation", "lib/data", "lib/domain"]
        .iter()
        .any(|p| root.join(p).is_dir())
    {
        ("layer-first", "lib/features")
    } else {
        ("feature-first", "lib/features")
    };
    let layout = pinned.layout.unwrap_or_else(|| detected.into());
    if !["feature-first", "layer-first"].contains(&layout.as_str()) {
        return Err(
            "scaffold.yaml layout must be feature-first or layer-first".into()
        );
    }
    let states: Vec<&str> = [
        "flutter_bloc",
        "bloc",
        "flutter_riverpod",
        "provider",
        "get",
        "mobx",
        "signals",
    ]
    .into_iter()
    .filter(|p| {
        pubspec
            .get("dependencies")
            .and_then(|d| d.get(*p))
            .is_some()
    })
    .collect();
    Ok(Context {
        registration: pinned.registration,
        root,
        package,
        layout,
        feature_root: feature_root.into(),
        signals: if states.is_empty() {
            "No external state package declared".into()
        } else {
            format!("Declared state packages: {}", states.join(", "))
        },
    })
}
pub fn load_pack(path: &Path) -> Result<Pack, String> {
    if fs::symlink_metadata(path)
        .map_err(|e| e.to_string())?
        .file_type()
        .is_symlink()
    {
        return Err("Pack manifests cannot be symlinks".into());
    }
    let text = read(path)?.ok_or("Pack manifest is missing")?;
    let base = path
        .parent()
        .ok_or("Missing pack folder")?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let mut observed = vec![(path.to_path_buf(), Some(text.clone()))];
    let mut pack = parse_pack(&text, |source| {
        let path = base.join(relative(source)?);
        checked_path(&base, &path)?;
        let text = read(&path)?
            .ok_or_else(|| format!("Missing template source: {}", path.display()))?;
        observed.push((path, Some(text.clone())));
        Ok(text)
    })?;
    pack.loaded_inputs = observed;
    Ok(pack)
}
pub(crate) fn parse_pack(
    text: &str,
    mut source: impl FnMut(&str) -> Result<String, String>,
) -> Result<Pack, String> {
    let mut pack: Pack = serde_yaml::from_str(text).map_err(|e| e.to_string())?;
    let mut total = 0usize;
    for file in &mut pack.files {
        if let Some(path) = &file.source {
            if !file.content.is_empty() {
                return Err("Specify content or source, not both".into());
            }
            relative(path)?;
            file.content = source(path)?;
        }
        total = total.saturating_add(file.content.len());
        if total > 2 * 1024 * 1024 {
            return Err("Pack contents exceed 2 MB".into());
        }
    }
    validate_pack(&pack)?;
    Ok(pack)
}
pub(crate) fn validate_pack(pack: &Pack) -> Result<(), String> {
    if ![1, 2].contains(&pack.schema) {
        return Err("Unsupported scaffold schema; expected 1 or 2".into());
    }
    if !pack
        .id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
        || pack.id == "."
        || pack.id == ".."
    {
        return Err("Pack id must be a safe directory name".into());
    }
    if pack.registration.is_some() && pack.target != Target::Package {
        return Err("Registration requires a package-target pack".into());
    }
    if let Some(suffix) = &pack.suffix {
        if !suffix.is_empty()
            && (!suffix.starts_with('_')
                || !suffix.chars().all(|c| {
                    c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit()
                }))
        {
            return Err("Suffix must use lower_snake_case starting with _".into());
        }
    }
    for requirement in pack.compatible_with.values() {
        version_requirement(requirement)?;
    }
    semver::Version::parse(&pack.version)
        .map_err(|e| format!("Invalid pack version: {e}"))?;
    if pack.id.is_empty()
        || pack.title.is_empty()
        || pack.files.is_empty()
        || pack.files.len() > 64
    {
        return Err("A pack needs an id, title and 1–64 files".into());
    }
    if pack.files.iter().map(|f| f.content.len()).sum::<usize>() > 2 * 1024 * 1024 {
        return Err("Pack contents exceed 2 MB".into());
    }
    for dep in &pack.dependencies {
        if dep.package.is_empty()
            || !dep
                .package
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            return Err("Invalid dependency package name".into());
        }
        if dep.sdk.as_deref().is_some_and(|sdk| sdk != "flutter")
            || dep.sdk.is_some() == dep.version.is_some()
        {
            return Err(format!(
                "{} needs either sdk: flutter or a version constraint",
                dep.package
            ));
        }
        if let Some(v) = &dep.version {
            version_requirement(v)
                .map_err(|e| format!("{} version constraint: {e}", dep.package))?;
        }
    }
    Ok(())
}
fn version_requirement(value: &str) -> Result<semver::VersionReq, String> {
    let value = value.trim();
    if value == "any" {
        return Ok(semver::VersionReq::STAR);
    }
    if value.is_empty() || value.contains([',', '\'', '\n', '\r']) {
        return Err("Use a Pub caret, exact version, or whitespace-separated comparison range".into());
    }
    let requirement = if value.starts_with(|c: char| c.is_ascii_digit()) {
        format!("={value}")
    } else {
        value.split_whitespace().collect::<Vec<_>>().join(", ")
    };
    semver::VersionReq::parse(&requirement).map_err(|e| e.to_string())
}
fn inputs(root: &Path) -> Result<Vec<(PathBuf, Option<String>)>, String> {
    ["pubspec.yaml", "pubspec.lock", ".sparrow/scaffold.yaml"]
        .into_iter()
        .map(|p| {
            let path = root.join(p);
            checked_path(root, &path)?;
            let text = read(&path)?;
            Ok((path, text))
        })
        .collect()
}
fn substitute(text: &str, vars: &BTreeMap<&str, String>) -> Result<String, String> {
    let mut result = String::new();
    let mut remaining = text;
    while let Some(start) = remaining.find("{{") {
        result.push_str(&remaining[..start]);
        let tail = &remaining[start + 2..];
        // A Dart named-parameter brace can immediately precede {{name}}.
        // Treat the outer brace literally, matching the original renderer.
        if tail.starts_with('{') {
            result.push('{');
            remaining = &remaining[start + 1..];
            continue;
        }
        let end = tail.find("}}").ok_or("Unclosed template variable")?;
        let key = tail[..end].trim();
        let value = vars
            .get(key)
            .ok_or_else(|| format!("Unknown template variable: {key}"))?;
        result.push_str(value);
        remaining = &tail[end + 2..];
    }
    result.push_str(remaining);
    Ok(result)
}
/// Add only absent dependencies, retaining comments and every existing constraint.
fn dependencies(
    before: &str,
    deps: &[Dependency],
) -> Result<(String, Vec<String>), String> {
    let original: serde_yaml::Value =
        serde_yaml::from_str(before).map_err(|e| format!("pubspec.yaml: {e}"))?;
    let mut after = before.to_string();
    let mut added = vec![];
    for dep in deps {
        let section = if dep.dev {
            "dev_dependencies"
        } else {
            "dependencies"
        };
        if original
            .get("dependencies")
            .and_then(|v| v.get(&dep.package))
            .is_some()
            || original
                .get("dev_dependencies")
                .and_then(|v| v.get(&dep.package))
                .is_some()
            || added.contains(&dep.package)
        {
            continue;
        }
        let entry = if dep.sdk.is_some() {
            format!("  {}:\n    sdk: flutter\n", dep.package)
        } else {
            format!("  {}: '{}'\n", dep.package, dep.version.as_deref().unwrap())
        };
        let mut offset = 0;
        let mut insertion = None;
        for line in after.split_inclusive('\n') {
            if line.split('#').next().unwrap_or("").trim_end()
                == format!("{section}:")
            {
                insertion = Some(offset + line.len());
                break;
            }
            offset += line.len();
        }
        if let Some(index) = insertion {
            if index == after.len() && !after.ends_with('\n') {
                after.push('\n');
            }
            let index = if index > 0 && !after[..index].ends_with('\n') {
                index + 1
            } else {
                index
            };
            after.insert_str(index, &entry);
        } else if original.get(section).is_some() {
            return Err(format!(
                "Use a block mapping for {section} before adding scaffold dependencies"
            ));
        } else {
            if !after.ends_with('\n') {
                after.push('\n');
            }
            after.push_str(&format!("{section}:\n{entry}"));
        }
        added.push(dep.package.clone());
    }
    // Parsing the result catches unsupported YAML shapes rather than writing them.
    let result: serde_yaml::Value = serde_yaml::from_str(&after)
        .map_err(|e| format!("Cannot safely update pubspec.yaml: {e}"))?;
    for (key, value) in original
        .as_mapping()
        .ok_or("pubspec.yaml must be a mapping")?
    {
        if key
            .as_str()
            .is_some_and(|s| s == "dependencies" || s == "dev_dependencies")
        {
            if value.is_null() {
                continue;
            }
            for (name, constraint) in value
                .as_mapping()
                .ok_or("Dependency sections must be mappings")?
            {
                if result.get(key).and_then(|v| v.get(name)) != Some(constraint) {
                    return Err(
                        "Dependency update would alter an existing constraint"
                            .into(),
                    );
                }
            }
        } else if result.get(key) != Some(value) {
            return Err(
                "Dependency update would change unrelated project settings".into()
            );
        }
    }
    Ok((after, added))
}
pub fn name_parts(name: &str) -> Result<(String, String), String> {
    if name.is_empty()
        || !name.as_bytes()[0].is_ascii_uppercase()
        || !name.chars().all(|c| c.is_ascii_alphanumeric())
    {
        return Err(
            "Use a PascalCase Dart class name, for example CounterCubit.".into(),
        );
    }
    let mut snake = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            snake.push('_');
        }
        snake.push(c.to_ascii_lowercase());
    }
    Ok((
        snake,
        format!("{}{}", name[..1].to_ascii_lowercase(), &name[1..]),
    ))
}
pub fn plan_pack(ctx: &Context, pack: &Pack, name: &str) -> Result<Plan, String> {
    if pack.target != Target::Package {
        return Err("Sibling packs need the selected directory".into());
    }
    plan_pack_at(ctx, &ctx.root, pack, name)
}
pub fn plan_sibling(
    ctx: &Context,
    base: &Path,
    pack: &Pack,
    name: &str,
) -> Result<Plan, String> {
    if pack.target != Target::Sibling {
        return Err("Expected a sibling-target pack".into());
    }
    plan_pack_at(ctx, base, pack, name)
}
fn plan_pack_at(
    ctx: &Context,
    base: &Path,
    pack: &Pack,
    name: &str,
) -> Result<Plan, String> {
    validate_pack(pack)?;
    if pack.disabled {
        return Err("This pack is disabled".into());
    }
    let base = base.canonicalize().map_err(|e| e.to_string())?;
    if !base.starts_with(&ctx.root) || !base.is_dir() {
        return Err("Choose a directory inside the target package".into());
    }
    let (snake, variable) = name_parts(name)?;
    let suffix = pack.suffix.as_deref().unwrap_or("");
    let suffixed_snake = if snake.ends_with(suffix) {
        snake.clone()
    } else {
        format!("{snake}{suffix}")
    };
    let pascal_suffix = suffix
        .split('_')
        .filter(|s| !s.is_empty())
        .map(|s| format!("{}{}", s[..1].to_ascii_uppercase(), &s[1..]))
        .collect::<String>();
    let suffixed_name = if name.ends_with(&pascal_suffix) {
        name.to_string()
    } else {
        format!("{name}{pascal_suffix}")
    };
    let feature = format!("{}/{snake}", ctx.feature_root);
    let layer = ctx.layout == "layer-first";
    let vars = BTreeMap::from([
        ("name", name.to_string()),
        ("suffixed_name", suffixed_name),
        ("suffixed_snake", suffixed_snake),
        ("snake", snake.clone()),
        ("variable", variable),
        ("package", ctx.package.clone()),
        ("feature_root", feature.clone()),
        (
            "screen_dir",
            if layer {
                format!("lib/presentation/{snake}")
            } else {
                format!("{feature}/presentation")
            },
        ),
        (
            "state_dir",
            if layer {
                format!("lib/presentation/{snake}")
            } else {
                format!("{feature}/presentation")
            },
        ),
        (
            "model_dir",
            if layer {
                format!("lib/domain/{snake}")
            } else {
                format!("{feature}/domain")
            },
        ),
        (
            "repository_dir",
            if layer {
                format!("lib/data/{snake}")
            } else {
                format!("{feature}/data")
            },
        ),
        (
            "screen_import",
            if layer {
                format!("presentation/{snake}")
            } else {
                format!("features/{snake}/presentation").replacen(
                    "features/",
                    &format!("{}/", ctx.feature_root.trim_start_matches("lib/")),
                    1,
                )
            },
        ),
        (
            "model_import",
            if layer {
                format!("domain/{snake}")
            } else {
                format!(
                    "{}/{snake}/domain",
                    ctx.feature_root.trim_start_matches("lib/")
                )
            },
        ),
        (
            "repository_import",
            if layer {
                format!("data/{snake}")
            } else {
                format!(
                    "{}/{snake}/data",
                    ctx.feature_root.trim_start_matches("lib/")
                )
            },
        ),
    ]);
    let mut changes = vec![];
    let mut seen = BTreeSet::new();
    for file in &pack.files {
        let output = relative(&substitute(&file.path, &vars)?)?;
        let path = base.join(output);
        let rel = path
            .strip_prefix(&ctx.root)
            .map_err(|_| "Output escapes target package")?
            .to_path_buf();
        if rel.components().next().is_some_and(|c| {
            [".sparrow", ".git", ".dart_tool", "build"]
                .iter()
                .any(|p| c.as_os_str() == *p)
        }) || rel == Path::new("pubspec.yaml")
        {
            return Err("Pack files cannot replace pubspec.yaml or IDE configuration; declare dependencies instead".into());
        }
        if !seen.insert(rel.clone()) {
            return Err(format!("Duplicate output: {}", rel.display()));
        }
        checked_path(&ctx.root, &path)?;
        let after = substitute(&file.content, &vars)?;
        let before = read(&path)?;
        if before.as_ref().is_some_and(|old| old != &after) {
            return Err(format!(
                "Already exists with different content: {}",
                path.display()
            ));
        }
        if before.as_ref() != Some(&after) {
            changes.push(Change {
                path,
                before,
                after: Some(after),
            });
        }
    }
    let path = ctx.root.join("pubspec.yaml");
    checked_path(&ctx.root, &path)?;
    let before = read(&path)?.ok_or("pubspec.yaml is missing")?;
    let (after, added) = dependencies(&before, &pack.dependencies)?;
    let mut notes = vec![
        format!(
            "Package: {} · {} · {}",
            ctx.package,
            ctx.layout,
            ctx.root.display()
        ),
        ctx.signals.clone(),
        format!("Pack {} @ {}", pack.id, pack.version),
        pack.description.clone(),
    ];
    if before != after {
        changes.push(Change {
            path,
            before: Some(before),
            after: Some(after),
        });
        notes.push(format!("Adds {}. Run Pub Get after applying; dependency resolution is a separate visible step.",added.join(", ")));
    }
    let lock: Option<serde_yaml::Value> = read(&ctx.root.join("pubspec.lock"))?
        .and_then(|s| serde_yaml::from_str(&s).ok());
    for (package, req) in pack
        .dependencies
        .iter()
        .filter_map(|d| d.version.as_deref().map(|v| (d.package.as_str(), v)))
        .chain(
            pack.compatible_with
                .iter()
                .map(|(p, v)| (p.as_str(), v.as_str())),
        )
    {
        if let Some(version) = lock
            .as_ref()
            .and_then(|v| v.get("packages"))
            .and_then(|v| v.get(package))
            .and_then(|v| v.get("version"))
            .and_then(|v| v.as_str())
        {
            if !version_requirement(req)?.matches(
                &semver::Version::parse(version).map_err(|e| e.to_string())?,
            ) {
                notes.push(format!("Compatibility warning: {} resolved to {version}; this pack targets {req}. Existing constraints are preserved.",package));
            }
        }
    }
    Ok(Plan {
        root: ctx.root.clone(),
        title: pack.title.clone(),
        changes,
        notes,
        inputs: inputs(&ctx.root)?
            .into_iter()
            .chain(pack.loaded_inputs.clone())
            .collect(),
    })
}
pub(crate) fn plan_export(
    ctx: &Context,
    files: Vec<(PathBuf, String)>,
) -> Result<Plan, String> {
    let mut changes = vec![];
    for (rel, after) in files {
        let relative = relative(rel.to_str().ok_or("Invalid export path")?)?;
        let path = ctx.root.join(".sparrow/scaffolds").join(relative);
        checked_path(&ctx.root, &path)?;
        let before = read(&path)?;
        if before.as_ref().is_some_and(|b| b != &after) {
            return Err(format!(
                "Export destination already exists: {}",
                path.display()
            ));
        }
        if before.as_ref() != Some(&after) {
            changes.push(Change {
                path,
                before,
                after: Some(after),
            });
        }
    }
    Ok(Plan{root:ctx.root.clone(),title:"Export bundled scaffold".into(),changes,notes:vec!["Copy the bundled default into this package's .sparrow/scaffolds folder. Edit its files and reopen New to use the override.".into()],inputs:inputs(&ctx.root)?})
}
/// Management operations use the same conflict checks, atomic commit and undo.
pub(crate) fn management_plan(
    root: &Path, title: &str, files: Vec<(PathBuf, Option<String>)>,
    create_only: bool,
) -> Result<Plan, String> {
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let mut changes = vec![];
    let mut seen = std::collections::BTreeSet::new();
    for (path, after) in files {
        checked_path(&root, &path)?;
        if !seen.insert(path.clone()) { return Err("Duplicate management destination".into()); }
        let before = read(&path)?;
        if create_only && before.is_some() { return Err(format!("Destination already exists: {}", path.display())); }
        if before != after { changes.push(Change { path, before, after }); }
    }
    let plan = Plan { root, title: title.into(), changes, notes: vec![], inputs: vec![] };
    plan.validate()?;
    Ok(plan)
}
impl Plan {
    pub(crate) fn with_management_inputs(mut self, inputs: Vec<(PathBuf, Option<String>)>) -> Result<Self, String> {
        self.inputs.extend(inputs);
        self.validate()?;
        Ok(self)
    }
    /// Merge only snapshot-checked existing Dart edits into this transaction.
    pub(crate) fn add_registration(
        &mut self,
        changes: Vec<Change>,
        sources: Vec<(PathBuf, Option<String>)>,
        notes: Vec<String>,
    ) -> Result<(), String> {
        for change in &changes {
            checked_path(&self.root, &change.path)?;
            if !change.path.starts_with(self.root.join("lib"))
                || change.path.extension().is_none_or(|e| e != "dart")
                || change.before.is_none()
                || change.after.is_none()
                || self.changes.iter().any(|c| c.path == change.path)
            {
                return Err(
                    "Analyzer registration returned an invalid or overlapping edit"
                        .into(),
                );
            }
        }
        for (path, _) in &sources {
            checked_path(&self.root, path)?;
        }
        self.changes.extend(changes);
        self.inputs.extend(sources);
        self.notes.extend(notes);
        self.validate()
    }
    pub fn validate(&self) -> Result<(), String> {
        for (path, before) in &self.inputs {
            if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err("Template input became a symlink".into());
            }
            if read(path)? != *before {
                return Err(format!(
                    "Project settings changed since preview: {}. Preview again.",
                    path.display()
                ));
            }
        }
        for change in &self.changes {
            checked_path(&self.root, &change.path)?;
            if read(&change.path)? != change.before {
                return Err(format!(
                    "Changed since preview: {}. Preview again.",
                    change.path.display()
                ));
            }
        }
        Ok(())
    }
    pub fn diff(&self, index: usize) -> String {
        let Some(c) = self.changes.get(index) else {
            return "No changes. This scaffold is already applied.".into();
        };
        let path = c.path.strip_prefix(&self.root).unwrap_or(&c.path).display();
        let before = c.before.as_deref().unwrap_or("");
        let after = c.after.as_deref().unwrap_or("");
        let mut diff = format!(
            "--- {}\n+++ {}\n",
            if c.before.is_some() {
                path.to_string()
            } else {
                "/dev/null".into()
            },
            if c.after.is_some() {
                path.to_string()
            } else {
                "/dev/null".into()
            }
        );
        let before: Vec<_> = before.lines().collect();
        let after: Vec<_> = after.lines().collect();
        let prefix = before
            .iter()
            .zip(&after)
            .take_while(|(a, b)| a == b)
            .count();
        let suffix = before[prefix..]
            .iter()
            .rev()
            .zip(after[prefix..].iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
        let start = prefix.saturating_sub(3);
        let tail = suffix.min(3);
        let old_end = before.len() - suffix;
        let new_end = after.len() - suffix;
        diff.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            if before.is_empty() { 0 } else { start + 1 },
            old_end + tail - start,
            if after.is_empty() { 0 } else { start + 1 },
            new_end + tail - start
        ));
        for line in &before[start..prefix] {
            diff.push_str(&format!(" {line}\n"));
        }
        for line in &before[prefix..old_end] {
            diff.push_str(&format!("-{line}\n"));
        }
        for line in &after[prefix..new_end] {
            diff.push_str(&format!("+{line}\n"));
        }
        for line in &after[new_end..new_end + tail] {
            diff.push_str(&format!(" {line}\n"));
        }
        diff
    }
    pub fn apply(&self) -> Result<Applied, String> {
        self.validate()?;
        let mut dirs = vec![];
        let result = commit(&self.root, &self.changes, &mut dirs);
        if result.is_err() {
            for dir in dirs.iter().rev() {
                let _ = fs::remove_dir(dir);
            }
        }
        result?;
        Ok(Applied {
            plan: self.clone(),
            created_dirs: dirs,
        })
    }
}
// Prepare all bytes before the first mutation. Created files never clobber a
// destination, and rollback restores only writes made by this transaction.
fn commit(
    root: &Path,
    changes: &[Change],
    dirs: &mut Vec<PathBuf>,
) -> Result<(), String> {
    commit_with_hook(root, changes, dirs, |_, _| Ok(()))
}
fn commit_with_hook(
    root: &Path,
    changes: &[Change],
    dirs: &mut Vec<PathBuf>,
    mut before_write: impl FnMut(usize, &[Change]) -> Result<(), String>,
) -> Result<(), String> {
    let mut staged = vec![];
    for change in changes {
        checked_path(root, &change.path)?;
        let parent = change.path.parent().ok_or("Missing parent")?;
        let mut missing = vec![];
        let mut p = parent;
        while !p.exists() {
            missing.push(p.to_path_buf());
            p = p.parent().ok_or("Missing package directory")?;
        }
        for p in missing.into_iter().rev() {
            fs::create_dir(&p).map_err(|e| e.to_string())?;
            dirs.push(p);
        }
        let temporary = if let Some(after) = &change.after {
            let mut file = tempfile::NamedTempFile::new_in(parent)
                .map_err(|e| e.to_string())?;
            file.write_all(after.as_bytes())
                .map_err(|e| e.to_string())?;
            if change.before.is_some() {
                let metadata =
                    fs::metadata(&change.path).map_err(|e| e.to_string())?;
                file.as_file()
                    .set_permissions(metadata.permissions())
                    .map_err(|e| e.to_string())?;
            }
            Some(file)
        } else {
            None
        };
        staged.push(temporary);
    }
    let mut completed: Vec<&Change> = vec![];
    for (index, (change, temporary)) in changes.iter().zip(staged).enumerate() {
        let result = (|| {
            before_write(index, changes)?;
            checked_path(root, &change.path)?;
            if read(&change.path)? != change.before {
                return Err(format!(
                    "Changed during apply: {}",
                    change.path.display()
                ));
            }
            match temporary {
                Some(file) if change.before.is_none() => file
                    .persist_noclobber(&change.path)
                    .map(|_| ())
                    .map_err(|e| e.to_string()),
                Some(file) => file
                    .persist(&change.path)
                    .map(|_| ())
                    .map_err(|e| e.to_string()),
                None => fs::remove_file(&change.path).map_err(|e| e.to_string()),
            }
        })();
        if let Err(error) = result {
            let mut failures = vec![];
            for c in completed.into_iter().rev() {
                // Do not overwrite a concurrent edit even during rollback.
                let restored = if read(&c.path).ok() != Some(c.after.clone()) {
                    Err("File changed after scaffold write".into())
                } else if let Some(before) = &c.before {
                    fs::write(&c.path, before).map_err(|e| e.to_string())
                } else {
                    fs::remove_file(&c.path).map_err(|e| e.to_string())
                };
                if let Err(e) = restored {
                    failures.push(format!("{}: {e}", c.path.display()));
                }
            }
            return Err(if failures.is_empty() {
                error
            } else {
                format!("{error}; rollback needs attention: {}", failures.join("; "))
            });
        }
        completed.push(change);
    }
    Ok(())
}
#[cfg(test)]
mod failure_tests {
    use super::*;
    #[test]
    fn mid_commit_failure_restores_edits_and_removes_created_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let edited = root.join("existing.dart");
        fs::write(&edited, "original").unwrap();
        let created = root.join("new.dart");
        let changes = vec![
            Change {
                path: edited.clone(),
                before: Some("original".into()),
                after: Some("replacement".into()),
            },
            Change {
                path: created.clone(),
                before: None,
                after: Some("generated".into()),
            },
            Change {
                path: root.join("last.dart"),
                before: None,
                after: Some("never written".into()),
            },
        ];
        let result = commit_with_hook(&root, &changes, &mut vec![], |i, _| {
            if i == 2 {
                Err("injected write failure".into())
            } else {
                Ok(())
            }
        });
        assert!(result.unwrap_err().contains("injected"));
        assert_eq!(fs::read_to_string(edited).unwrap(), "original");
        assert!(!created.exists());
        assert!(!root.join("last.dart").exists());
    }
    #[test]
    fn rollback_preserves_a_concurrent_user_edit_and_reports_it() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let path = root.join("first.dart");
        let changes = vec![
            Change {
                path: path.clone(),
                before: None,
                after: Some("generated".into()),
            },
            Change {
                path: root.join("second.dart"),
                before: None,
                after: Some("second".into()),
            },
        ];
        let result = commit_with_hook(&root, &changes, &mut vec![], |i, _| {
            if i == 1 {
                fs::write(&path, "concurrent user edit").unwrap();
                Err("injected failure".into())
            } else {
                Ok(())
            }
        });
        assert!(result.unwrap_err().contains("rollback needs attention"));
        assert_eq!(fs::read_to_string(path).unwrap(), "concurrent user edit");
        assert!(!root.join("second.dart").exists());
    }
}
impl Applied {
    pub fn undo_plan(&self) -> Plan {
        Plan { root:self.plan.root.clone(),title:format!("Undo {}",self.plan.title),notes:vec!["Removes generated files and restores edited files only if they still match the applied scaffold.".into()],changes:self.plan.changes.iter().map(|c|Change{path:c.path.clone(),before:c.after.clone(),after:c.before.clone()}).collect(),inputs:vec![] }
    }
    pub fn undo(&self) -> Result<(), String> {
        self.undo_plan().apply()?;
        for dir in self.created_dirs.iter().rev() {
            let _ = fs::remove_dir(dir);
        }
        Ok(())
    }
}
