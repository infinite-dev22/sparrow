use std::{
    collections::HashMap,
    io::{BufReader, BufWriter, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
};

use anyhow::{Result, anyhow};
use crossbeam_channel::{Receiver, Sender};
use parking_lot::Mutex;
use serde_json::Value;
use sparrow_rpc::{
    RpcError,
    dap_types::{
        self, ConfigurationDone, Continue, ContinueArguments, ContinueResponse,
        DapEvent, DapId, DapPayload, DapRequest, DapResponse, DapServer,
        DebuggerCapabilities, Disconnect, Initialize, Launch, Next, NextArguments,
        Pause, PauseArguments, Request, RunDebugConfig, RunInTerminal,
        RunInTerminalArguments, RunInTerminalResponse, Scope, Scopes,
        ScopesArguments, ScopesResponse, SetBreakpoints, SetBreakpointsArguments,
        SetBreakpointsResponse, Source, SourceBreakpoint, StackTrace,
        StackTraceArguments, StackTraceResponse, StepIn, StepInArguments, StepOut,
        StepOutArguments, Terminate, ThreadId, Threads, ThreadsResponse, Variable,
        Variables, VariablesArguments, VariablesResponse,
    },
    terminal::TermId,
};

use super::{
    PluginCatalogRpcHandler,
    rpc::{ResponseHandler, RpcCallback},
};

pub struct DapClient {
    plugin_rpc: PluginCatalogRpcHandler,
    pub(crate) dap_rpc: DapRpcHandler,
    dap_server: DapServer,
    config: RunDebugConfig,
    breakpoints: HashMap<PathBuf, Vec<SourceBreakpoint>>,
    term_id: Option<TermId>,
    capabilities: Option<DebuggerCapabilities>,
    terminated: bool,
    disconnected: bool,
    restarted: bool,
    forced_step: Option<(bool, bool)>,
    debug_options: (bool, bool),
}

impl DapClient {
    pub fn new(
        dap_server: DapServer,
        config: RunDebugConfig,
        breakpoints: HashMap<PathBuf, Vec<SourceBreakpoint>>,
        plugin_rpc: PluginCatalogRpcHandler,
    ) -> Result<Self> {
        let dap_rpc = DapRpcHandler::new(config.dap_id);

        Ok(Self {
            plugin_rpc,
            dap_server,
            config: config.clone(),
            dap_rpc,
            breakpoints,
            term_id: None,
            capabilities: None,
            terminated: false,
            disconnected: false,
            restarted: false,
            forced_step: None,
            debug_options: { let settings = debugger_settings(&config); (settings.debug_sdk, settings.debug_external) },
        })
    }

    fn update_options(&mut self, options: (bool, bool)) -> Result<()> {
        self.dap_rpc.request_raw("updateDebugOptions", serde_json::json!({
            "debugSdkLibraries":options.0, "debugExternalPackageLibraries":options.1 }))?;
        self.debug_options = options;
        Ok(())
    }
    fn restore_forced_step(&mut self) -> Result<()> {
        if let Some(options) = self.forced_step {
            let mut errors = Vec::new();
            for (path, points) in &self.breakpoints {
                match self.dap_rpc.set_breakpoints(path.clone(), points.clone()) {
                    Ok(response) => self.plugin_rpc.core_rpc.dap_breakpoints_resp(self.config.dap_id, path.clone(), response.breakpoints.unwrap_or_default()),
                    Err(error) => errors.push(error.to_string()),
                }
            }
            if let Err(error) = self.update_options(options) { errors.push(error.to_string()); }
            if !errors.is_empty() { return Err(anyhow!("Unable to restore debugger state: {}", errors.join("; "))); }
            self.forced_step = None;
        }
        Ok(())
    }
    fn advanced_action(&mut self, action: dap_types::DebugAction) -> Result<Value> {
        use dap_types::DebugAction;
        match action {
            DebugAction::Evaluate { expression, frame_id, request_id: _, watch } => {
                self.dap_rpc.request_raw("evaluate", serde_json::json!({
                    "expression": expression, "frameId": frame_id,
                    "context": if watch { "watch" } else { "repl" }
                }))
            }
            DebugAction::UpdateOptions { debug_sdk, debug_external } => {
                let options = (debug_sdk, debug_external);
                if let Some(restore) = &mut self.forced_step { *restore = options; }
                else { self.update_options(options)?; }
                Ok(Value::Null)
            }
            DebugAction::StepTargets { frame_id, generation } => {
                if !self.capabilities.as_ref().is_some_and(|c| c.supports_step_in_targets_request == Some(true)) {
                    return Ok(serde_json::json!({"targets":[], "supported":false, "frameId":frame_id, "generation":generation}));
                }
                let mut result = self.dap_rpc.request_raw("stepInTargets", serde_json::json!({"frameId":frame_id}))?;
                result["frameId"] = serde_json::json!(frame_id);
                result["generation"] = serde_json::json!(generation);
                Ok(result)
            }
            DebugAction::StepIntoTarget { thread_id, target_id } => self.dap_rpc.request_raw("stepIn",
                serde_json::json!({"threadId":thread_id,"targetId":target_id})),
            DebugAction::ForceStep { thread_id, into } => {
                if self.forced_step.is_some() { return Err(anyhow!("A forced step is already in progress")); }
                self.forced_step = Some(self.debug_options);
                let result = (|| {
                    for path in self.breakpoints.keys() { self.dap_rpc.set_breakpoints(path.clone(), Vec::new())?; }
                    if into { self.update_options((true, true))?; }
                    self.dap_rpc.request_raw(if into { "stepIn" } else { "next" }, serde_json::json!({"threadId":thread_id}))
                })();
                if result.is_err() { self.restore_forced_step()?; }
                result
            }
        }
    }

    pub fn start(
        dap_server: DapServer,
        config: RunDebugConfig,
        breakpoints: HashMap<PathBuf, Vec<SourceBreakpoint>>,
        plugin_rpc: PluginCatalogRpcHandler,
    ) -> Result<DapRpcHandler> {
        let mut dap = Self::new(dap_server, config, breakpoints, plugin_rpc)?;
        dap.start_process()?;

        let dap_rpc = dap.dap_rpc.clone();
        dap.initialize()?;

        {
            let dap_rpc = dap_rpc.clone();
            thread::spawn(move || {
                dap_rpc.mainloop(&mut dap);
            });
        }

        Ok(dap_rpc)
    }

    fn start_process(&self) -> Result<()> {
        let program = self.dap_server.program.clone();
        let mut process = Self::process(
            &program,
            &self.dap_server.args,
            self.dap_server.cwd.as_ref(),
        )?;
        let stdin = process.stdin.take().unwrap();
        let stdout = process.stdout.take().unwrap();
        // let stderr = process.stderr.take().unwrap();

        let dap_rpc = self.dap_rpc.clone();
        let io_rx = self.dap_rpc.io_rx.clone();
        let io_tx = self.dap_rpc.io_tx.clone();
        let mut writer = Box::new(BufWriter::new(stdin));
        thread::spawn(move || -> Result<()> {
            for msg in io_rx {
                if let Ok(msg) = serde_json::to_string(&msg) {
                    tracing::debug!("write to dap server: {}", msg);
                    let msg =
                        format!("Content-Length: {}\r\n\r\n{}", msg.len(), msg);
                    writer.write_all(msg.as_bytes())?;
                    writer.flush()?;
                }
            }
            tracing::debug!("thread(write to dap) exited");
            Ok(())
        });

        {
            let plugin_rpc = self.plugin_rpc.clone();
            thread::spawn(move || {
                let mut reader = Box::new(BufReader::new(stdout));
                loop {
                    match crate::plugin::rpc::read_message(&mut reader) {
                        Ok(message_str) => {
                            tracing::debug!("read from dap server: {}", message_str);
                            dap_rpc.handle_server_message(&message_str);
                        }
                        Err(_err) => {
                            if let Err(err) = io_tx
                                .send(DapPayload::Event(DapEvent::Initialized(None)))
                            {
                                tracing::error!("{:?}", err);
                            }
                            plugin_rpc.core_rpc.log(
                                sparrow_rpc::core::LogLevel::Error,
                                format!("dap server {program} stopped!"),
                                None,
                            );

                            dap_rpc.disconnected();
                            tracing::debug!("thread(read from dap) exited");
                            return;
                        }
                    };
                }
            });
        }

        Ok(())
    }

    fn process(
        server: &str,
        args: &[String],
        cwd: Option<&PathBuf>,
    ) -> Result<Child> {
        let mut process = Command::new(server);
        if let Some(cwd) = cwd {
            process.current_dir(cwd);
        }

        process.args(args);

        // CREATE_NO_WINDOW
        // (https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags)
        // TODO: We set this because
        #[cfg(target_os = "windows")]
        std::os::windows::process::CommandExt::creation_flags(
            &mut process,
            0x08000000,
        );
        let child = process
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        Ok(child)
    }

    fn handle_host_request(&mut self, req: &DapRequest) -> Result<Value> {
        match req.command.as_str() {
            RunInTerminal::COMMAND => {
                let value = req
                    .arguments
                    .as_ref()
                    .ok_or_else(|| anyhow!("no arguments"))?;
                let args: RunInTerminalArguments =
                    serde_json::from_value(value.clone())?;
                let mut config = self.config.clone();
                config.debug_command = Some(args.args);
                self.plugin_rpc.core_rpc.run_in_terminal(config);
                let (term_id, process_id) =
                    self.dap_rpc.termain_process_rx.recv()?;
                self.term_id = Some(term_id);
                let resp = RunInTerminalResponse {
                    process_id,
                    shell_process_id: None,
                };
                let resp = serde_json::to_value(resp)?;
                Ok(resp)
            }
            _ => Err(anyhow!("not implemented")),
        }
    }

    fn handle_host_event(&mut self, event: &DapEvent) -> Result<()> {
        match event {
            DapEvent::Initialized(_) => {
                for (path, breakpoints) in self.breakpoints.clone().into_iter() {
                    match self.dap_rpc.set_breakpoints(path.clone(), breakpoints) {
                        Ok(breakpoints) => {
                            self.plugin_rpc.core_rpc.dap_breakpoints_resp(
                                self.config.dap_id,
                                path,
                                breakpoints.breakpoints.unwrap_or_default(),
                            );
                        }
                        Err(err) => {
                            tracing::error!("{:?}", err);
                        }
                    }
                }
                // send dap configurations here
                self.dap_rpc.request_async::<ConfigurationDone>((), |rs| {
                    if let Err(e) = rs {
                        tracing::error!("request ConfigurationDone: {:?}", e)
                    }
                });
            }
            DapEvent::Stopped(stopped) => {
                if let Err(error) = self.restore_forced_step() {
                    self.plugin_rpc.core_rpc.dap_session_event(self.config.dap_id,
                        serde_json::json!({"type":"sparrow.debugAction", "error":error.to_string()}));
                }
                // The SDK emits a transient entry pause while wiring the VM,
                // then resumes automatically. No stop-on-entry option is exposed.
                if stopped.reason == "entry" { return Ok(()); }
                let all_threads_stopped =
                    stopped.all_threads_stopped.unwrap_or_default();
                let mut stack_frames = HashMap::new();
                if all_threads_stopped {
                    if let Ok(response) = self.dap_rpc.threads() {
                        for thread in response.threads {
                            if let Ok(frames) = self.dap_rpc.stack_trace(thread.id) {
                                stack_frames.insert(thread.id, frames.stack_frames);
                            }
                        }
                    }
                }

                // Dart commonly pauses one isolate, with allThreadsStopped=false.
                // Its thread still needs a stack/scopes request.
                if let Some(thread) = stopped.thread_id {
                    if !stack_frames.contains_key(&thread) {
                        if let Ok(frames) = self.dap_rpc.stack_trace(thread) {
                            stack_frames.insert(thread, frames.stack_frames);
                        }
                    }
                }

                let current_thread = if all_threads_stopped {
                    Some(stopped.thread_id.unwrap_or_default())
                } else {
                    stopped.thread_id
                };

                let active_frame = current_thread
                    .and_then(|thread_id| stack_frames.get(&thread_id))
                    .and_then(|stack_frames| stack_frames.first());

                let mut vars = Vec::new();
                if let Some(frame) = active_frame {
                    if let Ok(scopes) = self.dap_rpc.scopes(frame.id) {
                        for scope in scopes {
                            let result =
                                self.dap_rpc.variables(scope.variables_reference);
                            vars.push((scope, result.unwrap_or_default()));
                        }
                    }
                }

                self.plugin_rpc.core_rpc.dap_stopped(
                    self.config.dap_id,
                    stopped.clone(),
                    stack_frames,
                    vars,
                );

                // if all_threads_stopped {
                //     if let Ok(response) = self.dap_rpc.threads() {
                //         for thread in response.threads {
                //             self.fetch_stack_trace(thread.id);
                //         }
                //         self.select_thread_id(
                //             stopped.thread_id.unwrap_or_default(),
                //             false,
                //         );
                //     }
                // } else if let Some(thread_id) = stopped.thread_id {
                //     self.select_thread_id(thread_id, false);
                // }
            }
            DapEvent::Continued(_) => {
                self.plugin_rpc.core_rpc.dap_continued(self.dap_rpc.dap_id);
            }
            DapEvent::Exited(exited) => {
                self.plugin_rpc.core_rpc.dap_session_event(self.config.dap_id, serde_json::json!({"type":"exited"}));
                if self.config.is_test() {
                    self.plugin_rpc.core_rpc.flutter_test_event(self.config.dap_id,
                        serde_json::json!({"type": "sparrow.exit", "success": exited.exit_code == 0}));
                }
            }
            DapEvent::Terminated(_) => {
                self.plugin_rpc.core_rpc.dap_session_event(self.config.dap_id, serde_json::json!({"type":"terminated"}));
                self.terminated = true;
                if self.config.is_test() {
                    self.plugin_rpc.core_rpc.flutter_test_event(
                        self.config.dap_id,
                        serde_json::json!({"type": "sparrow.terminated"}),
                    );
                }
                // self.plugin_rpc.core_rpc.dap_terminated(self.dap_rpc.dap_id);
                if let Some(term_id) = self.term_id {
                    self.plugin_rpc.proxy_rpc.terminal_close(term_id);
                }
                if let Err(err) = self.check_restart() {
                    tracing::error!("{:?}", err);
                }
            }
            DapEvent::Thread { .. } => {}
            DapEvent::DartTestNotification(event) => {
                self.plugin_rpc
                    .core_rpc
                    .flutter_test_event(self.config.dap_id, event.clone());
            }
            DapEvent::Output(output) => {
                self.plugin_rpc.core_rpc.dap_session_event(self.config.dap_id, serde_json::json!({"type":"output", "output":output.output}));
                if self.config.is_test() {
                    self.plugin_rpc.core_rpc.flutter_test_event(self.config.dap_id,
                        serde_json::json!({"type": "sparrow.output", "output": output.output}));
                }
            }
            DapEvent::Breakpoint { .. } => {}
            DapEvent::Module { .. } => {}
            DapEvent::LoadedSource { .. } => {}
            DapEvent::Process(_) => {}
            DapEvent::Capabilities(change) => {
                let mut merged = self.capabilities.as_ref().map(serde_json::to_value).transpose()?.unwrap_or_else(||serde_json::json!({}));
                let delta = serde_json::to_value(&change.capabilities)?;
                if let Some(current) = merged.as_object_mut() { current.extend(delta.as_object().cloned().unwrap_or_default()); }
                self.capabilities = Some(serde_json::from_value(merged)?);
            }
            DapEvent::Memory(_) => {}
        }
        Ok(())
    }

    pub(crate) fn initialize(&mut self) -> Result<()> {
        let params = dap_types::InitializeParams {
            client_id: Some("sparrow".to_owned()),
            client_name: Some("sparrow".to_owned()),
            adapter_id: "".to_string(),
            locale: Some("en-us".to_owned()),
            lines_start_at_one: Some(true),
            columns_start_at_one: Some(true),
            path_format: Some("path".to_owned()),
            supports_variable_type: Some(true),
            supports_variable_paging: Some(false),
            supports_run_in_terminal_request: Some(true),
            supports_memory_references: Some(false),
            supports_progress_reporting: Some(true),
            supports_invalidated_event: Some(false),
        };

        let resp = self
            .dap_rpc
            .request::<Initialize>(params)
            .map_err(|e| anyhow!(e.message))?;
        self.plugin_rpc.core_rpc.dap_session_event(self.config.dap_id,
            serde_json::json!({"type":"capabilities", "body":resp}));
        self.capabilities = Some(resp);

        Ok(())
    }

    fn stop(&self) {
        let dap_rpc = self.dap_rpc.clone();
        if self
            .capabilities
            .as_ref()
            .and_then(|c| c.supports_terminate_request)
            .unwrap_or(false)
        {
            thread::spawn(move || {
                if let Err(err) = dap_rpc.terminate() {
                    tracing::error!("{:?}", err);
                }
            });
        } else {
            thread::spawn(move || {
                if let Err(err) = dap_rpc.disconnect() {
                    tracing::error!("{:?}", err);
                }
            });
        }
    }

    // check if the DAP was restared when we received terminated or disconnected
    // if the DAP doesn't supports terminate request, then we also need to wait for
    // disconnected
    fn check_restart(&mut self) -> Result<()> {
        if !self.restarted {
            return Ok(());
        }
        if !self
            .capabilities
            .as_ref()
            .and_then(|c| c.supports_terminate_request)
            .unwrap_or(false)
            && !self.disconnected
        {
            return Ok(());
        }

        self.restarted = false;

        if self.disconnected {
            self.start_process()?;
            self.initialize()?;
        }
        self.terminated = false;
        self.disconnected = false;

        let dap_rpc = self.dap_rpc.clone();
        let config = self.config.clone();
        thread::spawn(move || {
            if let Err(err) = dap_rpc.launch(&config) {
                tracing::error!("{:?}", err);
            }
        });

        Ok(())
    }

    fn restart(&mut self, breakpoints: HashMap<PathBuf, Vec<SourceBreakpoint>>) {
        self.restarted = true;
        self.breakpoints = breakpoints;
        if !self.terminated {
            self.stop();
        } else if let Err(err) = self.check_restart() {
            tracing::error!("{:?}", err);
        }
    }
}

#[allow(clippy::large_enum_variant)]
pub enum DapRpc {
    HostRequest(DapRequest),
    HostEvent(DapEvent),
    UiEvent(Value),
    Advanced(dap_types::DebugAction),
    UpdateBreakpoints(PathBuf, Vec<SourceBreakpoint>),
    Stop,
    Restart(HashMap<PathBuf, Vec<SourceBreakpoint>>),
    Shutdown,
    Disconnected,
}

#[derive(Clone)]
pub struct DapRpcHandler {
    pub dap_id: DapId,
    rpc_tx: Sender<DapRpc>,
    rpc_rx: Receiver<DapRpc>,
    io_tx: Sender<DapPayload>,
    io_rx: Receiver<DapPayload>,
    pub(crate) termain_process_tx: Sender<(TermId, Option<u32>)>,
    termain_process_rx: Receiver<(TermId, Option<u32>)>,
    seq_counter: Arc<AtomicU64>,
    server_pending: Arc<Mutex<HashMap<u64, ResponseHandler<DapResponse, RpcError>>>>,
}

impl DapRpcHandler {
    fn new(dap_id: DapId) -> Self {
        let (rpc_tx, rpc_rx) = crossbeam_channel::unbounded();
        let (io_tx, io_rx) = crossbeam_channel::unbounded();
        let (termain_process_tx, termain_process_rx) =
            crossbeam_channel::unbounded();
        Self {
            dap_id,
            io_tx,
            io_rx,
            rpc_rx,
            rpc_tx,
            termain_process_tx,
            termain_process_rx,
            seq_counter: Arc::new(AtomicU64::new(0)),
            server_pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn mainloop(&self, dap_client: &mut DapClient) {
        for msg in &self.rpc_rx {
            match msg {
                DapRpc::HostRequest(req) => {
                    let result = dap_client.handle_host_request(&req);
                    let seq = self.seq_counter.fetch_add(1, Ordering::Relaxed);
                    let resp = DapResponse {
                        seq,
                        request_seq: req.seq,
                        success: result.is_ok(),
                        command: req.command.clone(),
                        message: result.as_ref().err().map(|e| e.to_string()),
                        body: result.ok(),
                    };
                    if let Err(err) = self.io_tx.send(DapPayload::Response(resp)) {
                        tracing::error!("{:?}", err);
                    }
                }
                DapRpc::Advanced(action) => {
                    let evaluation_id = match &action {
                        dap_types::DebugAction::Evaluate { request_id, .. } => Some(*request_id),
                        _ => None,
                    };
                    let target_request = matches!(action, dap_types::DebugAction::StepTargets { .. });
                    let result = dap_client.advanced_action(action);
                    dap_client.plugin_rpc.core_rpc.dap_session_event(self.dap_id,
                        serde_json::json!({"type": if evaluation_id.is_some() { "sparrow.evaluation" } else if target_request { "sparrow.stepTargets" } else { "sparrow.debugAction" },
                            "requestId": evaluation_id, "body": result.as_ref().ok(), "error": result.err().map(|e|e.to_string())}));
                }
                DapRpc::UpdateBreakpoints(path, breakpoints) => {
                    dap_client.breakpoints.insert(path.clone(), breakpoints.clone());
                    if dap_client.forced_step.is_none() {
                        match dap_client.dap_rpc.set_breakpoints(path.clone(), breakpoints) {
                            Ok(response) => dap_client.plugin_rpc.core_rpc.dap_breakpoints_resp(self.dap_id, path, response.breakpoints.unwrap_or_default()),
                            Err(error) => tracing::error!("{error}"),
                        }
                    }
                }
                DapRpc::UiEvent(event) => { dap_client.plugin_rpc.core_rpc.dap_session_event(self.dap_id, event); }
                DapRpc::HostEvent(event) => {
                    if let Err(err) = dap_client.handle_host_event(&event) {
                        tracing::error!("{:?}", err);
                    }
                }
                DapRpc::Stop => {
                    dap_client.stop();
                }
                DapRpc::Restart(breakpoints) => {
                    dap_client.restart(breakpoints);
                }
                DapRpc::Shutdown => {
                    if let Some(term_id) = dap_client.term_id {
                        dap_client.plugin_rpc.proxy_rpc.terminal_close(term_id);
                    }
                    return;
                }
                DapRpc::Disconnected => {
                    dap_client.plugin_rpc.core_rpc.dap_session_event(self.dap_id, serde_json::json!({"type":"terminated"}));
                    dap_client.disconnected = true;
                    if dap_client.config.is_test() {
                        dap_client.plugin_rpc.core_rpc.flutter_test_event(
                            dap_client.config.dap_id,
                            serde_json::json!({"type": "sparrow.terminated"}),
                        );
                    }
                    if let Some(term_id) = dap_client.term_id {
                        dap_client.plugin_rpc.proxy_rpc.terminal_close(term_id);
                    }
                    if let Err(err) = dap_client.check_restart() {
                        tracing::error!("{:?}", err);
                    }
                }
            }
        }
    }

    fn request_async<R: Request>(
        &self,
        params: R::Arguments,
        f: impl RpcCallback<R::Result, RpcError> + 'static,
    ) {
        self.request_common::<R>(
            R::COMMAND,
            params,
            ResponseHandler::Callback(Box::new(
                |result: Result<DapResponse, RpcError>| {
                    let result = match result {
                        Ok(resp) => {
                            if resp.success {
                                serde_json::from_value(resp.body.into()).map_err(
                                    |e| RpcError {
                                        code: 0,
                                        message: e.to_string(),
                                    },
                                )
                            } else {
                                Err(RpcError {
                                    code: 0,
                                    message: resp.message.unwrap_or_default(),
                                })
                            }
                        }
                        Err(e) => Err(e),
                    };
                    Box::new(f).call(result);
                },
            )),
        );
    }

    fn request<R: Request>(
        &self,
        params: R::Arguments,
    ) -> Result<R::Result, RpcError> {
        let (tx, rx) = crossbeam_channel::bounded(1);
        self.request_common::<R>(R::COMMAND, params, ResponseHandler::Chan(tx));
        let resp = rx
            .recv_timeout(std::time::Duration::from_secs(30))
            .map_err(|_| RpcError {
                code: 0,
                message: "io error".to_string(),
            })??;
        if resp.success {
            let resp: R::Result =
                serde_json::from_value(resp.body.into()).map_err(|e| RpcError {
                    code: 0,
                    message: e.to_string(),
                })?;
            Ok(resp)
        } else {
            Err(RpcError {
                code: 0,
                message: resp.message.unwrap_or_default(),
            })
        }
    }

    fn request_common<R: Request>(
        &self,
        command: &'static str,
        arguments: R::Arguments,
        rh: ResponseHandler<DapResponse, RpcError>,
    ) {
        let seq = self.seq_counter.fetch_add(1, Ordering::Relaxed);
        let arguments: Value = serde_json::to_value(arguments).unwrap();

        {
            let mut pending = self.server_pending.lock();
            pending.insert(seq, rh);
        }
        if let Err(err) = self.io_tx.send(DapPayload::Request(DapRequest {
            seq,
            command: command.to_string(),
            arguments: Some(arguments),
        })) {
            tracing::error!("{:?}", err);
        }
    }

    fn handle_server_response(&self, resp: DapResponse) {
        if let Some(rh) = { self.server_pending.lock().remove(&resp.request_seq) } {
            rh.invoke(Ok(resp));
        }
    }

    pub fn handle_server_message(&self, message_str: &str) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(message_str) {
            if value["type"] == "event" {
                if let Some(kind) = value["event"].as_str() {
                    if matches!(kind, "progressStart" | "progressUpdate" | "progressEnd" | "dart.progressStart" | "dart.progressUpdate" | "dart.progressEnd" | "flutter.appStarted" | "dart.debuggerUris") {
                        let _ = self.rpc_tx.send(DapRpc::UiEvent(serde_json::json!({"type":kind,"body":value["body"]})));
                        return;
                    }
                }
            }
        }
        if let Ok(payload) = serde_json::from_str::<DapPayload>(message_str) {
            match payload {
                DapPayload::Request(req) => {
                    if let Err(err) = self.rpc_tx.send(DapRpc::HostRequest(req)) {
                        tracing::error!("{:?}", err);
                    }
                }
                DapPayload::Event(event) => {
                    if let Err(err) = self.rpc_tx.send(DapRpc::HostEvent(event)) {
                        tracing::error!("{:?}", err);
                    }
                }
                DapPayload::Response(resp) => {
                    self.handle_server_response(resp);
                }
            }
        }
    }

    pub fn launch(&self, config: &RunDebugConfig) -> Result<()> {
        let params = dart_launch_arguments(config)?;
        let attaching=config.args.as_ref().and_then(|args|args.first()).is_some_and(|a|a=="attach");
        if attaching{self.request::<dap_types::Attach>(params).map_err(|e|anyhow!(e.message))?;}else{self.request::<Launch>(params).map_err(|e|anyhow!(e.message))?;}
        Ok(())
    }

    pub fn advanced(&self, action: dap_types::DebugAction) {
        let _ = self.rpc_tx.send(DapRpc::Advanced(action));
    }
    pub fn update_breakpoints(&self, path: PathBuf, breakpoints: Vec<SourceBreakpoint>) {
        let _ = self.rpc_tx.send(DapRpc::UpdateBreakpoints(path, breakpoints));
    }
    fn request_raw(&self, command: &'static str, args: Value) -> Result<Value> {
        let (tx, rx) = crossbeam_channel::bounded(1);
        self.request_common::<RawDebugRequest>(command, args, ResponseHandler::Chan(tx));
        let response = rx.recv_timeout(std::time::Duration::from_secs(10))
            .map_err(|_| anyhow!("Debugger request timed out: {command}"))?
            .map_err(|error| anyhow!(error.message))?;
        if !response.success { return Err(anyhow!(response.message.unwrap_or_else(|| format!("{command} failed")))); }
        Ok(response.body.unwrap_or(Value::Null))
    }

    pub fn hot_reload(&self, restart: bool) {
        let args = serde_json::json!({"reason":"manual"});
        let result = if restart {
            self.request::<dap_types::FlutterHotRestart>(args)
        } else {
            self.request::<dap_types::FlutterHotReload>(args)
        };
        if let Err(err) = result { tracing::error!("Flutter reload failed: {}", err.message); }
    }

    pub fn stop(&self) {
        if let Err(err) = self.rpc_tx.send(DapRpc::Stop) {
            tracing::error!("{:?}", err);
        }
    }

    pub fn restart(&self, breakpoints: HashMap<PathBuf, Vec<SourceBreakpoint>>) {
        if let Err(err) = self.rpc_tx.send(DapRpc::Restart(breakpoints)) {
            tracing::error!("{:?}", err);
        }
    }

    fn disconnected(&self) {
        if let Err(err) = self.rpc_tx.send(DapRpc::Disconnected) {
            tracing::error!("{:?}", err);
        }
    }

    pub fn disconnect(&self) -> Result<()> {
        self.request::<Disconnect>(())
            .map_err(|e| anyhow!(e.message))?;
        Ok(())
    }

    fn terminate(&self) -> Result<()> {
        self.request::<Terminate>(())
            .map_err(|e| anyhow!(e.message))?;
        Ok(())
    }

    pub fn set_breakpoints_async(
        &self,
        file: PathBuf,
        breakpoints: Vec<SourceBreakpoint>,
        f: impl RpcCallback<SetBreakpointsResponse, RpcError> + 'static,
    ) {
        let params = SetBreakpointsArguments {
            source: Source {
                path: Some(file),
                name: None,
                source_reference: None,
                presentation_hint: None,
                origin: None,
                sources: None,
                adapter_data: None,
                checksums: None,
            },
            breakpoints: Some(breakpoints),
            source_modified: Some(false),
        };
        self.request_async::<SetBreakpoints>(params, f);
    }

    pub fn set_breakpoints(
        &self,
        file: PathBuf,
        breakpoints: Vec<SourceBreakpoint>,
    ) -> Result<SetBreakpointsResponse> {
        let params = SetBreakpointsArguments {
            source: Source {
                path: Some(file),
                name: None,
                source_reference: None,
                presentation_hint: None,
                origin: None,
                sources: None,
                adapter_data: None,
                checksums: None,
            },
            breakpoints: Some(breakpoints),
            source_modified: Some(false),
        };
        let resp = self
            .request::<SetBreakpoints>(params)
            .map_err(|e| anyhow!(e.message))?;
        Ok(resp)
    }

    pub fn continue_thread(&self, thread_id: ThreadId) -> Result<ContinueResponse> {
        let params = ContinueArguments { thread_id };
        let resp = self
            .request::<Continue>(params)
            .map_err(|e| anyhow!(e.message))?;
        Ok(resp)
    }

    pub fn pause_thread(&self, thread_id: ThreadId) -> Result<()> {
        let thread_ids = if thread_id == ThreadId::default() {
            self.threads()?.threads.into_iter().map(|thread| thread.id).collect::<Vec<_>>()
        } else { vec![thread_id] };
        if thread_ids.is_empty() { return Err(anyhow!("No running Dart isolates to pause")); }
        for thread_id in thread_ids {
            self.request::<Pause>(PauseArguments { thread_id })
                .map_err(|error| anyhow!(error.message))?;
        }
        Ok(())
    }

    pub fn threads(&self) -> Result<ThreadsResponse> {
        let resp = self
            .request::<Threads>(())
            .map_err(|e| anyhow!(e.message))?;
        Ok(resp)
    }

    pub fn stack_trace(&self, thread_id: ThreadId) -> Result<StackTraceResponse> {
        let params = StackTraceArguments {
            thread_id,
            ..Default::default()
        };
        let resp = self
            .request::<StackTrace>(params)
            .map_err(|e| anyhow!(e.message))?;
        Ok(resp)
    }

    pub fn scopes(&self, frame_id: usize) -> Result<Vec<Scope>> {
        let args = ScopesArguments { frame_id };

        let response = self
            .request::<Scopes>(args)
            .map_err(|e| anyhow!(e.message))?;
        Ok(response.scopes)
    }

    pub fn scopes_async(
        &self,
        frame_id: usize,
        f: impl RpcCallback<ScopesResponse, RpcError> + 'static,
    ) {
        let args = ScopesArguments { frame_id };

        self.request_async::<Scopes>(args, f);
    }

    pub fn variables(&self, variables_reference: usize) -> Result<Vec<Variable>> {
        let args = VariablesArguments {
            variables_reference,
            filter: None,
            start: None,
            count: None,
            format: None,
        };

        let response = self
            .request::<Variables>(args)
            .map_err(|e| anyhow!(e.message))?;
        Ok(response.variables)
    }

    pub fn variables_async(
        &self,
        variables_reference: usize,
        f: impl RpcCallback<VariablesResponse, RpcError> + 'static,
    ) {
        let args = VariablesArguments {
            variables_reference,
            filter: None,
            start: None,
            count: None,
            format: None,
        };

        self.request_async::<Variables>(args, f);
    }

    pub fn next(&self, thread_id: ThreadId) {
        let args = NextArguments {
            thread_id,
            granularity: None,
        };

        self.request_async::<Next>(args, move |_| {});
    }

    pub fn step_in(&self, thread_id: ThreadId) {
        let args = StepInArguments {
            thread_id,
            target_id: None,
            granularity: None,
        };

        self.request_async::<StepIn>(args, move |_| {});
    }

    pub fn step_out(&self, thread_id: ThreadId) {
        let args = StepOutArguments {
            thread_id,
            granularity: None,
        };

        self.request_async::<StepOut>(args, move |_| {});
    }
}

fn debugger_settings(config: &RunDebugConfig) -> dap_types::DebuggerSettings {
    config.cwd.as_deref().and_then(|cwd| std::path::Path::new(cwd).ancestors()
        .find_map(|root| std::fs::read_to_string(root.join(".sparrow/debugger.toml")).ok()))
        .and_then(|text| toml::from_str(&text).ok()).unwrap_or_default()
}

/// Translate SDK CLI configurations into the Dart DAP launch schema. The
/// debug adapter launches an entrypoint, never the SDK executable as a script.
fn dart_launch_arguments(config: &RunDebugConfig) -> Result<Value> {
    if !config.is_dart_flutter() {
        return Err(anyhow!("Only Dart and Flutter debug targets are supported"));
    }
    let mut program = config.program.clone();
    let mut args = config.args.clone().unwrap_or_default();
    let mut tool_args = Vec::new();
    let settings = debugger_settings(config);
    if config.is_test() && std::path::Path::new(&program).file_stem().is_some_and(|p|p=="flutter"||p=="dart") {
        if args.first().is_some_and(|a|a=="test"){args.remove(0);}program=if args.first().is_some_and(|a|!a.starts_with('-')){args.remove(0)}else{"test".into()};
    } else if !config.is_test() {
        let flutter = std::path::Path::new(&program)
            .file_stem()
            .is_some_and(|s| s == "flutter");
        if !flutter&&args.first().is_some_and(|s|s=="attach") {let uri=args.get(1).ok_or_else(||anyhow!("Set a VM service URI for remote debugging"))?;return Ok(serde_json::json!({"vmServiceUri":uri,"cwd":config.cwd,"env":config.env.clone().unwrap_or_default(),
            "debugSdkLibraries":settings.debug_sdk,"debugExternalPackageLibraries":settings.debug_external,"evaluateToStringInDebugViews":settings.value_renderers}));}
        if args.first().is_some_and(|s| s == "run" || s=="attach") {
            args.remove(0);
        }
        if flutter {
            program = "lib/main.dart".into();
            let mut iter = args.into_iter();
            while let Some(arg) = iter.next() {
                if arg == "-t" || arg == "--target" {
                    program = iter
                        .next()
                        .ok_or_else(|| anyhow!("Flutter target is missing"))?;
                } else if let Some(target) = arg.strip_prefix("--target=") {
                    program = target.to_string();
                } else {
                    tool_args.push(arg);
                }
            }
            args = Vec::new();
        } else {
            while args.first().is_some_and(|s| s.starts_with('-')) {
                tool_args.push(args.remove(0));
            }
            if args.is_empty() {
                return Err(anyhow!("Choose a Dart entrypoint for debugging"));
            }
            program = args.remove(0);
        }
    }
    Ok(serde_json::json!({"console":"debugConsole","program":program,"args":args,"toolArgs":tool_args,"cwd":config.cwd,"runInTerminal":false,"env":config.env.clone().unwrap_or_default(),
        "debugSdkLibraries":settings.debug_sdk, "debugExternalPackageLibraries":settings.debug_external,
        "evaluateToStringInDebugViews":settings.value_renderers }))
}
#[cfg(test)]
mod scope_tests {
    use super::*;
    #[test]
    fn native_flutter_launch_uses_entrypoint_not_tool_executable() {
        let c:RunDebugConfig=serde_json::from_value(serde_json::json!({"type":"flutter","name":"app","program":"flutter","args":["run","--target","example/lib/main.dart","--flavor","dev"]})).unwrap();
        let p = dart_launch_arguments(&c).unwrap();
        assert_eq!(p["program"], "example/lib/main.dart");
        assert_eq!(p["toolArgs"], serde_json::json!(["--flavor", "dev"]));
    }
    #[test]
    fn native_sdk_launch_receives_an_environment_map() {
        let mut config: RunDebugConfig = serde_json::from_value(serde_json::json!({"name":"main","program":"dart","args":["run","main.dart"]})).unwrap();
        assert_eq!(dart_launch_arguments(&config).unwrap()["env"], serde_json::json!({}));
        config.env = Some(std::collections::HashMap::from([("APP_MODE".into(), "preview".into())]));
        assert_eq!(dart_launch_arguments(&config).unwrap()["env"], serde_json::json!({"APP_MODE":"preview"}));
    }
    #[test]
    fn native_dart_launch_keeps_script_arguments() {
        let c:RunDebugConfig=serde_json::from_value(serde_json::json!({"name":"script","program":"dart","args":["run","tool/main.dart","hello world"]})).unwrap();
        let p = dart_launch_arguments(&c).unwrap();
        assert_eq!(p["program"], "tool/main.dart");
        assert_eq!(p["args"], serde_json::json!(["hello world"]));
    }
}

#[cfg(test)]mod preset_argument_tests {use super::*;
 #[test]fn sdk_test_configurations_launch_test_paths_instead_of_the_sdk(){for (kind,program)in[("flutter-test","flutter"),("dart-test","dart")]{let config:RunDebugConfig=serde_json::from_value(serde_json::json!({"type":kind,"name":"Tests","program":program,"args":["test","test","--plain-name","counter"]})).unwrap();let args=dart_launch_arguments(&config).unwrap();assert_eq!(args["program"],"test");assert_eq!(args["args"],serde_json::json!(["--plain-name","counter"]));assert_eq!(args["runInTerminal"],false);}}
 #[test]fn attach_arguments_strip_the_cli_command(){let dart:RunDebugConfig=serde_json::from_value(serde_json::json!({"type":"dart","name":"Attach","program":"dart","args":["attach","http://127.0.0.1:8181/abc/"]})).unwrap();assert_eq!(dart_launch_arguments(&dart).unwrap()["vmServiceUri"],"http://127.0.0.1:8181/abc/");let flutter:RunDebugConfig=serde_json::from_value(serde_json::json!({"type":"flutter","name":"Attach","program":"flutter","args":["attach","--target","lib/main.dart","-d","linux"]})).unwrap();let args=dart_launch_arguments(&flutter).unwrap();assert_eq!(args["program"],"lib/main.dart");assert_eq!(args["toolArgs"],serde_json::json!(["-d","linux"]));}
}

#[cfg(test)]
mod progress_forwarding_tests {
    use super::*;
    #[test]
    fn standard_and_dart_progress_and_readiness_reach_ui() {
        let handler = DapRpcHandler::new(DapId::default());
        for event in ["progressStart", "progressUpdate", "progressEnd", "dart.progressStart", "dart.progressUpdate", "dart.progressEnd", "flutter.appStarted", "dart.debuggerUris"] {
            let body = serde_json::json!({"progressId":"compile","percentage":42});
            handler.handle_server_message(&serde_json::json!({"type":"event","event":event,"body":body}).to_string());
            assert!(matches!(handler.rpc_rx.try_recv().unwrap(), DapRpc::UiEvent(value) if value["type"] == event && value["body"] == body));
        }
    }
}

struct RawDebugRequest;
impl Request for RawDebugRequest {
    type Arguments = Value;
    type Result = Value;
    const COMMAND: &'static str = "";
}

#[cfg(test)]
mod advanced_tests {
    use super::*;
    fn client() -> DapClient {
        DapClient::new(DapServer { program:"unused".into(), args:Vec::new(), cwd:None },
            serde_json::from_value(serde_json::json!({"name":"test","program":"dart"})).unwrap(),
            HashMap::from([(PathBuf::from("main.dart"), vec![SourceBreakpoint { line:5, column:None,
                condition:None, hit_condition:None, log_message:None }])]),
            PluginCatalogRpcHandler::new(sparrow_rpc::core::CoreRpcHandler::new(), sparrow_rpc::proxy::ProxyRpcHandler::new())).unwrap()
    }
    fn peer(client: &DapClient, count: usize, reject_step: bool) -> std::thread::JoinHandle<Vec<DapRequest>> {
        let rpc = client.dap_rpc.clone();
        std::thread::spawn(move || {
            let mut requests = Vec::new();
            for _ in 0..count {
                let DapPayload::Request(request) = rpc.io_rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap() else { panic!("expected request") };
                let success = !(reject_step && matches!(request.command.as_str(), "next"|"stepIn"));
                rpc.handle_server_response(DapResponse { seq:0, request_seq:request.seq, success,
                    command:request.command.clone(), message:Some("test rejection".into()),
                    body:Some(if request.command=="setBreakpoints" {serde_json::json!({"breakpoints":[]})}
                        else if request.command=="stepInTargets" {serde_json::json!({"targets":[{"id":7,"label":"work()"}]})}
                        else {serde_json::json!({})}) });
                requests.push(request);
            }
            requests
        })
    }
    #[test]
    fn evaluation_uses_selected_frame_and_standard_context() {
        for watch in [false, true] {
            let mut client = client();
            let peer = peer(&client, 1, false);
            client.advanced_action(dap_types::DebugAction::Evaluate {
                expression: "count + 1".into(), frame_id: 42, request_id: 9, watch,
            }).unwrap();
            let requests = peer.join().unwrap();
            assert_eq!(requests[0].command,"evaluate");
            let args = requests[0].arguments.as_ref().unwrap();
            assert_eq!(args["expression"],"count + 1");
            assert_eq!(args["frameId"],42);
            assert_eq!(args["context"],if watch {"watch"} else {"repl"});
        }
    }
    #[test]
    fn force_step_restores_breakpoints_and_filters_on_stop() {
        for into in [false,true] {
            let mut client = client();
            let peer = peer(&client, if into {5}else{4}, false);
            client.advanced_action(dap_types::DebugAction::ForceStep { thread_id:ThreadId::default(), into }).unwrap();
            assert!(client.forced_step.is_some());
            client.restore_forced_step().unwrap();
            assert!(client.forced_step.is_none());
            let requests = peer.join().unwrap();
            assert_eq!(requests[0].arguments.as_ref().unwrap()["breakpoints"],serde_json::json!([]));
            let restored = &requests[requests.len()-2];
            assert_eq!(restored.arguments.as_ref().unwrap()["breakpoints"][0]["line"],5);
            assert_eq!(requests.last().unwrap().arguments.as_ref().unwrap()["debugSdkLibraries"],false);
        }
    }
    #[test]
    fn failed_step_also_restores_and_reports_error() {
        let mut client = client();
        let peer = peer(&client,4,true);
        assert!(client.advanced_action(dap_types::DebugAction::ForceStep { thread_id:ThreadId::default(), into:false }).is_err());
        assert!(client.forced_step.is_none());
        assert_eq!(peer.join().unwrap()[2].arguments.as_ref().unwrap()["breakpoints"][0]["line"],5);
    }
    #[test]
    fn smart_targets_obey_capabilities_and_keep_frame_identity() {
        let mut client = client();
        let action = dap_types::DebugAction::StepTargets { frame_id:42, generation:3 };
        assert_eq!(client.advanced_action(action.clone()).unwrap()["supported"],false);
        client.capabilities = Some(serde_json::from_value(serde_json::json!({"supportsTerminateRequest":true})).unwrap());
        client.handle_host_event(&DapEvent::Capabilities(serde_json::from_value(serde_json::json!({"capabilities":{"supportsStepInTargetsRequest":true}})).unwrap())).unwrap();
        assert_eq!(client.capabilities.as_ref().unwrap().supports_terminate_request,Some(true));
        let peer = peer(&client,1,false);
        let response = client.advanced_action(action).unwrap();
        assert_eq!(response["targets"][0]["id"],7);
        assert_eq!(response["frameId"],42);
        assert_eq!(response["generation"],3);
        assert_eq!(peer.join().unwrap()[0].command,"stepInTargets");
    }
}
