//! Selected diagnostic details, fixes and source navigation.

use crate::{
    command::InternalCommand,
    config::color::SparrowColor,
    editor::location::{EditorLocation, EditorPosition},
    window_tab::WindowTabData,
};
use floem::{
    View,
    reactive::SignalGet,
    views::{Decorators, dyn_stack, label, scroll, stack},
};
use std::rc::Rc;

use super::{Issue, jump, rank};

pub(super) fn details(window: Rc<WindowTabData>, issue: Issue) -> impl View {
    let config = window.common.config;
    let diagnostic = issue.diagnostic.clone();
    let message = diagnostic.message.clone();
    let severity = match rank(&diagnostic) {
        0 => "Error",
        1 => "Warning",
        _ => "Information",
    };
    let code = diagnostic.code.as_ref().map(|code| match code {
        lsp_types::NumberOrString::Number(n) => n.to_string(),
        lsp_types::NumberOrString::String(s) => s.clone(),
    });
    let metadata = [Some(severity.to_string()), diagnostic.source.clone(), code]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
    let position = diagnostic.range.start;
    let location = format!(
        "{}:{}:{}",
        issue.path.display(),
        position.line + 1,
        position.character + 1
    );
    let related = diagnostic.related_information.unwrap_or_default();
    let description = diagnostic
        .code_description
        .map(|d| d.href)
        .filter(|url| matches!(url.scheme(), "http" | "https"));
    let open = window.clone();
    let fix = window.clone();
    let links = window.clone();
    let docs = window.clone();
    let target = issue.clone();
    scroll(
        stack((
            label(move || metadata.clone()).style(move |s| {
                s.color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
                    .width_full()
            }),
            label(move || message.clone())
                .style(|s| s.width_full().selectable(true)),
            label(move || location.clone())
                .on_click_stop(move |_| jump(&open, &target))
                .style(move |s| {
                    s.width_full()
                        .color(config.get().color(SparrowColor::EDITOR_LINK))
                }),
            label(|| "Show Quick Fixes  Alt+Enter")
                .on_click_stop(move |_| {
                    fix.common
                        .internal_command
                        .send(InternalCommand::QuickFixAt {
                            location: EditorLocation {
                                path: issue.path.clone(),
                                position: Some(EditorPosition::Position(position)),
                                scroll_offset: None,
                                ignore_unconfirmed: false,
                                same_editor_tab: false,
                            },
                        })
                })
                .style(move |s| {
                    s.height(26.0)
                        .items_center()
                        .color(config.get().color(SparrowColor::EDITOR_LINK))
                }),
            dyn_stack(
                move || description.clone().into_iter().collect::<Vec<_>>(),
                |url| url.clone(),
                move |url| {
                    let docs = docs.clone();
                    label(|| "Diagnostic documentation ↗")
                        .on_click_stop(move |_| {
                            docs.common.internal_command.send(
                                InternalCommand::OpenWebUri {
                                    uri: url.to_string(),
                                },
                            )
                        })
                        .style(move |s| {
                            s.height(24.0)
                                .color(config.get().color(SparrowColor::EDITOR_LINK))
                        })
                },
            ),
            dyn_stack(
                move || related.clone().into_iter().enumerate().collect::<Vec<_>>(),
                |(i, _)| *i,
                move |(_, related)| {
                    let links = links.clone();
                    let text = format!(
                        "{}  ({}:{})",
                        related.message,
                        related.location.uri.path(),
                        related.location.range.start.line + 1
                    );
                    label(move || text.clone())
                        .on_click_stop(move |_| {
                            if let Ok(path) = related.location.uri.to_file_path() {
                                links.common.internal_command.send(
                                    InternalCommand::JumpToLocation {
                                        location: EditorLocation {
                                            path,
                                            position: Some(
                                                EditorPosition::Position(
                                                    related.location.range.start,
                                                ),
                                            ),
                                            scroll_offset: None,
                                            ignore_unconfirmed: false,
                                            same_editor_tab: false,
                                        },
                                    },
                                );
                            }
                        })
                        .style(move |s| {
                            s.width_full()
                                .padding_top(6.0)
                                .color(config.get().color(SparrowColor::EDITOR_LINK))
                        })
                },
            )
            .style(|s| s.flex_col().width_full()),
        ))
        .style(|s| s.flex_col().gap(8.0).padding(12.0).width_full()),
    )
    .style(|s| s.size_full())
}
