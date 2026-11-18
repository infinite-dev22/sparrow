//! Problems grouped by file, backed by the workspace's live analyzer results.
use super::kind::PanelKind;
use crate::{
    app::clickable_icon,
    command::InternalCommand,
    config::{color::SparrowColor, icon::SparrowIcons},
    editor::location::{EditorLocation, EditorPosition},
    window_tab::{Focus, WindowTabData},
};
use floem::{
    View,
    event::{Event, EventListener, EventPropagation},
    keyboard::{Key, NamedKey},
    reactive::{SignalGet, SignalUpdate, SignalWith, create_memo, create_rw_signal},
    views::{Decorators, dyn_stack, empty, label, scroll, stack, svg},
};
use lsp_types::{Diagnostic, DiagnosticSeverity};
use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
    rc::Rc,
};

#[derive(Clone, PartialEq)]
struct Issue {
    path: PathBuf,
    diagnostic: Diagnostic,
    offset: Option<usize>,
}
fn rank(d: &Diagnostic) -> u8 {
    match d.severity {
        Some(DiagnosticSeverity::ERROR) => 0,
        Some(DiagnosticSeverity::WARNING) => 1,
        Some(DiagnosticSeverity::INFORMATION) => 2,
        _ => 3,
    }
}
fn groups(
    issues: &[Issue],
    file: Option<&std::path::Path>,
    project_errors: bool,
    severity: Option<u8>,
) -> Vec<(PathBuf, Vec<Issue>)> {
    let mut groups = BTreeMap::<PathBuf, Vec<Issue>>::new();
    for issue in issues {
        if (if project_errors {
            rank(&issue.diagnostic) == 0
        } else {
            file == Some(issue.path.as_path())
        }) && severity.is_none_or(|s| rank(&issue.diagnostic).min(2) == s)
        {
            groups
                .entry(issue.path.clone())
                .or_default()
                .push(issue.clone());
        }
    }
    for rows in groups.values_mut() {
        rows.sort_by_key(|r| (rank(&r.diagnostic), r.diagnostic.range.start));
    }
    groups.into_iter().collect()
}
fn jump(window: &WindowTabData, issue: &Issue) {
    window
        .common
        .internal_command
        .send(InternalCommand::JumpToLocation {
            location: EditorLocation {
                path: issue.path.clone(),
                position: Some(issue.offset.map(EditorPosition::Offset).unwrap_or(
                    EditorPosition::Position(issue.diagnostic.range.start),
                )),
                scroll_offset: None,
                ignore_unconfirmed: false,
                same_editor_tab: false,
            },
        });
}
fn details(window: Rc<WindowTabData>, issue: Issue) -> impl View {
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
pub fn panel(window: Rc<WindowTabData>) -> impl View {
    let config = window.common.config;
    let diagnostics = window.main_split.diagnostics;
    let editor = window.main_split.active_editor;
    let tab = create_rw_signal(false);
    let severity = create_rw_signal(None::<u8>);
    let collapsed = create_rw_signal(HashSet::<PathBuf>::new());
    let selected = create_rw_signal(None::<Issue>);
    let view_id = create_rw_signal(None::<floem::ViewId>);
    let file = create_memo(move |_| {
        editor
            .get()
            .and_then(|e| e.doc_signal().get().content.with(|c| c.path().cloned()))
    });
    let issues = create_memo(move |_| {
        let mut issues = Vec::new();
        for (path, data) in diagnostics.get() {
            let spans = data.diagnostics_span.get();
            if spans.is_empty() {
                issues.extend(data.diagnostics.get().into_iter().map(
                    |diagnostic| Issue {
                        path: path.clone(),
                        diagnostic,
                        offset: None,
                    },
                ));
            } else {
                issues.extend(spans.iter().map(|(range, diagnostic)| Issue {
                    path: path.clone(),
                    diagnostic: diagnostic.clone(),
                    offset: Some(range.start),
                }));
            }
        }
        issues
    });
    let grouped = create_memo(move |_| {
        groups(
            &issues.get(),
            file.get().as_deref(),
            tab.get(),
            severity.get(),
        )
    });
    let visible = create_memo(move |_| {
        grouped
            .get()
            .into_iter()
            .filter(|(p, _)| !collapsed.with(|c| c.contains(p)))
            .flat_map(|(_, rows)| rows)
            .collect::<Vec<_>>()
    });
    let tabs =
        dyn_stack(
            || vec![(false, "File"), (true, "Project Errors")],
            |(i, _)| *i,
            move |(i, title)| {
                label(move || {
                    if !i {
                        let count = issues.with(|rows| {
                            rows.iter()
                                .filter(|r| Some(&r.path) == file.get().as_ref())
                                .count()
                        });
                        if count > 0 {
                            format!("File  {count}")
                        } else {
                            title.into()
                        }
                    } else {
                        title.into()
                    }
                })
                .on_click_stop(move |_| {
                    tab.set(i);
                    selected.set(None);
                })
                .style(move |s| {
                    s.height(24.0)
                        .padding_horiz(12.0)
                        .items_center()
                        .border_radius(3.0)
                        .apply_if(tab.get() == i, |s| {
                            s.border(1.0)
                                .border_color(
                                    config.get().color(
                                        SparrowColor::SPARROW_TAB_ACTIVE_BORDER,
                                    ),
                                )
                                .background(config.get().color(
                                    SparrowColor::SPARROW_TAB_ACTIVE_BACKGROUND,
                                ))
                        })
                })
            },
        );
    floem::reactive::create_effect(move |_| {
        let rows = grouped.get();
        if selected
            .get_untracked()
            .is_some_and(|item| !rows.iter().any(|(_, rows)| rows.contains(&item)))
        {
            selected.set(None);
        }
    });
    let detail_window = window.clone();
    let detail_view = dyn_stack(
        move || selected.get().into_iter().collect::<Vec<_>>(),
        |item| {
            (
                item.path.clone(),
                item.diagnostic.message.clone(),
                item.diagnostic.range,
            )
        },
        move |item| details(detail_window.clone(), item),
    )
    .style(move |s| {
        s.width_pct(42.0)
            .height_full()
            .flex_col()
            .border_left(1.0)
            .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
            .apply_if(selected.get().is_none(), |s| s.hide())
    });
    let nav_window = window.clone();
    let row_window = window.clone();
    let root = window.workspace.path.clone();
    let body=stack((
        stack((
            clickable_icon(||SparrowIcons::PROBLEM,move ||severity.set(None),move ||severity.get().is_none(),||false,||"Show All Problems",config),
            clickable_icon(||SparrowIcons::ERROR,move ||severity.set(Some(0)),move ||severity.get()==Some(0),||false,||"Errors",config),
            clickable_icon(||SparrowIcons::WARNING,move ||severity.set(Some(1)),move ||severity.get()==Some(1),||false,||"Warnings",config),
            clickable_icon(||SparrowIcons::LIGHTBULB,move ||severity.set(Some(2)),move ||severity.get()==Some(2),||false,||"Hints and Information",config),
            clickable_icon(||SparrowIcons::ITEM_OPENED,move ||collapsed.set(HashSet::new()),||false,||false,||"Expand All",config),
            clickable_icon(||SparrowIcons::ITEM_CLOSED,move ||collapsed.set(grouped.get().into_iter().map(|(p,_)|p).collect()),||false,||false,||"Collapse All",config),
        )).style(|s|s.width(32.0).height_full().flex_col().items_center().padding_top(2.0).flex_shrink(0.0_f32)),
        stack((
            label(move ||if tab.get(){"No project errors".into()}else {format!("No problems in {}",file.get().and_then(|p|p.file_name().map(|n|n.to_string_lossy().into_owned())).unwrap_or_else(||"the current file".into()))})
                .style(move |s|s.absolute().size_full().items_center().justify_center().color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM)).apply_if(!grouped.with(|r|r.is_empty()),|s|s.hide())),
            scroll(dyn_stack(move ||grouped.get(), |(p, rows)|(p.clone(), rows.iter().map(|r|(r.diagnostic.range, rank(&r.diagnostic), r.diagnostic.message.clone(),r.offset)).collect::<Vec<_>>()),move |(path,rows)| {
                let group_path=path.clone(); let style_path=path.clone(); let icon_path=path.clone();
                let name=path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                let folder=path.parent().map(|p|root.as_ref().and_then(|r|p.strip_prefix(r).ok()).unwrap_or(p).display().to_string()).unwrap_or_default();
                let count=rows.len(); let rows_path=path.clone();
                let w=row_window.clone();
                stack((
                    stack((svg(move ||config.get().ui_svg(if collapsed.with(|c|c.contains(&style_path)){SparrowIcons::ITEM_CLOSED}else{SparrowIcons::ITEM_OPENED})).style(|s|s.size(14.0,14.0).margin_right(4.0)),
                        svg(move ||config.get().file_svg(&icon_path).0).style(|s|s.size(16.0,16.0).margin_right(4.0)),
                        label(move ||name.clone()), label(move ||folder.clone()).style(move |s|s.margin_left(8.0).color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))),
                        label(move ||format!("{count} problem{}",if count==1{""}else{"s"})).style(move |s|s.margin_left(8.0).color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))),
                    )).on_click_stop(move |_| {collapsed.update(|c|{if !c.remove(&group_path){c.insert(group_path.clone());}});if let Some(id)=view_id.get_untracked(){id.request_focus();}}).style(|s|s.height(24.0).items_center().padding_left(8.0).min_width_full()),
                    dyn_stack(move ||if collapsed.with(|c|c.contains(&rows_path)){vec![]}else{rows.clone()},|r|(r.path.clone(),r.diagnostic.range,r.diagnostic.message.clone()),move |issue| {
                        let click_issue=issue.clone(); let double_issue=issue.clone(); let style_issue=issue.clone(); let w=w.clone();let dw=w.clone();
                        let severity_rank=rank(&issue.diagnostic); let line=issue.diagnostic.range.start.line+1;let message=issue.diagnostic.message.replace('\n'," ");
                        stack((svg(move ||config.get().ui_svg(if severity_rank==0 {SparrowIcons::ERROR}else if severity_rank==1 {SparrowIcons::WARNING}else{SparrowIcons::LIGHTBULB})).style(move |s|s.size(14.0,14.0).margin_right(5.0).color(config.get().color(if severity_rank==0 {SparrowColor::SPARROW_ERROR}else{SparrowColor::SPARROW_WARN}))),
                            label(move ||message.clone()).style(|s|s.min_width(0.0).text_ellipsis()),label(move ||format!(":{line}")).style(move |s|s.margin_left(4.0).color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))),empty().style(|s|s.flex_grow(1.0_f32)),
                        )).on_click_stop(move |_| {selected.set(Some(click_issue.clone()));w.common.focus.set(Focus::Panel(PanelKind::Problems));if let Some(id)=view_id.get_untracked(){id.request_focus();}})
                            .on_double_click_stop(move |_|jump(&dw,&double_issue))
                            .style(move |s|s.height(22.0).items_center().padding_left(32.0).padding_right(12.0).min_width_full().border_radius(4.0).hover(|s|s.background(config.get().color(SparrowColor::PANEL_HOVERED_BACKGROUND))).apply_if(selected.with(|r|r.as_ref()==Some(&style_issue)),|s|s.background(config.get().color(SparrowColor::PANEL_CURRENT_BACKGROUND))))
                    }).style(|s|s.flex_col().min_width_full()),
                )).style(|s|s.flex_col().min_width_full())
            }).style(|s|s.flex_col().min_width_full())).style(|s|s.size_full()).scroll_to(move ||{
                let issue=selected.get()?;let groups=grouped.get();let mut index=0;
                for (path,rows) in groups {index+=1;if !collapsed.with(|c|c.contains(&path)){for r in rows {if r==issue {return Some((0.0,(index as f64*22.0-80.0).max(0.0)).into());}index+=1;}}}None
            }),
        )).style(|s|s.flex_grow(1.0_f32).min_width(0.0).height_full()),
        detail_view,
    )).style(|s|s.flex_grow(1.0_f32).flex_basis(0.0).min_height(0.0).width_full());
    view_id.set(Some(body.id()));
    let panel_focus = window.common.focus;
    floem::reactive::create_effect(move |_| {
        if panel_focus.get() == Focus::Panel(PanelKind::Problems) {
            // The window dispatcher restores its own native focus after the shortcut.
            // Focus the panel after that dispatch has completed.
            floem::action::exec_after(std::time::Duration::ZERO, move |_| {
                if panel_focus.get_untracked() == Focus::Panel(PanelKind::Problems) {
                    if let Some(id) = view_id.get_untracked() {
                        id.request_focus();
                    }
                }
            });
        }
    });
    let body =
        body.keyboard_navigable()
            .on_event(EventListener::KeyDown, move |event| {
                let Event::KeyDown(key) = event else {
                    return EventPropagation::Continue;
                };
                if key.modifiers.is_empty() {
                    match &key.key.logical_key {
                        Key::Named(
                            NamedKey::ArrowDown
                            | NamedKey::ArrowUp
                            | NamedKey::Home
                            | NamedKey::End,
                        ) => {
                            let rows = visible.get_untracked();
                            if !rows.is_empty() {
                                let current = selected
                                    .get_untracked()
                                    .and_then(|r| rows.iter().position(|i| *i == r));
                                let index = match key.key.logical_key {
                                    Key::Named(NamedKey::Home) => 0,
                                    Key::Named(NamedKey::End) => rows.len() - 1,
                                    Key::Named(NamedKey::ArrowUp) => {
                                        current.unwrap_or(0).saturating_sub(1)
                                    }
                                    _ => current
                                        .map(|i| (i + 1).min(rows.len() - 1))
                                        .unwrap_or(0),
                                };
                                selected.set(Some(rows[index].clone()));
                            }
                            return EventPropagation::Stop;
                        }
                        Key::Named(NamedKey::Enter | NamedKey::F4) => {
                            if let Some(issue) = selected.get_untracked() {
                                jump(&nav_window, &issue);
                            }
                            return EventPropagation::Stop;
                        }
                        _ => {}
                    }
                }
                if nav_window.key_down(key) {
                    EventPropagation::Stop
                } else {
                    EventPropagation::Continue
                }
            });
    stack((
        super::chrome::header(window, PanelKind::Problems, "Problems", tabs),
        body,
    ))
    .style(|s| s.absolute().size_full().flex_col().font_size(13.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scopes_filter_and_sort_real_severities_per_file() {
        let issue = |path: &str, line, severity| Issue {
            path: PathBuf::from(path),
            diagnostic: Diagnostic {
                range: lsp_types::Range::new(
                    lsp_types::Position::new(line, 0),
                    lsp_types::Position::new(line, 1),
                ),
                severity,
                ..Default::default()
            },
            offset: None,
        };
        let rows = vec![
            issue("/lib/a.dart", 0, Some(DiagnosticSeverity::WARNING)),
            issue("/lib/a.dart", 8, Some(DiagnosticSeverity::ERROR)),
            issue("/lib/b.dart", 3, Some(DiagnosticSeverity::ERROR)),
        ];
        let file = groups(
            &rows,
            Some(std::path::Path::new("/lib/a.dart")),
            false,
            None,
        );
        assert_eq!(file.len(), 1);
        assert_eq!(rank(&file[0].1[0].diagnostic), 0);
        assert_eq!(
            groups(&rows, None, true, None)
                .iter()
                .map(|(_, r)| r.len())
                .sum::<usize>(),
            2
        );
        assert_eq!(
            groups(
                &rows,
                Some(std::path::Path::new("/lib/a.dart")),
                false,
                Some(1)
            )[0]
            .1
            .len(),
            1
        );
        assert!(
            groups(&rows, Some(std::path::Path::new("/README.md")), false, None)
                .is_empty()
        );
    }
}
