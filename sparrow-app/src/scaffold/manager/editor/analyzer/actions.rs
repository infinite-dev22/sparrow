//! Preview-only analyzer edits. No action command or project edit is executed.
use super::*;
use floem::views::editor::{
    Editor,
    core::{editor::EditType, selection::Selection},
    text::Document,
    text_document::TextDocument,
};
use lsp_types::{
    CodeAction, CodeActionOrCommand, DocumentChangeOperation, DocumentChanges, OneOf,
};

#[derive(Clone, Debug)]
pub struct Action {
    pub title: String,
    pub disabled: Option<String>,
    action: CodeAction,
}
#[derive(Clone, Debug)]
pub struct FileChange {
    pub source: PathBuf,
    pub before: String,
    pub after: String,
    edits: Vec<(Range<usize>, String)>,
}
#[derive(Clone, Debug)]
pub struct Preview {
    pub title: String,
    version: i64,
    expected: BTreeMap<PathBuf, String>,
    pub files: Vec<FileChange>,
}
impl Preview {
    pub fn diff(&self) -> String {
        format!(
            "DART ACTION · {}\nDraft only · review before Apply Action\n\n{}",
            self.title,
            self.files
                .iter()
                .map(|f| engine::change_diff(
                    Path::new(""),
                    &engine::Change {
                        path: f.source.clone(),
                        before: Some(f.before.clone()),
                        after: Some(f.after.clone()),
                    }
                ))
                .collect::<Vec<_>>()
                .join("\n")
        )
    }
    fn validate(&self, draft: &Draft, version: i64) -> Result<(), String> {
        if self.version != version || draft.files != self.expected {
            return Err("Draft or sample changed since action preview; request the action again.".into());
        }
        Ok(())
    }
}
#[derive(Clone)]
pub struct State {
    pub list: RwSignal<Vec<Action>>,
    pub message: RwSignal<String>,
    pub preview: RwSignal<Option<Preview>>,
    pub undo: RwSignal<Option<Preview>>,
    ticket: Rc<std::cell::Cell<u64>>,
    pending: Rc<std::cell::Cell<bool>>,
}
impl State {
    pub fn new(scope: Scope) -> Self {
        Self {
            list: scope.create_rw_signal(vec![]),
            message: scope.create_rw_signal(String::new()),
            preview: scope.create_rw_signal(None),
            undo: scope.create_rw_signal(None),
            ticket: Rc::new(std::cell::Cell::new(0)),
            pending: Rc::new(std::cell::Cell::new(false)),
        }
    }
    pub fn clear(&self) {
        self.ticket.set(self.ticket.get().wrapping_add(1));
        self.pending.set(false);
        self.list.set(vec![]);
        self.preview.set(None);
        self.message.set(String::new());
    }
}
// Dart attaches this optional analytics command to ordinary inline fixes.
// The text edit is complete without it; omit logging and never execute it.
fn requires_command(action: &CodeAction) -> bool {
    action
        .command
        .as_ref()
        .is_some_and(|command| command.command != "dart.logAction")
}
fn actions(items: Vec<CodeActionOrCommand>) -> Vec<Action> {
    let mut result = items.into_iter().map(|item| {
        let action = match item {
            CodeActionOrCommand::CodeAction(action) => action,
            CodeActionOrCommand::Command(command) => CodeAction {
                title: command.title.clone(), command: Some(command), ..Default::default()
            },
        };
        let disabled = action.disabled.as_ref().map(|d|d.reason.clone()).or_else(|| {
            requires_command(&action).then(|| "Requires a server command; cannot preview safely in a template.".into())
        });
        Action { title:action.title.clone(),disabled,action }
    }).collect::<Vec<_>>();
    // Preserve the analyzer's ordering within each group (it ranks fixes).
    result.sort_by_key(|a| {
        (
            a.disabled.is_some(),
            !a.action.is_preferred.unwrap_or(false),
        )
    });
    result
}

