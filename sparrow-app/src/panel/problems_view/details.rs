//! Selected finding details and language-service actions.
use std::rc::Rc;

use floem::{
    View,
    ext_event::create_ext_action,
    reactive::{RwSignal, SignalGet, SignalUpdate, SignalWith},
    views::{Decorators, dyn_stack, label, stack},
};
use lsp_types::{CodeActionOrCommand, Range};
use sparrow_core::rope_text_pos::RopeTextPosition;
use sparrow_rpc::{plugin::PluginId, proxy::ProxyResponse};

use crate::{
    command::InternalCommand,
    config::color::SparrowColor,
    doc::Doc,
    editor::location::{EditorLocation, EditorPosition},
    widgets::{
        ButtonKind, button,
        popup_menu::{Menu, MenuItem, show_context_menu},
        sparrow_scroll,
    },
    window_tab::WindowTabData,
};

use super::{Issue, jump, rank};

pub(super) fn details(
    window: Rc<WindowTabData>,
    issue: Issue,
    selection: RwSignal<Option<Issue>>,
) -> impl View {
    let config = window.common.config;
    let message = issue.diagnostic.message.clone();
    let severity =
        ["Error", "Warning", "Information", "Hint"][rank(&issue.diagnostic)];
    let metadata = format!("{severity} · {}", issue.rule());
    let position = issue.diagnostic.range.start;
    let location = format!(
        "{}:{}:{}",
        issue.path.display(),
        position.line + 1,
        position.character + 1
    );
    let related = issue
        .diagnostic
        .related_information
        .clone()
        .unwrap_or_default();
    let description = issue
        .diagnostic
        .code_description
        .clone()
        .map(|d| d.href)
        .filter(|url| matches!(url.scheme(), "http" | "https"));
    let open = window.clone();
    let fix = window.clone();
    let suppress = window.clone();
    let links = window.clone();
    let target = issue.clone();
    let fix_issue = issue.clone();
    let suppress_issue = issue.clone();
    sparrow_scroll(
        stack((
            label(move || metadata.clone()).style(move |s| {
                s.color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
            }),
            stack((
                button(|| "Quick Fixes…", ButtonKind::Outlined, config)
                    .on_click_stop(move |_| {
                        show_actions(
                            fix.clone(),
                            fix_issue.clone(),
                            selection,
                            false,
                        )
                    }),
                button(|| "Suppress…", ButtonKind::Outlined, config).on_click_stop(
                    move |_| {
                        show_actions(
                            suppress.clone(),
                            suppress_issue.clone(),
                            selection,
                            true,
                        )
                    },
                ),
            ))
            .style(|s| {
                s.items_center()
                    .gap(8.0)
                    .flex_wrap(floem::style::FlexWrap::Wrap)
            }),
            label(|| "Problem details").style(|s| s.font_bold()),
            label(move || message.clone())
                .style(|s| s.width_full().selectable(true)),
            button(move || location.clone(), ButtonKind::Outlined, config)
                .on_click_stop(move |_| jump(&open, &target, false))
                .style(|s| s.max_width_full()),
            dyn_stack(
                move || description.clone().into_iter(),
                |url| url.clone(),
                move |url| {
                    let commands = window.common.internal_command;
                    button(
                        || "Diagnostic documentation ↗",
                        ButtonKind::Outlined,
                        config,
                    )
                    .on_click_stop(move |_| {
                        commands.send(InternalCommand::OpenWebUri {
                            uri: url.to_string(),
                        })
                    })
                },
            ),
            dyn_stack(
                move || related.clone().into_iter().enumerate(),
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

pub(super) fn show_actions(
    window: Rc<WindowTabData>,
    issue: Issue,
    selection: RwSignal<Option<Issue>>,
    suppress: bool,
) {
    let request = window.inspections.action_request;
    request.update(|id| *id = id.wrapping_add(1));
    let token = request.get_untracked();
    let (doc, _) = window.main_split.get_doc(issue.path.clone(), None);
    if doc.loaded() {
        request_actions(window, issue, doc, selection, suppress, token);
        return;
    }
    let scope = window.scope.create_child();
    scope.create_effect(move |_| {
        if request.try_get() != Some(token)
            || selection.try_get().flatten().as_ref() != Some(&issue)
        {
            scope.dispose();
        } else if doc.loaded.get() {
            request_actions(
                window.clone(),
                issue.clone(),
                doc.clone(),
                selection,
                suppress,
                token,
            );
            scope.dispose();
        }
    });
}

fn request_actions(
    window: Rc<WindowTabData>,
    issue: Issue,
    doc: Rc<Doc>,
    selection: RwSignal<Option<Issue>>,
    suppress: bool,
    token: u64,
) {
    if window.inspections.tab.get_untracked() == super::Tab::Inspection
        && !doc.is_pristine()
    {
        window.show_alert(
            "Inspection results".into(),
            "Save this file and rerun the inspection to apply actions to its current contents.".into(),
            Vec::new(),
        );
        return;
    }
    let revision = doc.rev();
    let mut diagnostic = issue.diagnostic.clone();
    if let Some(offset) = issue.offset {
        let start = doc
            .buffer
            .with_untracked(|buffer| buffer.offset_to_position(offset));
        if start != diagnostic.range.start {
            diagnostic.range = Range::new(start, start);
        }
    }
    let range = diagnostic.range;
    let path = issue.path.clone();
    let request_window = window.clone();
    let selected_tab = window.inspections.tab.get_untracked();
    let complete = move |result: Result<
        (PluginId, Vec<CodeActionOrCommand>),
        String,
    >| {
        if window.inspections.action_request.try_get_untracked() != Some(token)
            || selection.try_get_untracked().flatten().as_ref() != Some(&issue)
            || window.inspections.tab.get_untracked() != selected_tab
            || !window.panel.is_panel_visible(&super::PanelKind::Problems)
        {
            return;
        }
        let title = if suppress { "Suppress" } else { "Quick Fixes" };
        let mut menu = Menu::new(title);
        match result {
            Ok((plugin, actions)) if doc.rev() == revision => {
                let mut found = false;
                for action in actions {
                    let (label, enabled) = match &action {
                        CodeActionOrCommand::CodeAction(action) => {
                            (&action.title, action.disabled.is_none())
                        }
                        CodeActionOrCommand::Command(action) => {
                            (&action.title, true)
                        }
                    };
                    if is_suppression(label) != suppress {
                        continue;
                    }
                    found = true;
                    let current = doc.clone();
                    let target = window.clone();
                    menu = menu.entry(MenuItem::new(label.clone()).enabled(enabled).action(move || {
                        if current.rev() == revision {
                            target.common.internal_command.send(InternalCommand::RunCodeAction { plugin_id: plugin, action: action.clone() });
                        } else {
                            target.show_alert("Problems".into(), "The source changed. Request the action again.".into(), Vec::new());
                        }
                    }));
                }
                if !found {
                    menu = menu.entry(
                        MenuItem::new(if suppress {
                            "No suppression available"
                        } else {
                            "No quick fixes available"
                        })
                        .enabled(false),
                    );
                }
            }
            Ok(_) => {
                menu = menu.entry(
                    MenuItem::new("Source changed. Request the action again.")
                        .enabled(false),
                );
            }
            Err(error) => {
                window.show_alert(title.into(), error, Vec::new());
                return;
            }
        }
        show_context_menu(menu, None);
    };

    if crate::doc::yaml_schema::supports(&path) {
        let (doc, _) = request_window.main_split.get_doc(path.clone(), None);
        let source = doc.buffer.with_untracked(|buffer| buffer.to_string());
        let mut actions =
            crate::doc::yaml_schema::code_actions(&path, &source, range.start.line);
        actions.extend(crate::doc::pubdev::code_actions(
            &path,
            &source,
            range.start.line,
        ));
        complete(Ok((PluginId(0), actions)));
    } else if crate::dart::is_dart_path(&path) {
        if !request_window.common.dart_lsp.is_available() {
            complete(Err(request_window.common.dart_lsp.unavailable_reason()));
            return;
        }
        let send = create_ext_action(request_window.scope, complete);
        request_window.common.dart_lsp.code_actions_range_result(
            &path,
            range,
            &[diagnostic],
            Some("quickfix"),
            move |actions| send(actions.map(|actions| (PluginId(0), actions))),
        );
    } else {
        let send = create_ext_action(request_window.scope, complete);
        request_window.common.proxy.get_code_actions(path, range.start, vec![diagnostic], move |result| {
            send(match result {
                Ok(ProxyResponse::GetCodeActionsResponse { plugin_id, resp }) => Ok((plugin_id, resp)),
                _ => Err("The language service did not provide quick fixes for this file.".into()),
            });
        });
    }
}

fn is_suppression(title: &str) -> bool {
    let title = title.to_ascii_lowercase();
    title.starts_with("ignore ") || title.starts_with("suppress ")
}
