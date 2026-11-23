pub(crate) mod view;

use std::{path::PathBuf, rc::Rc};

use floem::{
    ext_event::create_ext_action,
    keyboard::Modifiers,
    peniko::kurbo::Rect,
    reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith},
};
use lapce_xi_rope::Rope;
use lsp_types::Position;
use sparrow_core::{command::FocusCommand, mode::Mode, selection::Selection};
use sparrow_rpc::proxy::ProxyResponse;

use crate::{
    command::{CommandExecuted, CommandKind, InternalCommand, SparrowCommand},
    editor::EditorData,
    keypress::{KeyPressFocus, condition::Condition},
    main_split::Editors,
    window_tab::{CommonData, Focus},
};

#[derive(Clone, Debug)]
pub struct RenameData {
    pub active: RwSignal<bool>,
    pub pending: RwSignal<bool>,
    pub error: RwSignal<String>,
    pub token: RwSignal<u64>,
    pub original_name: RwSignal<String>,
    pub source: RwSignal<Option<EditorData>>,
    pub revision: RwSignal<u64>,
    pub editor: EditorData,
    pub start: RwSignal<usize>,
    pub position: RwSignal<Position>,
    pub path: RwSignal<PathBuf>,
    pub layout_rect: RwSignal<Rect>,
    pub common: Rc<CommonData>,
}

impl KeyPressFocus for RenameData {
    fn get_mode(&self) -> Mode {
        Mode::Insert
    }

    fn check_condition(&self, condition: Condition) -> bool {
        matches!(condition, Condition::RenameFocus | Condition::ModalFocus)
    }

    fn run_command(
        &self,
        command: &SparrowCommand,
        count: Option<usize>,
        mods: Modifiers,
    ) -> CommandExecuted {
        match &command.kind {
            CommandKind::Workbench(_) => return CommandExecuted::No,
            CommandKind::Scroll(_) => return CommandExecuted::No,
            CommandKind::Focus(cmd) => {
                return self.run_focus_command(cmd);
            }
            CommandKind::Edit(_)
            | CommandKind::Move(_)
            | CommandKind::MultiSelection(_) => {
                if !self.pending.get_untracked() {
                    self.error.set(String::new());
                    return self.editor.run_command(command, count, mods);
                }
            }
            CommandKind::MotionMode(_) => {}
        }
        CommandExecuted::Yes
    }

    fn receive_char(&self, c: &str) {
        if !self.pending.get_untracked() {
            self.error.set(String::new());
            self.editor.receive_char(c);
        }
    }
}

impl RenameData {
    pub fn new(cx: Scope, editors: Editors, common: Rc<CommonData>) -> Self {
        let active = cx.create_rw_signal(false);
        let start = cx.create_rw_signal(0);
        let position = cx.create_rw_signal(Position::default());
        let layout_rect = cx.create_rw_signal(Rect::ZERO);
        let path = cx.create_rw_signal(PathBuf::new());
        let editor = editors.make_local(cx, common.clone());
        let rename = Self {
            active,
            pending: cx.create_rw_signal(false),
            error: cx.create_rw_signal(String::new()),
            token: cx.create_rw_signal(0),
            original_name: cx.create_rw_signal(String::new()),
            source: cx.create_rw_signal(None),
            revision: cx.create_rw_signal(0),
            editor,
            start,
            position,
            layout_rect,
            path,
            common,
        };
        let data = rename.clone();
        cx.create_effect(move |_| {
            let focus = data.common.focus.get();
            if focus != Focus::Rename && data.active.get_untracked() {
                data.cancel();
            }
        });
        rename
    }

