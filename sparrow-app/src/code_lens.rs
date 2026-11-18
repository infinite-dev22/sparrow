use std::rc::Rc;

use serde_json::Value;

use crate::{command::InternalCommand, window_tab::CommonData};

#[derive(Clone)]
pub struct CodeLensData {
    common: Rc<CommonData>,
}

impl CodeLensData {
    pub fn new(common: Rc<CommonData>) -> Self {
        Self { common }
    }

    pub fn run(&self, command: &str, args: Vec<Value>) {
        match command {
            "dart.edit.codeAction.apply" => {
                // Dart sends a WorkspaceEdit as the first argument. This is
                // what the analyzer returns from `textDocument/codeAction`
                // when the fix is a "structured" one (unused import,
                // missing override, etc.). Apply it atomically; the edit
                // engine already handles multi-file changes.
                match args.first() {
                    Some(value) => {
                        match serde_json::from_value::<lsp_types::WorkspaceEdit>(
                            value.clone(),
                        ) {
                            Ok(edit) => {
                                self.common.internal_command.send(
                                    InternalCommand::ApplyWorkspaceEdit { edit },
                                );
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "dart.edit.codeAction.apply: failed to parse WorkspaceEdit: {e}"
                                );
                            }
                        }
                    }
                    None => {
                        tracing::warn!("dart.edit.codeAction.apply: no arguments");
                    }
                }
            }
            _ => {
                tracing::debug!("unhandled code action command: {command}");
            }
        }
    }
}