impl Snapshot {
    fn action_preview(
        &self,
        draft: &Draft,
        action: &CodeAction,
    ) -> Result<Preview, String> {
        if let Some(disabled) = &action.disabled {
            return Err(disabled.reason.clone());
        }
        if requires_command(action) {
            return Err("This action requires a server command and cannot be previewed in a template.".into());
        }
        let edit = action
            .edit
            .as_ref()
            .ok_or("This action has no previewable text edits.")?;
        let mut grouped = BTreeMap::<PathBuf, Vec<TextEdit>>::new();
        if edit.changes.is_some() && edit.document_changes.is_some() {
            return Err(
                "Ambiguous workspace edit contains both edit formats.".into()
            );
        }
        if edit.change_annotations.as_ref().is_some_and(|annotations| {
            annotations
                .values()
                .any(|a| a.needs_confirmation == Some(true))
        }) {
            return Err("This action requires additional change confirmation and cannot be applied here.".into());
        }
        let mut add = |uri: &lsp_types::Url,
                       version: Option<i32>,
                       edits: Vec<TextEdit>|
         -> Result<(), String> {
            let output = uri
                .to_file_path()
                .map_err(|_| "Action contains a non-file URI")?;
            if !self.files.contains_key(&output) {
                return Err(
                    "Action edits a project file outside this template preview."
                        .into(),
                );
            }
            if version.is_some_and(|v| i64::from(v) != self.version) {
                return Err("Action contains a stale document version.".into());
            }
            grouped.entry(output).or_default().extend(edits);
            Ok(())
        };
        if let Some(changes) = &edit.changes {
            for (uri, edits) in changes {
                add(uri, None, edits.clone())?;
            }
        }
        let mut document = |doc: &lsp_types::TextDocumentEdit| {
            add(
                &doc.text_document.uri,
                doc.text_document.version,
                doc.edits
                    .iter()
                    .map(|e| match e {
                        OneOf::Left(edit) => edit.clone(),
                        OneOf::Right(edit) => edit.text_edit.clone(),
                    })
                    .collect(),
            )
        };
        match &edit.document_changes {
            Some(DocumentChanges::Edits(edits)) => {
                for edit in edits {
                    document(edit)?;
                }
            }
            Some(DocumentChanges::Operations(operations)) => {
                for operation in operations {
                    match operation {
                DocumentChangeOperation::Edit(edit)=>document(edit)?,
                DocumentChangeOperation::Op(_)=>return Err("File creation, rename and deletion actions are not supported in template buffers.".into()),
            }
                }
            }
            None => {}
        }
        let mut mapped = BTreeMap::<PathBuf, Vec<(Range<usize>, String)>>::new();
        for (output, edits) in grouped {
            let projection = &self.files[&output];
            let source = projection.source.as_ref().ok_or("This action targets inline YAML content. Move it to a Dart template source file to apply fixes.")?;
            if draft.files.get(source) != Some(&projection.raw) {
                return Err(
                    "Template changed since analysis; request the action again."
                        .into(),
                );
            }
            // One source reused at multiple output paths can have different semantics.
            if self
                .files
                .values()
                .filter(|p| p.source.as_ref() == Some(source))
                .count()
                != 1
            {
                return Err("This source is reused by multiple outputs; edit it manually to avoid an ambiguous fix.".into());
            }
            for edit in edits {
                let range = position_range(&projection.generated, edit.range)
                    .ok_or("Invalid analyzer edit range")?;
                let raw = projection.edit_range(range).ok_or("Action would replace a template variable or a rewritten import URI; edit it manually.")?;
                let text = self.restore_template_names(&edit.new_text);
                mapped.entry(source.clone()).or_default().push((raw, text));
            }
        }
        let mut files = vec![];
        for (source, mut edits) in mapped {
            edits.sort_by_key(|(range, _)| (range.start, range.end));
            if edits
                .windows(2)
                .any(|p| p[0].0.end > p[1].0.start || p[0].0.start == p[1].0.start)
            {
                return Err("Analyzer action contains overlapping edits.".into());
            }
            let before = draft.files[&source].clone();
            let mut after = before.clone();
            for (range, text) in edits.iter().rev() {
                after.replace_range(range.clone(), text);
            }
            if before != after {
                files.push(FileChange {
                    source,
                    before,
                    after,
                    edits,
                });
            }
        }
        if files.is_empty() {
            return Err("This action makes no template changes.".into());
        }
        // Generated identifiers restored in new text must still be valid template syntax.
        let mut updated = draft.clone();
        for file in &files {
            updated
                .files
                .insert(file.source.clone(), file.after.clone());
        }
        updated.validate(&self.sample)?;
        Ok(Preview {
            title: action.title.clone(),
            version: self.version,
            expected: draft.files.clone(),
            files,
        })
    }
}
impl Session {
    fn action_deadline(self: &Rc<Self>, ticket: u64) {
        self.actions.pending.set(true);
        let target = self.clone();
        floem::action::exec_after(std::time::Duration::from_secs(12), move |_| {
            if target.alive.get()
                && target.actions.ticket.get() == ticket
                && target.actions.pending.replace(false)
            {
                target.actions.ticket.set(ticket.wrapping_add(1));
                target.actions.message.set("Dart action request timed out. Try again when analysis has finished.".into());
            }
        });
    }
    pub fn apply_action(
        &self,
        draft: RwSignal<Option<Draft>>,
        documents: &BTreeMap<PathBuf, Rc<TextDocument>>,
        plan: RwSignal<Option<engine::Plan>>,
        undo: bool,
    ) -> Result<(), String> {
        let version = self
            .snapshot
            .get_untracked()
            .map(|s| s.version)
            .unwrap_or(-1);
        apply(&self.actions, draft, version, documents, plan, undo)
    }
    pub fn request_actions(
        self: &Rc<Self>,
        ed: &Editor,
        selected: RwSignal<PathBuf>,
    ) {
        self.actions.clear();
        let Some(snapshot) = self.snapshot.get_untracked() else {
            self.actions.message.set(
                "Dart actions need a valid rendered draft and a running analyzer."
                    .into(),
            );
            return;
        };
        let source = selected.get_untracked();
        let Some((output, p)) = snapshot.file_for(&source) else {
            self.actions
                .message
                .set("Open a file-backed Dart template to request actions.".into());
            return;
        };
        if ed.doc().text().to_string() != p.raw {
            return;
        }
        let cursor = ed.cursor.get_untracked();
        let (a, b) = cursor
            .get_selection()
            .unwrap_or((cursor.offset(), cursor.offset()));
        let range = p
            .to_generated(a.min(b))
            .and_then(|start| position(&p.generated, start))
            .zip(
                p.to_generated(a.max(b))
                    .and_then(|end| position(&p.generated, end)),
            );
        let Some((start, end)) = range else {
            self.actions.message.set("Move the caret outside a template variable to request Dart actions.".into());
            return;
        };
        let diagnostics = self
            .raw_diagnostics
            .borrow()
            .get(output)
            .cloned()
            .unwrap_or_default();
        self.actions.message.set("Requesting Dart actions…".into());
        let ticket = self.actions.ticket.get();
        self.action_deadline(ticket);
        let target = self.clone();
        let version = snapshot.version;
        let cb = floem::ext_event::create_ext_action(
            self.scope,
            move |result: Result<Vec<CodeActionOrCommand>, String>| {
                if !target.alive.get()
                    || target.actions.ticket.get() != ticket
                    || selected.get_untracked() != source
                    || target
                        .snapshot
                        .get_untracked()
                        .is_none_or(|s| s.version != version)
                {
                    return;
                }
                target.actions.pending.set(false);
                match result {
                    Err(error) => {
                        target.actions.message.set(format!("Dart actions: {error}"))
                    }
                    Ok(items) => {
                        let items = actions(items);
                        target.actions.message.set(if items.is_empty() {
                            "No Dart actions at this location.".into()
                        } else {
                            format!(
                                "{} Dart actions · choose one to preview",
                                items.len()
                            )
                        });
                        target.actions.list.set(items);
                    }
                }
            },
        );
        self.service.code_actions_range_result(
            output,
            lsp_types::Range::new(start, end),
            &diagnostics,
            None,
            cb,
        );
    }
    pub fn preview_action(
        self: &Rc<Self>,
        choice: &Action,
        draft: RwSignal<Option<Draft>>,
    ) {
        if let Some(reason) = &choice.disabled {
            self.actions.message.set(reason.clone());
            return;
        }
        let Some(snapshot) = self.snapshot.get_untracked() else {
            return;
        };
        let Some(expected) = draft.get_untracked() else {
            return;
        };
        self.actions
            .ticket
            .set(self.actions.ticket.get().wrapping_add(1));
        let ticket = self.actions.ticket.get();
        self.action_deadline(ticket);
        self.actions.preview.set(None);
        self.actions.message.set("Preparing action diff…".into());
        let target = self.clone();
        let finish = move |result: Result<CodeAction, String>| {
            if !target.alive.get()
                || target.actions.ticket.get() != ticket
                || target
                    .snapshot
                    .get_untracked()
                    .is_none_or(|s| s.version != snapshot.version)
                || draft
                    .get_untracked()
                    .is_none_or(|d| d.files != expected.files)
            {
                return;
            }
            target.actions.pending.set(false);
            match result
                .and_then(|action| snapshot.action_preview(&expected, &action))
            {
                Ok(preview) => {
                    target.actions.message.set(format!("Review {} file(s), then Apply Action. Files are saved only through Preview Save / Apply Save.",preview.files.len()));
                    target.actions.preview.set(Some(preview));
                }
                Err(error) => target.actions.message.set(error),
            }
        };
        if choice.action.edit.is_some() {
            finish(Ok(choice.action.clone()));
        } else {
            let cb = floem::ext_event::create_ext_action(self.scope, finish);
            self.service
                .resolve_code_action_full(choice.action.clone(), cb);
        }
    }
}

