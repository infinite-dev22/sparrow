//! Real Flutter-web previews in isolated, touch-enabled mobile browser viewports.
use std::{
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
mod view;

pub use view::panel;

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
