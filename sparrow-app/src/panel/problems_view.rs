//! Live diagnostics and saved project inspection results.
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    rc::Rc,
};

use floem::{
    IntoView, View,
    event::{Event, EventListener, EventPropagation},
    keyboard::{Key, NamedKey},
    reactive::{
        RwSignal, SignalGet, SignalUpdate, SignalWith, create_effect, create_memo,
        create_rw_signal,
    },
    style::CursorStyle,
    views::{Decorators, dyn_container, dyn_stack, empty, label, stack, svg},
};
use lsp_types::{Diagnostic, DiagnosticSeverity};

use super::{data::PanelSection, kind::PanelKind, position::PanelPosition};
use crate::{
    config::{color::SparrowColor, icon::SparrowIcons},
    editor::location::{EditorLocation, EditorPosition},
    widgets::{
        clickable_icon, panel_title_bar,
        popup_menu::{Menu, MenuItem, PopupMenuExt, show_context_menu},
        sparrow_scroll, tab_button, tab_strip,
    },
    window_tab::{Focus, WindowTabData},
};

mod details;
mod inspection;
mod options;
mod preview;
mod row;
pub(crate) use inspection::Inspections;
use inspection::Report;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Tab {
    File,
    Project,
    Inspection,
}

#[derive(Clone, Debug, PartialEq)]
struct Issue {
    path: PathBuf,
    diagnostic: Diagnostic,
    offset: Option<usize>,
}

impl Issue {
    fn location(&self, preview_tab: bool) -> EditorLocation {
        EditorLocation {
            path: self.path.clone(),
            position: Some(
                self.offset.map(EditorPosition::Offset).unwrap_or(
                    EditorPosition::Position(self.diagnostic.range.start),
                ),
            ),
            scroll_offset: None,
            ignore_unconfirmed: !preview_tab,
            same_editor_tab: false,
        }
    }

    fn rule(&self) -> String {
        match &self.diagnostic.code {
            Some(lsp_types::NumberOrString::String(code)) => code.clone(),
            Some(lsp_types::NumberOrString::Number(code)) => code.to_string(),
            None => self
                .diagnostic
                .source
                .clone()
                .unwrap_or_else(|| "Other diagnostics".into()),
        }
    }