/// Apply all affected source buffers together. Their native histories remain
/// available; Undo Action restores a multi-file action together, after checking
/// that no affected buffer has been edited since Apply.
pub fn apply(
    state: &State,
    draft: RwSignal<Option<Draft>>,
    version: i64,
    documents: &BTreeMap<PathBuf, Rc<TextDocument>>,
    plan: RwSignal<Option<engine::Plan>>,
    undo: bool,
) -> Result<(), String> {
    let preview = if undo {
        state.undo.get_untracked()
    } else {
        state.preview.get_untracked()
    }
    .ok_or("No Dart action to apply")?;
    let current = draft.get_untracked().ok_or("No template draft")?;
    if undo {
        if preview
            .files
            .iter()
            .any(|f| current.files.get(&f.source) != Some(&f.after))
        {
            return Err("An affected file changed after the action. Use its editor undo history or undo the later edits first.".into());
        }
    } else {
        preview.validate(&current, version)?;
    }
    // Preflight every cached document before mutating any of them.
    for file in &preview.files {
        if let Some(doc) = documents.get(&file.source) {
            if doc.text().to_string()
                != if undo {
                    file.after.clone()
                } else {
                    file.before.clone()
                }
            {
                return Err("An editor buffer changed since preview; request the action again.".into());
            }
        }
    }
    for file in &preview.files {
        if let Some(doc) = documents.get(&file.source) {
            if undo {
                doc.edit_single(
                    Selection::region(0, file.after.len()),
                    &file.before,
                    EditType::Other,
                );
            } else {
                doc.edit(
                    &mut file.edits.iter().map(|(r, s)| {
                        (Selection::region(r.start, r.end), s.as_str())
                    }),
                    EditType::Other,
                );
            }
        }
    }
    plan.set(None);
    draft.update(|d| {
        if let Some(d) = d {
            for file in &preview.files {
                d.files.insert(
                    file.source.clone(),
                    if undo {
                        file.before.clone()
                    } else {
                        file.after.clone()
                    },
                );
            }
        }
    });
    state.preview.set(None);
    state.list.set(vec![]);
    state.undo.set(if undo { None } else { Some(preview) });
    state.message.set(if undo{"Dart action undone in template draft."}else{"Dart action applied to draft. Ctrl+Z undoes this file; Undo Action restores all affected files."}.into());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::tests::fixture;
    use super::*;
    use floem::keyboard::Modifiers;
    use floem::views::editor::{
        command::Command, core::command::EditCommand, text::SimpleStyling,
    };
    use lsp_types::{
        OptionalVersionedTextDocumentIdentifier, TextDocumentEdit, WorkspaceEdit,
    };
    fn replacement(
        snapshot: &Snapshot,
        source: &str,
        needle: &str,
        text: &str,
    ) -> (lsp_types::Url, TextEdit) {
        let (path, p) = snapshot.file_for(Path::new(source)).unwrap();
        let start = p.generated.find(needle).unwrap();
        (
            lsp_types::Url::from_file_path(path).unwrap(),
            TextEdit {
                range: lsp_types::Range::new(
                    position(&p.generated, start).unwrap(),
                    position(&p.generated, start + needle.len()).unwrap(),
                ),
                new_text: text.into(),
            },
        )
    }
    fn action(edits: Vec<(lsp_types::Url, TextEdit)>) -> CodeAction {
        let mut changes = std::collections::HashMap::<_, Vec<_>>::new();
        for (uri, edit) in edits {
            changes.entry(uri).or_default().push(edit);
        }
        CodeAction {
            title: "Change type to int".into(),
            edit: Some(WorkspaceEdit {
                changes: Some(changes),
                ..Default::default()
            }),
            ..Default::default()
        }
    }
    fn two_files() -> (tempfile::TempDir, Draft, Snapshot, CodeAction) {
        let (root, mut draft) = fixture("class {{name}} { String count = 42; }\n");
        draft.files.get_mut(Path::new("pack.yaml")).unwrap().push_str("  - path: lib/second.dart\n    source: templates/second.dart.tmpl\n");
        draft.files.insert(
            "templates/second.dart.tmpl".into(),
            "String count = 7;\n".into(),
        );
        let s = Snapshot::new(&draft, "Orders", 5).unwrap();
        let a = action(vec![
            replacement(&s, "templates/main.dart.tmpl", "String", "int"),
            replacement(&s, "templates/second.dart.tmpl", "String", "int"),
        ]);
        (root, draft, s, a)
    }
    #[test]
    fn maps_multi_file_actions_to_template_diffs_without_writes() {
        let (root, draft, s, action) = two_files();
        let preview = s.action_preview(&draft, &action).unwrap();
        assert_eq!(preview.files.len(), 2);
        let diff = preview.diff();
        assert!(diff.contains("-class {{name}} { String count = 42; }"));
        assert!(diff.contains("+class {{name}} { int count = 42; }"));
        assert!(diff.contains("templates/second.dart.tmpl"));
        assert!(preview.files[0].after.contains("{{name}}"));
        assert!(!root.path().join("lib").exists());
        preview.validate(&draft, 5).unwrap();
        assert!(preview.validate(&draft, 6).is_err());
        let mut changed = draft.clone();
        changed
            .files
            .get_mut(Path::new("pack.yaml"))
            .unwrap()
            .push('\n');
        assert!(preview.validate(&changed, 5).is_err());
    }
    #[test]
    fn rejects_unsafe_ranges_commands_project_edits_and_stale_versions() {
        let (_root, draft) = fixture("class {{name}} { String count = 42; }\n");
        let s = Snapshot::new(&draft, "Orders", 4).unwrap();
        let protected = action(vec![replacement(
            &s,
            "templates/main.dart.tmpl",
            "Orders",
            "Other",
        )]);
        assert!(
            s.action_preview(&draft, &protected)
                .unwrap_err()
                .contains("template variable")
        );
        let (uri, edit) =
            replacement(&s, "templates/main.dart.tmpl", "String", "int");
        let mut a = action(vec![(uri.clone(), edit.clone())]);
        a.command = Some(lsp_types::Command {
            title: "command".into(),
            command: "dart.edit".into(),
            arguments: None,
        });
        assert!(
            s.action_preview(&draft, &a)
                .unwrap_err()
                .contains("server command")
        );
        a.command = None;
        a.edit = Some(WorkspaceEdit {
            document_changes: Some(DocumentChanges::Edits(vec![TextDocumentEdit {
                text_document: OptionalVersionedTextDocumentIdentifier {
                    uri: uri.clone(),
                    version: Some(3),
                },
                edits: vec![OneOf::Left(edit.clone())],
            }])),
            ..Default::default()
        });
        assert!(s.action_preview(&draft, &a).unwrap_err().contains("stale"));
        a.edit.as_mut().unwrap().document_changes =
            Some(DocumentChanges::Operations(vec![
                DocumentChangeOperation::Op(lsp_types::ResourceOp::Delete(
                    lsp_types::DeleteFile {
                        uri: uri.clone(),
                        options: None,
                    },
                )),
            ]));
        assert!(
            s.action_preview(&draft, &a)
                .unwrap_err()
                .contains("creation, rename and deletion")
        );
        let outside =
            lsp_types::Url::from_file_path(draft.ctx.root.join("lib/project.dart"))
                .unwrap();
        assert!(
            s.action_preview(&draft, &action(vec![(outside, edit.clone())]))
                .unwrap_err()
                .contains("outside")
        );
        assert!(
            s.action_preview(
                &draft,
                &action(vec![
                    (uri.clone(), edit.clone()),
                    (uri.clone(), edit.clone())
                ])
            )
            .unwrap_err()
            .contains("overlapping")
        );
        let mut invalid = edit;
        invalid.range.start.character = 9999;
        assert!(
            s.action_preview(&draft, &action(vec![(uri, invalid)]))
                .unwrap_err()
                .contains("Invalid")
        );
    }
    #[test]
    fn versioned_annotated_edits_restore_names_and_reject_confirmation() {
        let (_root, draft) = fixture("class {{name}} { String count = 42; }\n");
        let s = Snapshot::new(&draft, "Orders", 4).unwrap();
        let (uri, edit) =
            replacement(&s, "templates/main.dart.tmpl", "String", "Orders");
        let mut a=CodeAction{title:"Use class type".into(),edit:Some(serde_json::from_value(serde_json::json!({
            "documentChanges":[{"textDocument":{"uri":uri,"version":4},"edits":[{"range":edit.range,"newText":"Orders","annotationId":"fix"}]}],
            "changeAnnotations":{"fix":{"label":"Fix type","needsConfirmation":false}}
        })).unwrap()),..Default::default()};
        let preview = s.action_preview(&draft, &a).unwrap();
        assert!(preview.files[0].after.contains("{{name}} count"));
        a.edit
            .as_mut()
            .unwrap()
            .change_annotations
            .as_mut()
            .unwrap()
            .get_mut("fix")
            .unwrap()
            .needs_confirmation = Some(true);
        assert!(
            s.action_preview(&draft, &a)
                .unwrap_err()
                .contains("confirmation")
        );
        a.edit.as_mut().unwrap().changes = Some(Default::default());
        assert!(
            s.action_preview(&draft, &a)
                .unwrap_err()
                .contains("Ambiguous")
        );
    }
    #[test]
    fn action_list_preserves_disabled_reasons_and_prefers_recommended_fix() {
        let command = lsp_types::Command {
            title: "Extract widget".into(),
            command: "dart.refactor".into(),
            arguments: None,
        };
        let result = actions(vec![
            CodeActionOrCommand::Command(command),
            CodeActionOrCommand::CodeAction(CodeAction {
                title: "Preferred fix".into(),
                is_preferred: Some(true),
                ..Default::default()
            }),
            CodeActionOrCommand::CodeAction(CodeAction {
                title: "Disabled".into(),
                disabled: Some(lsp_types::CodeActionDisabled {
                    reason: "Requires Flutter".into(),
                }),
                ..Default::default()
            }),
        ]);
        let logged = actions(vec![CodeActionOrCommand::CodeAction(CodeAction {
            title: "Inline fix".into(),
            edit: Some(WorkspaceEdit::default()),
            command: Some(lsp_types::Command {
                title: "Log Action".into(),
                command: "dart.logAction".into(),
                arguments: None,
            }),
            ..Default::default()
        })]);
        assert!(logged[0].disabled.is_none());
        assert_eq!(result[0].title, "Preferred fix");
        assert_eq!(
            result
                .iter()
                .find(|a| a.title == "Disabled")
                .unwrap()
                .disabled
                .as_deref(),
            Some("Requires Flutter")
        );
        assert!(
            result
                .iter()
                .find(|a| a.title == "Extract widget")
                .unwrap()
                .disabled
                .is_some()
        );
    }
    #[test]
    fn apply_is_atomic_across_buffers_and_grouped_undo_preserves_later_edits() {
        let (_root, initial, s, a) = two_files();
        let preview = s.action_preview(&initial, &a).unwrap();
        let scope = Scope::new();
        let draft = scope.create_rw_signal(Some(initial.clone()));
        let plan = scope.create_rw_signal(None);
        let state = State::new(scope);
        let first = preview.files[0].source.clone();
        let doc = Rc::new(TextDocument::new(scope, initial.files[&first].clone()));
        let documents = BTreeMap::from([(first.clone(), doc.clone())]);
        state.preview.set(Some(preview.clone()));
        // A stale buffer prevents every file in the action from changing.
        doc.edit_single(Selection::caret(0), "//later\n", EditType::Other);
        assert!(apply(&state, draft, 5, &documents, plan, false).is_err());
        assert_eq!(draft.get_untracked().unwrap().files, initial.files);
        doc.edit_single(Selection::region(0, 8), "", EditType::Other);
        apply(&state, draft, 5, &documents, plan, false).unwrap();
        assert_eq!(doc.text().to_string(), preview.files[0].after);
        assert!(
            draft.get_untracked().unwrap().files
                [Path::new("templates/second.dart.tmpl")]
            .starts_with("int")
        );
        draft.update(|d| {
            d.as_mut()
                .unwrap()
                .files
                .get_mut(Path::new("templates/second.dart.tmpl"))
                .unwrap()
                .push_str("// later");
        });
        assert!(apply(&state, draft, 5, &documents, plan, true).is_err());
        assert_eq!(doc.text().to_string(), preview.files[0].after);
        draft.update(|d| {
            d.as_mut().unwrap().files.insert(
                "templates/second.dart.tmpl".into(),
                preview.files[1].after.clone(),
            );
        });
        apply(&state, draft, 5, &documents, plan, true).unwrap();
        assert_eq!(doc.text().to_string(), initial.files[&first]);
        assert_eq!(draft.get_untracked().unwrap().files, initial.files);
        scope.dispose();
    }
    #[test]
    fn native_editor_undo_reverses_a_multi_edit_fix_as_one_operation() {
        let (_root, initial, s, mut a) = two_files();
        let (uri, edit) =
            replacement(&s, "templates/main.dart.tmpl", "String", "int");
        let (same, second) = replacement(&s, "templates/main.dart.tmpl", "42", "43");
        a.edit = action(vec![(uri, edit), (same, second)]).edit;
        let preview = s.action_preview(&initial, &a).unwrap();
        let scope = Scope::new();
        let state = State::new(scope);
        state.preview.set(Some(preview.clone()));
        let draft = scope.create_rw_signal(Some(initial.clone()));
        let plan = scope.create_rw_signal(None);
        let path = preview.files[0].source.clone();
        let doc = Rc::new(TextDocument::new(scope, initial.files[&path].clone()));
        let ed =
            Editor::new(scope, doc.clone(), Rc::new(SimpleStyling::new()), false);
        apply(
            &state,
            draft,
            5,
            &BTreeMap::from([(path.clone(), doc.clone())]),
            plan,
            false,
        )
        .unwrap();
        assert!(doc.text().to_string().contains("int count = 43"));
        ed.doc().run_command(
            &ed,
            &Command::Edit(EditCommand::Undo),
            Some(1),
            Modifiers::empty(),
        );
        assert_eq!(doc.text().to_string(), initial.files[&path]);
        scope.dispose();
    }
    #[test]
    fn rejects_inline_reused_sources_rewritten_imports_and_invalid_template_edits() {
        let (_root, mut draft) = fixture(
            "import 'relative.dart';\nclass {{name}} { String count = 42; }\n",
        );
        let snapshot = Snapshot::new(&draft, "Orders", 4).unwrap();
        let (uri, p) = snapshot
            .file_for(Path::new("templates/main.dart.tmpl"))
            .unwrap();
        let start = p.generated.find("file:").unwrap();
        let end = start + p.generated[start..].find("'").unwrap();
        let edit = TextEdit {
            range: lsp_types::Range::new(
                position(&p.generated, start).unwrap(),
                position(&p.generated, end).unwrap(),
            ),
            new_text: "other.dart".into(),
        };
        assert!(
            snapshot
                .action_preview(
                    &draft,
                    &action(vec![(
                        lsp_types::Url::from_file_path(uri).unwrap(),
                        edit
                    )])
                )
                .unwrap_err()
                .contains("rewritten import")
        );
        let bad = action(vec![replacement(
            &snapshot,
            "templates/main.dart.tmpl",
            "String",
            "{{unknown}}",
        )]);
        assert!(snapshot.action_preview(&draft, &bad).is_err());
        draft
            .files
            .get_mut(Path::new("pack.yaml"))
            .unwrap()
            .push_str(
                "  - path: lib/other.dart\n    source: templates/main.dart.tmpl\n",
            );
        let repeated = Snapshot::new(&draft, "Orders", 4).unwrap();
        assert!(
            repeated
                .action_preview(
                    &draft,
                    &action(vec![replacement(
                        &repeated,
                        "templates/main.dart.tmpl",
                        "String",
                        "int"
                    )])
                )
                .unwrap_err()
                .contains("reused")
        );
        draft.files.insert("pack.yaml".into(),"schema: 2\nid: team.inline\nversion: 1.0.0\ntitle: Inline\nfiles:\n  - path: lib/inline.dart\n    content: 'String count = 42;'\n".into());
        let inline = Snapshot::new(&draft, "Orders", 5).unwrap();
        let (output, p) = inline.files.iter().next().unwrap();
        let start = p.generated.find("String").unwrap();
        let edit = TextEdit {
            range: lsp_types::Range::new(
                position(&p.generated, start).unwrap(),
                position(&p.generated, start + 6).unwrap(),
            ),
            new_text: "int".into(),
        };
        assert!(
            inline
                .action_preview(
                    &draft,
                    &action(vec![(
                        lsp_types::Url::from_file_path(output).unwrap(),
                        edit
                    )])
                )
                .unwrap_err()
                .contains("inline YAML")
        );
    }
    #[test]
    #[ignore = "requires SPARROW_TEMPLATE_DART_SDK for real Dart quick fixes"]
    fn real_dart_quick_fix_previews_applies_and_clears_diagnostic_without_project_writes()
     {
        use std::{
            fs,
            time::{Duration, Instant},
        };
        let sdk = std::env::var("SPARROW_TEMPLATE_DART_SDK").unwrap();
        let (root, initial) =
            fixture("class {{name}} { String bad = 42; int other = 'wrong'; }\n");
        fs::create_dir(root.path().join(".sparrow")).unwrap();
        fs::write(
            root.path().join(".sparrow/settings.toml"),
            format!("[core]\ndart-sdk-path = '{sdk}'\n"),
        )
        .unwrap();
        fs::create_dir(root.path().join(".dart_tool")).unwrap();
        fs::write(root.path().join(".dart_tool/package_config.json"),r#"{"configVersion":2,"packages":[{"name":"analyzer_fixture","rootUri":"../","packageUri":"lib/","languageVersion":"3.9"}]}"#).unwrap();
        let pubspec = fs::read(root.path().join("pubspec.yaml")).unwrap();
        let (service, notifications) =
            DartLspService::spawn_for_templates(root.path().to_path_buf());
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            match notifications
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap()
            {
                Notification::Ready => break,
                Notification::Error(e) => panic!("{e}"),
                _ => {}
            }
        }
        let snapshot = Snapshot::new(&initial, "Orders", 8).unwrap();
        let source = Path::new("templates/main.dart.tmpl");
        let (path, p) = snapshot.file_for(source).unwrap();
        service.send_change(path.clone(), p.generated.clone(), 8);
        let deadline = Instant::now() + Duration::from_secs(30);
        let diagnostics = loop {
            match notifications
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap()
            {
                Notification::Diagnostics {
                    path: got,
                    diagnostics,
                    ..
                } if got == *path
                    && diagnostics
                        .iter()
                        .any(|d| d.message.contains("assigned")) =>
                {
                    break diagnostics;
                }
                Notification::Error(e) => panic!("{e}"),
                _ => {}
            }
        };
        let at = position(&p.generated, p.generated.find("42").unwrap()).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        service.code_actions_result(
            path,
            at.line,
            at.character,
            &diagnostics,
            Some("quickfix"),
            move |result| {
                let _ = tx.send(result);
            },
        );
        let items = rx.recv_timeout(Duration::from_secs(25)).unwrap().unwrap();
        let mut previews = vec![];
        let titles = items
            .iter()
            .map(|item| match item {
                CodeActionOrCommand::CodeAction(a) => format!(
                    "{} [edit={}, command={:?}]",
                    a.title,
                    a.edit.is_some(),
                    a.command
                ),
                CodeActionOrCommand::Command(c) => c.title.clone(),
            })
            .collect::<Vec<_>>();
        for item in items {
            let CodeActionOrCommand::CodeAction(mut action) = item else {
                continue;
            };
            if action.edit.is_none() && action.command.is_none() {
                let (tx, rx) = std::sync::mpsc::channel();
                service.resolve_code_action_full(action, move |result| {
                    let _ = tx.send(result);
                });
                let Ok(resolved) = rx.recv_timeout(Duration::from_secs(25)).unwrap()
                else {
                    continue;
                };
                action = resolved;
            }
            if let Ok(preview) = snapshot.action_preview(&initial, &action) {
                previews.push(preview);
            }
        }
        let preview = previews
            .into_iter()
            .find(|p| p.files.iter().any(|f| f.after.contains("int bad = 42")))
            .unwrap_or_else(|| panic!("Missing previewable type fix: {titles:?}"));
        assert!(preview.diff().contains("{{name}}"));
        assert_eq!(initial.files[source], p.raw);
        let scope = Scope::new();
        let state = State::new(scope);
        let draft = scope.create_rw_signal(Some(initial.clone()));
        let plan = scope.create_rw_signal(None);
        let doc = Rc::new(TextDocument::new(scope, p.raw.clone()));
        state.preview.set(Some(preview));
        let documents = BTreeMap::from([(source.to_path_buf(), doc.clone())]);
        apply(&state, draft, 8, &documents, plan, false).unwrap();
        let fixed = draft.get_untracked().unwrap();
        let revised = Snapshot::new(&fixed, "Orders", 9).unwrap();
        let (next, projection) = revised.file_for(source).unwrap();
        service.send_change(next.clone(), projection.generated.clone(), 9);
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            match notifications
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap()
            {
                Notification::Diagnostics {
                    path: got,
                    diagnostics,
                    ..
                } if got == *next => {
                    let at = projection.generated.find("42").unwrap();
                    assert!(!diagnostics.iter().any(|d| {
                        position_range(&projection.generated, d.range)
                            .is_some_and(|r| r.contains(&at))
                    }));
                    assert!(
                        diagnostics.iter().any(|d| d.message.contains("assigned")),
                        "the independent other-field error should remain"
                    );
                    break;
                }
                Notification::Error(e) => panic!("{e}"),
                _ => {}
            }
        }
        apply(&state, draft, 9, &documents, plan, true).unwrap();
        assert_eq!(draft.get_untracked().unwrap().files, initial.files);
        assert_eq!(doc.text().to_string(), p.raw);
        assert_eq!(fs::read(root.path().join("pubspec.yaml")).unwrap(), pubspec);
        assert!(!root.path().join("lib").exists());
        assert!(!root.path().join("pubspec.lock").exists());
        assert!(
            snapshot
                .files
                .keys()
                .chain(revised.files.keys())
                .all(|p| !p.exists())
        );
        service.shutdown_template_session();
        scope.dispose();
    }
}
