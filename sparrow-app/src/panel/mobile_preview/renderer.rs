//! A windowless rendering process. Sparrow paints and owns the interactive surface.
use std::{
    process::{Child, Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

use base64::Engine;
use crossbeam_channel::{Receiver, Sender};
use serde_json::{Value, json};
use tungstenite::{Message, WebSocket, stream::MaybeTlsStream};

use super::device::Viewport;

pub(super) enum Input {
    Open(String),
    Configure(Viewport),
    Visible(bool),
    Touch(&'static str, f64, f64),
    Wheel(f64, f64, f64, f64),
    Text(String),
    Key(String, u32),
    Back,
}

#[derive(Clone)]
pub(super) enum Update {
    Frame(floem::peniko::Image),
    Failed(String),
    Home,
}

struct Process {
    child: Child,
    _profile: tempfile::TempDir,
}
impl Drop for Process {
    fn drop(&mut self) {
        super::server::stop(&mut self.child);
    }
}

type Socket = WebSocket<MaybeTlsStream<std::net::TcpStream>>;

struct Browser {
    _process: Process,
    client: reqwest::blocking::Client,
    base: String,
}

struct Tab {
    target: String,
    cdp: Cdp,
}

impl Tab {
    fn connect(value: &Value) -> Result<Self, String> {
        let endpoint = value["webSocketDebuggerUrl"]
            .as_str()
            .ok_or("The preview page did not expose a connection.")?;
        let target = value["id"]
            .as_str()
            .ok_or("The preview page has no ID.")?
            .to_owned();
        let (mut socket, _) =
            tungstenite::connect(endpoint).map_err(|error| error.to_string())?;
        if let MaybeTlsStream::Plain(stream) = socket.get_mut() {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        }
        Ok(Self {
            target,
            cdp: Cdp { socket, id: 0 },
        })
    }
}

impl Browser {
    fn create_tab(&self) -> Result<Tab, String> {
        let page = self
            .client
            .put(format!("{}/json/new?about:blank", &self.base))
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .and_then(|response| response.json::<Value>())
            .map_err(|error| error.to_string())?;
        Tab::connect(&page)
    }

    fn close_tab(&self, tab: Tab) {
        let _ = self
            .client
            .get(format!("{}/json/close/{}", &self.base, tab.target))
            .send();
    }
}

fn open() -> Result<(Browser, Tab), String> {
    let browser = [
        "chromium",
        "chromium-browser",
        "google-chrome",
        "google-chrome-stable",
        "chrome",
        "msedge",
    ]
    .iter()
    .find_map(|name| crate::dart::sdk::find_in_path(name))
    .or_else(|| {
        [
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
        ]
        .iter()
        .map(std::path::PathBuf::from)
        .find(|p| p.is_file())
    })
    .ok_or(
        "Install Chrome, Chromium or Edge to use the headless preview renderer.",
    )?;
    let profile = tempfile::tempdir().map_err(|e| e.to_string())?;
    let mut command = Command::new(browser);
    let child = super::server::group(&mut command)
        .arg(format!("--user-data-dir={}", profile.path().display()))
        .args([
            "--remote-debugging-address=127.0.0.1",
            "--remote-debugging-port=0",
            "--no-first-run",
            "--no-default-browser-check",
        ])
        .args([
            "--headless",
            "--disable-background-timer-throttling",
            "about:blank",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut preview = Process {
        child,
        _profile: profile,
    };
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|e| e.to_string())?;
    let start = Instant::now();

    while start.elapsed() < Duration::from_secs(12) {
        if let Ok(port) = std::fs::read_to_string(
            preview._profile.path().join("DevToolsActivePort"),
        ) {
            if let Some(port) =
                port.lines().next().and_then(|p| p.parse::<u16>().ok())
            {
                let base = format!("http://127.0.0.1:{port}");
                if let Ok(response) = client.get(format!("{base}/json/list")).send()
                    && let Ok(tabs) = response.json::<Vec<Value>>()
                    && let Some(page) = tabs.iter().find(|tab| tab["type"] == "page")
                {
                    let tab = Tab::connect(page)?;
                    return Ok((
                        Browser {
                            _process: preview,
                            client,
                            base,
                        },
                        tab,
                    ));
                }
            }
        }
        if preview
            .child
            .try_wait()
            .map_err(|e| e.to_string())?
            .is_some()
        {
            return Err("The preview renderer exited before it was ready.".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err("The preview renderer did not expose a viewport connection.".into())
}

pub(super) fn start(
    url: String,
    viewport: Viewport,
) -> (Sender<Input>, std::sync::mpsc::Receiver<Update>) {
    let (tx, rx) = crossbeam_channel::unbounded();
    let (updates, output) = std::sync::mpsc::sync_channel(2);
    std::thread::spawn(move || {
        if let Err(error) = render(rx, &updates, url, viewport) {
            let _ = updates.send(Update::Failed(error));
        }
    });
    (tx, output)
}

fn render(
    rx: Receiver<Input>,
    updates: &std::sync::mpsc::SyncSender<Update>,
    url: String,
    mut viewport: Viewport,
) -> Result<(), String> {
    let (browser, mut active) = open()?;
    let mut active_url = url;
    let mut background: Vec<(String, Tab)> = Vec::new();
    active.cdp.configure(viewport)?;
    active.cdp.navigate(&active_url)?;
    active.cdp.call("Page.bringToFront", json!({}))?;
    let mut visible = true;
    let mut next = Instant::now();
    let mut previous = Vec::new();
    loop {
        let timeout = if visible {
            next.saturating_duration_since(Instant::now())
        } else {
            Duration::from_secs(1)
        };
        match rx.recv_timeout(timeout) {
            Ok(Input::Open(url)) if url != active_url => {
                let mut tab = if let Some(index) =
                    background.iter().position(|(previous, _)| previous == &url)
                {
                    let (_, mut tab) = background.remove(index);
                    tab.cdp.configure(viewport)?;
                    tab
                } else {
                    let mut tab = browser.create_tab()?;
                    tab.cdp.configure(viewport)?;
                    tab.cdp.navigate(&url)?;
                    tab
                };
                tab.cdp.call("Page.bringToFront", json!({}))?;
                background.push((
                    std::mem::replace(&mut active_url, url),
                    std::mem::replace(&mut active, tab),
                ));
                if background.len() >= 12 {
                    let (_, tab) = background.remove(0);
                    browser.close_tab(tab);
                }
                previous.clear();
            }
            Ok(Input::Open(_)) => {}
            Ok(Input::Configure(new)) => {
                viewport = new;
                active.cdp.configure(viewport)?;
            }
            Ok(Input::Visible(show)) => visible = show,
            Ok(Input::Touch(kind, x, y)) => {
                let points = if matches!(kind, "touchEnd" | "touchCancel") {
                    json!([])
                } else {
                    json!([{"x": x, "y":y, "id":0}])
                };
                active.cdp.call(
                    "Input.dispatchTouchEvent",
                    json!({"type":kind,"touchPoints":points}),
                )?;
            }
            Ok(Input::Wheel(x, y, dx, dy)) => {
                active.cdp.call(
                    "Input.dispatchMouseEvent",
                    json!({"type":"mouseWheel","x":x,"y":y,"deltaX":dx,"deltaY":dy}),
                )?;
            }
            Ok(Input::Text(text)) => {
                active.cdp.call("Input.insertText", json!({"text":text}))?;
            }
            Ok(Input::Key(key, code)) => {
                for kind in ["keyDown", "keyUp"] {
                    active.cdp.call(
                        "Input.dispatchKeyEvent",
                        json!({"type":kind,"key":key,"windowsVirtualKeyCode":code}),
                    )?;
                }
            }
            Ok(Input::Back) => {
                if !active.cdp.back()? {
                    let _ = updates.send(Update::Home);
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return Ok(()),
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
        }
        if visible && Instant::now() >= next {
            let bytes = active.cdp.capture(viewport)?;
            if bytes != previous {
                let pixels = image::load_from_memory_with_format(
                    &bytes,
                    image::ImageFormat::Jpeg,
                )
                .map_err(|error| format!("Could not decode preview frame: {error}"))?
                .into_rgba8();
                let (width, height) = pixels.dimensions();
                let frame = floem::peniko::Image::new(
                    floem::peniko::Blob::new(Arc::new(pixels.into_raw())),
                    floem::peniko::ImageFormat::Rgba8,
                    width,
                    height,
                );
                match updates.try_send(Update::Frame(frame)) {
                    Ok(()) => previous = bytes,
                    Err(std::sync::mpsc::TrySendError::Full(_)) => {}
                    Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                        return Ok(());
                    }
                }
            }
            next = Instant::now() + Duration::from_millis(90);
        }
    }
}

struct Cdp {
    socket: Socket,
    id: u64,
}

impl Cdp {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.id += 1;
        self.socket
            .send(Message::Text(
                json!({"id":self.id,"method":method,"params":params})
                    .to_string()
                    .into(),
            ))
            .map_err(|e| format!("{method}: {e}"))?;
        let until = Instant::now() + Duration::from_secs(5);
        while Instant::now() < until {
            let message =
                self.socket.read().map_err(|e| format!("{method}: {e}"))?;
            if let Ok(text) = message.into_text()
                && let Ok(reply) = serde_json::from_str::<Value>(&text)
                && reply["id"].as_u64() == Some(self.id)
            {
                return if let Some(error) = reply.get("error") {
                    Err(format!("{method}: {error}"))
                } else {
                    Ok(reply["result"].clone())
                };
            }
        }
        Err(format!("{method} timed out."))
    }

    fn navigate(&mut self, url: &str) -> Result<(), String> {
        let url = url::Url::parse(url).map_err(|e| e.to_string())?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err("Enter an HTTP or HTTPS preview URL.".into());
        }
        let result = self.call("Page.navigate", json!({"url":url.as_str()}))?;
        if let Some(error) = result["errorText"].as_str() {
            return Err(error.into());
        }
        Ok(())
    }

    fn configure(&mut self, viewport: Viewport) -> Result<(), String> {
        let (width, height) = viewport.size;
        self.call("Emulation.setDeviceMetricsOverride", json!({
            "width":width,"height":height,"deviceScaleFactor":viewport.scale,
            "mobile":viewport.mobile,"screenWidth":width,"screenHeight":height,
            "screenOrientation":{
                "type":if viewport.landscape{"landscapePrimary"}else{"portraitPrimary"},
                "angle":if viewport.landscape{90}else{0}
            }
        }))?;
        self.call(
            "Emulation.setTouchEmulationEnabled",
            json!({"enabled":viewport.mobile,"maxTouchPoints":1}),
        )?;
        self.call("Emulation.setEmulatedMedia", json!({
            "features":[{"name":"prefers-color-scheme","value":if viewport.dark{"dark"}else{"light"}}]
        }))?;
        if viewport.mobile {
            self.call(
                "Emulation.setNavigatorOverrides",
                json!({
                    "platform": if viewport.ios { "iPhone" } else { "Linux armv8l" }
                }),
            )?;
        }
        Ok(())
    }

    fn back(&mut self) -> Result<bool, String> {
        let history = self.call("Page.getNavigationHistory", json!({}))?;
        let Some(current) = history["currentIndex"].as_u64() else {
            return Ok(false);
        };
        let Some(previous) = current.checked_sub(1) else {
            return Ok(false);
        };
        let entries = &history["entries"];
        let origin = |index: u64| {
            let parsed =
                url::Url::parse(entries[index as usize]["url"].as_str()?).ok()?;
            matches!(parsed.scheme(), "http" | "https").then(|| parsed.origin())
        };
        let Some(id) = entries[previous as usize]["id"].as_u64() else {
            return Ok(false);
        };
        let Some(current_origin) = origin(current) else {
            return Ok(false);
        };
        if origin(previous) != Some(current_origin) {
            return Ok(false);
        }
        self.call("Page.navigateToHistoryEntry", json!({"entryId":id}))?;
        Ok(true)
    }

    fn capture_page(&mut self, parameters: Value) -> Result<Value, String> {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match self.call("Page.captureScreenshot", parameters.clone()) {
                Err(error)
                    if error.contains("Not attached to an active page")
                        && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(40));
                }
                result => return result,
            }
        }
    }

    fn capture(&mut self, viewport: Viewport) -> Result<Vec<u8>, String> {
        let (width, height) = viewport.size;
        let scale = (900.0 / f64::from(width.max(height))).min(1.0) / viewport.scale;
        let result = self.capture_page(json!({
            "format":"jpeg","quality":82,"fromSurface":true,"captureBeyondViewport":false,
            "clip":{"x":0,"y":0,"width":width,"height":height,"scale":scale}
        }))?;
        base64::engine::general_purpose::STANDARD
            .decode(
                result["data"]
                    .as_str()
                    .ok_or("The preview renderer returned no image.")?,
            )
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires an installed Chromium renderer and a local socket"]
    fn captures_and_interacts_with_a_windowless_device_surface() {
        let (browser, tab) = open().unwrap();
        let mut cdp = tab.cdp;
        let portrait = Viewport {
            size: (390, 844),
            scale: 3.0,
            landscape: false,
            dark: true,
            ios: true,
            mobile: true,
        };
        cdp.configure(portrait).unwrap();
        let html = "<meta name=viewport content='width=device-width,initial-scale=1'><button style='width:100px;height:100px' onclick='this.textContent=1'>0</button>";
        let page = format!(
            "data:text/html;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(html)
        );
        cdp.call("Page.navigate", json!({"url": page})).unwrap();
        cdp.call("Page.bringToFront", json!({})).unwrap();
        let frame = cdp.capture(portrait).unwrap();
        let image =
            image::load_from_memory_with_format(&frame, image::ImageFormat::Jpeg)
                .unwrap();
        assert!(image.width() > 0 && image.height() > image.width());
        let eval = |cdp: &mut Cdp, expression: &str| {
            cdp.call(
                "Runtime.evaluate",
                json!({"expression": expression,"returnByValue":true}),
            )
            .unwrap()["result"]["value"]
                .clone()
        };
        assert_eq!(
            eval(
                &mut cdp,
                "[innerWidth,innerHeight,devicePixelRatio,matchMedia('(prefers-color-scheme: dark)').matches]"
            ),
            json!([390, 844, 3, true])
        );
        cdp.call(
            "Input.dispatchTouchEvent",
            json!({"type":"touchStart","touchPoints":[{"x":40,"y":40,"id":0}]}),
        )
        .unwrap();
        cdp.call(
            "Input.dispatchTouchEvent",
            json!({"type":"touchEnd","touchPoints":[]}),
        )
        .unwrap();
        assert_eq!(
            eval(&mut cdp, "document.querySelector('button').textContent"),
            json!("1")
        );
        let landscape = Viewport {
            size: (844, 390),
            landscape: true,
            dark: false,
            ..portrait
        };
        cdp.configure(landscape).unwrap();
        assert_eq!(
            eval(
                &mut cdp,
                "[innerWidth,innerHeight,screen.orientation.type,matchMedia('(prefers-color-scheme: dark)').matches]"
            ),
            json!([844, 390, "landscape-primary", false])
        );
        let frame = cdp.capture(landscape).unwrap();
        let image =
            image::load_from_memory_with_format(&frame, image::ImageFormat::Jpeg)
                .unwrap();
        assert!(image.width() > image.height());
        let profile = browser._process._profile.path().to_owned();
        drop(cdp);
        drop(browser);
        assert!(!profile.exists());
    }
}
