//! Manage template data; all mutations are previewed transactions.
mod editor;
mod view;

use super::{catalog, engine};
use crate::window_tab::WindowTabData;
use floem::reactive::{RwSignal, Scope, SignalGet, SignalUpdate};
use std::{
    fs,
    path::{Path, PathBuf},
};
pub(super) use view::view;

#[derive(Clone)]
pub(super) struct State {
    root: RwSignal<PathBuf>,
    entries: RwSignal<Vec<catalog::Entry>>,
    selected: RwSignal<Option<usize>>,
    query: RwSignal<String>,
    id: RwSignal<String>,
    global: RwSignal<bool>,
    editor: editor::State,
}
impl State {
    pub fn new(cx: Scope) -> Self {
        Self {
            root: cx.create_rw_signal(PathBuf::new()),
            entries: cx.create_rw_signal(vec![]),
            selected: cx.create_rw_signal(None),
            query: cx.create_rw_signal(String::new()),
            id: cx.create_rw_signal("my.custom.template".into()),
            global: cx.create_rw_signal(false),
            editor: editor::State::new(cx),
        }
    }
    fn refresh(&self) {
        let previous = self.entry().map(|e| (e.id, e.provenance));
        let entries = catalog::manager_entries(&self.root.get_untracked());
        let index = previous
            .and_then(|key| {
                entries
                    .iter()
                    .position(|e| (e.id.clone(), e.provenance.clone()) == key)
            })
            .or_else(|| (!entries.is_empty()).then_some(0));
        let query = self.query.get_untracked().to_lowercase();
        let index = index
            .filter(|i| matches_query(&entries[*i], &query))
            .or_else(|| entries.iter().position(|e| matches_query(e, &query)));
        self.selected.set(index);
        self.entries.set(entries);
    }
    fn entry(&self) -> Option<catalog::Entry> {
        self.selected
            .get_untracked()
            .and_then(|i| self.entries.get_untracked().get(i).cloned())
            .filter(|e| matches_query(e, &self.query.get_untracked().to_lowercase()))
    }
    fn destination(&self) -> Result<PathBuf, String> {
        if self.global.get_untracked() {
            catalog::user_root().ok_or_else(|| {
                "Cannot locate the user configuration directory".into()
            })
        } else {
            Ok(self.root.get_untracked().join(".sparrow/scaffolds"))
        }
    }
}
impl super::State {
    pub fn open_manager(&self, window: &WindowTabData) {
        let result = (|| {
            let root = window
                .workspace
                .path
                .as_deref()
                .ok_or("Open a project first")?;
            let base = window
                .main_split
                .active_editor
                .get_untracked()
                .and_then(|e| {
                    use floem::reactive::SignalWith;
                    e.doc().content.with_untracked(|c| {
                        c.path().and_then(|p| p.parent()).map(Path::to_path_buf)
                    })
                })
                .unwrap_or_else(|| root.to_path_buf());
            engine::context(root, &base)
        })();
        self.generation.update(|g| *g += 1);
        self.busy.set(false);
        self.plan.set(None);
        self.error.set(String::new());
        self.undoing.set(false);
        self.manager.editor.open.set(false);
        self.manager_mode.set(true);
        match result {
            Ok(ctx) => {
                self.manager.root.set(ctx.root);
                self.manager.refresh();
            }
            Err(e) => {
                self.manager.entries.set(vec![]);
                self.manager.root.set(PathBuf::new());
                self.error.set(e);
            }
        }
        self.active.set(true);
    }
}

