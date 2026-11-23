//! Logcat query history and suggestions.

use crate::panel::logcat::{Pane, State, query::Query};
use crate::widgets::popup_menu::{Menu, MenuItem};
use floem::reactive::{SignalGet, SignalUpdate, SignalWith};

pub(super) fn history(state: State, pane: Pane, suggestions: bool) -> Menu {
    let mut menu = Menu::new(if suggestions {
        "Logcat query suggestions"
    } else {
        "Filter history"
    });
    let mut filters = state.saved.get_untracked().favorites;
    filters.extend(state.saved.get_untracked().history);
    if suggestions {
        filters.extend(
            [
                "package:mine",
                "tag:",
                "-tag:",
                "message:",
                "level:ERROR",
                "level:WARN",
                "age:5m",
                "is:crash",
                "is:stacktrace",
            ]
            .map(String::from),
        );
        filters.extend(pane.data.with_untracked(|b| {
            b.entries
                .iter()
                .rev()
                .take(100)
                .map(|e| format!("tag:{}", e.tag))
                .collect::<Vec<_>>()
        }));
    }
    let mut seen = std::collections::HashSet::new();
    filters.retain(|q| seen.insert(q.clone()));
    for query in filters.into_iter().take(50) {
        let p = pane.clone();
        let remember = state.clone();
        let title = query.clone();
        let count = Query::parse(&query, pane.case.get_untracked())
            .ok()
            .map(|q| {
                pane.data.with_untracked(|b| {
                    b.entries
                        .iter()
                        .filter(|e| {
                            q.matches(
                                e,
                                &state.packages.get_untracked(),
                                chrono::Local::now().timestamp(),
                            )
                        })
                        .count()
                })
            })
            .unwrap_or(0);
        menu = menu.entry(MenuItem::new(format!("{title}    {count}")).action(
            move || {
                p.query.set(query.clone());
                remember.remember(&query, false);
            },
        ));
    }
    let remove = state.clone();
    menu.entry(MenuItem::new("Clear filter history").action(move || {
        remove.saved.update(|s| s.history.clear());
        remove.save();
    }))
}
