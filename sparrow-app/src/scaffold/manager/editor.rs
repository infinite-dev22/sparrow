//! In-memory pack editing with the generator's strict renderer and transactional saves.
mod action_controls;
mod analyzer;
mod language;
mod view;

use super::{
    boundary, catalog, copy_files, editable, engine, pack_from_files, valid_id,
};
use crate::window_tab::WindowTabData;
use floem::{
    reactive::{RwSignal, Scope, SignalGet, SignalUpdate},
    views::editor::{
        Editor,
        core::{editor::EditType, selection::Selection},
    },
};
use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
};
pub(super) use view::view;

type Rendered = Result<Vec<(PathBuf, String)>, String>;

#[derive(Clone, Debug)]
pub(super) struct Draft {
    files: BTreeMap<PathBuf, String>,
    original: BTreeMap<PathBuf, String>,
    directory: PathBuf,
    create_only: bool,
    ctx: engine::Context,
    source: String,
}
impl Draft {
    fn load(
        entry: &catalog::Entry,
        root: &Path,
        destination: &Path,
    ) -> Result<Self, String> {
        let ctx = engine::context(root, root)?;
        let (files, directory, create_only) = match entry.manifest() {
            Some(manifest) if manifest.file_name().is_some_and(|n|n=="pack.yaml") => {
                let manifest = editable(entry,root)?;
                (copy_files(entry)?,manifest.parent().unwrap().to_path_buf(),false)
            }
            Some(_) => return Err("Duplicate this legacy template into a pack first, then open Template Editor.".into()),
            None => {
                valid_id(&entry.id)?;
                (copy_files(entry)?,destination.join(&entry.id),true)
            }
        };
        let files = files.into_iter().collect::<BTreeMap<_, _>>();
        Ok(Self {
            original: files.clone(),
            files,
            directory,
            create_only,
            ctx,
            source: entry.provenance.clone(),
        })
    }
    pub(super) fn dirty(&self) -> bool {
        self.files != self.original
    }
    fn validate(&self, sample: &str) -> Rendered {
        let files = self
            .files
            .iter()
            .map(|(p, s)| (p.clone(), s.clone()))
            .collect::<Vec<_>>();
        let pack = pack_from_files(&files)?;
        valid_id(&pack.id)?;
        // Disabled packs can still be inspected and edited without enabling them.
        engine::render_pack(&self.ctx, &pack, sample)
    }
    fn save_plan(&self, sample: &str) -> Result<engine::Plan, String> {
        self.validate(sample)?;
        if !self.dirty() && !self.create_only {
            return Err("No draft changes to save".into());
        }
        let paths = self
            .original
            .keys()
            .chain(self.files.keys())
            .collect::<std::collections::BTreeSet<_>>();
        let changes = paths
            .into_iter()
            .map(|p| (self.directory.join(p), self.files.get(p).cloned()))
            .collect();
        let inputs = if self.create_only {
            vec![]
        } else {
            self.original
                .keys()
                .chain(self.files.keys())
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .map(|p| (self.directory.join(p), self.original.get(p).cloned()))
                .collect()
        };
        engine::management_plan(
            &boundary(&self.directory)?,
            "Save template edits",
            changes,
            self.create_only,
        )?
        .with_management_inputs(inputs)
    }
    fn add_file(&mut self, path: &str) -> Result<PathBuf, String> {
        let path = PathBuf::from(path);
        if path.as_os_str().is_empty()
            || path
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err("Use a relative file path without traversal".into());
        }
        if self.files.contains_key(&path) {
            return Err("This draft file already exists".into());
        }
        if self.files.len() >= 256 {
            return Err("A pack may contain at most 256 files".into());
        }
        self.files.insert(path.clone(), String::new());
        Ok(path)
    }
    fn remove_file(&mut self, path: &Path) -> Result<(), String> {
        if path == Path::new("pack.yaml") {
            return Err("The pack manifest cannot be removed".into());
        }
        self.files.remove(path).ok_or("Select a draft file")?;
        Ok(())
    }
    fn mark_saved(&mut self) {
        self.original = self.files.clone();
        self.create_only = false;
        self.source = format!("Editable: {}", self.directory.display());
    }
}