    pub fn start(
        &self,
        path: PathBuf,
        placeholder: String,
        start: usize,
        position: Position,
        source: EditorData,
    ) {
        self.token.update(|token| *token += 1);
        self.pending.set(false);
        self.error.set(String::new());
        self.original_name.set(placeholder.clone());
        self.revision.set(source.doc().rev());
        self.source.set(Some(source));
        self.editor.doc().reload(Rope::from(&placeholder), true);
        self.editor.cursor().update(|cursor| {
            cursor.set_insert(Selection::region(0, placeholder.len()))
        });
        self.path.set(path);
        self.start.set(start);
        self.position.set(position);
        self.active.set(true);
        self.common.focus.set(Focus::Rename);
    }

    fn run_focus_command(&self, cmd: &FocusCommand) -> CommandExecuted {
        match cmd {
            FocusCommand::ModalClose => {
                self.cancel();
            }
            FocusCommand::ConfirmRename => {
                self.confirm(false);
            }
            _ => return CommandExecuted::No,
        }
        CommandExecuted::Yes
    }

    pub fn cancel(&self) {
        self.active.set(false);
        self.token.update(|token| *token += 1);
        self.pending.set(false);
        if self.common.focus.get_untracked() == Focus::Rename {
            self.common.focus.set(Focus::Workbench);
            self.common
                .window_common
                .app_view_id
                .get_untracked()
                .request_focus();
        }
    }

    pub fn confirm(&self, preview: bool) {
        if self.pending.get_untracked() {
            return;
        }
        let name = self
            .editor
            .doc()
            .buffer
            .with_untracked(|buffer| buffer.to_string());
        if let Some(error) = validate_name(&name) {
            self.error.set(error.into());
            return;
        }
        if name == self.original_name.get_untracked() {
            self.cancel();
            return;
        }
        self.error.set(String::new());
        self.pending.set(true);
        self.token.update(|token| *token += 1);
        self.common
            .internal_command
            .send(InternalCommand::RequestSymbolRename {
                token: self.token.get_untracked(),
                preview,
            });
    }
}

/// Reject obvious input mistakes; the analyzer remains authoritative for language rules and conflicts.
fn validate_name(name: &str) -> Option<&'static str> {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Some("Enter a new name.");
    };
    if !(first.is_alphabetic() || first == '_' || first == '$')
        || chars.any(|c| !(c.is_alphanumeric() || c == '_' || c == '$'))
    {
        return Some("Enter an identifier without spaces or punctuation.");
    }
    None
}

