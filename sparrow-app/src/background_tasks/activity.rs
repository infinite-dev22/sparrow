//! Reactive task list shared by the status indicator and detail panel.

use crate::window_tab::WindowTabData;
use floem::reactive::{Memo, SignalGet, SignalWith, create_memo};
use std::rc::Rc;

use super::Activity;

pub(super) fn activities(window: Rc<WindowTabData>) -> Memo<Vec<Activity>> {
    create_memo(move |_| {
        let mut items = window.common.background_tasks.items.get();
        window.progresses.with(|progresses| {
            items.extend(progresses.values().map(|p| Activity {
                id: format!("lsp:{:?}", p.token),
                title: p.title.clone(),
                message: p.message.clone().unwrap_or_default(),
                percentage: p.percentage.map(|p| p.min(100)),
                cancellable: false,
                cancelling: false,
            }));
        });
        if let Some(message) = window.flutter_hot_reload.progress_message.get() {
            items.push(Activity {
                id: "flutter".into(),
                title: "Flutter".into(),
                message,
                percentage: None,
                cancellable: false,
                cancelling: false,
            });
        }
        if window.flutter_hot_reload.is_fetching_devices.get() {
            items.push(Activity {
                id: "devices".into(),
                title: "Discovering devices".into(),
                message: String::new(),
                percentage: None,
                cancellable: false,
                cancelling: false,
            });
        }
        window.flutter_test.run.with(|run| {
            if run.running {
                items.push(Activity {
                    id: "flutter-tests".into(),
                    title: "Running Flutter tests".into(),
                    message: format!("{} tests discovered", run.tests.len()),
                    percentage: None,
                    cancellable: false,
                    cancelling: false,
                });
            }
        });
        items
    })
}
