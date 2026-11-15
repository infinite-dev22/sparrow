//! SDK DevTools server ownership and browser launch integration.
use floem::reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith};
use std::{
    io::{BufRead, BufReader},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DevToolsPage {
    Home,
    Memory,
    Network,
}
impl DevToolsPage {
    fn name(self) -> Option<&'static str> {
        match self {
            Self::Home => None,
            Self::Memory => Some("memory"),
            Self::Network => Some("network"),
        }
    }
}
struct Server {
    child: Child,
    base: url::Url,
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
struct Owner {
    server: Arc<Mutex<Option<Server>>>,
    cancelled: Arc<AtomicBool>,
}
impl Drop for Owner {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        self.server.lock().unwrap().take();
    }
}
#[derive(Clone)]
pub struct DevToolsData {
    pub busy: RwSignal<bool>,
    pub message: RwSignal<String>,
    owner: RwSignal<Owner>,
    scope: Scope,
}
impl DevToolsData {
    pub fn new(scope: Scope) -> Self {
        Self {
            scope,
            busy: scope.create_rw_signal(false),
            message: scope
                .create_rw_signal("DevTools opens in your browser.".into()),
            owner: scope.create_rw_signal(Owner {
                server: Arc::new(Mutex::new(None)),
                cancelled: Arc::new(AtomicBool::new(false)),
            }),
        }
    }
    pub fn open(
        &self,
        root: Option<PathBuf>,
        vm: Option<String>,
        page: DevToolsPage,
    ) {
        if self.busy.get_untracked() {
            return;
        }
        if page != DevToolsPage::Home && vm.is_none() {
            self.message.set("Run a Flutter app in debug or profile mode before opening a profiler.".into());
            return;
        }
        self.busy.set(true);
        self.message.set("Starting DevTools…".into());
        let (server, cancelled) = self
            .owner
            .with_untracked(|o| (o.server.clone(), o.cancelled.clone()));
        let busy = self.busy;
        let message = self.message;
        let send = floem::ext_event::create_ext_action(
            self.scope,
            move |result: Result<(), String>| {
                busy.set(false);
                message.set(match result { Ok(()) => "DevTools opened in your browser. Reopen to connect to the current app.".into(), Err(e) => e });
            },
        );
        std::thread::spawn(move || {
            let result = (|| {
                // Validate the connection before launching a server or a browser.
                connection_url(
                    url::Url::parse("http://127.0.0.1/").unwrap(),
                    vm.as_deref(),
                    page,
                )?;
                let existing = {
                    let mut guard = server.lock().map_err(|e| e.to_string())?;
                    if guard
                        .as_mut()
                        .is_some_and(|s| !matches!(s.child.try_wait(), Ok(None)))
                    {
                        guard.take();
                    }
                    guard.as_ref().map(|s| s.base.clone())
                };
                let base = if let Some(base) = existing {
                    base
                } else {
                    let dart = super::sdk::find_dart_binary(root.as_deref()).ok_or(
                        "Dart SDK not found. Configure a Flutter SDK first.",
                    )?;
                    let started = start_server(dart, &cancelled)?;
                    let mut guard = server.lock().map_err(|e| e.to_string())?;
                    if cancelled.load(Ordering::Relaxed) {
                        return Err("DevTools launch cancelled.".into());
                    }
                    let base = started.base.clone();
                    *guard = Some(started);
                    base
                };
                if cancelled.load(Ordering::Relaxed) {
                    return Err("DevTools launch cancelled.".into());
                }
                let target = connection_url(base, vm.as_deref(), page)?;
                open::that(target.as_str()).map_err(|e| format!("Unable to open the browser: {e}. Check your default browser and try again."))?;
                Ok(())
            })();
            send(result);
        });
    }
}
fn connection_url(
    mut base: url::Url,
    vm: Option<&str>,
    page: DevToolsPage,
) -> Result<url::Url, String> {
    if let Some(vm) = vm {
        let uri = url::Url::parse(vm).map_err(|_| "Invalid VM Service URI.")?;
        if !matches!(uri.scheme(), "ws" | "wss" | "http" | "https")
            || uri.host_str().is_none()
        {
            return Err("Invalid VM Service URI.".into());
        }
        base.query_pairs_mut().append_pair("uri", vm);
    }
    if let Some(page) = page.name() {
        base.query_pairs_mut().append_pair("page", page);
    }
    Ok(base)
}
fn server_url(line: &str) -> Option<url::Url> {
    let event: serde_json::Value = serde_json::from_str(line).ok()?;
    if event["event"] != "server.started" {
        return None;
    }
    let port = u16::try_from(event["params"]["port"].as_u64()?).ok()?;
    if port == 0 {
        return None;
    }
    url::Url::parse(&format!("http://127.0.0.1:{port}/")).ok()
}
fn start_server(dart: PathBuf, cancelled: &AtomicBool) -> Result<Server, String> {
    let mut command = Command::new(dart);
    command
        .args([
            "devtools",
            "--machine",
            "--no-launch-browser",
            "--host=127.0.0.1",
            "--port=0",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let child = command
        .spawn()
        .map_err(|e| format!("Unable to launch SDK DevTools: {e}"))?;
    let mut server = Server {
        child,
        base: url::Url::parse("http://127.0.0.1/").unwrap(),
    };
    let (tx, rx) = std::sync::mpsc::channel();
    for reader in [
        server
            .child
            .stdout
            .take()
            .map(|r| Box::new(r) as Box<dyn std::io::Read + Send>),
        server
            .child
            .stderr
            .take()
            .map(|r| Box::new(r) as Box<dyn std::io::Read + Send>),
    ]
    .into_iter()
    .flatten()
    {
        let tx = tx.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(reader).lines() {
                let Ok(line) = line else {
                    break;
                };
                let _ = tx.send(line);
            }
        });
    }
    drop(tx);
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut diagnostic = String::new();
    while Instant::now() < deadline && !cancelled.load(Ordering::Relaxed) {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(line) => {
                if let Some(url) = server_url(&line) {
                    server.base = url;
                    return Ok(server);
                }
                if line.len() < 2000 {
                    diagnostic = line;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            Err(_) => {}
        }
        if matches!(server.child.try_wait(), Ok(Some(_))) {
            break;
        }
    }
    Err(format!(
        "DevTools did not start. Verify that your SDK supports `dart devtools`. {diagnostic}"
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "starts the installed SDK server; requires SDK cache access"]
    fn installed_sdk_serves_devtools() {
        let dart = std::env::var_os("SPARROW_DEVTOOLS_TEST_DART")
            .map(PathBuf::from)
            .or_else(|| super::super::sdk::find_dart_binary(None))
            .expect("Install an SDK or set SPARROW_DEVTOOLS_TEST_DART");
        let server =
            start_server(dart, &AtomicBool::new(false)).expect("start SDK server");
        let response = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap()
            .get(server.base.clone())
            .send()
            .expect("HTTP response");
        assert!(response.status().is_success());
        let body = response.text().unwrap();
        assert!(body.contains("flutter_bootstrap.js"), "body: {body}");
        // Dropping Server kills and reaps the process.
    }
    #[test]
    fn decodes_only_valid_startup_events() {
        assert_eq!(server_url(r#"{"event":"server.started","params":{"host":"127.0.0.1","port":9101}}"#).unwrap().as_str(), "http://127.0.0.1:9101/");
        for line in [
            "text",
            r#"{"event":"server.dtdStarted","params":{"port":123}}"#,
            r#"{"event":"server.started","params":{"port":0}}"#,
            r#"{"event":"server.started","params":{"port":99999}}"#,
        ] {
            assert!(server_url(line).is_none());
        }
    }
    #[test]
    fn encodes_vm_auth_tokens_and_profiler_pages() {
        let vm = "ws://127.0.0.1:1234/a+b=/ws?x=1&y=2";
        let url = connection_url(
            url::Url::parse("http://127.0.0.1:9100/").unwrap(),
            Some(vm),
            DevToolsPage::Memory,
        )
        .unwrap();
        assert_eq!(
            url.query_pairs().collect::<Vec<_>>(),
            [("uri".into(), vm.into()), ("page".into(), "memory".into())]
        );
        assert!(
            connection_url(url.clone(), Some("file:///tmp/a"), DevToolsPage::Home)
                .is_err()
        );
        assert_eq!(
            connection_url(
                url::Url::parse("http://localhost/").unwrap(),
                None,
                DevToolsPage::Network
            )
            .unwrap()
            .query(),
            Some("page=network")
        );
    }
}
