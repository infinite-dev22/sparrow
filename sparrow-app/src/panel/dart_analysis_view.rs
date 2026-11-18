//! Flat, navigable diagnostic table matching the Dart Analysis tool window.
use super::kind::PanelKind;
use crate::popup_menu::{Menu, MenuItem, PopupMenuExt};
use crate::{
    app::clickable_icon,
    command::InternalCommand,
    config::{color::SparrowColor, icon::SparrowIcons},
    editor::location::{EditorLocation, EditorPosition},
    window_tab::WindowTabData,
};
use floem::{
    View,
    reactive::{SignalGet, SignalUpdate, SignalWith, create_memo, create_rw_signal},
    views::{Decorators, VirtualVector, label, scroll, stack, svg, virtual_stack},
};
use lsp_types::{Diagnostic, DiagnosticSeverity};
use std::{path::PathBuf, rc::Rc};
#[derive(Clone, PartialEq)]
struct Row {
    path: PathBuf,
    diagnostic: Diagnostic,
    offset: Option<usize>,
}
fn rank(severity: Option<DiagnosticSeverity>) -> u8 {
    match severity {
        Some(DiagnosticSeverity::ERROR) => 0,
        Some(DiagnosticSeverity::WARNING) => 1,
        _ => 2,
    }
}
struct Rows(Vec<Row>);
impl VirtualVector<Row> for Rows {
    fn total_len(&self) -> usize {
        self.0.len()
    }
    fn slice(&mut self, range: std::ops::Range<usize>) -> impl Iterator<Item = Row> {
        self.0[range].iter().cloned()
    }
}
pub fn panel(window: Rc<WindowTabData>) -> impl View {
    let config = window.common.config;
    let diagnostics = window.main_split.diagnostics;
    let internal = window.common.internal_command;
    let root = window.workspace.path.clone();
    let filter = create_rw_signal(None::<u8>);
    let sort_location = create_rw_signal(true);
    let ascending = create_rw_signal(true);
    let selected = create_rw_signal(None::<(PathBuf, lsp_types::Position)>);
    let detail_window = window.clone();
    let reanalyze = window.common.dart_lsp.clone();
    let notifications = window.notifications;
    let scope = window.scope;
    let settings = window.settings_modal_visible;
    let rows = create_memo(move |_| {
        let mut rows = Vec::new();
        for (path, data) in diagnostics.get() {
            let spans = data.diagnostics_span.get();
            if !spans.is_empty() {
                rows.extend(spans.iter().map(|(interval, diagnostic)| Row {
                    path: path.clone(),
                    diagnostic: diagnostic.clone(),
                    offset: Some(interval.start),
                }));
            } else {
                rows.extend(data.diagnostics.get().iter().cloned().map(
                    |diagnostic| Row {
                        path: path.clone(),
                        diagnostic,
                        offset: None,
                    },
                ));
            }
        }
        rows.sort_by(|left, right| {
            let order = if sort_location.get() {
                (
                    left.path.clone(),
                    left.diagnostic.range.start.line,
                    left.diagnostic.range.start.character,
                )
                    .cmp(&(
                        right.path.clone(),
                        right.diagnostic.range.start.line,
                        right.diagnostic.range.start.character,
                    ))
            } else {
                left.diagnostic.message.cmp(&right.diagnostic.message)
            };
            if ascending.get() {
                order
            } else {
                order.reverse()
            }
        });
        rows
    });
    let visible = create_memo(move |_| {
        rows.with(|r| {
            r.iter()
                .filter(|r| {
                    filter
                        .get()
                        .is_none_or(|f| f == rank(r.diagnostic.severity))
                })
                .cloned()
                .collect::<Vec<_>>()
        })
    });
    let summary = label(move || {
        rows.with(|rows| {
            let errors = rows
                .iter()
                .filter(|r| rank(r.diagnostic.severity) == 0)
                .count();
            let warnings = rows
                .iter()
                .filter(|r| rank(r.diagnostic.severity) == 1)
                .count();
            let hints = rows.len() - errors - warnings;
            let mut parts = Vec::new();
            for (count, word) in
                [(errors, "error"), (warnings, "warning"), (hints, "hint")]
            {
                if count > 0 {
                    parts.push(format!(
                        "{count} {word}{}",
                        if count == 1 { "" } else { "s" }
                    ));
                }
            }
            if parts.is_empty() {
                "No issues".into()
            } else {
                parts.join(", ")
            }
        })
    })
    .style(move |s| {
        s.font_size(12.0)
            .padding_horiz(8.0)
            .padding_vert(3.0)
            .border_radius(4.0)
            .border(1.0)
            .border_color(
                config.get().color(SparrowColor::SPARROW_TAB_ACTIVE_BORDER),
            )
            .background(
                config
                    .get()
                    .color(SparrowColor::SPARROW_TAB_ACTIVE_BACKGROUND),
            )
    });
    let height = 24.0;
    stack((
        super::chrome::header(window.clone(), PanelKind::Problem, "Dart Analysis", summary),
        stack((
            stack((
                clickable_icon(||SparrowIcons::IMAGE_LOADING,move ||{
                    let send=floem::ext_event::create_ext_action(scope,move |result:serde_json::Value|{
                        if let Some(error)=result.get("error"){super::workflow_tools::record(notifications,"Dart analysis",&error.to_string());}
                    });
                    reanalyze.request_async("dart/reanalyze",serde_json::Value::Null,move |result|send(result));
                },||false,||false,||"Reanalyze project",config),
                clickable_icon(||SparrowIcons::SETTINGS,move ||settings.set(true),||false,||false,||"Analysis settings",config),
                clickable_icon(
                    || SparrowIcons::PROBLEM,
                    move || filter.set(None),
                    move || filter.get().is_none(),
                    || false,
                    || "Show All Diagnostics",
                    config,
                ),
                clickable_icon(
                    || SparrowIcons::ERROR,
                    move || filter.set(Some(0)),
                    move || filter.get() == Some(0),
                    || false,
                    || "Errors",
                    config,
                ),
                clickable_icon(
                    || SparrowIcons::WARNING,
                    move || filter.set(Some(1)),
                    move || filter.get() == Some(1),
                    || false,
                    || "Warnings",
                    config,
                ),
                clickable_icon(
                    || SparrowIcons::LIGHTBULB,
                    move || filter.set(Some(2)),
                    move || filter.get() == Some(2),
                    || false,
                    || "Hints and Lints",
                    config,
                ),
            ))
            .style(|s| {
                s.width(32.0)
                    .height_full()
                    .flex_col()
                    .items_center()
                    .padding_top(4.0)
                    .flex_shrink(0.0_f32)
            }),
            stack((
                stack((
                    label(move ||if !sort_location.get(){if ascending.get(){"Description ↑"}else{"Description ↓"}}else{"Description"})
                        .on_click_stop(move |_|{if !sort_location.get_untracked(){ascending.update(|v|*v=!*v);}else{sort_location.set(false);ascending.set(true);}}).style(|s| s.padding_left(6.0).width_pct(50.0)),
                    label(move ||if sort_location.get(){if ascending.get(){"Location ↑"}else{"Location ↓"}}else{"Location"})
                        .on_click_stop(move |_|{if sort_location.get_untracked(){ascending.update(|v|*v=!*v);}else{sort_location.set(true);ascending.set(true);}})
                        .style(move |s| s.padding_left(8.0).width_pct(50.0).border_left(1.0).border_color(config.get().color(SparrowColor::SPARROW_BORDER))),
                ))
                .style(move |s| {
                    s.height(24.0)
                        .items_center()
                        .border_bottom(1.0)
                        .border_color(
                            config.get().color(SparrowColor::SPARROW_BORDER),
                        )
                }),
                label(move || match filter.get() {
                    Some(0) => "No errors",
                    Some(1) => "No warnings",
                    Some(2) => "No hints or lints",
                    _ => "No analysis issues",
                })
                .style(move |s| {
                    s.padding(24.0)
                        .color(
                            config.get().color(SparrowColor::PANEL_FOREGROUND_DIM),
                        )
                        .apply_if(!visible.with(|r| r.is_empty()), |s| s.hide())
                }),
                scroll(
                    virtual_stack(
                        move || Rows(visible.get()),
                        |r| {
                            (
                                r.path.clone(),
                                r.diagnostic.range.start,
                                r.diagnostic.message.clone(),
                            )
                        },
                        move |row| {
                            let details=detail_window.clone();let diagnostic=row.diagnostic.clone();let full_text=format!("{}:{}:{}\n{}\n{}",row.path.display(),row.diagnostic.range.start.line+1,row.diagnostic.range.start.character+1,row.diagnostic.message,row.diagnostic.code.as_ref().map(|code|format!("{code:?}")).unwrap_or_default());
                            let selection=(row.path.clone(),row.diagnostic.range.start);
                            let check=selection.clone();
                            let location_text = format!(
                                "{}:{}",
                                root.as_ref()
                                    .and_then(|root| row
                                        .path
                                        .strip_prefix(root)
                                        .ok())
                                    .unwrap_or(&row.path)
                                    .display(),
                                row.diagnostic.range.start.line + 1
                            );
                            let message = row.diagnostic.message.replace('\n', " ");
                            let severity = rank(row.diagnostic.severity);
                            let location = EditorLocation {
                                path: row.path,
                                position: Some(
                                    row.offset
                                        .map(EditorPosition::Offset)
                                        .unwrap_or(EditorPosition::Position(
                                            row.diagnostic.range.start,
                                        )),
                                ),
                                scroll_offset: None,
                                ignore_unconfirmed: false,
                                same_editor_tab: false,
                            };
                            stack((
                                stack((
                                    svg(move ||config.get().ui_svg(match severity{0=>SparrowIcons::ERROR,1=>SparrowIcons::WARNING,_=>SparrowIcons::PROBLEM})).style(move |s|s.size(14.0,14.0).flex_shrink(0.0_f32).margin_horiz(6.0).color(config.get().color(match severity{0=>SparrowColor::SPARROW_ERROR,1=>SparrowColor::SPARROW_WARN,_=>SparrowColor::EDITOR_LINK}))),
                                    label(move || message.clone())
                                        .style(|s| s.min_width(0.0).text_ellipsis()),
                                ))
                                .style(|s| {
                                    s.width_pct(50.0).items_center().min_width(0.0)
                                }),
                                label(move || location_text.clone()).style(
                                    move |s| {
                                        s.width_pct(50.0)
                                            .padding_left(8.0)
                                            .min_width(0.0)
                                            .text_ellipsis()
                                            .color(
                                                config.get().color(
                                                    SparrowColor::EDITOR_LINK,
                                                ),
                                            )
                                    },
                                ),
                            ))
                            .on_click_stop(move |_| {
                                selected.set(Some(selection.clone()));
                                internal.send(InternalCommand::JumpToLocation {
                                    location: location.clone(),
                                })
                            })
                            .ide_context_menu(move ||{
                                let show=full_text.clone();let copy=full_text.clone();let details=details.clone();
                                let mut menu=Menu::new("").entry(MenuItem::new("Show diagnostic details").action(move ||details.show_alert("Dart diagnostic".into(),show.clone(),Vec::new())))
                                    .entry(MenuItem::new("Copy diagnostic").action(move ||crate::menus::copy_text(&copy)));
                                if let Some(description)=diagnostic.code_description.as_ref(){let url=description.href.to_string();menu=menu.entry(MenuItem::new("Open documentation").action(move ||{let _=open::that(&url);}));}menu
                            })
                            .style(move |s| {
                                s.height(height).apply_if(selected.with(|selected|selected.as_ref()==Some(&check)),|s|s.background(config.get().color(SparrowColor::SPARROW_TAB_ACTIVE_BACKGROUND)))
                                    .width_full()
                                    .items_center()
                                    .hover(|s| {
                                        s.background(config.get().color(
                                            SparrowColor::PANEL_HOVERED_BACKGROUND,
                                        ))
                                    })
                            })
                        },
                    )
                    .item_size_fixed(move || f64::from(height))
                    .style(|s| s.absolute().width_full().flex_col()),
                )
                .style(|s| {
                    s.flex_grow(1.0_f32)
                        .flex_basis(0.0)
                        .min_height(0.0)
                        .width_full()
                }),
            ))
            .style(|s| {
                s.flex_col()
                    .flex_grow(1.0_f32)
                    .flex_basis(0.0)
                    .min_width(0.0)
                    .height_full()
            }),
        ))
        .style(|s| {
            s.flex_grow(1.0_f32)
                .flex_basis(0.0)
                .min_height(0.0)
                .width_full()
        }),
    ))
    .style(|s| s.absolute().size_full().flex_col())
    .debug_name("Dart Analysis Table")
}
