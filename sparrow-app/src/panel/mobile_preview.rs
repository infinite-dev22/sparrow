//! Real Flutter-web previews in isolated, touch-enabled mobile browser viewports.
use crate::text_input::text_input;
use crate::{
    config::color::SparrowColor,
    popup_menu::{Menu, MenuItem, PopupMenuExt},
    window_tab::WindowTabData,
};
use floem::{
    View,
    reactive::{Scope, SignalGet, SignalUpdate, create_rw_signal},
    views::{Decorators, empty, label, stack},
};
use std::{
    cell::RefCell,
    process::{Child, Command, Stdio},
    rc::Rc,
    time::{Duration, Instant},
};
#[derive(Clone, Copy)]
struct Device {
    name: &'static str,
    width: u32,
    height: u32,
    dpr: f64,
    ios: bool,
}
const DEVICES: &[Device] = &[
    Device {
        name: "iPhone SE",
        width: 375,
        height: 667,
        dpr: 2.0,
        ios: true,
    },
    Device {
        name: "iPhone 16",
        width: 393,
        height: 852,
        dpr: 3.0,
        ios: true,
    },
    Device {
        name: "iPhone Pro Max",
        width: 440,
        height: 956,
        dpr: 3.0,
        ios: true,
    },
    Device {
        name: "Android large",
        width: 412,
        height: 915,
        dpr: 2.625,
        ios: false,
    },
    Device {
        name: "Android compact",
        width: 360,
        height: 780,
        dpr: 3.0,
        ios: false,
    },
    Device {
        name: "Android tablet",
        width: 800,
        height: 1280,
        dpr: 2.0,
        ios: false,
    },
    Device {
        name: "iPad",
        width: 820,
        height: 1180,
        dpr: 2.0,
        ios: true,
    },
];
struct Preview {
    child: Child,
    _profile: tempfile::TempDir,
    // Emulation overrides are scoped to this CDP connection.
    _connection: Option<
        tungstenite::WebSocket<
            tungstenite::stream::MaybeTlsStream<std::net::TcpStream>,
        >,
    >,
}
impl Drop for Preview {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn metrics(device: Device, landscape: bool) -> serde_json::Value {
    let (width, height) = if landscape {
        (device.height, device.width)
    } else {
        (device.width, device.height)
    };
    serde_json::json!({"width":width,"height":height,"deviceScaleFactor":device.dpr,"mobile":true,"screenWidth":width,"screenHeight":height,"screenOrientation":{"type":if landscape{"landscapePrimary"}else{"portraitPrimary"},"angle":if landscape{90}else{0}}})
}
fn launch(url: &str, device: Device, landscape: bool) -> Result<Preview, String> {
    let url = url::Url::parse(url).map_err(|e| e.to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("Enter an HTTP or HTTPS app URL.".into());
    }
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
    .ok_or("Install Chrome, Chromium or Edge to use interactive mobile previews.")?;
    let profile = tempfile::tempdir().map_err(|e| e.to_string())?;
    let (width, height) = if landscape {
        (device.height, device.width)
    } else {
        (device.width, device.height)
    };
    let child = Command::new(browser)
        .arg(format!("--user-data-dir={}", profile.path().display()))
        .args([
            "--remote-debugging-address=127.0.0.1",
            "--remote-debugging-port=0",
            "--no-first-run",
            "--no-default-browser-check",
        ])
        .arg(format!(
            "--window-size={},{}",
            (width + 24).clamp(500, 1250),
            (height + 100).clamp(400, 1040)
        ))
        .arg("--app=about:blank")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut preview = Preview {
        child,
        _profile: profile,
        _connection: None,
    };
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|e| e.to_string())?;
    let start = Instant::now();
    let mut endpoint = None;
    while start.elapsed() < Duration::from_secs(12) {
        if let Ok(port) = std::fs::read_to_string(
            preview._profile.path().join("DevToolsActivePort"),
        ) {
            if let Some(port) =
                port.lines().next().and_then(|p| p.parse::<u16>().ok())
            {
                if let Ok(response) = client
                    .get(format!("http://127.0.0.1:{port}/json/list"))
                    .send()
                {
                    if let Ok(tabs) = response.json::<Vec<serde_json::Value>>() {
                        endpoint = tabs
                            .iter()
                            .find(|t| t["type"] == "page")
                            .and_then(|t| t["webSocketDebuggerUrl"].as_str())
                            .map(str::to_owned);
                        if endpoint.is_some() {
                            break;
                        }
                    }
                }
            }
        }
        if preview
            .child
            .try_wait()
            .map_err(|e| e.to_string())?
            .is_some()
        {
            return Err("The preview browser exited before it was ready.".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let endpoint = endpoint
        .ok_or("The preview browser did not expose a viewport connection.")?;
    let (mut socket, _) =
        tungstenite::connect(endpoint.as_str()).map_err(|e| e.to_string())?;
    if let tungstenite::stream::MaybeTlsStream::Plain(stream) = socket.get_mut() {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    }
    let commands = [
        (
            "Emulation.setDeviceMetricsOverride",
            metrics(device, landscape),
        ),
        (
            "Emulation.setTouchEmulationEnabled",
            serde_json::json!({"enabled":true,"maxTouchPoints":5}),
        ),
        (
            "Emulation.setUserAgentOverride",
            serde_json::json!({"userAgent":if device.ios{"Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/537.36 Mobile Safari/537.36"}else{"Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36 Chrome/130.0.0.0 Mobile Safari/537.36"},"platform":if device.ios{"iPhone"}else{"Linux armv8l"}}),
        ),
        ("Page.navigate", serde_json::json!({"url":url.as_str()})),
    ];
    for (i, (method, params)) in commands.into_iter().enumerate() {
        let id = i + 1;
        socket
            .send(tungstenite::Message::Text(
                serde_json::json!({"id":id,"method":method,"params":params})
                    .to_string()
                    .into(),
            ))
            .map_err(|e| e.to_string())?;
        loop {
            let message = socket.read().map_err(|e| e.to_string())?;
            if let Ok(text) = message.into_text() {
                if let Ok(result) = serde_json::from_str::<serde_json::Value>(&text)
                {
                    if result["id"].as_u64() == Some(id as u64) {
                        if let Some(error) = result.get("error") {
                            return Err(format!(
                                "Viewport configuration failed: {error}"
                            ));
                        }
                        break;
                    }
                }
            }
        }
    }
    preview._connection = Some(socket);
    Ok(preview)
}
pub fn panel(window: Rc<WindowTabData>) -> impl View {
    let config = window.common.config;
    let device = create_rw_signal(3usize);
    let landscape = create_rw_signal(false);
    let url = create_rw_signal("http://127.0.0.1:7357".to_string());
    let status = create_rw_signal(String::new());
    let busy = create_rw_signal(false);
    let preview = Rc::new(RefCell::new(None::<Preview>));
    let session = preview.clone();
    let stop = preview.clone();
    let commands = window.common.internal_command;
    let root = window.workspace.path.clone();
    let notices = window.notifications;
    let launch_button=label(move ||if busy.get(){"Opening…"}else{"Open interactive preview"}).on_click_stop(move |_|{
 if busy.get_untracked(){return;}busy.set(true);status.set(String::new());let session=session.clone();let send=floem::ext_event::create_ext_action(Scope::current(),move |result:Result<Preview,String>|{busy.set(false);match result{Ok(browser)=>{session.borrow_mut().replace(browser);status.set("Preview is open in its own mobile viewport. Touch, scrolling and app navigation are enabled.".into());},Err(e)=>status.set(e)}});let url=url.get_untracked();let d=DEVICES[device.get_untracked()];let rotate=landscape.get_untracked();std::thread::spawn(move ||send(launch(&url,d,rotate)));
 }).disabled(move ||busy.get()).style(move |s|s.padding(8.0).border_radius(4.0).background(config.get().color(SparrowColor::SPARROW_BUTTON_PRIMARY_BACKGROUND)));
    stack((super::chrome::header(window,super::kind::PanelKind::MobilePreview,"Mobile Preview",empty()),stack((
 label(||"Preview your app without an emulator").style(|s|s.font_bold()),
 label(move ||DEVICES[device.get()].name).ide_popout_menu(move ||{let mut menu=Menu::new("Device");for (i,d) in DEVICES.iter().enumerate(){menu=menu.entry(MenuItem::new(d.name).action(move ||device.set(i)));}menu}).style(move |s|s.padding(8.0).border(1.0).border_color(config.get().color(SparrowColor::SPARROW_BORDER))),
 label(move ||{let d=DEVICES[device.get()];let (w,h)=if landscape.get(){(d.height,d.width)}else{(d.width,d.height)};format!("{w} × {h} logical pixels · {}× density",d.dpr)}),
 label(move ||if landscape.get(){"↻ Landscape"}else{"↻ Portrait"}).on_click_stop(move |_|landscape.update(|v|*v=!*v)),
 text_input(url, config).placeholder("App URL"),
 label(||"Start Flutter web server").on_click_stop(move |_|{let Some(root)=root.as_deref() else{return;};if let Some(flutter)=crate::dart::sdk::find_flutter_binary(Some(root)){commands.send(crate::command::InternalCommand::NewTerminal{profile:Some(sparrow_rpc::terminal::TerminalProfile{name:"Mobile Preview server".into(),command:Some(flutter.display().to_string()),arguments:Some(vec!["run".into(),"-d".into(),"web-server".into(),"--web-hostname".into(),"127.0.0.1".into(),"--web-port".into(),"7357".into()]),workdir:lsp_types::Url::from_directory_path(root).ok(),..Default::default()})});}else{super::workflow_tools::record(notices,"Mobile Preview","Configure a Flutter SDK before starting the web server.");}}),
 launch_button,label(||"Close preview").on_click_stop(move |_|{stop.borrow_mut().take();status.set("Preview closed.".into());}),label(move ||status.get()),
 label(||"Uses your app’s Flutter web build. Camera, native plugins and iOS/Android OS behavior require a real device. Change a preset or orientation, then reopen to apply.").style(move |s|s.width_full().color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))),
 )).style(|s|s.flex_col().gap(14.0).padding(16.0).width_full()))).style(|s|s.absolute().size_full().flex_col())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rotation_preserves_density_and_swaps_viewport() {
        let d = DEVICES[3];
        assert_eq!(metrics(d, false)["width"], 412);
        assert_eq!(metrics(d, true)["width"], 915);
        assert_eq!(metrics(d, true)["deviceScaleFactor"], 2.625);
    }
}
