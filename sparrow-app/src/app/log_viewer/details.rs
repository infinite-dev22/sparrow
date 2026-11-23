//! Inspection pane for a selected log entry.

use std::sync::Arc;

use floem::{
    IntoView, View,
    reactive::{Memo, ReadSignal, RwSignal, SignalGet, SignalUpdate},
    views::{
        Decorators, container, dyn_container,
        editor::{core::register::Clipboard, text::SystemClipboard},
        label, stack, stack_from_iter,
    },
};

use crate::{
    config::{SparrowConfig, color::SparrowColor},
    widgets::{
        ButtonKind, Divider, button, button_with_content, empty_state,
        sparrow_scroll, tab_strip,
    },
};

use super::entry::Entry;

const CONTEXT_ROWS: usize = 3;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum DetailTab {
    Overview,
    Technical,
    Context,
    Raw,
}

impl DetailTab {
    const ALL: [Self; 4] =
        [Self::Overview, Self::Technical, Self::Context, Self::Raw];

    fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Technical => "Technical",
            Self::Context => "Context",
            Self::Raw => "Raw",
        }
    }
}

type Config = ReadSignal<Arc<SparrowConfig>>;

pub(super) fn pane(
    selection: RwSignal<Option<Arc<Entry>>>,
    entries: Memo<Vec<Arc<Entry>>>,
    file: Memo<Option<String>>,
    tab: RwSignal<DetailTab>,
    query: RwSignal<String>,
    reveal: impl Fn(Arc<Entry>) + Clone + 'static,
    config: Config,
) -> impl View {
    let tabs = tab_strip(|strip| {
        stack_from_iter(DetailTab::ALL.map(|page| {
            strip.button(
                label(move || page.label()).style(|s| s.selectable(false)),
                move || tab.get() == page,
                move || tab.set(page),
                config,
            )
        }))
    });
    let controls = stack((
        label(|| "Log details").style(|s| s.font_bold().flex_grow(1.0_f32)),
        button(
            move || match tab.get() {
                DetailTab::Overview => "Copy summary",
                DetailTab::Technical => "Copy JSON",
                DetailTab::Context | DetailTab::Raw => "Copy entry",
            },
            ButtonKind::Plain,
            config,
        )
        .on_click_stop(move |_| {
            if let Some(entry) = selection.get_untracked() {
                let file = file.get_untracked();
                let value = match tab.get_untracked() {
                    DetailTab::Overview => copy_summary(&entry, file.as_deref()),
                    DetailTab::Technical => copy_technical(&entry, file.as_deref()),
                    DetailTab::Context | DetailTab::Raw => entry.raw.clone(),
                };
                SystemClipboard::new().put_string(&value);
            }
        }),
        button(|| "Close details", ButtonKind::Plain, config)
            .on_click_stop(move |_| selection.set(None)),
    ))
    .style(|s| {
        s.width_full()
            .items_center()
            .gap(6.0)
            .flex_wrap(floem::style::FlexWrap::Wrap)
    });

    let contents = dyn_container(
        move || {
            let tab = tab.get();
            let context = (tab == DetailTab::Context).then(|| entries.get());
            (tab, selection.get(), context)
        },
        move |(tab, selection, neighbors)| {
            let Some(entry) = selection else {
                return empty_state(|| "Select a log entry to inspect it", config)
                    .into_any();
            };
            let reveal = reveal.clone();
            let page = match tab {
                DetailTab::Overview => overview(entry, config).into_any(),
                DetailTab::Technical => {
                    technical(entry, file.get_untracked(), query, config).into_any()
                }
                DetailTab::Context => {
                    context(entry, neighbors.unwrap_or_default(), reveal, config)
                        .into_any()
                }
                DetailTab::Raw => code_block(entry.raw.clone(), config).into_any(),
            };
            sparrow_scroll(page)
                .style(|s| s.size_full().min_height(0.0).min_width(0.0))
                .into_any()
        },
    )
    .style(|s| {
        s.width_full()
            .min_height(0.0)
            .flex_grow(1.0_f32)
            .flex_basis(0.0)
    });

    stack((
        controls,
        tabs.style(|s| s.width_full().flex_shrink(0.0_f32)),
        Divider::new(config).into_view(),
        contents,
    ))
    .style(move |s| {
        s.flex_col()
            .min_width(0.0)
            .min_height(0.0)
            .padding(12.0)
            .gap(8.0)
            .border_radius(6.0)
            .border(1.0)
            .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
            .background(config.get().color(SparrowColor::PANEL_BACKGROUND))
    })
}

