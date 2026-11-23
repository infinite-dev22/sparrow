use std::{
    collections::HashMap,
    sync::{
        Arc,
        mpsc::{Receiver, Sender},
    },
    time::Instant,
};

use parking_lot::RwLock;
use sparrow_rpc::terminal::TermId;

use super::raw::RawTerminal;

/// The notifications for terminals to send back to main thread
pub enum TermNotification {
    SetTitle {
        term_id: TermId,
        title: String,
    },
    RequestPaint,
    /// Emitted when the terminal output contains a Dart VM service URL
    /// (i.e. `flutter run` printed its banner).
    VmServiceUrl {
        term_id: TermId,
        url: String,
    },
}

pub enum TermEvent {
    NewTerminal(
        Arc<RwLock<RawTerminal>>,
        Option<crate::background_tasks::TaskProgress>,
    ),
    UpdateContent(Vec<u8>),
    Progress(crate::background_tasks::TaskProgress),
    CloseTerminal,
}

pub fn terminal_update_process(
    receiver: Receiver<(TermId, TermEvent)>,
    term_notification_tx: Sender<TermNotification>,
) {
    let mut terminals = HashMap::new();
    let mut progress = HashMap::new();
    let mut lines = HashMap::<TermId, OutputLines>::new();
    let mut last_redraw = Instant::now();
    let mut last_event = None;
    loop {
        let (term_id, event) = if let Some((term_id, event)) = last_event.take() {
            (term_id, event)
        } else {
            match receiver.recv() {
                Ok((term_id, event)) => (term_id, event),
                Err(_) => return,
            }
        };
        match event {
            TermEvent::Progress(activity) => {
                progress.insert(term_id, activity);
            }
            TermEvent::CloseTerminal => {
                terminals.remove(&term_id);
                lines.remove(&term_id);
                progress.remove(&term_id);
            }
            TermEvent::NewTerminal(raw, activity) => {
                if let Some(activity) = activity {
                    progress.insert(term_id, activity);
                } else {
                    progress.remove(&term_id);
                }
                terminals.insert(term_id, raw);
                lines.insert(term_id, OutputLines::default());
            }
            TermEvent::UpdateContent(content) => {
                if let Some(text) = lines.entry(term_id).or_default().push(&content)
                {
                    if let Some(activity) = progress.get(&term_id) {
                        activity.output(&text);
                    }
                }
                // Detect the VM service banner before we hand the bytes to the
                // terminal emulator — the URL is plaintext so no ANSI parsing
                // needed.
                if let Some(url) = scan_vm_service_url(&content) {
                    if let Err(err) = term_notification_tx
                        .send(TermNotification::VmServiceUrl { term_id, url })
                    {
                        tracing::error!("{:?}", err);
                    }
                }
                if let Some(raw) = terminals.get(&term_id) {
                    {
                        raw.write().update_content(content);
                    }
                    last_event = receiver.try_recv().ok();
                    if last_event.is_some() {
                        if last_redraw.elapsed().as_millis() > 10 {
                            last_redraw = Instant::now();
                            if let Err(err) = term_notification_tx
                                .send(TermNotification::RequestPaint)
                            {
                                tracing::error!("{:?}", err);
                            }
                        }
                    } else {
                        last_redraw = Instant::now();
                        if let Err(err) =
                            term_notification_tx.send(TermNotification::RequestPaint)
                        {
                            tracing::error!("{:?}", err);
                        }
                    }
                }
            }
        }
    }
}

use once_cell::sync::Lazy;
use regex::Regex;

/// Matches the banner line printed by `flutter run`:
///   http://127.0.0.1:12345/ABCdefGHIjkl=/
static VM_URL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"http://(?:127\.0\.0\.1|localhost|0\.0\.0\.0|[\d.:]+):\d+/[A-Za-z0-9_\-=]+/",
    )
        .expect("VM service URL regex")
});

fn scan_vm_service_url(content: &[u8]) -> Option<String> {
    // `flutter run`'s URL is printable ASCII; escape codes around it
    // don't affect the match. Lossy decode is safe here.
    let text = String::from_utf8_lossy(content);
    VM_URL_RE.find(&text).map(|m| m.as_str().to_string())
}

/// Retains partial lines across PTY chunks and bounds memory for noisy output.
#[derive(Default)]
struct OutputLines {
    pending: Vec<u8>,
}
impl OutputLines {
    fn push(&mut self, bytes: &[u8]) -> Option<String> {
        self.pending.extend_from_slice(bytes);
        let end = self
            .pending
            .iter()
            .rposition(|b| matches!(b, b'\n' | b'\r'));
        // Flutter writes build messages without a newline while its spinner is
        // running. Report that partial line immediately, retaining it so a
        // later chunk can complete it.
        let raw = String::from_utf8_lossy(&self.pending);
        static ANSI: Lazy<Regex> =
            Lazy::new(|| Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]").unwrap());
        let clean = ANSI.replace_all(&raw, "");
        let ready = clean
            .lines()
            .find(|line| crate::background_tasks::launch_ready(line));
        let result = ready
            .or_else(|| clean.lines().rev().find(|s| !s.trim().is_empty()))
            .into_iter()
            .map(str::trim)
            .find(|s| !s.is_empty())
            .map(|s| s.chars().filter(|c| !c.is_control()).take(240).collect());
        if let Some(end) = end {
            self.pending.drain(..=end);
        }
        if self.pending.len() > 8192 {
            self.pending.clear();
            return None;
        }
        result
    }
}
#[cfg(test)]
mod progress_tests {
    use super::*;
    #[test]
    fn output_progress_handles_split_lines_ansi_and_carriage_returns() {
        let mut lines = OutputLines::default();
        assert_eq!(lines.push(b"Building Lin"), Some("Building Lin".into()));
        assert_eq!(
            lines.push(b"ux application...\r"),
            Some("Building Linux application...".into())
        );
        assert_eq!(
            lines.push(b"\x1b[32mCompiled\x1b[0m\nLaunching\n"),
            Some("Launching".into())
        );
        assert_eq!(lines.push(&vec![b'x'; 9000]), None);
        assert!(lines.pending.is_empty());
    }
}