pub fn request(
    window: Rc<crate::window_tab::WindowTabData>,
    token: u64,
    preview: bool,
) {
    let rename = window.rename.clone();
    if !rename.active.get_untracked() || rename.token.get_untracked() != token {
        return;
    }
    let Some(source) = rename.source.get_untracked() else {
        return;
    };
    let doc = source.doc();
    let path = rename.path.get_untracked();
    let revision = rename.revision.get_untracked();
    if doc.rev() != revision
        || doc
            .content
            .with_untracked(|c| c.path() != Some(&path) || c.read_only())
    {
        rename.pending.set(false);
        rename
            .error
            .set("The source changed or is read-only. Reopen Rename.".into());
        return;
    }
    let snapshots = window.main_split.docs.with_untracked(|docs| {
        docs.iter()
            .map(|(p, doc)| (p.clone(), doc.rev()))
            .collect::<std::collections::HashMap<_, _>>()
    });
    let scope = window.scope.create_child();
    let current = rename.clone();
    let active_editor = window.main_split.active_editor;
    let watch_doc = doc.clone();
    let watch_source = source.clone();
    let watch_path = path.clone();
    scope.create_effect(move |_| {
        if !current.active.get() || current.token.get() != token {
            scope.dispose();
            return;
        }
        if active_editor
            .get()
            .is_none_or(|e| e.id() != watch_source.id())
            || watch_source
                .doc_signal()
                .get()
                .content
                .with(|c| c.path() != Some(&watch_path))
        {
            current.cancel();
            scope.dispose();
            return;
        }
        if watch_doc.buffer.with(|b| b.rev()) != revision {
            current.pending.set(false);
            current
                .error
                .set("The source changed. Reopen Rename before applying.".into());
            scope.dispose();
        }
    });
    let data = rename.clone();
    let position = rename.position.get_untracked();
    let name = rename.editor.doc().buffer.with_untracked(|b| b.to_string());
    let title = format!(
        "Rename {} to {}",
        rename.original_name.get_untracked(),
        name
    );
    let w = window.clone();
    let origin_path = path.clone();
    let send = create_ext_action(
        scope,
        move |result: Result<Option<lsp_types::WorkspaceEdit>, String>| {
            if !data.active.get_untracked() || data.token.get_untracked() != token {
                return;
            }
            let edit = match result {
                Ok(Some(edit)) => edit,
                other => {
                    data.pending.set(false);
                    data.error.set(other.err().unwrap_or_else(|| {
                        "The analyzer did not return any rename changes.".into()
                    }));
                    return;
                }
            };
            let files = match crate::code_action_preview::edit_files(&edit) {
                Ok(files) => files,
                Err(error) => {
                    data.pending.set(false);
                    data.error.set(error);
                    return;
                }
            };
            if files.is_empty() {
                data.pending.set(false);
                data.error
                    .set("No references were changed by this rename.".into());
                return;
            }
            if files.iter().any(|file| {
                snapshots.get(&file.path).is_some_and(|rev| {
                    w.main_split.docs.with_untracked(|docs| {
                        docs.get(&file.path).is_none_or(|doc| doc.rev() != *rev)
                    })
                })
            }) {
                data.pending.set(false);
                data.error
                    .set("An affected file changed. Reopen Rename.".into());
                return;
            }
            let action =
                lsp_types::CodeActionOrCommand::CodeAction(lsp_types::CodeAction {
                    title,
                    edit: Some(edit),
                    ..Default::default()
                });
            if preview {
                data.cancel();
                let mut actions = w.code_action.get_untracked();
                actions.show(
                    sparrow_rpc::plugin::PluginId(0),
                    std::iter::once(action).collect(),
                    data.start.get_untracked(),
                    false,
                );
                actions.origin = Some((origin_path, revision));
                w.code_action.set(actions);
                return;
            }
            let prepared =
                scope.create_rw_signal(crate::code_action_preview::Preview::Empty);
            crate::code_action_preview::prepare(
                w.clone(),
                scope,
                prepared,
                sparrow_rpc::plugin::PluginId(0),
                action,
                Some((origin_path, revision)),
            );
            scope.create_effect(move |_| match prepared.get() {
                crate::code_action_preview::Preview::Ready(edit) => {
                    // Applying changes updates buffer revisions, so dismiss before the edit.
                    data.cancel();
                    if let Err(error) = crate::code_action_preview::apply(&w, edit) {
                        w.common.internal_command.send(InternalCommand::ShowAlert {
                            title: "Rename".into(),
                            msg: error,
                            buttons: Vec::new(),
                        });
                    }
                }
                crate::code_action_preview::Preview::Blocked(error) => {
                    data.pending.set(false);
                    data.error.set(error);
                }
                _ => {}
            });
        },
    );
    if crate::dart::is_dart_path(&path) {
        if window.common.dart_lsp.is_available() {
            window.common.dart_lsp.rename_result(
                &path,
                position.line,
                position.character,
                &name,
                move |result| send(result),
            );
        } else {
            send(Err(
                window.common.dart_lsp.unavailable_reason(),
            ));
        }
    } else {
        window
            .common
            .proxy
            .rename(path, position, name, move |result| {
                send(match result {
                    Ok(ProxyResponse::Rename { edit }) => Ok(Some(edit)),
                    _ => Err("Could not rename this symbol.".into()),
                })
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_obvious_identifier_mistakes_without_changing_user_input() {
        for name in ["newName", "_private", "$value", "value2"] {
            assert_eq!(validate_name(name), None);
        }
        for name in ["", " name", "two names", "42value", "value;", "value\n"] {
            assert!(validate_name(name).is_some());
        }
    }
}