fn overview(entry: Arc<Entry>, config: Config) -> impl View {
    let (title, explanation) = entry.overview();
    let level = entry.level;
    let time = entry
        .timestamp_label()
        .unwrap_or_else(|| "Time unavailable".into());
    let message = entry.summary.clone();
    stack((
        stack((
            label(move || level.label()).style(move |s| {
                s.font_bold().color(config.get().color(level.color()))
            }),
            label(move || time.clone()).style(move |s| {
                s.color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
                    .selectable(true)
            }),
        ))
        .style(|s| {
            s.width_full()
                .items_center()
                .gap(8.0)
                .flex_wrap(floem::style::FlexWrap::Wrap)
        }),
        label(move || title).style(|s| s.font_size(17.0).font_bold().width_full()),
        label(move || message.clone()).style(move |s| {
            let cfg = config.get();
            s.width_full()
                .min_width(0.0)
                .selectable(true)
                .padding(10.0)
                .border_radius(5.0)
                .background(cfg.color(SparrowColor::EDITOR_BACKGROUND))
        }),
        label(move || explanation.unwrap_or_default()).style(move |s| {
            s.width_full()
                .color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
                .apply_if(explanation.is_none(), |s| s.hide())
        }),
    ))
    .style(|s| s.flex_col().width_full().min_width(0.0).gap(10.0))
}

fn technical(
    entry: Arc<Entry>,
    file: Option<String>,
    query: RwSignal<String>,
    config: Config,
) -> impl View {
    let fields = metadata(&entry, file.as_deref(), true);
    let filter = entry.component.clone();
    let has_component = filter.is_some();
    let body = entry.continuation().unwrap_or_default().to_owned();
    let has_body = !body.is_empty();
    let continuation_label =
        if entry.component.as_deref() == Some("sparrow_app::panic_hook") {
            "Backtrace"
        } else {
            "Additional output"
        };

    stack((
        stack_from_iter(fields.into_iter().map(move |(name, value)| {
            stack((
                label(move || name).style(move |s| {
                    s.width(94.0).flex_shrink(0.0_f32).color(
                        config.get().color(SparrowColor::PANEL_FOREGROUND_DIM),
                    )
                }),
                label(move || value.clone()).style(|s| {
                    s.flex_grow(1.0_f32)
                        .flex_basis(0.0)
                        .min_width(0.0)
                        .selectable(true)
                }),
            ))
            .style(|s| s.width_full().items_start().gap(8.0))
        }))
        .style(|s| s.flex_col().width_full().gap(6.0)),
        button(|| "Filter by component", ButtonKind::Plain, config)
            .on_click_stop(move |_| {
                if let Some(component) = &filter {
                    query.set(component.clone());
                }
            })
            .style(move |s| {
                s.align_self(Some(floem::style::AlignItems::FlexStart))
                    .apply_if(!has_component, |s| s.hide())
            }),
        stack((
            label(move || continuation_label).style(|s| s.font_bold()),
            code_block(body, config),
        ))
        .style(move |s| {
            s.flex_col()
                .width_full()
                .gap(6.0)
                .apply_if(!has_body, |s| s.hide())
        }),
    ))
    .style(|s| s.flex_col().width_full().min_width(0.0).gap(10.0))
}