    fn summary(&self) -> &str {
        self.diagnostic.message.lines().next().unwrap_or_default()
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Group {
    File(PathBuf),
    Rule(String),
}

fn rank(diagnostic: &Diagnostic) -> usize {
    match diagnostic.severity {
        Some(DiagnosticSeverity::ERROR) => 0,
        Some(DiagnosticSeverity::WARNING) => 1,
        Some(DiagnosticSeverity::INFORMATION) => 2,
        _ => 3,
    }
}

fn jump(window: &WindowTabData, issue: &Issue, preview_tab: bool) {
    window
        .main_split
        .jump_to_location(issue.location(preview_tab), None);
    if !preview_tab {
        if let Some(editor) = window.main_split.active_editor.get_untracked() {
            editor.confirmed.set(true);
        }
    }
}

#[derive(Clone, Copy)]
struct Options {
    show: RwSignal<[bool; 4]>,
    sort_by_name: RwSignal<bool>,
    group_by_rule: RwSignal<bool>,
    live_group_by_rule: RwSignal<bool>,
    preview: RwSignal<bool>,
    single_click: RwSignal<bool>,
    preview_tab: RwSignal<bool>,
}

impl Options {
    fn new(scope: floem::reactive::Scope) -> Self {
        Self {
            show: scope.create_rw_signal([true; 4]),
            sort_by_name: scope.create_rw_signal(false),
            group_by_rule: scope.create_rw_signal(true),
            live_group_by_rule: scope.create_rw_signal(false),
            preview: scope.create_rw_signal(false),
            single_click: scope.create_rw_signal(true),
            preview_tab: scope.create_rw_signal(true),
        }
    }

    fn grouping(self, tab: Tab) -> RwSignal<bool> {
        if tab == Tab::Inspection {
            self.group_by_rule
        } else {
            self.live_group_by_rule
        }
    }

    fn filter_menu(self, tab: Tab) -> Menu {
        let mut menu = Menu::new("Diagnostics");
        for (index, name) in [
            "Errors",
            "Warnings",
            "Information",
            "Hints and other diagnostics",
        ]
        .into_iter()
        .enumerate()
        {
            menu = menu.entry(
                toggle_item(name, self.show.get_untracked()[index], move || {
                    self.show.update(|show| show[index] = !show[index]);
                })
                .enabled(tab != Tab::Project || index == 0),
            );
        }
        let grouping = self.grouping(tab);
        menu.separator()
            .entry(toggle_item(
                "Sort by severity",
                !self.sort_by_name.get_untracked(),
                move || self.sort_by_name.set(false),
            ))
            .entry(toggle_item(
                "Sort by name",
                self.sort_by_name.get_untracked(),
                move || self.sort_by_name.set(true),
            ))
            .separator()
            .entry(toggle_item(
                "Group by inspection",
                grouping.get_untracked(),
                move || toggle(grouping),
            ))
    }
}

fn toggle(signal: RwSignal<bool>) {
    signal.update(|value| *value = !*value);
}

fn toggle_item(title: &str, checked: bool, action: impl Fn() + 'static) -> MenuItem {
    MenuItem::new(if checked {
        format!("✓  {title}")
    } else {
        title.to_owned()
    })
    .action(action)
}

fn grouped(
    rows: Vec<Issue>,
    show: [bool; 4],
    by_rule: bool,
    by_name: bool,
) -> Vec<(Group, Vec<Issue>)> {
    let mut groups = BTreeMap::<Group, Vec<Issue>>::new();
    for issue in rows {
        if show[rank(&issue.diagnostic)] {
            let key = if by_rule {
                Group::Rule(issue.rule())
            } else {
                Group::File(issue.path.clone())
            };
            groups.entry(key).or_default().push(issue);
        }
    }
    for rows in groups.values_mut() {
        rows.sort_by(|a, b| {
            let order = if by_name {
                a.summary().to_lowercase().cmp(&b.summary().to_lowercase())
            } else {
                rank(&a.diagnostic).cmp(&rank(&b.diagnostic))
            };
            order.then_with(|| a.path.cmp(&b.path)).then_with(|| {
                a.diagnostic.range.start.cmp(&b.diagnostic.range.start)
            })
        });
    }
    groups.into_iter().collect()
}

pub fn panel(window: Rc<WindowTabData>) -> impl View {
    let config = window.common.config;
    let diagnostics = window.main_split.diagnostics;
    let editor = window.main_split.active_editor;
    let inspection = window.inspections;
    let tab = inspection.tab;
    let report = inspection.report;
    let options = inspection.options;
    let selection = create_rw_signal(None::<Issue>);
    let excluded = inspection.excluded;
    let collapsed = create_rw_signal(BTreeSet::<Group>::new());
    let view_id = create_rw_signal(None::<floem::ViewId>);
    let focus = window.common.focus;
    let file = create_memo(move |_| {
        editor.get().and_then(|editor| {
            editor
                .doc_signal()
                .get()
                .content
                .with(|content| content.path().cloned())
        })
    });
    let live = create_memo(move |_| {
        diagnostics.with(|map| {
            map.iter()
                .flat_map(|(path, data)| {
                    let spans = data.diagnostics_span.get();
                    if spans.is_empty() {
                        data.diagnostics
                            .get()
                            .into_iter()
                            .map(|diagnostic| Issue {
                                path: path.clone(),
                                diagnostic,
                                offset: None,
                            })
                            .collect::<Vec<_>>()
                    } else {
                        spans
                            .iter()
                            .map(|(range, diagnostic)| Issue {
                                path: path.clone(),
                                diagnostic: diagnostic.clone(),
                                offset: Some(range.start),
                            })
                            .collect()
                    }
                })
                .collect::<Vec<_>>()
        })
    });

    create_effect(move |_| {
        tab.get();
        collapsed.set(BTreeSet::new());
        selection.set(None);
    });

    let groups = create_memo(move |_| {
        let current = tab.get();
        let rows = match current {
            Tab::File => {
                let file = file.get();
                live.with(|rows| {
                    rows.iter()
                        .filter(|issue| file.as_ref() == Some(&issue.path))
                        .cloned()
                        .collect()
                })
            }
            Tab::Project => live.with(|rows| {
                rows.iter()
                    .filter(|issue| rank(&issue.diagnostic) == 0)
                    .cloned()
                    .collect()
            }),
            Tab::Inspection => report.with(|report| match report {
                Report::Finished(rows) => excluded.with(|excluded| {
                    rows.iter()
                        .filter(|issue| !excluded.contains(issue))
                        .cloned()
                        .collect()
                }),
                _ => Vec::new(),
            }),
        };
        grouped(
            rows,
            options.show.get(),
            options.grouping(current).get(),
            options.sort_by_name.get(),
        )
    });
    let visible = create_memo(move |_| {
        groups.with(|groups| {
            collapsed.with(|collapsed| {
                groups
                    .iter()
                    .filter(|(group, _)| !collapsed.contains(group))
                    .flat_map(|(_, rows)| rows.iter().cloned())
                    .collect::<Vec<_>>()
            })
        })
    });
    create_effect(move |_| {
        if selection
            .get_untracked()
            .is_some_and(|issue| !visible.with(|rows| rows.contains(&issue)))
        {
            selection.set(None);
        }
    });

    let title = window
        .workspace
        .path
        .as_deref()
        .and_then(|path| path.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Workspace".into());
    let tabs = tab_strip(
        dyn_stack(
            move || {
                let mut tabs = vec![Tab::File, Tab::Project];
                if !report.with(|report| *report == Report::Closed) {
                    tabs.push(Tab::Inspection);
                }
                tabs
            },
            |tab| *tab,
            move |item| {
                let title = title.clone();
                tab_button(
                    stack((
                        label(move || match item {
                            Tab::File => {
                                let file = file.get();
                                let count = live.with(|rows| {
                                    rows.iter()
                                        .filter(|issue| {
                                            file.as_ref() == Some(&issue.path)
                                        })
                                        .count()
                                });
                                if count == 0 {
                                    "File".into()
                                } else {
                                    format!("File  {count}")
                                }
                            }
                            Tab::Project => "Project Errors".into(),
                            Tab::Inspection => format!("Inspection · {title}"),
                        })
                        .style(|s| s.max_width(240.0).text_ellipsis()),
                        clickable_icon(
                            || SparrowIcons::CLOSE,
                            move || inspection.close(),
                            || false,
                            || false,
                            || "Close inspection",
                            config,
                        )
                        .style(move |s| {
                            s.margin_left(6.0)
                                .apply_if(item != Tab::Inspection, |s| s.hide())
                        }),
                    ))
                    .style(|s| s.items_center()),
                    move || tab.get() == item,
                    move || tab.set(item),
                    config,
                )
            },
        )
        .style(|s| s.height_full().items_center()),
    );

    let toolbar_window = window.clone();
    let fix_window = window.clone();
    let toolbar = stack((
        clickable_icon(
            || SparrowIcons::LOGCAT_FILTER,
            || {},
            || false,
            || false,
            || "Filter and sort problems",
            config,
        )
        .ide_popout_menu(move || options.filter_menu(tab.get_untracked())),
        clickable_icon(
            || SparrowIcons::LIGHTBULB,
            move || {
                if let Some(issue) = selection.get_untracked() {
                    details::show_actions(
                        fix_window.clone(),
                        issue,
                        selection,
                        false,
                    );
                }
            },
            || false,
            move || selection.get().is_none(),
            || "Show quick fixes",
            config,
        ),
        clickable_icon(
            || SparrowIcons::SPLIT_HORIZONTAL,
            move || toggle(options.preview),
            move || options.preview.get(),
            || false,
            || "Editor preview",
            config,
        ),
        clickable_icon(
            || SparrowIcons::FOLD_OPEN,
            move || collapsed.set(BTreeSet::new()),
            || false,
            || false,
            || "Expand all groups",
            config,
        ),
        clickable_icon(
            || SparrowIcons::FOLD_CLOSE,
            move || {
                collapsed.set(groups.with_untracked(|groups| {
                    groups.iter().map(|(key, _)| key.clone()).collect()
                }))
            },
            || false,
            || false,
            || "Collapse all groups",
            config,
        ),
        clickable_icon(
            || SparrowIcons::LOGCAT_REFRESH,
            move || inspection.start(&toolbar_window),
            || false,
            move || report.get() == Report::Running,
            || "Inspect saved project files",
            config,
        ),
    ))
    .style(|s| {
        s.width(34.0)
            .height_full()
            .flex_col()
            .items_center()
            .gap(2.0)
            .padding_top(5.0)
            .flex_shrink(0.0_f32)
    });

    let rows_window = window.clone();
    let list = sparrow_scroll(
        dyn_stack(
            move || groups.get().into_iter().enumerate(),
            |(index, (group, rows))| {
                (
                    *index,
                    group.clone(),
                    rows.iter()
                        .map(|issue| {
                            (
                                issue.path.clone(),
                                issue.diagnostic.range.start,
                                issue.diagnostic.range.end,
                                issue.rule(),
                                issue.diagnostic.message.clone(),
                                rank(&issue.diagnostic),
                                issue.offset,
                            )
                        })
                        .collect::<Vec<_>>(),
                )
            },
            move |(_, (group, rows))| {
                let name = match &group {
                    Group::File(path) => rows_window
                        .workspace
                        .path
                        .as_deref()
                        .and_then(|root| path.strip_prefix(root).ok())
                        .unwrap_or(path)
                        .display()
                        .to_string(),
                    Group::Rule(rule) => rule.replace('_', " "),
                };
                let count = rows.len();
                let group_in_style = group.clone();
                let group_in_click = group.clone();
                let row_window = rows_window.clone();
                stack((
                    stack((
                        svg(move || {
                            config.get().ui_svg(
                                if collapsed
                                    .with(|closed| closed.contains(&group_in_style))
                                {
                                    SparrowIcons::ITEM_CLOSED
                                } else {
                                    SparrowIcons::ITEM_OPENED
                                },
                            )
                        })
                        .style(|s| s.size(14.0, 14.0)),
                        label(move || name.clone())
                            .style(|s| s.min_width(0.0).text_ellipsis().font_bold()),
                        label(move || count.to_string()).style(move |s| {
                            s.color(
                                config
                                    .get()
                                    .color(SparrowColor::PANEL_FOREGROUND_DIM),
                            )
                        }),
                    ))
                    .style(|s| {
                        s.height(24.0)
                            .width_full()
                            .items_center()
                            .gap(7.0)
                            .padding_horiz(8.0)
                            .cursor(CursorStyle::Pointer)
                    })
                    .on_click_stop(move |_| {
                        collapsed.update(|closed| {
                            if !closed.remove(&group_in_click) {
                                closed.insert(group_in_click.clone());
                            }
                        });
                        if let Some(id) = view_id.get_untracked() {
                            id.request_focus();
                        }
                    }),
                    dyn_stack(
                        move || {
                            if collapsed.with(|closed| closed.contains(&group)) {
                                Vec::new()
                            } else {
                                rows.clone()
                            }
                        },
                        |issue| {
                            (
                                issue.path.clone(),
                                issue.diagnostic.range.start,
                                issue.diagnostic.range.end,
                                issue.rule(),
                                issue.diagnostic.message.clone(),
                            )
                        },
                        move |issue| {
                            row::row(
                                row_window.clone(),
                                issue,
                                selection,
                                excluded,
                                options,
                                view_id,
                            )
                        },
                    )
                    .style(|s| s.flex_col().width_full()),
                ))
                .style(|s| s.flex_col().width_full())
            },
        )
        .style(|s| s.flex_col().width_full()),
    )
    .scroll_to(move || {
        let issue = selection.get()?;
        let mut y = 0.0_f64;
        for (group, rows) in groups.get() {
            y += 24.0;
            if !collapsed.with(|closed| closed.contains(&group)) {
                if let Some(index) = rows.iter().position(|row| row == &issue) {
                    return Some(
                        (0.0, (y + index as f64 * 23.0 - 65.0).max(0.0)).into(),
                    );
                }
                y += rows.len() as f64 * 23.0;
            }
        }
        None
    })
    .style(move |s| {
        s.size_full()
            .apply_if(groups.with(|g| g.is_empty()), |s| s.hide())
    });

    let summary = label(move || {
        let counts = groups.with(|groups| {
            let mut counts = [0_u32; 4];
            for (_, rows) in groups {
                for issue in rows {
                    counts[rank(&issue.diagnostic)] += 1;
                }
            }
            counts
        });
        let breakdown = ["errors", "warnings", "information", "hints"]
            .into_iter()
            .zip(counts)
            .filter(|(_, count)| *count != 0)
            .map(|(severity, count)| format!("{count} {severity}"))
            .collect::<Vec<_>>()
            .join(" · ");
        format!("Dart inspection  {breakdown}")
    })
    .style(move |s| {
        s.height(27.0)
            .min_width(0.0)
            .padding_horiz(8.0)
            .items_center()
            .text_ellipsis()
            .color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
            .apply_if(
                tab.get() != Tab::Inspection
                    || groups.with(|groups| groups.is_empty()),
                |s| s.hide(),
            )
    });
    let empty_view = empty_state(window.clone(), groups, file, options, excluded);
    let side = window.panel.panels;
    let vertical = move || {
        super::data::panel_position(&side.get(), &PanelKind::Problems)
            .is_some_and(|(_, position)| !position.is_bottom())
    };
    let detail_window = window.clone();
    let details_view = dyn_container(
        move || (tab.get(), options.preview.get(), selection.get()),
        move |(tab, preview_open, issue)| match (tab, preview_open, issue) {
            (_, true, Some(issue)) => {
                preview::editor_preview(detail_window.clone(), issue).into_any()
            }
            (Tab::Inspection, _, Some(issue)) => {
                details::details(detail_window.clone(), issue, selection).into_any()
            }
            (Tab::Inspection, _, None) => {
                label(|| "Select a finding to see its details.")
                    .style(|s| s.padding(12.0).min_width(0.0).text_ellipsis())
                    .into_any()
            }
            _ => empty().into_any(),
        },
    )
    .style(move |s| {
        let show = tab.get() == Tab::Inspection
            || (options.preview.get() && selection.get().is_some());
        s.apply_if(!show, |s| s.hide())
            .apply_if(vertical(), |s| {
                s.width_full().height_pct(46.0).border_top(1.0)
            })
            .apply_if(!vertical(), |s| {
                s.width_pct(46.0).height_full().border_left(1.0)
            })
            .flex_shrink(0.0_f32)
            .min_width(0.0)
            .min_height(0.0)
            .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
    });

    let navigator = window.clone();
    let content = stack((
        toolbar,
        stack((
            stack((
                summary,
                stack((list, empty_view)).style(|s| s.flex_grow(1.0_f32).flex_basis(0.0).width_full().min_height(0.0)),
            )).style(|s| s.flex_col().flex_grow(1.0_f32).flex_basis(0.0).min_width(0.0).min_height(0.0).size_full()),
            details_view,
        )).style(move |s| s.flex_grow(1.0_f32).min_width(0.0).height_full().apply_if(vertical(), |s| s.flex_col())),
    )).style(|s| s.flex_grow(1.0_f32).flex_basis(0.0).min_height(0.0).width_full())
        .keyboard_navigable()
        .on_event(EventListener::KeyDown, move |event| {
            let Event::KeyDown(key) = event else { return EventPropagation::Continue; };
            if key.modifiers.alt() && key.key.logical_key == Key::Named(NamedKey::Enter) {
                if let Some(issue) = selection.get_untracked() {
                    details::show_actions(navigator.clone(), issue, selection, false);
                    return EventPropagation::Stop;
                }
            }
            if key.modifiers.control() && matches!(&key.key.logical_key, Key::Character(ch) if ch.as_str() == "c") {
                if let Some(issue) = selection.get_untracked() {
                    crate::menus::copy_text(&issue.diagnostic.message);
                    return EventPropagation::Stop;
                }
            }
            if !key.modifiers.is_empty() { return EventPropagation::Continue; }
            match key.key.logical_key {
                Key::Named(NamedKey::ArrowDown | NamedKey::ArrowUp | NamedKey::Home | NamedKey::End) => {
                    let rows = visible.get_untracked();
                    if !rows.is_empty() {
                        let current = selection.get_untracked().and_then(|issue| rows.iter().position(|row| row == &issue));
                        let index = match key.key.logical_key {
                            Key::Named(NamedKey::Home) => 0,
                            Key::Named(NamedKey::End) => rows.len() - 1,
                            Key::Named(NamedKey::ArrowUp) => current.unwrap_or(0).saturating_sub(1),
                            _ => current.map_or(0, |index| (index + 1).min(rows.len() - 1)),
                        };
                        selection.set(Some(rows[index].clone()));
                    }
                }
                Key::Named(NamedKey::Enter | NamedKey::F4) => {
                    if let Some(issue) = selection.get_untracked() { jump(&navigator, &issue, options.preview_tab.get_untracked()); }
                }
                Key::Named(NamedKey::Delete) if tab.get_untracked() == Tab::Inspection => {
                    if let Some(issue) = selection.get_untracked() { excluded.update(|rows| rows.push(issue)); }
                }
                _ => return EventPropagation::Continue,
            }
            EventPropagation::Stop
        });
    view_id.set(Some(content.id()));
    create_effect(move |_| {
        if focus.get() == Focus::Panel(PanelKind::Problems) {
            floem::action::exec_after(std::time::Duration::ZERO, move |_| {
                if focus.try_get_untracked()
                    == Some(Focus::Panel(PanelKind::Problems))
                {
                    if let Some(Some(id)) = view_id.try_get_untracked() {
                        id.request_focus();
                    }
                }
            });
        }
    });

    let header_window = window.clone();
    let hide = window.panel.clone();
    let header = panel_title_bar(
        "Problems",
        tabs,
        stack((
            clickable_icon(
                || SparrowIcons::WINDOW_MORE,
                || {},
                || false,
                || false,
                || "Problems options",
                config,
            )
            .ide_popout_menu(move || {
                options::panel_options(&header_window, options, excluded)
            }),
            clickable_icon(
                || SparrowIcons::WINDOW_MINIMIZE,
                move || hide.hide_panel(&PanelKind::Problems),
                || false,
                || false,
                || "Hide Problems",
                config,
            ),
        )),
    )
    .style(move |s| {
        s.border_bottom(1.0)
            .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
    });
    stack((header, content)).style(|s| s.absolute().size_full().flex_col())
}

fn empty_state(
    window: Rc<WindowTabData>,
    groups: floem::reactive::Memo<Vec<(Group, Vec<Issue>)>>,
    file: floem::reactive::Memo<Option<PathBuf>>,
    options: Options,
    excluded: RwSignal<Vec<Issue>>,
) -> impl View {
    let config = window.common.config;
    let inspection = window.inspections;
    let tab = inspection.tab;
    let report = inspection.report;
    let start = window.clone();
    stack((
        label(move || match tab.get() {
            Tab::Project => {
                if options.show.get()[0] {
                    "No live project errors.".into()
                } else {
                    "Project errors are hidden by the current filter.".into()
                }
            }
            Tab::File => {
                let path = file.get();
                match path.as_deref().and_then(|path| path.file_name()) {
                    Some(name) => {
                        format!("No visible problems in {}", name.to_string_lossy())
                    }
                    None => "Open a file to see its problems.".into(),
                }
            }
            Tab::Inspection => report.with(|report| match report {
                Report::Running => "Inspecting saved project files…".into(),
                Report::Cancelled => "Project inspection cancelled.".into(),
                Report::Failed(error) => error.clone(),
                Report::Finished(rows) if rows.is_empty() => {
                    "No problems found in saved project files.".into()
                }
                _ => "No findings match the current filters.".into(),
            }),
        })
        .style(move |s| {
            s.width_full()
                .text_align(floem::text::Align::Center)
                .color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
        }),
        crate::widgets::button(
            || "Inspect Code…",
            crate::widgets::ButtonKind::Outlined,
            config,
        )
        .on_click_stop(move |_| inspection.start(&start))
        .style(move |s| {
            s.apply_if(
                tab.get() == Tab::File || report.get() == Report::Running,
                |s| s.hide(),
            )
        }),
        crate::widgets::button(
            || "Reset filters",
            crate::widgets::ButtonKind::Outlined,
            config,
        )
        .on_click_stop(move |_| {
            options.show.set([true; 4]);
            excluded.set(Vec::new());
        })
        .style(move |s| {
            s.apply_if(
                options.show.get() == [true; 4]
                    && excluded.with(|rows| rows.is_empty()),
                |s| s.hide(),
            )
        }),
    ))
    .style(move |s| {
        s.absolute()
            .size_full()
            .padding(14.0)
            .flex_col()
            .justify_center()
            .items_center()
            .gap(10.0)
            .apply_if(groups.with(|groups| !groups.is_empty()), |s| s.hide())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspection_groups_by_rule_and_filters_severity() {
        let issue = |path: &str, rule: &str, severity| Issue {
            path: PathBuf::from(path),
            diagnostic: Diagnostic {
                code: Some(lsp_types::NumberOrString::String(rule.into())),
                severity: Some(severity),
                ..Default::default()
            },
            offset: None,
        };
        let rows = vec![
            issue(
                "a.dart",
                "unused_local_variable",
                DiagnosticSeverity::WARNING,
            ),
            issue(
                "b.dart",
                "unused_local_variable",
                DiagnosticSeverity::WARNING,
            ),
            issue("b.dart", "undefined_identifier", DiagnosticSeverity::ERROR),
        ];
        let by_rule = grouped(rows.clone(), [true; 4], true, false);
        assert_eq!(by_rule.len(), 2);
        assert_eq!(by_rule[1].1.len(), 2);
        let errors = grouped(rows, [true, false, false, false], false, false);
        assert!(
            matches!(&errors[0].0, Group::File(path) if path == &PathBuf::from("b.dart"))
        );
        assert_eq!(errors[0].1.len(), 1);
    }
}