fn matches_query(entry: &catalog::Entry, query: &str) -> bool {
    format!("{} {} {}", entry.title, entry.id, entry.provenance)
        .to_lowercase()
        .contains(query)
}
/// Existing ancestor is the transaction boundary when the user root is new.
fn boundary(path: &Path) -> Result<PathBuf, String> {
    let mut ancestor = path;
    loop {
        if ancestor.exists() {
            return ancestor.canonicalize().map_err(|e| e.to_string());
        }
        ancestor = ancestor
            .parent()
            .ok_or("No existing template parent directory")?;
    }
}
fn valid_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > 120
        || id == "."
        || id == ".."
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return Err(
            "Use a pack ID of 1–120 letters, digits, dots, underscores or hyphens"
                .into(),
        );
    }
    Ok(())
}
fn editable(entry: &catalog::Entry, root: &Path) -> Result<PathBuf, String> {
    let path = entry
        .manifest()
        .ok_or("Export the bundled pack to an editable override first")?;
    let allowed = path.starts_with(root.join(".sparrow/scaffolds"))
        || path.starts_with(root.join(".sparrow/templates"))
        || catalog::user_root().is_some_and(|r| path.starts_with(r));
    if !allowed {
        return Err("Pack is outside the template discovery roots".into());
    }
    Ok(path.to_path_buf())
}
fn tree(dir: &Path) -> Result<Vec<(PathBuf, String)>, String> {
    fn visit(
        base: &Path,
        dir: &Path,
        out: &mut Vec<(PathBuf, String)>,
    ) -> Result<(), String> {
        if dir
            .strip_prefix(base)
            .map_err(|e| e.to_string())?
            .components()
            .count()
            > 16
        {
            return Err("Template folder nesting exceeds 16 levels".into());
        }
        if dir.is_symlink() {
            return Err("Template folders cannot be symlinks".into());
        }
        let mut paths = fs::read_dir(dir)
            .map_err(|e| e.to_string())?
            .map(|e| e.map(|e| e.path()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        paths.sort();
        for path in paths {
            if path.is_symlink() {
                return Err("Template files cannot be symlinks".into());
            }
            if path.is_dir() {
                visit(base, &path, out)?;
            } else {
                if !path.is_file() {
                    return Err(
                        "Only regular text files are supported in template packs"
                            .into(),
                    );
                }
                if out.len() >= 256 {
                    return Err(
                        "A managed template may contain at most 256 files".into()
                    );
                }
                out.push((
                    path.strip_prefix(base)
                        .map_err(|e| e.to_string())?
                        .to_path_buf(),
                    engine::read(&path)?.ok_or("Template file disappeared")?,
                ));
            }
        }
        Ok(())
    }
    let mut files = vec![];
    visit(dir, dir, &mut files)?;
    Ok(files)
}
fn copy_files(entry: &catalog::Entry) -> Result<Vec<(PathBuf, String)>, String> {
    if let Some(manifest) = entry.manifest() {
        if manifest.file_name().is_none_or(|n| n != "pack.yaml") {
            return Err("Legacy templates can be edited or removed; create a pack to export or disable them".into());
        }
        tree(manifest.parent().ok_or("Missing pack directory")?)
    } else {
        Ok(catalog::export_files(&entry.id)?
            .into_iter()
            .map(|(p, s)| (p.strip_prefix(&entry.id).unwrap().to_path_buf(), s))
            .collect())
    }
}
fn create(destination: &Path, id: &str) -> Result<engine::Plan, String> {
    valid_id(id)?;
    let manifest = format!(
        "schema: 2\nid: {id}\nversion: 1.0.0\ntitle: My Template\ncategory: other\ntarget: sibling\nfiles:\n  - path: '{{{{snake}}}}.dart'\n    source: templates/template.dart.tmpl\n"
    );
    engine::management_plan(
        &boundary(destination)?,
        "Create template pack",
        vec![
            (destination.join(id).join("pack.yaml"), Some(manifest)),
            (
                destination.join(id).join("templates/template.dart.tmpl"),
                Some("class {{name}} {\n  const {{name}}();\n}\n".into()),
            ),
        ],
        true,
    )
}
fn export(
    entry: &catalog::Entry,
    destination: &Path,
) -> Result<engine::Plan, String> {
    valid_id(&entry.id)?;
    let contents = copy_files(entry)?;
    let inputs = entry
        .manifest()
        .map(|manifest| {
            contents
                .iter()
                .map(|(p, s)| (manifest.parent().unwrap().join(p), Some(s.clone())))
                .collect()
        })
        .unwrap_or_default();
    let files = contents
        .into_iter()
        .map(|(p, s)| (destination.join(&entry.id).join(p), Some(s)))
        .collect();
    engine::management_plan(
        &boundary(destination)?,
        "Export template pack",
        files,
        true,
    )?
    .with_management_inputs(inputs)
}
/// Validate the bytes actually being copied, rather than rereading a mutable source.
fn pack_from_files(files: &[(PathBuf, String)]) -> Result<engine::Pack, String> {
    let manifest = files
        .iter()
        .find(|(p, _)| p == Path::new("pack.yaml"))
        .map(|(_, body)| body)
        .ok_or("Choose a pack folder containing pack.yaml")?;
    engine::parse_pack(manifest, |source| {
        files
            .iter()
            .find(|(p, _)| p == Path::new(source))
            .map(|(_, body)| body.clone())
            .ok_or_else(|| format!("Missing template source: {source}"))
    })
}
fn copy_plan(
    destination: &Path,
    id: &str,
    title: &str,
    files: Vec<(PathBuf, String)>,
    inputs: Vec<(PathBuf, Option<String>)>,
) -> Result<engine::Plan, String> {
    valid_id(id)?;
    let files = files
        .into_iter()
        .map(|(p, s)| (destination.join(id).join(p), Some(s)))
        .collect();
    engine::management_plan(&boundary(destination)?, title, files, true)?
        .with_management_inputs(inputs)
}
fn import(source: &Path, destination: &Path) -> Result<engine::Plan, String> {
    if source.is_symlink() || !source.is_dir() {
        return Err("Choose a regular pack folder containing pack.yaml".into());
    }
    let source = source.canonicalize().map_err(|e| e.to_string())?;
    let files = tree(&source)?;
    let pack = pack_from_files(&files)?;
    let inputs = files
        .iter()
        .map(|(p, s)| (source.join(p), Some(s.clone())))
        .collect();
    let mut plan =
        copy_plan(destination, &pack.id, "Import template pack", files, inputs)?;
    plan.notes.push(format!(
        "Import {} from {}. The original pack is unchanged.",
        pack.id,
        source.display()
    ));
    Ok(plan)
}
fn duplicate(
    entry: &catalog::Entry,
    destination: &Path,
    id: &str,
) -> Result<engine::Plan, String> {
    valid_id(id)?;
    if id == entry.id {
        return Err("Choose a different New pack ID for the duplicate. Use Export to keep the original ID.".into());
    }
    // Legacy single-file templates become ordinary manifest-based packs.
    let (mut files, inputs) = if entry
        .manifest()
        .is_some_and(|p| p.file_name().is_none_or(|n| n != "pack.yaml"))
    {
        let pack = entry.load()?;
        let body = pack
            .files
            .first()
            .ok_or("Template has no output files")?
            .content
            .clone();
        let manifest = format!(
            "schema: 2\nid: {id}\nversion: 1.0.0\ntitle: Legacy Copy\ntarget: sibling\nfiles:\n  - path: '{{{{snake}}}}.dart'\n    source: templates/template.dart.tmpl\n"
        );
        (
            vec![
                (PathBuf::from("pack.yaml"), manifest),
                (PathBuf::from("templates/template.dart.tmpl"), body),
            ],
            pack.loaded_inputs,
        )
    } else {
        let files = copy_files(entry)?;
        let inputs = entry
            .manifest()
            .map(|manifest| {
                files
                    .iter()
                    .map(|(p, s)| {
                        (manifest.parent().unwrap().join(p), Some(s.clone()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        (files, inputs)
    };
    let pack = pack_from_files(&files)?;
    let manifest = files
        .iter_mut()
        .find(|(p, _)| p == Path::new("pack.yaml"))
        .unwrap();
    let mut value: serde_yaml::Value =
        serde_yaml::from_str(&manifest.1).map_err(|e| e.to_string())?;
    let map = value
        .as_mapping_mut()
        .ok_or("Manifest must be a YAML mapping")?;
    map.insert("id".into(), id.into());
    map.insert("title".into(), format!("{} (Copy)", pack.title).into());
    map.insert("disabled".into(), false.into());
    manifest.1 = serde_yaml::to_string(&value).map_err(|e| e.to_string())?;
    pack_from_files(&files)?;
    let mut plan =
        copy_plan(destination, id, "Duplicate template pack", files, inputs)?;
    plan.notes.push(format!("Independent enabled copy of {}. Template bodies, dependencies and registration metadata are preserved.",entry.id));
    Ok(plan)
}
fn toggle(
    entry: &catalog::Entry,
    root: &Path,
    destination: &Path,
) -> Result<engine::Plan, String> {
    if entry.manifest().is_none() {
        let mut plan = export(entry, destination)?;
        for change in &mut plan.changes {
            if change.path.file_name().is_some_and(|n| n == "pack.yaml") {
                change.after =
                    Some(disabled_manifest(change.after.as_deref().unwrap(), true)?);
            }
        }
        plan.title = "Disable bundled template with an override".into();
        return Ok(plan);
    }
    let path = editable(entry, root)?;
    if path.file_name().is_none_or(|n| n != "pack.yaml") {
        return Err("Legacy templates do not have a disabled flag; edit or remove the template instead".into());
    }
    let body = engine::read(&path)?.ok_or("Manifest disappeared")?;
    let after = disabled_manifest(&body, !entry.is_disabled())?;
    engine::management_plan(
        &boundary(path.parent().unwrap())?,
        "Change template enabled state",
        vec![(path.clone(), Some(after))],
        false,
    )?
    .with_management_inputs(vec![(path, Some(body))])
}
fn disabled_manifest(body: &str, disabled: bool) -> Result<String, String> {
    let mut value: serde_yaml::Value =
        serde_yaml::from_str(body).map_err(|e| e.to_string())?;
    let map = value
        .as_mapping_mut()
        .ok_or("Manifest must be a YAML mapping")?;
    map.insert(
        serde_yaml::Value::String("disabled".into()),
        serde_yaml::Value::Bool(disabled),
    );
    serde_yaml::to_string(&value).map_err(|e| e.to_string())
}
fn remove(entry: &catalog::Entry, root: &Path) -> Result<engine::Plan, String> {
    let manifest = editable(entry, root)?;
    let dir = manifest.parent().ok_or("Missing template directory")?;
    let snapshots = if manifest.file_name().is_some_and(|n| n == "pack.yaml") {
        tree(dir)?
            .into_iter()
            .map(|(p, s)| (dir.join(p), Some(s)))
            .collect::<Vec<_>>()
    } else {
        vec![(manifest.clone(), engine::read(&manifest)?)]
    };
    let files = snapshots.iter().map(|(p, _)| (p.clone(), None)).collect();
    let mut plan = engine::management_plan(
        &boundary(dir)?,
        "Remove template override",
        files,
        false,
    )?
    .with_management_inputs(snapshots)?;

    plan.notes.push("Removing an override reveals any lower-priority pack with the same ID. Bundled defaults remain installed.".into());
    Ok(plan)
}
fn stage(window: &WindowTabData, result: Result<engine::Plan, String>) {
    let state = &window.scaffold;
    state.plan.set(None);
    state.selected.set(0);
    match result {
        Ok(plan) => {
            state.planned_name.set(state.name.get_untracked());
            state.error.set(String::new());
            state.plan.set(Some(plan));
        }
        Err(e) => state.error.set(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("pubspec.yaml"), "name: test_app\n").unwrap();
        dir
    }
    fn project_entry(root: &Path, id: &str) -> catalog::Entry {
        catalog::manager_entries(root)
            .into_iter()
            .find(|e| e.id == id && e.provenance.starts_with("Project:"))
            .unwrap()
    }
    #[test]
    fn refresh_keeps_selection_inside_the_active_filter() {
        let root = project();
        create(&root.path().join(".sparrow/scaffolds"), "my.template")
            .unwrap()
            .apply()
            .unwrap();
        let cx = Scope::new();
        let manager = State::new(cx);
        manager.root.set(root.path().to_path_buf());
        manager.refresh();
        assert!(manager.entry().is_some());
        manager.query.set("my.template".into());
        // Actions refuse a stale invisible selection even before refresh.
        assert!(manager.entry().is_none());
        manager.refresh();
        assert_eq!(manager.entry().unwrap().id, "my.template");
        manager.query.set("nothing matches this".into());
        manager.refresh();
        assert!(manager.selected.get_untracked().is_none());
        manager.query.set(String::new());
        manager.refresh();
        assert!(manager.entry().is_some());
        cx.dispose();
    }
    #[test]
    fn create_edit_disable_enable_remove_and_undo() {
        let root = project();
        let destination = root.path().join(".sparrow/scaffolds");
        let created = create(&destination, "my.template")
            .unwrap()
            .apply()
            .unwrap();
        let entry = project_entry(root.path(), "my.template");
        let manifest = editable(&entry, root.path()).unwrap();
        let source = manifest
            .parent()
            .unwrap()
            .join("templates/template.dart.tmpl");
        fs::write(&source, "// edited\nclass {{name}} {}\n").unwrap();
        assert!(
            entry.load().unwrap().files[0]
                .content
                .starts_with("// edited")
        );
        let disabled = toggle(&entry, root.path(), &destination)
            .unwrap()
            .apply()
            .unwrap();
        assert!(project_entry(root.path(), "my.template").is_disabled());
        assert!(
            !catalog::discover_at(None, root.path())
                .iter()
                .any(|e| e.id == "my.template")
        );
        let disabled_entry = project_entry(root.path(), "my.template");
        toggle(&disabled_entry, root.path(), &destination)
            .unwrap()
            .apply()
            .unwrap();
        assert!(!project_entry(root.path(), "my.template").is_disabled());
        // Undo guards refuse to overwrite edits made after the original create.
        assert!(created.undo().is_err());
        assert!(disabled.undo().is_err());
        let removed =
            remove(&project_entry(root.path(), "my.template"), root.path())
                .unwrap()
                .apply()
                .unwrap();
        assert!(!source.exists());
        assert!(!manifest.exists());
        removed.undo().unwrap();
        assert!(source.exists());
        assert!(
            engine::load_pack(&manifest).unwrap().files[0]
                .content
                .starts_with("// edited")
        );
    }
    #[test]
    fn bundled_disable_creates_override_and_undo_reveals_default() {
        let root = project();
        let entry = catalog::bundled_entries()
            .into_iter()
            .find(|e| e.id == "sparrow.state.cubit")
            .unwrap();
        let destination = root.path().join(".sparrow/scaffolds");
        let plan = toggle(&entry, root.path(), &destination).unwrap();
        assert!(plan.changes.len() > 1);
        assert!(!destination.exists());
        let applied = plan.apply().unwrap();
        assert!(project_entry(root.path(), &entry.id).is_disabled());
        assert!(
            !catalog::discover_at(None, root.path())
                .iter()
                .any(|e| e.id == entry.id)
        );
        applied.undo().unwrap();
        assert!(
            catalog::discover_at(None, root.path())
                .iter()
                .any(|e| e.id == entry.id)
        );
        assert!(remove(&entry, root.path()).is_err());
    }
    #[test]
    fn export_copies_sources_without_clobber_and_can_be_undone() {
        let root = project();
        let destination = root.path().join(".sparrow/scaffolds");
        let entry = catalog::bundled_entries().remove(0);
        let applied = export(&entry, &destination).unwrap().apply().unwrap();
        let copied = project_entry(root.path(), &entry.id);
        assert_eq!(
            copied.load().unwrap().files.len(),
            entry.load().unwrap().files.len()
        );
        assert!(export(&entry, &destination).is_err());
        applied.undo().unwrap();
        assert!(!destination.join(&entry.id).exists());
    }
    #[test]
    fn exports_are_invalidated_by_source_edits() {
        let root = project();
        let local = root.path().join(".sparrow/scaffolds");
        create(&local, "my.template").unwrap().apply().unwrap();
        let entry = project_entry(root.path(), "my.template");
        let elsewhere = root.path().join("personal/scaffolds");
        let plan = export(&entry, &elsewhere).unwrap();
        fs::write(
            local.join("my.template/templates/template.dart.tmpl"),
            "// changed",
        )
        .unwrap();
        assert!(plan.apply().is_err());
        assert!(!elsewhere.exists());
    }
    #[test]
    fn removal_preview_rejects_later_edits_and_can_remove_invalid_pack() {
        let root = project();
        let local = root.path().join(".sparrow/scaffolds");
        create(&local, "my.template").unwrap().apply().unwrap();
        let entry = project_entry(root.path(), "my.template");
        let plan = remove(&entry, root.path()).unwrap();
        let manifest = local.join("my.template/pack.yaml");
        fs::write(&manifest, "not valid yaml: [").unwrap();
        assert!(plan.apply().is_err());
        let invalid = project_entry(root.path(), "my.template");
        assert!(invalid.problem().is_some());
        let removed = remove(&invalid, root.path()).unwrap().apply().unwrap();
        assert!(!manifest.exists());
        removed.undo().unwrap();
        assert_eq!(fs::read_to_string(manifest).unwrap(), "not valid yaml: [");
    }
    #[test]
    fn create_rejects_invalid_ids_and_stale_previews() {
        let root = project();
        let local = root.path().join(".sparrow/scaffolds");
        for id in ["", "..", "../escape", "a/b", "a\nb", "x: yaml"] {
            assert!(create(&local, id).is_err());
        }
        let plan = create(&local, "safe").unwrap();
        fs::create_dir_all(local.join("safe")).unwrap();
        fs::write(local.join("safe/pack.yaml"), "external change").unwrap();
        assert!(plan.apply().is_err());
        assert!(!local.join("safe/templates/template.dart.tmpl").exists());
    }
    #[test]
    #[cfg(unix)]
    fn symlink_sources_cannot_be_removed_or_exported() {
        use std::os::unix::fs::symlink;
        let root = project();
        let local = root.path().join(".sparrow/scaffolds");
        create(&local, "safe").unwrap().apply().unwrap();
        let entry = project_entry(root.path(), "safe");
        let source = local.join("safe/templates/template.dart.tmpl");
        fs::remove_file(&source).unwrap();
        symlink(root.path().join("pubspec.yaml"), source).unwrap();
        assert!(remove(&entry, root.path()).is_err());
        assert!(export(&entry, &root.path().join("other")).is_err());
        assert!(root.path().join("pubspec.yaml").exists());
    }
    #[test]
    fn import_previews_whole_pack_preserves_metadata_and_undo() {
        let source = project();
        let destination = project();
        let src = source.path().join("external");
        let entry = catalog::bundled_entries()
            .into_iter()
            .find(|e| e.id == "sparrow.feature.mvvm")
            .unwrap();
        export(&entry, &src).unwrap().apply().unwrap();
        let folder = src.join(&entry.id);
        fs::write(folder.join("README.md"), "Pack instructions").unwrap();
        let target = destination.path().join(".sparrow/scaffolds");
        let plan = import(&folder, &target).unwrap();
        assert!(!target.exists());
        assert_eq!(plan.changes.len(), 9); // manifest, seven feature sources, README
        let applied = plan.apply().unwrap();
        let imported =
            engine::load_pack(&target.join(&entry.id).join("pack.yaml")).unwrap();
        assert_eq!(imported.registration, entry.load().unwrap().registration);
        assert_eq!(imported.files.len(), 7);
        assert_eq!(
            fs::read_to_string(target.join(&entry.id).join("README.md")).unwrap(),
            "Pack instructions"
        );
        assert!(import(&folder, &target).is_err());
        applied.undo().unwrap();
        assert!(!target.exists());
        assert!(folder.join("pack.yaml").exists());
    }
    #[test]
    fn import_rejects_invalid_packs_and_missing_or_escaping_sources() {
        let source = project();
        let destination = project();
        let dir = source.path().join("external");
        fs::create_dir(&dir).unwrap();
        let target = destination.path().join(".sparrow/scaffolds");
        assert!(import(&dir, &target).is_err());
        for body in [
            "not yaml: [",
            "schema: 99\nid: invalid\nversion: 1.0.0\ntitle: Invalid\nfiles: []",
            "schema: 2\nid: invalid\nversion: 1.0.0\ntitle: Invalid\nfiles:\n  - path: x.dart\n    source: missing",
            "schema: 2\nid: invalid\nversion: 1.0.0\ntitle: Invalid\nfiles:\n  - path: x.dart\n    source: ../pubspec.yaml",
        ] {
            fs::write(dir.join("pack.yaml"), body).unwrap();
            assert!(import(&dir, &target).is_err());
        }
        assert!(!target.exists());
    }
    #[test]
    fn import_apply_rejects_source_and_destination_changes() {
        let source = project();
        let destination = project();
        let src = source.path().join("external");
        create(&src, "my.template").unwrap().apply().unwrap();
        let folder = src.join("my.template");
        let target = destination.path().join(".sparrow/scaffolds");
        let plan = import(&folder, &target).unwrap();
        fs::write(folder.join("templates/template.dart.tmpl"), "changed").unwrap();
        assert!(plan.apply().is_err());
        assert!(!target.exists());
        let plan = import(&folder, &target).unwrap();
        fs::create_dir_all(target.join("my.template")).unwrap();
        fs::write(target.join("my.template/pack.yaml"), "new external file")
            .unwrap();
        assert!(plan.apply().is_err());
        assert!(
            !target
                .join("my.template/templates/template.dart.tmpl")
                .exists()
        );
    }
    #[test]
    fn duplicate_bundled_feature_preserves_sources_and_registration() {
        let root = project();
        let target = root.path().join(".sparrow/scaffolds");
        let entry = catalog::bundled_entries()
            .into_iter()
            .find(|e| e.id == "sparrow.feature.mvvm")
            .unwrap();
        let original = entry.load().unwrap();
        let applied = duplicate(&entry, &target, "team.feature")
            .unwrap()
            .apply()
            .unwrap();
        let copied =
            engine::load_pack(&target.join("team.feature/pack.yaml")).unwrap();
        assert_eq!(copied.id, "team.feature");
        assert_eq!(copied.title, format!("{} (Copy)", original.title));
        assert_eq!(copied.registration, original.registration);
        assert_eq!(copied.target, original.target);
        assert_eq!(
            copied
                .files
                .iter()
                .map(|f| (&f.path, &f.content))
                .collect::<Vec<_>>(),
            original
                .files
                .iter()
                .map(|f| (&f.path, &f.content))
                .collect::<Vec<_>>()
        );
        assert!(duplicate(&entry, &target, "team.feature").is_err());
        assert!(duplicate(&entry, &target, &entry.id).is_err());
        applied.undo().unwrap();
        assert!(!target.exists());
    }
    #[test]
    fn duplicate_disabled_pack_is_enabled_and_independent() {
        let root = project();
        let target = root.path().join(".sparrow/scaffolds");
        create(&target, "original").unwrap().apply().unwrap();
        let original = project_entry(root.path(), "original");
        toggle(&original, root.path(), &target)
            .unwrap()
            .apply()
            .unwrap();
        let original = project_entry(root.path(), "original");
        let plan = duplicate(&original, &target, "my.copy").unwrap();
        fs::write(
            target.join("original/templates/template.dart.tmpl"),
            "// changed",
        )
        .unwrap();
        assert!(plan.apply().is_err());
        duplicate(&original, &target, "my.copy")
            .unwrap()
            .apply()
            .unwrap();
        let copied = project_entry(root.path(), "my.copy");
        assert!(!copied.is_disabled());
        assert!(original.load().unwrap().disabled);
        fs::write(
            target.join("my.copy/templates/template.dart.tmpl"),
            "// independent",
        )
        .unwrap();
        assert_eq!(original.load().unwrap().files[0].content, "// changed");
        for id in ["", "../escape", "bad\nid", "original"] {
            assert!(duplicate(&original, &target, id).is_err());
        }
    }
    #[test]
    fn duplicate_legacy_template_converts_to_pack_and_snapshots_input() {
        let root = project();
        let legacy = root.path().join(".sparrow/templates");
        fs::create_dir_all(&legacy).unwrap();
        let file = legacy.join("custom.dart.tmpl");
        let body = "class {{name}} {}";
        fs::write(&file, body).unwrap();
        let entry = catalog::manager_entries(root.path())
            .into_iter()
            .find(|e| e.id == "local.template.custom")
            .unwrap();
        let target = root.path().join(".sparrow/scaffolds");
        let plan = duplicate(&entry, &target, "legacy.copy").unwrap();
        fs::write(&file, "changed").unwrap();
        assert!(plan.apply().is_err());
        fs::write(&file, body).unwrap();
        plan.apply().unwrap();
        assert_eq!(
            engine::load_pack(&target.join("legacy.copy/pack.yaml"))
                .unwrap()
                .files[0]
                .content,
            body
        );
    }
    #[test]
    #[cfg(unix)]
    fn imports_and_duplicates_reject_symlinks_and_special_files() {
        use std::os::unix::fs::symlink;
        let root = project();
        let src = root.path().join(".sparrow/scaffolds");
        create(&src, "original").unwrap().apply().unwrap();
        let folder = src.join("original");
        let entry = project_entry(root.path(), "original");
        let target = root.path().join("elsewhere");
        symlink(&folder, root.path().join("alias")).unwrap();
        assert!(import(&root.path().join("alias"), &target).is_err());
        symlink(root.path().join("pubspec.yaml"), folder.join("extra.txt")).unwrap();
        assert!(import(&folder, &target).is_err());
        assert!(duplicate(&entry, &target, "copy").is_err());
        fs::remove_file(folder.join("extra.txt")).unwrap();
        let _socket =
            std::os::unix::net::UnixListener::bind(folder.join("extra.sock"))
                .unwrap();
        assert!(import(&folder, &target).is_err());
        assert!(duplicate(&entry, &target, "copy").is_err());
        assert!(!target.exists());
    }
    #[test]
    fn manager_includes_disabled_and_overridden_layers() {
        let root = project();
        let destination = root.path().join(".sparrow/scaffolds");
        let entry = catalog::bundled_entries().remove(0);
        toggle(&entry, root.path(), &destination)
            .unwrap()
            .apply()
            .unwrap();
        let all = catalog::manager_entries(root.path());
        assert!(
            all.iter()
                .any(|e| e.id == entry.id && e.manifest().is_none())
        );
        assert!(all.iter().any(|e| e.id == entry.id && e.is_disabled()));
    }
}
