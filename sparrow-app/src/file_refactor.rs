//! Analyzer-backed Dart file rename, presented as a project refactoring dialog.
mod view;

pub use view::view;

use crate::window_tab::WindowTabData;
use floem::{
    reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    rc::Rc,
};
#[derive(Clone)]
pub struct State {
    pub active: RwSignal<bool>,
    moving: RwSignal<bool>,
    deleting: RwSignal<bool>,
    path: RwSignal<PathBuf>,
    name: RwSignal<String>,
    comments: RwSignal<bool>,
    scope: RwSignal<bool>,
    error: RwSignal<String>,
    busy: RwSignal<bool>,
    plan: RwSignal<Option<Plan>>,
}
#[derive(Clone)]
struct Plan {
    root: PathBuf,
    deleting: bool,
    old: PathBuf,
    new: PathBuf,
    files: Vec<(PathBuf, String, String)>,
}
impl State {
    pub fn is_busy(&self) -> bool {
        self.busy.get_untracked()
    }

    pub fn new(cx: Scope) -> Self {
        Self {
            active: cx.create_rw_signal(false),
            moving: cx.create_rw_signal(false),
            deleting: cx.create_rw_signal(false),
            path: cx.create_rw_signal(PathBuf::new()),
            name: cx.create_rw_signal(String::new()),
            comments: cx.create_rw_signal(true),
            scope: cx.create_rw_signal(false),
            error: cx.create_rw_signal(String::new()),
            busy: cx.create_rw_signal(false),
            plan: cx.create_rw_signal(None),
        }
    }
    pub fn open(&self, path: PathBuf) {
        self.moving.set(false);
        self.deleting.set(false);
        self.name.set(
            path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into(),
        );
        self.path.set(path);
        self.plan.set(None);
        self.error.set(String::new());
        self.active.set(true);
    }
    pub fn open_move(&self, path: PathBuf, root: &Path) {
        self.open(path.clone());
        self.moving.set(true);
        self.name.set(
            path.strip_prefix(root)
                .unwrap_or(&path)
                .display()
                .to_string(),
        );
    }
    pub fn open_delete(&self, path: PathBuf) {
        self.open(path);
        self.scope.set(false);
        self.deleting.set(true);
    }
    fn close(&self, window: &WindowTabData) {
        if !self.busy.get_untracked() {
            self.active.set(false);
            window.common.keyboard_focus.set(None);
            window.common.view_id.get_untracked().request_focus();
        }
    }
    fn run(&self, window: Rc<WindowTabData>, preview: bool) {
        if self.busy.get_untracked() {
            return;
        }
        self.error.set(String::new());
        let old = self.path.get_untracked();
        let name = self.name.get_untracked();
        if name.is_empty()
            || (!self.moving.get_untracked() && name.contains(['/', '\\']))
            || name == "."
            || name == ".."
            || !name.ends_with(".dart")
        {
            self.error
                .set("Enter a Dart filename without path separators.".into());
            return;
        }
        let deleting = self.deleting.get_untracked();
        let new = if deleting {
            old.with_file_name(format!(
                ".__sparrow_usage_probe_{}.dart",
                std::process::id()
            ))
        } else if self.moving.get_untracked() {
            let Some(root) = window.workspace.path.as_deref() else {
                return;
            };
            let path = PathBuf::from(&name);
            if path.is_absolute()
                || path
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                self.error.set(
                    "Enter a project-relative destination without '..'.".into(),
                );
                return;
            }
            root.join(path)
        } else {
            old.with_file_name(&name)
        };
        if old == new {
            self.error.set("Enter a different filename.".into());
            return;
        }
        if new.exists() {
            self.error
                .set("A file with that name already exists.".into());
            return;
        }
        if window.main_split.docs.with_untracked(|docs| {
            docs.values().any(|doc| {
                !doc.is_pristine() && doc.content.with_untracked(|c| c.is_file())
            })
        }) {
            self.error
                .set("Save modified project files before file refactoring.".into());
            return;
        }
        if let Some(plan) = self
            .plan
            .get_untracked()
            .filter(|p| !deleting && p.old == old && p.new == new)
        {
            if !preview {
                self.apply(window, plan);
                return;
            }
        }
        if !window.common.dart_lsp.is_available() {
            self.error
                .set("Start Dart Analysis before refactoring a Dart file.".into());
            return;
        }
        let Ok(old_uri) = lsp_types::Url::from_file_path(&old) else {
            return;
        };
        let Ok(new_uri) = lsp_types::Url::from_file_path(&new) else {
            return;
        };
        let root = window
            .workspace
            .path
            .clone()
            .unwrap_or_else(|| old.parent().unwrap().to_path_buf());
        let comments = self.comments.get_untracked();
        let directory = self.scope.get_untracked();
        let data = self.clone();
        let w = window.clone();
        self.busy.set(true);
        let send = floem::ext_event::create_ext_action(
            window.scope,
            move |result: Result<Plan, String>| {
                data.busy.set(false);
                if !data.active.get_untracked()
                    || data.name.get_untracked() != name
                    || data.comments.get_untracked() != comments
                    || data.scope.get_untracked() != directory
                {
                    return;
                }
                match result {
                    Ok(plan) => {
                        data.plan.set(Some(plan.clone()));
                        if !preview {
                            data.apply(w.clone(), plan);
                        }
                    }
                    Err(e) => data.error.set(e),
                }
            },
        );
        window.common.dart_lsp.request_async("workspace/willRenameFiles",serde_json::json!({"files":[{"oldUri":old_uri,"newUri":new_uri}]}),move |response|{
   if let Some(error)=response.get("error"){send(Err(error["message"].as_str().unwrap_or("File rename analysis failed.").into()));return;}
   let edit=serde_json::from_value::<Option<lsp_types::WorkspaceEdit>>(response.get("result").cloned().unwrap_or(serde_json::Value::Null));
   send(edit.map_err(|e|e.to_string()).and_then(|edit|prepare(&root,&old,&new,edit.as_ref(),comments,directory).and_then(|plan|if deleting{plan.for_delete()}else{Ok(plan)})));
  });
    }
    fn apply(&self, window: Rc<WindowTabData>, plan: Plan) {
        self.busy.set(true);
        let data = self.clone();
        let w = window.clone();
        let completion = plan.clone();
        let send = floem::ext_event::create_ext_action(
            window.scope,
            move |result: Result<(), String>| {
                data.busy.set(false);
                match result {
                    Err(e) => {
                        data.plan.set(None);
                        data.error.set(e);
                    }
                    Ok(()) => {
                        for (path, _, text) in &completion.files {
                            if path != &completion.old {
                                w.main_split.docs.with_untracked(|docs| {
                                    if let Some(doc) = docs.get(path) {
                                        if doc.is_pristine() {
                                            doc.reload(
                                                lapce_xi_rope::Rope::from(
                                                    text.clone(),
                                                ),
                                                true,
                                            );
                                        }
                                    }
                                });
                            }
                        }
                        let editors =
                            w.main_split.editors.with_editors_untracked(|items| {
                                items
                                    .values()
                                    .filter(|e| {
                                        e.doc().content.with_untracked(|c| {
                                            c.path() == Some(&completion.old)
                                        })
                                    })
                                    .cloned()
                                    .collect::<Vec<_>>()
                            });
                        if completion.deleting {
                            for editor in editors {
                                if let Some(tab) =
                                    editor.editor_tab_id.get_untracked()
                                {
                                    w.main_split.editor_tab_child_close(
                                        tab,
                                        crate::editor_tab::EditorTabChild::Editor(
                                            editor.id(),
                                        ),
                                        true,
                                    );
                                }
                            }
                            crate::panel::workflow_tools::record(
                                w.notifications,
                                "Safe Delete",
                                &format!(
                                    "Deleted {}. Backup: {}",
                                    completion.old.display(),
                                    completion.new.display()
                                ),
                            );
                        } else if !editors.is_empty() {
                            let (doc, _) =
                                w.main_split.get_doc(completion.new.clone(), None);
                            for editor in editors {
                                editor.update_doc(doc.clone());
                            }
                        }
                        if !completion.deleting {
                            if let Some(root) = w.workspace.path.as_deref() {
                                w.run_configs.update(|configs| {
                                    for config in configs {
                                        crate::dart::run_config::rebind_file(
                                            config,
                                            root,
                                            &completion.old,
                                            &completion.new,
                                        );
                                    }
                                });
                            }
                        }
                        w.main_split.docs.update(|docs| {
                            docs.remove(&completion.old);
                        });
                        w.common.breakpoints.update(|items| {
                            if let Some(value) = items.remove(&completion.old) {
                                items.insert(completion.new.clone(), value);
                            }
                        });
                        w.common.document_views.update(|items| {
                            if let Some(value) = items.remove(&completion.old) {
                                items.insert(completion.new.clone(), value);
                            }
                        });
                        w.common.bookmarks.update(|items| {
                            if let Some(value) = items.remove(&completion.old) {
                                items.insert(completion.new.clone(), value);
                            }
                        });
                        w.common.dart_lsp.send_close(completion.old.clone());
                        w.file_explorer.reload();
                        w.file_explorer.reveal_in_file_tree(completion.new.clone());
                        data.active.set(false);
                        w.common.keyboard_focus.set(None);
                        w.common.view_id.get_untracked().request_focus();
                    }
                }
            },
        );
        let task = window.common.background_tasks.begin("Applying file refactor", None);
        std::thread::spawn(move || { let _task = task; send(plan.apply()); });
    }
}
fn prepare(
    root: &Path,
    old: &Path,
    new: &Path,
    edit: Option<&lsp_types::WorkspaceEdit>,
    comments: bool,
    directory: bool,
) -> Result<Plan, String> {
    let canonical = root.canonicalize().map_err(|e| e.to_string())?;
    if old.is_symlink()
        || !old
            .canonicalize()
            .map_err(|e| e.to_string())?
            .starts_with(&canonical)
    {
        return Err("Only writable project files can be renamed.".into());
    }
    if !new
        .parent()
        .ok_or("Destination has no directory")?
        .canonicalize()
        .map_err(|_| "Choose an existing destination directory.".to_string())?
        .starts_with(&canonical)
    {
        return Err("Choose a destination inside the project.".into());
    }
    let mut files = BTreeMap::new();
    if let Some(edit) = edit {
        for file in crate::code_action_preview::edit_files(edit)? {
            if !file
                .path
                .canonicalize()
                .map_err(|e| e.to_string())?
                .starts_with(&canonical)
            {
                return Err(
                    "Analyzer rename targets files outside the project.".into()
                );
            }
            if directory && !file.path.starts_with(old.parent().unwrap()) {
                return Err("This rename has usages outside the selected directory. Choose Project Files.".into());
            }
            let before =
                std::fs::read_to_string(&file.path).map_err(|e| e.to_string())?;
            let after = crate::code_action_preview::edited(&before, &file.edits)?;
            files.insert(file.path, (before, after));
        }
    }
    for (path, before, after) in
        crate::dart::run_config::refactored_configuration_files(root, old, new)?
    {
        files.insert(path, (before, after));
    }
    if comments {
        fn walk(path: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
            for e in std::fs::read_dir(path).map_err(|e| e.to_string())? {
                let e = e.map_err(|e| e.to_string())?;
                let ty = e.file_type().map_err(|e| e.to_string())?;
                if ty.is_symlink() {
                    continue;
                }
                if ty.is_dir() {
                    if !matches!(
                        e.file_name().to_str(),
                        Some(
                            "build"
                                | ".dart_tool"
                                | ".git"
                                | ".sparrow"
                                | "Pods"
                                | ".gradle"
                        )
                    ) {
                        walk(&e.path(), out)?;
                    }
                } else {
                    out.push(e.path());
                }
                if out.len() > 20000 {
                    return Err("Project is too large to scan comments.".into());
                }
            }
            Ok(())
        }
        let mut paths = Vec::new();
        walk(
            if directory {
                old.parent().unwrap()
            } else {
                root
            },
            &mut paths,
        )?;
        let old_name = old.file_name().unwrap().to_string_lossy();
        let new_name = new.file_name().unwrap().to_string_lossy();
        for path in paths {
            if std::fs::metadata(&path)
                .map(|m| m.len() > 2_000_000)
                .unwrap_or(true)
            {
                continue;
            }
            let Ok(before) = std::fs::read_to_string(&path) else {
                continue;
            };
            if before.contains('\0') {
                continue;
            }
            if path
                .file_name()
                .and_then(|p| p.to_str())
                .is_some_and(|p| p.ends_with(".sparrow.run.toml"))
            {
                continue;
            }
            let base = files
                .get(&path)
                .map(|(_, text)| text.clone())
                .unwrap_or_else(|| before.clone());
            let after = textual_references(
                &base,
                old_name.as_ref(),
                new_name.as_ref(),
                crate::dart::is_dart_path(&path),
            );
            if after != before {
                files.insert(path, (before, after));
            }
        }
    }
    if !files.contains_key(old) {
        let text = std::fs::read_to_string(old).map_err(|e| e.to_string())?;
        files.insert(old.to_path_buf(), (text.clone(), text));
    }
    Ok(Plan {
        root: root.into(),
        deleting: false,
        old: old.into(),
        new: new.into(),
        files: files.into_iter().map(|(p, (a, b))| (p, a, b)).collect(),
    })
}
fn textual_references(source: &str, old: &str, new: &str, dart: bool) -> String {
    if !dart {
        return source.replace(old, new);
    }
    let ranges = crate::dart::regex::skip_ranges(source);
    let mut code = source.as_bytes().to_vec();
    for &(a, b) in &ranges {
        if source.as_bytes()[a] == b'/' {
            code[a..b].fill(b' ');
        }
    }
    let code = String::from_utf8(code).expect("comment masking preserves UTF-8");
    let mut result = source.to_string();
    for (a, b) in ranges.into_iter().rev() {
        if matches!(source.as_bytes()[a], b'\'' | b'"') {
            let mut prefix = code[..a].trim_end();
            if source.as_bytes().get(a.wrapping_sub(1)) == Some(&b'r') {
                prefix = prefix.trim_end_matches('r').trim_end();
            }
            if ["import", "export", "part", "part of"]
                .iter()
                .any(|word| prefix.ends_with(word))
            {
                continue;
            }
        }
        result.replace_range(a..b, &source[a..b].replace(old, new));
    }
    result
}
impl Plan {
    fn for_delete(mut self) -> Result<Self, String> {
        let uses = self
            .files
            .iter()
            .filter(|(p, a, b)| p != &self.old && a != b)
            .map(|(p, _, _)| {
                p.strip_prefix(&self.root)
                    .unwrap_or(p)
                    .display()
                    .to_string()
            })
            .collect::<Vec<_>>();
        if !uses.is_empty() {
            return Err(format!(
                "Cannot delete: usages remain in {}. Update these usages first.",
                uses.join(", ")
            ));
        }
        self.files.retain(|(p, _, _)| p == &self.old);
        for (_, before, after) in &mut self.files {
            *after = before.clone();
        }
        if self.root.join(".sparrow").is_symlink()
            || self.root.join(".sparrow/deleted-files").is_symlink()
        {
            return Err("The backup directory must be inside the project.".into());
        }
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        self.new = self.root.join(".sparrow/deleted-files").join(format!(
            "{time}_{}",
            self.old.file_name().unwrap().to_string_lossy()
        ));
        self.deleting = true;
        Ok(self)
    }
    fn summary(&self) -> String {
        if self.deleting {
            return format!(
                "No usages found.\nDelete {}\nBackup: {}",
                self.old
                    .strip_prefix(&self.root)
                    .unwrap_or(&self.old)
                    .display(),
                self.new
                    .strip_prefix(&self.root)
                    .unwrap_or(&self.new)
                    .display()
            );
        }
        let mut lines = vec![format!(
            "{} → {}",
            self.old
                .strip_prefix(&self.root)
                .unwrap_or(&self.old)
                .display(),
            self.new
                .strip_prefix(&self.root)
                .unwrap_or(&self.new)
                .display()
        )];
        for (path, a, b) in &self.files {
            if a != b {
                lines.push(format!(
                    "Update {}",
                    path.strip_prefix(&self.root).unwrap_or(path).display()
                ));
                for (old, new) in
                    a.lines().zip(b.lines()).filter(|(a, b)| a != b).take(3)
                {
                    lines.push(format!("− {old}\n+ {new}"));
                }
            }
        }
        lines.join("\n")
    }
    fn apply(&self) -> Result<(), String> {
        if self.new.exists() {
            return Err("Rename destination now exists. Preview again.".into());
        }
        for (path, before, _) in &self.files {
            if std::fs::read_to_string(path).ok().as_ref() != Some(before) {
                return Err("A file changed after preview. Preview again.".into());
            }
        }
        let mut changes = self
            .files
            .iter()
            .map(|(path, _, text)| {
                (
                    if path == &self.old {
                        self.new.clone()
                    } else {
                        path.clone()
                    },
                    Some(text.clone()),
                )
            })
            .collect::<Vec<_>>();
        changes.push((self.old.clone(), None));
        crate::persistence::write_changes(changes)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("old.dart");
        let new = dir.path().join("new.dart");
        let usage = dir.path().join("usage.dart");
        std::fs::write(&old, "class Example {}\n").unwrap();
        std::fs::write(&usage, "import 'old.dart';\n// old.dart 📚\n").unwrap();
        (dir, old, new, usage)
    }
    #[test]
    fn rename_updates_analyzer_edits_and_optional_comments_atomically() {
        let (dir, old, new, usage) = fixture();
        let mut changes = std::collections::HashMap::new();
        changes.insert(
            lsp_types::Url::from_file_path(&usage).unwrap(),
            vec![lsp_types::TextEdit::new(
                lsp_types::Range::new(
                    lsp_types::Position::new(0, 8),
                    lsp_types::Position::new(0, 16),
                ),
                "new.dart".into(),
            )],
        );
        let edit = lsp_types::WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        };
        let plan =
            prepare(dir.path(), &old, &new, Some(&edit), true, false).unwrap();
        plan.apply().unwrap();
        assert!(!old.exists());
        assert!(new.exists());
        assert_eq!(
            std::fs::read_to_string(usage).unwrap(),
            "import 'new.dart';\n// new.dart 📚\n"
        );
    }
    #[test]
    fn changed_usage_or_destination_prevents_all_writes() {
        let (dir, old, new, usage) = fixture();
        let plan = prepare(dir.path(), &old, &new, None, true, false).unwrap();
        std::fs::write(&usage, "edited while preview open").unwrap();
        assert!(plan.apply().is_err());
        assert!(old.exists());
        assert!(!new.exists());
        assert_eq!(
            std::fs::read_to_string(usage).unwrap(),
            "edited while preview open"
        );
    }
    #[test]
    fn no_comment_search_preserves_text_and_skips_build_outputs() {
        let (dir, old, new, usage) = fixture();
        std::fs::create_dir(dir.path().join("build")).unwrap();
        let generated = dir.path().join("build/generated.dart");
        std::fs::write(&generated, "// old.dart").unwrap();
        let plan = prepare(dir.path(), &old, &new, None, false, false).unwrap();
        assert_eq!(plan.files.len(), 1);
        let plan = prepare(dir.path(), &old, &new, None, true, false).unwrap();
        plan.apply().unwrap();
        assert!(std::fs::read_to_string(usage).unwrap().contains("new.dart"));
        assert_eq!(std::fs::read_to_string(generated).unwrap(), "// old.dart");
    }
    #[test]
    fn unterminated_nested_unicode_comments_are_safe() {
        let source = "/* outer /* nested */ old.dart 📚";
        let ranges = crate::dart::regex::skip_ranges(source);
        assert_eq!(ranges, vec![(0, source.len())]);
    }
}