fn context(
    selected: Arc<Entry>,
    entries: Vec<Arc<Entry>>,
    reveal: impl Fn(Arc<Entry>) + Clone + 'static,
    config: Config,
) -> impl View {
    let neighbors = entries
        .iter()
        .position(|entry| Arc::ptr_eq(entry, &selected))
        .map(|index| {
            let start = index.saturating_sub(CONTEXT_ROWS);
            let end = (index + CONTEXT_ROWS + 1).min(entries.len());
            entries[start..end].to_vec()
        })
        .unwrap_or_default();
    let empty = neighbors.len() < 2;
    stack((
        label(move || {
            if empty {
                "No neighboring entries in this excerpt."
            } else {
                "Nearby entries in this file, newest first"
            }
        })
        .style(move |s| {
            s.color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
        }),
        stack_from_iter(neighbors.into_iter().map(move |entry| {
            let reveal = reveal.clone();
            let current = Arc::ptr_eq(&selected, &entry);
            let time = entry.time.clone();
            let level = entry.level;
            let text = entry.summary.clone();
            button_with_content(
                label(move || format!("{time}  {}  {text}", level.label())).style(
                    |s| {
                        s.width_full()
                            .min_width(0.0)
                            .text_ellipsis()
                            .selectable(false)
                    },
                ),
                ButtonKind::Plain,
                config,
            )
            .on_click_stop(move |_| reveal(entry.clone()))
            .style(move |s| {
                s.width_full()
                    .min_width(0.0)
                    .justify_start()
                    .padding(6.0)
                    .apply_if(current, |s| {
                        s.background(
                            config
                                .get()
                                .color(SparrowColor::PANEL_SELECTION_BACKGROUND),
                        )
                    })
            })
        }))
        .style(|s| s.flex_col().gap(4.0).width_full()),
    ))
    .style(|s| s.flex_col().gap(8.0).width_full())
}

fn code_block(text: String, config: Config) -> impl View {
    container(label(move || text.clone()).style(move |s| {
        s.width_full()
            .min_width(0.0)
            .selectable(true)
            .font_family(config.get().terminal.font_family.clone())
            .font_size(12.0)
    }))
    .style(move |s| {
        s.width_full()
            .min_width(0.0)
            .padding(9.0)
            .border_radius(5.0)
            .background(config.get().color(SparrowColor::EDITOR_BACKGROUND))
    })
}

fn metadata(
    entry: &Entry,
    file: Option<&str>,
    compact_time: bool,
) -> Vec<(&'static str, String)> {
    let mut fields = Vec::new();
    if let Some(time) = entry.timestamp {
        fields.push((
            "Timestamp",
            if compact_time {
                time.format("%Y-%m-%d %H:%M:%S%.3f UTC").to_string()
            } else {
                time.to_rfc3339()
            },
        ));
    }
    fields.push(("Severity", entry.level.label().into()));
    if let Some(component) = &entry.component {
        fields.push(("Component", component.clone()));
    }
    fields.extend(entry.fields.iter().map(|(name, value)| {
        let name = if compact_time && *name == "Sparrow version" {
            "IDE version"
        } else {
            *name
        };
        (name, value.clone())
    }));
    if let Some(file) = file {
        fields.push(("Log file", file.into()));
    }
    fields
}

fn copy_summary(entry: &Entry, file: Option<&str>) -> String {
    let (title, _) = entry.overview();
    let mut lines = vec![title.to_owned(), entry.summary.clone()];
    if let Some(time) = entry.timestamp_label() {
        lines.push(time);
    }
    if let Some(file) = file {
        lines.push(file.into());
    }
    lines.join("\n")
}

fn copy_technical(entry: &Entry, file: Option<&str>) -> String {
    let fields: std::collections::BTreeMap<_, _> =
        metadata(entry, file, false).into_iter().collect();
    serde_json::to_string_pretty(&serde_json::json!({
        "fields": fields,
        "message": entry.summary,
        "output": entry.continuation(),
    }))
    .expect("log details contain only strings")
}