#[derive(Clone)]
pub(super) struct State {
    pub open: RwSignal<bool>,
    pub draft: RwSignal<Option<Draft>>,
    file: RwSignal<PathBuf>,
    sample: RwSignal<String>,
    new_file: RwSignal<String>,
    rendered: RwSignal<Rendered>,
    epoch: RwSignal<u64>,
    revision: RwSignal<u64>,
    diagnostics: RwSignal<Vec<language::Diagnostic>>,
}
impl State {
    pub fn new(cx: Scope) -> Self {
        Self {
            open: cx.create_rw_signal(false),
            draft: cx.create_rw_signal(None),
            file: cx.create_rw_signal("pack.yaml".into()),
            sample: cx.create_rw_signal("Example".into()),
            new_file: cx.create_rw_signal("templates/new.dart.tmpl".into()),
            rendered: cx.create_rw_signal(Err("Validating…".into())),
            epoch: cx.create_rw_signal(0),
            revision: cx.create_rw_signal(0),
            diagnostics: cx.create_rw_signal(vec![]),
        }
    }
}
pub(super) fn open(window: &WindowTabData) -> Result<(), String> {
    let manager = &window.scaffold.manager;
    let editor = &manager.editor;
    if !editor.draft.get_untracked().is_some_and(|d| d.dirty()) {
        let entry = manager.entry().ok_or("Select a template pack")?;
        let draft = Draft::load(
            &entry,
            &manager.root.get_untracked(),
            &manager.destination()?,
        )?;
        editor.draft.set(Some(draft));
        editor.file.set("pack.yaml".into());
        editor.epoch.update(|v| *v += 1);
    }
    window.scaffold.plan.set(None);
    window.scaffold.error.set(String::new());
    editor.open.set(true);
    Ok(())
}

