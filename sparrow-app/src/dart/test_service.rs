//! A cancellable local test process. Worker snapshots preserve every JSON event
//! even when the UI coalesces notifications while rendering.
use super::test_runner::{TestRun, build_test_command};
use floem::{
    ext_event::create_signal_from_channel,
    reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith},
};
use std::{
    io::{BufRead, BufReader, Read},
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

struct CancelOnDrop {
    cancelled: Arc<AtomicBool>,
    debug: Option<(
        sparrow_rpc::proxy::ProxyRpcHandler,
        sparrow_rpc::dap_types::DapId,
    )>,
}
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        if let Some((proxy, id)) = &self.debug {
            proxy.dap_stop(*id);
        }
    }
}

#[derive(Clone)]
pub struct FlutterTestData {
    pub run: RwSignal<TestRun>,
    pub selected: RwSignal<Option<i64>>,
    pub debug_dap: RwSignal<Option<sparrow_rpc::dap_types::DapId>>,
    debug_stopping: RwSignal<bool>,
    proxy: sparrow_rpc::proxy::ProxyRpcHandler,
    pub last_targets: RwSignal<Vec<(PathBuf, String)>>,
    pub last_exact: RwSignal<bool>,
    cancel: RwSignal<CancelOnDrop>,
    sender: std::sync::mpsc::Sender<TestRun>,
}
impl FlutterTestData {
    pub fn new(cx: Scope, proxy: sparrow_rpc::proxy::ProxyRpcHandler) -> Self {
        let (sender, receiver) = std::sync::mpsc::channel();
        let incoming = create_signal_from_channel(receiver);
        let run = cx.create_rw_signal(TestRun::default());
        cx.create_effect(move |_| {
            incoming.with(|snapshot: &Option<TestRun>| {
                if let Some(snapshot) = snapshot {
                    run.set(snapshot.clone());
                }
            });
        });
        let cancel = cx.create_rw_signal(CancelOnDrop {
            cancelled: Arc::new(AtomicBool::new(false)),
            debug: None,
        });
        Self {
            run,
            proxy,
            debug_dap: cx.create_rw_signal(None),
            debug_stopping: cx.create_rw_signal(false),
            selected: cx.create_rw_signal(None),
            last_targets: cx.create_rw_signal(Vec::new()),
            last_exact: cx.create_rw_signal(false),
            cancel,
            sender,
        }
    }
    pub fn stop(&self) {
        if let Some(id) = self.debug_dap.get_untracked() {
            self.debug_stopping.set(true);
            self.proxy.dap_stop(id);
        } else {
            self.cancel
                .with_untracked(|c| c.cancelled.store(true, Ordering::Relaxed));
        }
    }
    pub fn begin_debug(
        &self,
        id: sparrow_rpc::dap_types::DapId,
        targets: Vec<(PathBuf, String)>,
    ) {
        self.cancel.set(CancelOnDrop {
            cancelled: Arc::new(AtomicBool::new(false)),
            debug: Some((self.proxy.clone(), id)),
        });
        self.debug_dap.set(Some(id));
        self.debug_stopping.set(false);
        self.selected.set(None);
        self.last_targets.set(targets);
        self.last_exact.set(false);
        self.run.set(TestRun {
            running: true,
            ..Default::default()
        });
    }
    pub fn debug_event(
        &self,
        id: sparrow_rpc::dap_types::DapId,
        event: &serde_json::Value,
    ) {
        if self.debug_dap.get_untracked() != Some(id) {
            return;
        }
        let kind = event["type"].as_str().unwrap_or_default();
        self.run.update(|r| match kind {
            "sparrow.launchError" => {
                r.console.push(
                    event["output"]
                        .as_str()
                        .unwrap_or("Debugger launch failed")
                        .into(),
                );
                r.finish(false, false);
            }
            "sparrow.output" => r.console.extend(
                event["output"]
                    .as_str()
                    .unwrap_or_default()
                    .lines()
                    .map(str::to_owned),
            ),
            "sparrow.exit" => r.finish(
                event["success"] == true,
                self.debug_stopping.get_untracked(),
            ),
            "sparrow.terminated" => r.finish(
                r.success == Some(true),
                self.debug_stopping.get_untracked(),
            ),
            _ => r.line(&event.to_string()),
        });
        if matches!(
            kind,
            "sparrow.launchError" | "sparrow.exit" | "sparrow.terminated"
        ) {
            self.debug_dap.set(None);
            self.cancel.update(|guard| guard.debug = None);
        }
    }
    pub fn start(&self, targets: Vec<(PathBuf, String)>, exact: bool) {
        if self.run.get_untracked().running || targets.is_empty() {
            return;
        }
        self.selected.set(None);
        self.last_targets.set(targets.clone());
        self.last_exact.set(exact);
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel.set(CancelOnDrop {
            cancelled: cancel.clone(),
            debug: None,
        });
        let sender = self.sender.clone();
        self.run.set(TestRun {
            running: true,
            ..Default::default()
        });
        thread::spawn(move || {
            let mut run = TestRun {
                running: true,
                ..Default::default()
            };
            let mut success = true;
            for (index, (file, name)) in targets.into_iter().enumerate() {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                // Reporter IDs are local to a process. Namespace each invocation.
                let namespace = (index as i64) << 32;
                let (program, args) = if exact {
                    super::test_runner::build_test_command_with_filter(
                        &file, &name, true,
                    )
                } else {
                    build_test_command(&file, &name)
                };
                run.console.push(format!("{program} {}", args.join(" ")));
                let mut command = Command::new(program);
                command
                    .args(args)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                if let Some(root) = super::test_runner::project_root(&file) {
                    command.current_dir(root);
                }
                #[cfg(unix)]
                {
                    use std::os::unix::process::CommandExt;
                    command.process_group(0);
                }
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    command.creation_flags(0x08000000);
                }
                let mut child = match command.spawn() {
                    Ok(child) => child,
                    Err(err) => {
                        run.console
                            .push(format!("Unable to start Flutter tests: {err}"));
                        success = false;
                        break;
                    }
                };
                let (tx, rx) = crossbeam_channel::unbounded();
                let stdout =
                    read_lines(child.stdout.take().unwrap(), tx.clone(), true);
                let stderr = read_lines(child.stderr.take().unwrap(), tx, false);
                let mut stopped = false;
                let mut last_publish = std::time::Instant::now();
                let mut dirty = true;
                let exit_ok = loop {
                    if cancel.load(Ordering::Relaxed) && !stopped {
                        stopped = true;
                        #[cfg(unix)]
                        {
                            // Flutter starts a Dart worker; stop the whole dedicated group.
                            let _ = Command::new("kill")
                                .args(["-TERM", "--", &format!("-{}", child.id())])
                                .status();
                        }
                        #[cfg(windows)]
                        {
                            let _ = Command::new("taskkill")
                                .args(["/PID", &child.id().to_string(), "/T", "/F"])
                                .status();
                        }
                        let _ = child.kill();
                    }
                    match rx.recv_timeout(Duration::from_millis(30)) {
                        Ok((line, json)) => {
                            apply_line(&mut run, &line, json, namespace);
                            dirty = true;
                        }
                        Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {}
                        Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                    }
                    while let Ok((line, json)) = rx.try_recv() {
                        apply_line(&mut run, &line, json, namespace);
                        dirty = true;
                    }
                    if dirty && last_publish.elapsed() >= Duration::from_millis(50) {
                        let _ = sender.send(run.clone());
                        dirty = false;
                        last_publish = std::time::Instant::now();
                    }
                    match child.try_wait() {
                        Ok(Some(status)) => break status.success(),
                        Ok(None) => {}
                        Err(err) => {
                            run.console.push(format!("Test process failed: {err}"));
                            let _ = child.kill();
                            let _ = child.wait();
                            break false;
                        }
                    }
                };
                let _ = stdout.join();
                let _ = stderr.join();
                for (line, json) in rx {
                    apply_line(&mut run, &line, json, namespace);
                }
                success &= exit_ok && run.success != Some(false);
            }
            let cancelled = cancel.load(Ordering::Relaxed);
            run.console.push(if cancelled {
                "Test run stopped".into()
            } else if success {
                "Test run finished".into()
            } else {
                "Test run failed (see output above)".into()
            });
            run.finish(success, cancelled);
            let _ = sender.send(run);
        });
    }
}
fn read_lines(
    reader: impl Read + Send + 'static,
    tx: crossbeam_channel::Sender<(String, bool)>,
    json: bool,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        for line in BufReader::new(reader).lines() {
            match line {
                Ok(line) => {
                    if tx.send((line, json)).is_err() {
                        break;
                    }
                }
                Err(err) => {
                    let _ = tx
                        .send((format!("Error reading test output: {err}"), false));
                    break;
                }
            }
        }
    })
}
fn apply_line(run: &mut TestRun, line: &str, json: bool, namespace: i64) {
    if !json {
        run.console.push(line.into());
        return;
    }
    if namespace == 0 {
        run.line(line);
        return;
    }
    let Ok(mut v) = serde_json::from_str::<serde_json::Value>(line) else {
        run.line(line);
        return;
    };
    fn shift(v: &mut serde_json::Value, key: &str, namespace: i64) {
        if let Some(id) = v[key].as_i64() {
            v[key] = (id + namespace).into();
        }
    }
    shift(&mut v, "testID", namespace);
    shift(&mut v, "suiteID", namespace);
    for field in ["test", "group", "suite"] {
        if let Some(item) = v.get_mut(field) {
            for key in ["id", "suiteID", "parentID"] {
                shift(item, key, namespace);
            }
            if let Some(ids) = item["groupIDs"].as_array_mut() {
                for id in ids {
                    if let Some(value) = id.as_i64() {
                        *id = (value + namespace).into();
                    }
                }
            }
        }
    }
    run.line(&v.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dart::test_runner::TestStatus;
    #[test]
    fn real_flutter_stream_and_multiple_process_ids() {
        let stream = include_str!("fixtures/flutter_test_events.jsonl");
        let mut run = TestRun::default();
        for namespace in [0, 1_i64 << 32] {
            for line in stream.lines() {
                apply_line(&mut run, line, true, namespace);
            }
        }
        assert_eq!(run.suites.len(), 2);
        assert_eq!(
            run.tests
                .values()
                .filter(|t| !t.hidden && t.status == TestStatus::Passed)
                .count(),
            6
        );
        assert_eq!(
            run.tests
                .values()
                .filter(|t| t.status == TestStatus::Failed)
                .count(),
            2
        );
        assert_eq!(
            run.tests
                .values()
                .filter(|t| t.status == TestStatus::Skipped)
                .count(),
            2
        );
        assert!(run.tests.values().all(|t| t.duration.is_some()));
        assert_eq!(
            run.failed_targets(),
            vec![(
                PathBuf::from("/project/test/widget_test.dart"),
                "outer inner fails".into()
            )]
        );
        assert_eq!(run.success, Some(false));
    }
}
