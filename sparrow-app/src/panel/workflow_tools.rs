//! Dart and Flutter workflow panels and workspace notification state.

use floem::reactive::{RwSignal, SignalUpdate, SignalWith, batch};
use lsp_types::ShowMessageParams;

use crate::{
    app::logging::CrashReport, widgets::action_link, window_tab::WindowTabData,
};
use floem::{View, views::Decorators};
use std::rc::Rc;

mod notifications;
mod pull_requests;
mod running_devices;

pub use notifications::notifications;
pub use pull_requests::pull_requests;
pub use running_devices::running_devices;

pub(crate) fn crash_report_button(
    window: Rc<WindowTabData>,
    report: Option<CrashReport>,
) -> impl View {
    let config = window.common.config;
    let has_report = report.is_some();
    action_link(
        || "Open crash report",
        move || {
            if let Some(report) = &report {
                window.open_logs(Some(report.clone()));
            }
        },
        config,
    )
    .style(move |s| {
        s.align_self(Some(floem::style::AlignItems::FlexStart))
            .apply_if(!has_report, |s| s.hide())
    })
}

#[derive(Clone, Debug)]
pub struct Notice {
    pub id: u64,
    pub resolution_id: Option<u64>,
    pub time: String,
    pub title: String,
    pub body: String,
    pub crash_report: Option<CrashReport>,
}

#[derive(Clone, PartialEq)]
pub struct PopupMessage {
    pub title: String,
    pub message: ShowMessageParams,
    pub crash_report: Option<CrashReport>,
}

pub fn record(notices: RwSignal<Vec<Notice>>, title: &str, body: &str) {
    record_notice(notices, title, body, None);
}

fn record_notice(
    notices: RwSignal<Vec<Notice>>,
    title: &str,
    body: &str,
    crash_report: Option<CrashReport>,
) {
    notices.update(|items| {
        let id = next_notice_id();
        items.push(Notice {
            id,
            resolution_id: None,
            time: chrono::Local::now().format("%H:%M").to_string(),
            title: title.into(),
            body: body.into(),
            crash_report,
        });
        if items.len() > 200 {
            items.remove(0);
        }
    });
}

fn next_notice_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

pub fn announce(
    notices: RwSignal<Vec<Notice>>,
    messages: RwSignal<Vec<PopupMessage>>,
    title: &str,
    message: &ShowMessageParams,
) {
    announce_message(
        notices,
        messages,
        PopupMessage {
            title: title.into(),
            message: message.clone(),
            crash_report: None,
        },
    );
}

pub(crate) fn announce_message(
    notices: RwSignal<Vec<Notice>>,
    messages: RwSignal<Vec<PopupMessage>>,
    popup: PopupMessage,
) {
    if messages.with_untracked(|items| items.contains(&popup)) {
        return;
    }
    batch(|| {
        record_notice(
            notices,
            &popup.title,
            &popup.message.message,
            popup.crash_report.clone(),
        );
        messages.update(|items| {
            items.push(popup);
            if items.len() > 5 {
                items.remove(0);
            }
        });
    });
}
/// One completion per process, even if automatic and manual requests joined it.
pub fn record_pub_get_result(
    notices: RwSignal<Vec<Notice>>,
    id: u64,
    result: Result<(), String>,
) {
    notices.update(|items| {
        if items.iter().any(|notice| notice.resolution_id == Some(id)) {
            return;
        }
        let next = next_notice_id();
        items.push(Notice {
            id: next,
            resolution_id: Some(id),
            time: chrono::Local::now().format("%H:%M").to_string(),
            title: "Pub Get".into(),
            body: match result {
                Ok(()) => "Package dependencies resolved.".into(),
                Err(error) => error,
            },
            crash_report: None,
        });
        if items.len() > 200 {
            items.remove(0);
        }
    });
}

#[cfg(test)]
mod notice_tests {
    use super::*;
    use floem::reactive::{Scope, SignalGet};

    #[test]
    fn actionable_event_is_shown_and_added_to_the_timeline_once() {
        let scope = Scope::new();
        let notices = scope.create_rw_signal(Vec::new());
        let messages = scope.create_rw_signal(Vec::new());
        let message = ShowMessageParams {
            typ: lsp_types::MessageType::ERROR,
            message: "Could not start the tool".into(),
        };
        announce(notices, messages, "Tool", &message);
        announce(notices, messages, "Tool", &message);
        let popups = messages.get_untracked();
        assert_eq!(popups.len(), 1);
        assert_eq!(popups[0].title, "Tool");
        assert_eq!(popups[0].message, message);
        let history = notices.get_untracked();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].body, "Could not start the tool");
        scope.dispose();
    }
}
