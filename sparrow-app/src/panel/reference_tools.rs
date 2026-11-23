//! Shared command execution and native tool-window entry points.
use crate::window_tab::WindowTabData;
use floem::View;
use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    rc::Rc,
    time::{Duration, Instant},
};

// Read stdout and stderr concurrently so a full pipe cannot block timeout/kill.
pub(crate) fn output(
    program: &Path,
    args: &[&str],
    cwd: Option<&Path>,
) -> Result<String, String> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("{}: {e}", program.display()))?;
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let out = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = stdout.read_to_end(&mut b);
        b
    });
    let err = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = stderr.read_to_end(&mut b);
        b
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Ok(s),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(e.to_string());
            }
            _ => {}
        }
        if start.elapsed() > Duration::from_secs(5) {
            let _ = child.kill();
            let _ = child.wait();
            break Err("Command timed out".into());
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    let bytes = out.join().unwrap_or_default();
    let errors = err.join().unwrap_or_default();
    if status?.success() {
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    } else {
        Err(String::from_utf8_lossy(&errors).trim().to_string())
    }
}
pub(super) fn adb() -> PathBuf {
    for var in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Some(root) = std::env::var_os(var) {
            let p = PathBuf::from(root).join("platform-tools/adb");
            if p.is_file() {
                return p;
            }
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        let p = PathBuf::from(home).join("Android/Sdk/platform-tools/adb");
        if p.is_file() {
            return p;
        }
    }
    PathBuf::from("adb")
}

pub fn problems(window: Rc<WindowTabData>) -> impl View {
    super::problems_view::panel(window)
}

pub fn history(window: Rc<WindowTabData>) -> impl View {
    super::git_history::panel(window)
}

pub fn logcat(window: Rc<WindowTabData>) -> impl View {
    super::logcat::panel(window)
}