#[cfg(test)]
mod delete_tests {
    use super::*;
    #[test]
    fn safe_delete_refuses_usages_and_preserves_a_backup_when_unused() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("model.dart");
        let probe = dir.path().join("probe.dart");
        let usage = dir.path().join("main.dart");
        std::fs::write(&old, "class Model {}\n").unwrap();
        std::fs::write(&usage, "import 'model.dart';\n").unwrap();
        let mut changes = std::collections::HashMap::new();
        changes.insert(
            lsp_types::Url::from_file_path(&usage).unwrap(),
            vec![lsp_types::TextEdit::new(
                lsp_types::Range::new(
                    lsp_types::Position::new(0, 8),
                    lsp_types::Position::new(0, 18),
                ),
                "probe.dart".into(),
            )],
        );
        let edit = lsp_types::WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        };
        assert!(
            prepare(dir.path(), &old, &probe, Some(&edit), true, false)
                .unwrap()
                .for_delete()
                .is_err()
        );
        std::fs::write(&usage, "void main() {}\n").unwrap();
        let plan = prepare(dir.path(), &old, &probe, None, true, false)
            .unwrap()
            .for_delete()
            .unwrap();
        let backup = plan.new.clone();
        plan.apply().unwrap();
        assert!(!old.exists());
        assert_eq!(std::fs::read_to_string(backup).unwrap(), "class Model {}\n");
    }
}

#[cfg(test)]
mod text_tests {
    use super::*;
    #[test]
    fn textual_search_preserves_other_library_uris_and_does_not_rename_twice() {
        let source = "import /* retained */ r'other/foo.dart';\nexport 'different/foo.dart';\npart 'already_new_foo.dart';\n// foo.dart 📚\nconst path = 'foo.dart';";
        let updated = textual_references(source, "foo.dart", "new_foo.dart", true);
        assert!(updated.contains("other/foo.dart"));
        assert!(updated.contains("different/foo.dart"));
        assert!(updated.contains("already_new_foo.dart"));
        assert!(!updated.contains("already_new_new_foo"));
        assert!(updated.contains("// new_foo.dart 📚"));
        assert!(updated.contains("const path = 'new_foo.dart'"));
    }
}
