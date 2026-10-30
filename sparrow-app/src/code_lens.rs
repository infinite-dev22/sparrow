use std::rc::Rc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sparrow_rpc::dap_types::{ConfigSource, RunDebugConfig};

use crate::{command::InternalCommand, debug::RunDebugMode, window_tab::CommonData};

#[derive(Serialize, Deserialize)]
struct CargoArgs {
    #[serde(rename = "cargoArgs")]
    pub cargo_args: Vec<String>,

    #[serde(rename = "cargoExtraArgs")]
    pub cargo_extra_args: Vec<String>,

    #[serde(rename = "executableArgs")]
    pub executable_args: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct RustArgs {
    pub args: CargoArgs,
    pub kind: String,
    pub label: String,
    pub location: lsp_types::LocationLink,
}

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
            "rust-analyzer.runSingle" | "rust-analyzer.debugSingle" => {
                let mode = if command == "rust-analyzer.runSingle" {
                    RunDebugMode::Run
                } else {
                    RunDebugMode::Debug
                };
                if let Some(config) = self.get_rust_command_config(&args, mode) {
                    self.common
                        .internal_command
                        .send(InternalCommand::RunAndDebug { mode, config });
                }
            }
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
                        tracing::warn!(
                        "dart.edit.codeAction.apply: no arguments"
                    );
                    }
                }
            }
            _ => {
                tracing::debug!("unhandled code action command: {command}");
            }
        }
    }

    fn get_rust_command_config(
        &self,
        args: &[Value],
        mode: RunDebugMode,
    ) -> Option<RunDebugConfig> {
        if let Some(args) = args.first() {
            let Ok(mut cargo_args) =
                serde_json::from_value::<RustArgs>(args.clone())
            else {
                tracing::error!("serde error");
                return None;
            };
            cargo_args
                .args
                .cargo_args
                .extend(cargo_args.args.cargo_extra_args);
            if !cargo_args.args.executable_args.is_empty() {
                cargo_args.args.cargo_args.push("--".to_string());
                cargo_args
                    .args
                    .cargo_args
                    .extend(cargo_args.args.executable_args);
            }
            Some(RunDebugConfig {
                ty: None,
                name: cargo_args.label,
                program: cargo_args.kind,
                args: Some(cargo_args.args.cargo_args),
                cwd: None,
                env: None,
                prelaunch: None,
                debug_command: None,
                dap_id: Default::default(),
                tracing_output: mode == RunDebugMode::Debug,
                config_source: ConfigSource::CodeLens,
            })
        } else {
            tracing::error!("no args");
            None
        }
    }
}
