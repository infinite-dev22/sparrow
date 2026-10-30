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
    NewTerminal(Arc<RwLock<RawTerminal>>),
    UpdateContent(Vec<u8>),
    CloseTerminal,
}

pub fn terminal_update_process(
    receiver: Receiver<(TermId, TermEvent)>,
    term_notification_tx: Sender<TermNotification>,
) {
    let mut terminals = HashMap::new();
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
            TermEvent::CloseTerminal => {
                terminals.remove(&term_id);
            }
            TermEvent::NewTerminal(raw) => {
                terminals.insert(term_id, raw);
            }
            TermEvent::UpdateContent(content) => {
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