fn accept_completion(
    ed: &Editor,
    choice: &language::Completion,
    draft: RwSignal<Option<Draft>>,
    plan: RwSignal<Option<engine::Plan>>,
    error: RwSignal<String>,
    selected: RwSignal<PathBuf>,
) {
    let body = ed.doc().text().to_string();
    if choice.range.end > body.len()
        || !body.is_char_boundary(choice.range.start)
        || !body.is_char_boundary(choice.range.end)
    {
        return;
    }
    ed.doc().edit_single(
        Selection::region(choice.range.start, choice.range.end),
        &choice.insert,
        EditType::InsertChars,
    );
    ed.cursor.update(|c| {
        c.set_offset(choice.range.start + choice.insert.len(), false, false)
    });
    let text = ed.doc().text().to_string();
    // Programmatic edits have no OnUpdate.editor; explicitly sync the draft.
    // The selected path is supplied by the live editor's completion caller.
    plan.set(None);
    draft.update(|d| {
        if let Some(d) = d {
            d.files.insert(selected.get_untracked(), text);
        }
    });
    error.set(String::new());
    if let Some(id) = ed.editor_view_id.get_untracked() {
        id.request_focus();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use floem::{keyboard::Modifiers, views::editor::{text::Document, text_document::TextDocument}};
    use std::{fs, rc::Rc};
    fn project() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        fs::write(
            d.path().join("pubspec.yaml"),
            "name: editor_test
",
        )
        .unwrap();
        d
    }
    fn local(root: &Path) -> Draft {
        let destination = root.join(".sparrow/scaffolds");
        super::super::create(&destination, "team.example")
            .unwrap()
            .apply()
            .unwrap();
        let entry = catalog::manager_entries(root)
            .into_iter()
            .find(|e| e.id == "team.example")
            .unwrap();
        Draft::load(&entry, root, &destination).unwrap()
    }
    fn body(d: &mut Draft, text: &str) {
        d.files
            .insert("templates/template.dart.tmpl".into(), text.into());
    }
    #[test]
    fn accepted_completion_updates_draft_cursor_and_document_undo() {
        use floem::views::editor::{
            command::Command, core::command::EditCommand, text::SimpleStyling,
        };
        let root = project();
        let mut initial = local(root.path());
        body(&mut initial, "class {{na}} {}");
        let source = PathBuf::from("templates/template.dart.tmpl");
        let text = initial.files[&source].clone();
        let choices =
            language::completions(&source, &text, 10, &initial.files, true);
        let name = choices.iter().find(|c| c.label == "{{name}}").unwrap();
        let cx = Scope::new();
        let draft = cx.create_rw_signal(Some(initial));
        let plan = cx.create_rw_signal(None);
        let error = cx.create_rw_signal(String::new());
        let selected = cx.create_rw_signal(source.clone());
        let doc = Rc::new(TextDocument::new(cx, text.clone()));
        let ed = Editor::new(cx, doc.clone(), Rc::new(SimpleStyling::new()), false);
        ed.cursor.update(|c| c.set_offset(10, false, false));
        accept_completion(&ed, name, draft, plan, error, selected);
        assert_eq!(doc.text().to_string(), "class {{name}} {}");
        assert_eq!(
            draft.get_untracked().unwrap().files[&source],
            "class {{name}} {}"
        );
        assert_eq!(ed.cursor.get_untracked().offset(), 14);
        ed.doc().run_command(
            &ed,
            &Command::Edit(EditCommand::Undo),
            Some(1),
            Modifiers::empty(),
        );
        assert_eq!(doc.text().to_string(), text);
        cx.dispose();
    }
    #[test]
    fn live_validation_checks_yaml_sources_metadata_variables_and_output_paths() {
        let root = project();
        let original = local(root.path());
        for invalid in [
            "not yaml: [",
            "schema: 99
id: bad
version: 1.0.0
title: Bad
files: []",
        ] {
            let mut d = original.clone();
            d.files.insert("pack.yaml".into(), invalid.into());
            assert!(d.validate("Counter").is_err());
            assert!(d.save_plan("Counter").is_err());
        }
        for invalid in ["{{unknown}}", "{{name", "{{ name }} {{missing}}"] {
            let mut d = original.clone();
            body(&mut d, invalid);
            assert!(d.validate("Counter").is_err());
        }
        let mut d = original.clone();
        d.files.remove(Path::new("templates/template.dart.tmpl"));
        assert!(
            d.validate("Counter")
                .unwrap_err()
                .contains("Missing template source")
        );
        let mut d = original.clone();
        let manifest = d.files.get_mut(Path::new("pack.yaml")).unwrap();
        *manifest = manifest.replace("{{snake}}.dart", "../escape.dart");
        assert!(d.validate("Counter").is_err());
        assert!(original.validate("lowercase").is_err());
        assert!(original.validate("Counter").is_ok());
        assert!(!original.dirty());
    }
    #[test]
    fn preview_uses_unsaved_buffers_and_does_not_touch_generated_files() {
        let root = project();
        let mut d = local(root.path());
        body(
            &mut d,
            "class {{ name }} { final value = '{{variable}}'; }
",
        );
        fs::write(root.path().join("counter.dart"), "existing app code").unwrap();
        let preview = d.validate("Counter").unwrap();
        assert_eq!(
            preview[0],
            (
                PathBuf::from("counter.dart"),
                "class Counter { final value = 'counter'; }
"
                .into()
            )
        );
        assert_eq!(
            fs::read_to_string(root.path().join("counter.dart")).unwrap(),
            "existing app code"
        );
        assert!(
            !fs::read_to_string(d.directory.join("templates/template.dart.tmpl"))
                .unwrap()
                .contains("final value")
        );
    }
    #[test]
    fn edited_pack_save_and_undo_use_original_snapshots() {
        let root = project();
        let mut d = local(root.path());
        let before = d.original.clone();
        body(
            &mut d,
            "class {{name}} { int value = 42; }
",
        );
        let manifest = d.files.get_mut(Path::new("pack.yaml")).unwrap();
        *manifest = manifest.replace("My Template", "Edited Template");
        let applied = d.save_plan("Counter").unwrap().apply().unwrap();
        let pack = engine::load_pack(&d.directory.join("pack.yaml")).unwrap();
        assert_eq!(pack.title, "Edited Template");
        assert!(pack.files[0].content.contains("42"));
        assert!(d.dirty());
        d.mark_saved();
        assert!(!d.dirty());
        assert!(d.save_plan("Counter").is_err());
        applied.undo().unwrap();
        for (p, s) in before {
            assert_eq!(fs::read_to_string(d.directory.join(p)).unwrap(), s);
        }
    }
    #[test]
    fn file_add_remove_and_external_conflicts_are_transactional() {
        let root = project();
        let mut d = local(root.path());
        for path in ["", "../escape", "/absolute", "pack.yaml"] {
            assert!(d.add_file(path).is_err());
        }
        assert!(d.remove_file(Path::new("pack.yaml")).is_err());
        let file = d.add_file("templates/extra.dart.tmpl").unwrap();
        d.files.insert(file.clone(), "// helper".into());
        assert!(d.dirty());
        let plan = d.save_plan("Counter").unwrap();
        fs::write(d.directory.join(&file), "external").unwrap();
        assert!(plan.apply().is_err());
        assert!(d.save_plan("Counter").is_err());
        fs::remove_file(d.directory.join(&file)).unwrap();
        let added = d.save_plan("Counter").unwrap().apply().unwrap();
        assert!(d.directory.join(&file).exists());
        d.mark_saved();
        d.remove_file(&file).unwrap();
        let removed = d.save_plan("Counter").unwrap().apply().unwrap();
        assert!(!d.directory.join(&file).exists());
        removed.undo().unwrap();
        assert_eq!(
            fs::read_to_string(d.directory.join(&file)).unwrap(),
            "// helper"
        );
        added.undo().unwrap();
        assert!(!d.directory.join(&file).exists());
    }
    #[test]
    fn external_source_changes_block_saving_even_when_source_not_edited() {
        let root = project();
        let mut d = local(root.path());
        let manifest = d.files.get_mut(Path::new("pack.yaml")).unwrap();
        *manifest = manifest.replace("My Template", "New Title");
        fs::write(
            d.directory.join("templates/template.dart.tmpl"),
            "// external",
        )
        .unwrap();
        assert!(d.save_plan("Counter").is_err());
        assert_eq!(
            engine::load_pack(&d.directory.join("pack.yaml"))
                .unwrap()
                .title,
            "My Template"
        );
    }
    #[test]
    fn bundled_edit_saves_override_without_mutating_defaults_or_clobbering() {
        let root = project();
        let entry = catalog::bundled_entries()
            .into_iter()
            .find(|e| e.id == "sparrow.state.cubit")
            .unwrap();
        let destination = root.path().join(".sparrow/scaffolds");
        let mut d = Draft::load(&entry, root.path(), &destination).unwrap();
        let original = entry.load().unwrap().files[0].content.clone();
        body(
            &mut d,
            "class {{name}} {}
",
        );
        let applied = d.save_plan("Counter").unwrap().apply().unwrap();
        assert_eq!(entry.load().unwrap().files[0].content, original);
        assert_eq!(
            engine::load_pack(&d.directory.join("pack.yaml"))
                .unwrap()
                .files[0]
                .content,
            "class {{name}} {}
"
        );
        assert!(d.save_plan("Counter").is_err());
        applied.undo().unwrap();
        assert!(!d.directory.exists());
    }
    #[test]
    fn invalid_pack_can_be_repaired_and_disabled_pack_can_be_previewed() {
        let root = project();
        let d = local(root.path());
        let manifest = d.directory.join("pack.yaml");
        let valid = d.files.get(Path::new("pack.yaml")).unwrap().clone();
        fs::write(&manifest, "invalid: [").unwrap();
        let entry = catalog::manager_entries(root.path())
            .into_iter()
            .find(|e| e.id == "team.example")
            .unwrap();
        assert!(entry.problem().is_some());
        let mut repaired = Draft::load(
            &entry,
            root.path(),
            &root.path().join(".sparrow/scaffolds"),
        )
        .unwrap();
        assert!(repaired.validate("Counter").is_err());
        repaired.files.insert(
            "pack.yaml".into(),
            format!(
                "{valid}
disabled: true
"
            ),
        );
        assert!(repaired.validate("Counter").is_ok());
        repaired.save_plan("Counter").unwrap().apply().unwrap();
        assert!(engine::load_pack(&manifest).unwrap().disabled);
    }
    #[test]
    fn all_bundled_draft_previews_match_generation_bytes() {
        let root = project();
        let ctx = engine::context(root.path(), root.path()).unwrap();
        for entry in catalog::bundled_entries() {
            let d = Draft::load(
                &entry,
                root.path(),
                &root.path().join(".sparrow/scaffolds"),
            )
            .unwrap();
            let pack = entry.load().unwrap();
            let plan = match pack.target {
                engine::Target::Package => engine::plan_pack(&ctx, &pack, "Counter"),
                engine::Target::Sibling => {
                    engine::plan_sibling(&ctx, root.path(), &pack, "Counter")
                }
            }
            .unwrap();
            let preview = d.validate("Counter").unwrap();
            for (path, body) in preview {
                assert!(
                    plan.changes
                        .iter()
                        .any(|c| c.path == root.path().join(&path)
                            && c.after.as_deref() == Some(body.as_str())),
                    "{}",
                    entry.id
                );
            }
        }
    }
}
