//! One catalog for bundled, user and team packs, with explicit provenance.
use super::engine::{self, Category, Pack, PackFile};
use include_dir::{Dir, include_dir};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
static BUNDLED: Dir = include_dir!("$CARGO_MANIFEST_DIR/resources/scaffolds");
#[derive(Clone, Debug)]
enum Origin {
    Bundled(String),
    File(PathBuf),
    Legacy(PathBuf),
}
#[derive(Clone, Debug)]
pub struct Entry {
    pub id: String,
    pub title: String,
    pub category: Category,
    pub provenance: String,
    pub registration: bool,
    origin: Origin,
    error: Option<String>,
    disabled: bool,
}
impl Entry {
    pub fn load(&self) -> Result<Pack, String> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        match &self.origin {
            Origin::Bundled(id) => bundled(id),
            Origin::File(path) => engine::load_pack(path),
            Origin::Legacy(path) => {
                if path.is_symlink() {
                    return Err("Legacy template cannot be a symlink".into());
                }
                let body =
                    engine::read(path)?.ok_or("Legacy template is missing")?;
                let mut pack:Pack=serde_yaml::from_str(&format!("schema: 2\nid: {}\nversion: 0.0.0\ntitle: Custom\ntarget: sibling\nfiles: []",self.id)).map_err(|e|e.to_string())?;
                pack.title = self.title.clone();
                pack.files = vec![PackFile {
                    path: "{{snake}}.dart".into(),
                    content: body.clone(),
                    source: None,
                }];
                pack.loaded_inputs = vec![(path.clone(), Some(body))];
                engine::validate_pack(&pack)?;
                Ok(pack)
            }
        }
    }
}
fn entry(id: String, origin: Origin, provenance: String) -> Entry {
    let mut e = Entry {
        id: id.clone(),
        title: format!("Invalid pack: {id}"),
        category: Category::Other,
        registration: false,
        origin,
        error: None,
        provenance,
        disabled: false,
    };
    match e.load() {
        Ok(p) => {
            e.id = p.id;
            e.title = p.title;
            e.category = p.category;
            e.registration = p.registration.is_some();
            e.disabled = p.disabled;
        }
        Err(error) => e.error = Some(error),
    }
    e
}
pub fn bundled(id: &str) -> Result<Pack, String> {
    let manifest = BUNDLED
        .get_file(format!("{id}/pack.yaml"))
        .and_then(|f| f.contents_utf8())
        .ok_or("Bundled pack not found")?;
    engine::parse_pack(manifest, |source| {
        BUNDLED
            .get_file(format!("{id}/{source}"))
            .and_then(|f| f.contents_utf8())
            .map(String::from)
            .ok_or_else(|| format!("Missing bundled source {source}"))
    })
}
pub fn user_root() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|d| d.config_dir().join("sparrow/scaffolds"))
}
pub fn bundled_entries() -> Vec<Entry> {
    let mut entries = BUNDLED
        .dirs()
        .map(|dir| {
            let id = dir
                .path()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string();
            entry(id.clone(), Origin::Bundled(id), "Bundled".into())
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|e| e.title.clone());
    entries
}
pub fn discover(root: &Path) -> Vec<Entry> {
    discover_at(user_root().as_deref(), root)
}
pub(crate) fn discover_at(global: Option<&Path>, root: &Path) -> Vec<Entry> {
    let mut packs = BTreeMap::new();
    for dir in BUNDLED.dirs() {
        let id = dir
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        packs.insert(
            id.clone(),
            entry(id.clone(), Origin::Bundled(id), "Bundled".into()),
        );
    }
    if let Some(global) = global {
        scan(global, "User", &mut packs);
    }
    scan(&root.join(".sparrow/scaffolds"), "Project", &mut packs);
    let legacy = root.join(".sparrow/templates");
    if !legacy.is_symlink() {
        if let Ok(entries) = fs::read_dir(&legacy) {
            let mut paths = entries.flatten().map(|e| e.path()).collect::<Vec<_>>();
            paths.sort();
            for path in paths {
                if path.is_file()
                    && !path.is_symlink()
                    && path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .is_some_and(|s| s.ends_with(".dart.tmpl"))
                {
                    let stem = path
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .trim_end_matches(".dart.tmpl")
                        .to_string();
                    let id = format!("local.template.{stem}");
                    let mut e = entry(
                        id.clone(),
                        Origin::Legacy(path.clone()),
                        format!("Legacy: {}", path.display()),
                    );
                    e.title = format!("Custom: {stem}");
                    packs.insert(id, e);
                }
            }
        }
    }
    let mut entries = packs
        .into_values()
        .filter(|e| !e.disabled)
        .collect::<Vec<_>>();
    entries.sort_by_key(|e| {
        (
            match e.category {
                Category::Widget => 0,
                Category::Feature => 1,
                Category::State => 2,
                Category::Other => 3,
            },
            e.title.clone(),
            e.id.clone(),
        )
    });
    entries
}
fn scan(root: &Path, label: &str, packs: &mut BTreeMap<String, Entry>) {
    if root.is_symlink() {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    let mut paths = entries.flatten().map(|e| e.path()).collect::<Vec<_>>();
    paths.sort();
    let mut seen = BTreeMap::new();
    for path in paths {
        if !path.is_dir() || path.is_symlink() {
            continue;
        }
        let manifest = path.join("pack.yaml");
        if !manifest.is_file() {
            continue;
        }
        let fallback = engine::read(&manifest)
            .ok()
            .flatten()
            .and_then(|s| serde_yaml::from_str::<serde_yaml::Value>(&s).ok())
            .and_then(|v| v.get("id").and_then(|v| v.as_str()).map(String::from))
            .unwrap_or_else(|| {
                path.file_name().unwrap().to_string_lossy().to_string()
            });
        let mut e = entry(
            fallback,
            Origin::File(manifest.clone()),
            format!("{label}: {}", manifest.display()),
        );
        if seen.insert(e.id.clone(), manifest).is_some() {
            e.error =
                Some(format!("Duplicate pack id {} in {}", e.id, root.display()));
            e.disabled = false;
            e.title = format!("Invalid duplicate: {}", e.id);
        }
        packs.insert(e.id.clone(), e);
    }
}
pub fn feature(root: &Path) -> Result<Entry, String> {
    discover(root)
        .into_iter()
        .find(|e| e.id == "sparrow.feature.mvvm")
        .ok_or_else(|| "The MVVM feature pack is disabled or missing".into())
}
pub fn export_files(id: &str) -> Result<Vec<(PathBuf, String)>, String> {
    bundled(id)?;
    fn walk(dir: &Dir, output: &mut Vec<(PathBuf, String)>) -> Result<(), String> {
        for file in dir.files() {
            output.push((
                file.path().to_path_buf(),
                file.contents_utf8()
                    .ok_or("Non-text bundled template")?
                    .to_string(),
            ));
        }
        for child in dir.dirs() {
            walk(child, output)?;
        }
        Ok(())
    }
    let mut files = vec![];
    walk(
        BUNDLED.get_dir(id).ok_or("Bundled pack not found")?,
        &mut files,
    )?;
    Ok(files)
}
