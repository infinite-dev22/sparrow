//! Native Dart SDK debugger sessions and replies for retired extension RPCs.
use super::{
    PluginCatalogNotification, PluginCatalogRpcHandler,
    dap::{DapClient, DapRpcHandler},
    rpc::{CloneableCallback, RpcCallback},
};
use lapce_xi_rope::{Rope, RopeDelta};
use lsp_types::{
    MessageType, SemanticTokens, ShowMessageParams, TextDocumentIdentifier,
    TextDocumentItem, VersionedTextDocumentIdentifier,
};
use sparrow_rpc::{
    RpcError,
    dap_types::{self, DapId, DapServer},
    plugin::{PluginId, VoltID, VoltInfo},
    style::LineStyle,
};
use std::{
    borrow::Cow,
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
};
pub struct PluginCatalog {
    workspace: Option<PathBuf>,
    plugin_rpc: PluginCatalogRpcHandler,
    daps: HashMap<DapId, DapRpcHandler>,
}
impl PluginCatalog {
    pub fn new(
        workspace: Option<PathBuf>,
        _disabled_volts: Vec<VoltID>,
        _extra_plugin_paths: Vec<PathBuf>,
        _configurations: HashMap<String, HashMap<String, serde_json::Value>>,
        plugin_rpc: PluginCatalogRpcHandler,
    ) -> Self {
        Self {
            workspace,
            plugin_rpc,
            daps: HashMap::new(),
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub fn handle_server_request(
        &mut self,
        plugin_id: Option<PluginId>,
        request_sent: Option<Arc<AtomicUsize>>,
        _method: Cow<'static, str>,
        _params: serde_json::Value,
        _language_id: Option<String>,
        _path: Option<PathBuf>,
        _check: bool,
        f: Box<dyn CloneableCallback<serde_json::Value, RpcError>>,
    ) {
        if let Some(sent) = request_sent {
            sent.fetch_add(1, Ordering::Relaxed);
        }
        f(
            plugin_id.unwrap_or(PluginId(0)),
            Err(RpcError {
                code: 0,
                message: "Language services are provided directly by the Dart SDK."
                    .into(),
            }),
        );
    }
    pub fn handle_server_notification(
        &mut self,
        _plugin_id: Option<PluginId>,
        _method: impl Into<Cow<'static, str>>,
        _params: serde_json::Value,
        _language_id: Option<String>,
        _path: Option<PathBuf>,
        _check: bool,
    ) {
    }
    pub fn shutdown_volt(
        &mut self,
        _volt: VoltInfo,
        f: Box<dyn CloneableCallback<serde_json::Value, RpcError>>,
    ) {
        f(PluginId(0), Ok(serde_json::Value::Null));
    }
    pub fn handle_did_open_text_document(&mut self, _document: TextDocumentItem) {}
    pub fn handle_did_save_text_document(
        &mut self,
        _language: String,
        _path: PathBuf,
        _document: TextDocumentIdentifier,
        _text: Rope,
    ) {
    }
    pub fn handle_did_change_text_document(
        &mut self,
        _language: String,
        _document: VersionedTextDocumentIdentifier,
        _delta: RopeDelta,
        _text: Rope,
        _new_text: Rope,
    ) {
    }
    pub fn format_semantic_tokens(
        &self,
        _plugin: PluginId,
        _tokens: SemanticTokens,
        _text: Rope,
        f: Box<dyn RpcCallback<Vec<LineStyle>, RpcError>>,
    ) {
        f.call(Err(RpcError {
            code: 0,
            message: "Dart semantic tokens are decoded by the IDE.".into(),
        }));
    }
    pub fn dap_variable(
        &self,
        dap_id: DapId,
        reference: usize,
        f: Box<dyn RpcCallback<Vec<dap_types::Variable>, RpcError>>,
    ) {
        if let Some(dap) = self.daps.get(&dap_id) {
            dap.variables_async(
                reference,
                |result: Result<dap_types::VariablesResponse, RpcError>| {
                    f.call(result.map(|resp| resp.variables))
                },
            );
        } else {
            f.call(Err(RpcError {
                code: 0,
                message: "plugin doesn't exist".to_string(),
            }));
        }
    }

    pub fn dap_get_scopes(
        &self,
        dap_id: DapId,
        frame_id: usize,
        f: Box<
            dyn RpcCallback<
                    Vec<(dap_types::Scope, Vec<dap_types::Variable>)>,
                    RpcError,
                >,
        >,
    ) {
        if let Some(dap) = self.daps.get(&dap_id) {
            let local_dap = dap.clone();
            dap.scopes_async(
                frame_id,
                move |result: Result<dap_types::ScopesResponse, RpcError>| {
                    match result {
                        Ok(resp) => {
                            let scopes = resp.scopes.clone();
                            if let Some(scope) = resp.scopes.first() {
                                let scope = scope.to_owned();
                                thread::spawn(move || {
                                    local_dap.variables_async(
                                        scope.variables_reference,
                                        move |result: Result<
                                            dap_types::VariablesResponse,
                                            RpcError,
                                        >| {
                                            let resp: Vec<(
                                                dap_types::Scope,
                                                Vec<dap_types::Variable>,
                                            )> = scopes
                                                .iter()
                                                .enumerate()
                                                .map(|(index, s)| {
                                                    (
                                                        s.clone(),
                                                        if index == 0 {
                                                            result
                                                                .as_ref()
                                                                .map(|resp| {
                                                                    resp.variables
                                                                        .clone()
                                                                })
                                                                .unwrap_or_default()
                                                        } else {
                                                            Vec::new()
                                                        },
                                                    )
                                                })
                                                .collect();
                                            f.call(Ok(resp));
                                        },
                                    );
                                });
                            } else {
                                f.call(Ok(Vec::new()));
                            }
                        }
                        Err(e) => {
                            f.call(Err(e));
                        }
                    }
                },
            );
        } else {
            f.call(Err(RpcError {
                code: 0,
                message: "plugin doesn't exist".to_string(),
            }));
        }
    }

    pub fn handle_notification(&mut self, notification: PluginCatalogNotification) {
        use PluginCatalogNotification::*;
        match notification {
            UpdatePluginConfigs(_) | ReloadVolt(_) | StopVolt(_) | EnableVolt(_) => {
            }
            InstallVolt(volt) => self.plugin_rpc.core_rpc.volt_installing(
                volt,
                "Executable extension installation is not supported.".into(),
            ),
            DapLoaded(dap_rpc) => {
                self.daps.insert(dap_rpc.dap_id, dap_rpc);
            }
            DapDisconnected(dap_id) => {
                self.daps.remove(&dap_id);
            }
            DapStart {
                config,
                breakpoints,
            } => {
                let workspace = self.workspace.clone();
                let plugin_rpc = self.plugin_rpc.clone();
                let adapter = config.dart_debug_adapter();
                if let Some(debugger) = adapter {
                    thread::spawn(move || {
                        match DapClient::start(
                            DapServer {
                                program: debugger.program,
                                args: debugger.args.unwrap_or_default(),
                                cwd: workspace,
                            },
                            config.clone(),
                            breakpoints,
                            plugin_rpc.clone(),
                        ) {
                            Ok(dap_rpc) => {
                                if let Err(err) =
                                    plugin_rpc.dap_loaded(dap_rpc.clone())
                                {
                                    tracing::error!("{:?}", err);
                                    if config.is_test() {
                                        plugin_rpc.core_rpc.flutter_test_event(config.dap_id,
                                            serde_json::json!({"type":"sparrow.launchError", "output":err.to_string()}));
                                    }
                                }

                                if let Err(err) = dap_rpc.launch(&config) {
                                    tracing::error!("{:?}", err);
                                    if config.is_test() {
                                        plugin_rpc.core_rpc.flutter_test_event(config.dap_id,
                                            serde_json::json!({"type":"sparrow.launchError", "output":err.to_string()}));
                                    }
                                }
                            }
                            Err(err) => {
                                tracing::error!("{:?}", err);
                                if config.is_test() {
                                    plugin_rpc.core_rpc.flutter_test_event(config.dap_id,
                                        serde_json::json!({"type":"sparrow.launchError", "output":err.to_string()}));
                                }
                            }
                        }
                    });
                } else {
                    self.plugin_rpc.core_rpc.show_message(
                        "debug fail".to_owned(),
                        ShowMessageParams {
                            typ: MessageType::ERROR,
                            message: "Debugger not found. Please install the appropriate plugin.".to_owned(),
                        },
                    )
                }
            }
            DapProcessId {
                dap_id,
                process_id,
                term_id,
            } => {
                if let Some(dap) = self.daps.get(&dap_id) {
                    if let Err(err) =
                        dap.termain_process_tx.send((term_id, process_id))
                    {
                        tracing::error!("{:?}", err);
                    }
                }
            }
            DapContinue { dap_id, thread_id } => {
                if let Some(dap) = self.daps.get(&dap_id).cloned() {
                    let plugin_rpc = self.plugin_rpc.clone();
                    thread::spawn(move || {
                        if dap.continue_thread(thread_id).is_ok() {
                            plugin_rpc.core_rpc.dap_continued(dap_id);
                        }
                    });
                }
            }
            DapPause { dap_id, thread_id } => {
                if let Some(dap) = self.daps.get(&dap_id).cloned() {
                    thread::spawn(move || {
                        if let Err(err) = dap.pause_thread(thread_id) {
                            tracing::error!("{:?}", err);
                        }
                    });
                }
            }
            DapStepOver { dap_id, thread_id } => {
                if let Some(dap) = self.daps.get(&dap_id).cloned() {
                    dap.next(thread_id);
                }
            }
            DapStepInto { dap_id, thread_id } => {
                if let Some(dap) = self.daps.get(&dap_id).cloned() {
                    dap.step_in(thread_id);
                }
            }
            DapStepOut { dap_id, thread_id } => {
                if let Some(dap) = self.daps.get(&dap_id).cloned() {
                    dap.step_out(thread_id);
                }
            }
            DapAdvanced { dap_id, action } => {
                if let Some(dap) = self.daps.get(&dap_id) { dap.advanced(action); }
            }
            DapHotReload { dap_id, restart } => {
                if let Some(dap) = self.daps.get(&dap_id).cloned() {
                    thread::spawn(move || dap.hot_reload(restart));
                }
            }
            DapStop { dap_id } => {
                if let Some(dap) = self.daps.get(&dap_id) {
                    dap.stop();
                }
            }
            DapDisconnect { dap_id } => {
                if let Some(dap) = self.daps.get(&dap_id).cloned() {
                    thread::spawn(move || {
                        if let Err(err) = dap.disconnect() {
                            tracing::error!("{:?}", err);
                        }
                    });
                }
            }
            DapRestart {
                dap_id,
                breakpoints,
            } => {
                if let Some(dap) = self.daps.get(&dap_id) {
                    dap.restart(breakpoints);
                }
            }
            DapSetBreakpoints {
                dap_id,
                path,
                breakpoints,
            } => {
                if let Some(dap) = self.daps.get(&dap_id) {
                    dap.update_breakpoints(path, breakpoints);
                }
            }
            Shutdown => {
                for dap in self.daps.values() {
                    let _ = dap.disconnect();
                }
            }
        }
    }
}
