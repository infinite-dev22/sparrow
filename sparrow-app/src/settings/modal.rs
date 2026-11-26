//! Reference-style modal settings with deferred, validated edits.
use super::{SettingsData, SettingsItem, SettingsValue};
use crate::widgets::{TextField, m3_from_config, modal_focus::ModalFocus};
use crate::{
    config::{SparrowConfig, color::SparrowColor, icon::SparrowIcons},
    panel::kind::PanelKind,
    widgets::{Divider, breadcrumb_item, floating_search::search_field_with_style},
    window_tab::WindowTabData,
};
use floem::{
    View,
    event::EventListener,
    reactive::{
        RwSignal, Scope, SignalGet, SignalUpdate, SignalWith, batch, use_context,
    },
    views::{Decorators, container, dyn_stack, empty, label, stack},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
    sync::Arc,
};
mod accounts;
mod catalog;
mod form;
mod layout;
mod menu_editor;
mod navigation;
mod setting_row;
mod tokens;

use crate::widgets::sparrow_scroll;
use navigation::navigation_tree;
use setting_row::setting_row;

use catalog::{PAGES, Page, owner};

fn visible_pages(expanded: &BTreeSet<Page>, matching: Option<&[Page]>) -> Vec<Page> {
    PAGES
        .iter()
        .map(|&(page, ..)| page)
        .filter(|&page| match matching {
            Some(matching) => {
                matching.contains(&page)
                    || matching
                        .iter()
                        .any(|item| item.ancestors().any(|parent| parent == page))
            }
            None => page.ancestors().all(|parent| expanded.contains(&parent)),
        })
        .collect()
}

#[derive(Clone)]
enum Type {
    Reset,
    String,
    StringList,
    Integer,
    Float,
    Bool,
}
#[derive(Clone)]
struct Edit {
    raw: String,
    kind: Type,
}
impl Edit {
    fn value(&self, section: &str) -> Result<toml_edit::Value, String> {
        match self.kind {
            Type::Reset => Err("An inherited setting has no stored value".into()),
            Type::String => Ok(self.raw.clone().into()),
            Type::StringList => {
                let items = serde_json::from_str::<Vec<String>>(&self.raw)
                    .map_err(|error| error.to_string())?;
                let array = items.into_iter().collect::<toml_edit::Array>();
                Ok(toml_edit::Value::Array(array))
            }
            Type::Bool => self
                .raw
                .parse::<bool>()
                .map(Into::into)
                .map_err(|_| "Expected true or false.".into()),
            Type::Integer => {
                let n = self
                    .raw
                    .trim()
                    .parse::<i64>()
                    .map_err(|_| "Expected a whole number.")?;
                if n < 0 && ["core", "ui", "editor", "terminal"].contains(&section) {
                    return Err(
                        "This setting requires a non-negative number.".into()
                    );
                }
                Ok(n.into())
            }
            Type::Float => {
                let n = self
                    .raw
                    .trim()
                    .parse::<f64>()
                    .map_err(|_| "Expected a number.")?;
                if !n.is_finite() {
                    return Err("Expected a finite number.".into());
                }
                if n < 0.0 && ["core", "ui", "editor", "terminal"].contains(&section)
                {
                    return Err(
                        "This setting requires a non-negative number.".into()
                    );
                }
                Ok(n.into())
            }
        }
    }
}
type Pending = BTreeMap<(String, String), Edit>;
fn initial(value: &SettingsValue) -> Option<(String, Type)> {
    match value {
        SettingsValue::String(s) => Some((s.clone(), Type::String)),
        SettingsValue::Bool(b) => Some((b.to_string(), Type::Bool)),
        SettingsValue::Integer(n) => Some((n.to_string(), Type::Integer)),
        SettingsValue::Float(n) => Some((n.to_string(), Type::Float)),
        SettingsValue::Dropdown(d) => Some((
            d.items
                .get(d.active_index)
                .or_else(|| d.items.last())
                .cloned()
                .unwrap_or_default(),
            Type::String,
        )),
        _ => None,
    }
}
fn validated_draft_value(
    section: &str,
    key: &str,
    edit: &Edit,
) -> Result<Option<toml_edit::Value>, String> {
    if matches!(edit.kind, Type::Reset) {
        return Ok(None);
    }
    let value = edit.value(section)?;
    if section == "core"
        && ["flutter-sdk-path", "dart-sdk-path"].contains(&key)
        && !edit.raw.trim().is_empty()
    {
        let path = std::path::Path::new(edit.raw.trim());
        if !path.is_absolute() {
            return Err("Use an absolute SDK directory path.".into());
        }
        crate::dart::sdk::validate_sdk_root(path, key == "flutter-sdk-path")?;
    }
    Ok(Some(value))
}
fn belongs(item: &SettingsItem, page: Page) -> bool {
    owner(&item.kind, &item.field) == page
}

struct HighlightMatcher(regex::Regex);

impl PartialEq for HighlightMatcher {
    fn eq(&self, other: &Self) -> bool {
        self.0.as_str() == other.0.as_str()
    }
}

fn matches_query(text: &str, query: &str) -> bool {
    let text = text.to_lowercase();
    query
        .split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .all(|token| text.contains(token))
}

fn matches_setting(item: &SettingsItem, query: &str) -> bool {
    matches_query(
        &format!(
            "{} {} {} {}",
            item.kind, item.field, item.name, item.description
        ),
        query,
    )
}

fn matching_pages(items: &[SettingsItem], query: &str) -> Vec<Page> {
    PAGES
        .iter()
        .map(|&(page, ..)| page)
        .filter(|&page| {
            matches_query(page.search_terms(), query)
                || items
                    .iter()
                    .any(|item| belongs(item, page) && matches_setting(item, query))
        })
        .collect()
}

fn search_page(selected: Page, matches: Option<&[Page]>) -> Page {
    match matches {
        Some([only_match])
            if !only_match.ancestors().any(|ancestor| ancestor == selected) =>
        {
            *only_match
        }
        _ => selected,
    }
}

fn modified_pages(active: &Pending, inactive: &Pending) -> Vec<Page> {
    let changed: BTreeSet<_> = active
        .keys()
        .chain(inactive.keys())
        .map(|(kind, field)| owner(kind, field))
        .collect();
    PAGES
        .iter()
        .filter_map(|&(page, ..)| changed.contains(&page).then_some(page))
        .collect()
}

fn select_page(
    page: Page,
    current: RwSignal<Page>,
    history: RwSignal<Vec<Page>>,
    index: RwSignal<usize>,
) {
    if current.get_untracked() == page {
        return;
    }
    batch(|| {
        history.update(|h| {
            h.truncate(index.get_untracked() + 1);
            h.push(page);
        });
        index.set(history.with_untracked(Vec::len) - 1);
        current.set(page);
    });
}

fn navigate(
    page: Page,
    current: RwSignal<Page>,
    history: RwSignal<Vec<Page>>,
    index: RwSignal<usize>,
    query: RwSignal<String>,
) {
    select_page(page, current, history, index);
    query.set(String::new());
}
pub fn view(window: Rc<WindowTabData>) -> impl View {
    let cx = Scope::current();
    let config = window.common.config;
    let visible = window.settings_modal_visible;
    let keyboard = ModalFocus::default();
    let can_project =
        window.workspace.path.is_some() && window.workspace.kind.is_local();
    let project_scope = cx.create_rw_signal(false);
    let debugger_pending = cx.create_rw_signal(Pending::new());
    let no_reset = cx.create_rw_signal(false);
    let scope_drafts = cx.create_rw_signal([Pending::new(), Pending::new()]);
    let settings_config = cx.create_rw_signal(config.get_untracked());
    let mut settings_common = (*window.common).clone();
    settings_common.config = settings_config.read_only();
    let data =
        SettingsData::new(cx, window.plugin.installed, Rc::new(settings_common));
    cx.create_effect({
        let window = window.clone();
        move |_| {
            let _ = config.get();
            let project = project_scope.get();
            if !visible.get() {
                return;
            }
            let mut workspace = (*window.workspace).clone();
            if !project {
                workspace.path = None;
            }
            settings_config
                .set(std::sync::Arc::new(window.config_for_workspace(&workspace)));
        }
    });
    let items = data.items;
    let plugins = data.plugin_items;
    let pending = cx.create_rw_signal(Pending::new());
    let query = window.settings_search_query;
    let db = use_context::<Arc<crate::db::SparrowDb>>();
    let initial_page = db
        .as_ref()
        .and_then(|db| db.settings_page().ok())
        .and_then(|id| PAGES.iter().find(|(page, ..)| page.id() == id))
        .map(|&(page, ..)| page)
        .unwrap_or(Page::Appearance);
    let page = cx.create_rw_signal(initial_page);
    cx.create_effect(move |previous: Option<Page>| {
        let selected = page.get();
        if previous.is_some_and(|previous| previous != selected)
            && let Some(db) = &db
        {
            db.save_settings_page(selected.id());
        }
        selected
    });
    let expanded = cx.create_rw_signal(BTreeSet::new());
    cx.create_effect(move |_| {
        let ancestors: Vec<_> = page.get().ancestors().collect();
        expanded.update(|expanded| expanded.extend(ancestors));
    });
    let history = cx.create_rw_signal(vec![initial_page]);
    let index = cx.create_rw_signal(0usize);
    let epoch = cx.create_rw_signal(0usize);
    let error = cx.create_rw_signal(String::new());
    let help_open = cx.create_rw_signal(false);
    let (search, search_id) = keyboard.text_field_with_id(
        TextField::new(query).colors(move || m3_from_config(config)),
    );
    window.settings_search_input.set(Some(search_id));
    let root_id = window.common.view_id;
    let close = move || {
        visible.set(false);
        root_id.get_untracked().request_focus();
    };
    cx.create_effect(move |_| {
        let is_open = visible.get();
        batch(|| {
            pending.set(Pending::new());
            debugger_pending.set(Pending::new());
            scope_drafts.set([Pending::new(), Pending::new()]);
            query.set(String::new());
            error.set(String::new());
            help_open.set(false);
        });
        if is_open {
            epoch.update(|n| *n += 1);
            floem::action::exec_after(
                std::time::Duration::from_millis(30),
                move |_| {
                    if visible.get_untracked() {
                        search_id.request_focus();
                    }
                },
            );
        }
    });
    let all = cx.create_rw_signal(Vec::<SettingsItem>::new());
    let debugger_settings = window.common.debugger_settings;
    let debugger_items = can_project
        .then(|| {
            crate::debug_settings::OPTIONS
                .iter()
                .map(|&(key, title, accessor)| {
                    (
                        accessor,
                        SettingsItem {
                            kind: "debugger".into(),
                            field: key.into(),
                            name: format!("Debugger: {title}"),
                            description: String::new(),
                            filter_text: String::new(),
                            value: SettingsValue::Empty,
                            serde_value: serde_json::Value::Null,
                            pos: cx.create_rw_signal(floem::kurbo::Point::ORIGIN),
                            size: cx.create_rw_signal(floem::kurbo::Size::ZERO),
                            header: false,
                        },
                    )
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    cx.create_effect(move |_| {
        let mut combined = items.get();
        combined.extend(plugins.get());
        if can_project {
            let mut values = debugger_settings.get();
            combined.extend(debugger_items.iter().map(|(accessor, item)| {
                let mut item = item.clone();
                let value = *accessor(&mut values);
                item.value = SettingsValue::Bool(value);
                item.serde_value = value.into();
                item
            }));
        }
        all.set(
            combined
                .into_iter()
                .filter(|item| !item.header && initial(&item.value).is_some())
                .collect(),
        );
    });
    let search_query = cx.create_memo(move |_| {
        query.with(|query| {
            query
                .chars()
                .any(char::is_alphanumeric)
                .then(|| query.to_lowercase())
        })
    });
    let highlight_matcher = cx.create_memo(move |_| {
        search_query.with(|pattern| {
            pattern.as_ref().and_then(|pattern| {
                let terms = pattern
                    .split(|character: char| !character.is_alphanumeric())
                    .filter(|term| !term.is_empty())
                    .map(regex::escape)
                    .collect::<Vec<_>>()
                    .join("|");
                regex::RegexBuilder::new(&terms)
                    .case_insensitive(true)
                    .build()
                    .ok()
                    .map(HighlightMatcher)
            })
        })
    });
    let matching = cx.create_memo(move |_| {
        search_query.with(|pattern| {
            pattern
                .as_ref()
                .map(|pattern| all.with(|items| matching_pages(items, pattern)))
        })
    });
    let displayed_page = cx.create_memo(move |_| {
        matching.with(|matches| search_page(page.get(), matches.as_deref()))
    });
    let shown_pages = cx.create_memo(move |_| {
        matching.with(|matching| {
            expanded.with(|expanded| visible_pages(expanded, matching.as_deref()))
        })
    });
    let modifications = cx.create_memo(move |_| {
        pending.with(|active| {
            scope_drafts.with(|drafts| {
                let mut pages = modified_pages(
                    active,
                    &drafts[usize::from(!project_scope.get())],
                );
                if !debugger_pending.with(Pending::is_empty) {
                    pages.push(Page::Debugger);
                }
                pages
            })
        })
    });
    let sections =
        cx.create_rw_signal(Vec::<(&'static str, Vec<SettingsItem>)>::new());
    cx.create_effect(move |_| {
        if !visible.get() {
            sections.set(Vec::new());
            return;
        }
        let selected = displayed_page.get();
        let items = search_query.with(|pattern| {
            let only_current = matching.with(|pages| {
                pages
                    .as_ref()
                    .is_some_and(|pages| pages.contains(&selected))
            });
            all.with(|items| {
                items
                    .iter()
                    .filter(|item| match pattern {
                        None => belongs(item, selected),
                        Some(pattern) if only_current => {
                            belongs(item, selected)
                                && (matches_query(selected.search_terms(), pattern)
                                    || matches_setting(item, pattern))
                        }
                        Some(pattern) => matches_setting(item, pattern),
                    })
                    .cloned()
                    .collect()
            })
        });
        sections.set(form::sections(items, search_query.with(Option::is_some)));
    });
    let dirty = cx.create_memo(move |_| {
        !pending.with(Pending::is_empty)
            || !debugger_pending.with(Pending::is_empty)
            || scope_drafts
                .with(|drafts| !drafts[usize::from(!project_scope.get())].is_empty())
    });
    let scope_label = cx.create_rw_signal(String::new());
    cx.create_effect(move |_| {
        scope_label.set(
            if project_scope.get() {
                "Settings for: This project"
            } else {
                "Settings for: IDE"
            }
            .into(),
        );
    });
    let scopes = if can_project {
        vec![
            "Settings for: IDE".into(),
            "Settings for: This project".into(),
        ]
    } else {
        vec!["Settings for: IDE".into()]
    };
    let scope_picker = keyboard.control(
        crate::widgets::dropdown(scope_label, scopes, config).on_accept(
            move |selected| {
                let project = selected == "Settings for: This project";
                if project == project_scope.get_untracked() {
                    return;
                }
                batch(|| {
                    scope_drafts.update(|drafts| {
                        drafts[usize::from(project_scope.get_untracked())] =
                            pending.get_untracked();
                    });
                    pending.set(scope_drafts.with_untracked(|drafts| {
                        drafts[usize::from(project)].clone()
                    }));
                    project_scope.set(project);
                    error.set(String::new());
                    epoch.update(|epoch| *epoch += 1);
                });
            },
        ),
    );
    let apply: Rc<dyn Fn() -> bool> = {
        let window = window.clone();
        Rc::new(move || {
            if !dirty.get_untracked() {
                error.set(String::new());
                return true;
            }
            let mut drafts = scope_drafts.get_untracked();
            drafts[project_scope.get_untracked() as usize] = pending.get_untracked();
            let result = (|| {
                let mut files = drafts
                    .into_iter()
                    .enumerate()
                    .filter(|(_, edits)| !edits.is_empty())
                    .map(|(scope, edits)| {
                        let path = SparrowConfig::scoped_settings_file(
                            window.workspace.path.as_deref(),
                            scope == 1,
                        )?;
                        let scope = if scope == 0 { "IDE" } else { "Project" };
                        let values = edits
                            .into_iter()
                            .map(|((section, key), edit)| {
                                let value =
                                    validated_draft_value(&section, &key, &edit)
                                        .map_err(|error| {
                                            format!(
                                                "{scope} / {section}.{key}: {error}"
                                            )
                                        })?;
                                Ok((section, key, value))
                            })
                            .collect::<Result<Vec<_>, String>>()?;
                        Ok((path, values))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                let debugger = debugger_pending.get_untracked();
                if !debugger.is_empty() {
                    let path = window
                        .workspace
                        .path
                        .as_ref()
                        .ok_or("Open a local project to edit debugger settings")?
                        .join(".sparrow/debugger.toml");
                    let values = debugger
                        .into_iter()
                        .map(|((section, key), edit)| {
                            let value =
                                validated_draft_value(&section, &key, &edit)?;
                            Ok((String::new(), key, value))
                        })
                        .collect::<Result<Vec<_>, String>>()?;
                    files.push((path, values));
                }
                SparrowConfig::update_scoped_settings(files)
            })();
            match result {
                Ok(()) => {
                    if !debugger_pending.with_untracked(Pending::is_empty) {
                        let settings = crate::debug_settings::load(
                            window.workspace.path.as_deref(),
                        );
                        crate::debug_settings::apply_to_window(&window, settings);
                        debugger_pending.set(Pending::new());
                    }
                    pending.set(Pending::new());
                    scope_drafts.set([Pending::new(), Pending::new()]);
                    error.set(String::new());
                    window.reload_config();
                    epoch.update(|epoch| *epoch += 1);
                    true
                }
                Err(e) => {
                    error.set(e);
                    false
                }
            }
        })
    };
    let accept = {
        let apply = apply.clone();
        move || {
            if apply() {
                close();
            }
        }
    };
    keyboard.set_submit(accept.clone());
    let save = apply;
    let show_page_actions = move || {
        search_query.with(|query| {
            query.as_ref().is_none_or(|query| {
                matches_query(displayed_page.get().search_terms(), query)
            })
        })
    };
    let overview_keyboard = keyboard.clone();
    let overview = dyn_stack(
        move || {
            let selected = displayed_page.get();
            if visible.get()
                && (show_page_actions()
                    || matching.with(|matches| {
                        matches.as_deref().is_some_and(|matches| {
                            matches.iter().any(|page| {
                                page.ancestors().any(|ancestor| ancestor == selected)
                            })
                        })
                    }))
            {
                selected
                    .summary()
                    .map(|summary| (selected, summary))
                    .into_iter()
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            }
        },
        |(page, _)| *page,
        move |(selected, summary)| {
            let keyboard = overview_keyboard.clone();
            stack((
                label(move || summary).style(|s| s.width_full()),
                dyn_stack(
                    move || form::children(selected),
                    |page| *page,
                    move |child| {
                        form::link(
                            &keyboard,
                            child.title(),
                            move || navigate(child, page, history, index, query),
                            config,
                        )
                    },
                )
                .style(|s| s.flex_col().items_start().width_full()),
            ))
            .style(|s| s.flex_col().width_full())
        },
    )
    .style(|s| s.flex_col().width_full());
    let tools_window = window.clone();
    let account_settings =
        accounts::view(config, window.notifications).style(move |s| {
            s.apply_if(displayed_page.get() != Page::VersionControl, |s| s.hide())
        });
    let tools_keyboard = keyboard.clone();
    let external_buttons = dyn_stack(
        move || {
            if visible.get() && show_page_actions() {
                match displayed_page.get() {
                    Page::RunConfigurations => vec![("Edit Run/Debug Configurations…", None)],
                    Page::Plugins => vec![("Manage Plugins and Themes", Some(PanelKind::Plugin))],
                    Page::Git => vec![("Open Commit", Some(PanelKind::SourceControl)), ("Open Git History", Some(PanelKind::GitHistory))],
                    Page::Devices => vec![("Open Device Manager", Some(PanelKind::DeviceManager)), ("Open Running Devices", Some(PanelKind::RunningDevices))],
                    Page::Logcat => vec![("Open Logcat", Some(PanelKind::Logcat))],
                    _ => Vec::new(),
                }
            } else { Vec::new() }
        },
        |(label, _)| *label,
        move |(label, panel)| {
            let window = tools_window.clone();
            form::link(&tools_keyboard, label, move || {
                if dirty.get_untracked() {
                    error.set("Apply or cancel your settings changes before opening this tool.".into());
                    return;
                }
                close();
                if let Some(panel) = panel {
                    window.show_panel(panel);
                } else {
                    window.run_config_modal_visible.set(true);
                }
            }, config)
        },
    ).style(|s| s.flex_col().items_start().width_full());
    let button_keyboard = keyboard.clone();
    let button = move |text: &'static str| {
        button_keyboard.control_if(
            crate::widgets::button(
                move || text,
                if text == "OK" {
                    crate::widgets::ButtonKind::Primary
                } else {
                    crate::widgets::ButtonKind::Outlined
                },
                config,
            )
            .style(layout::button_minimum),
            move || text != "Apply" || dirty.get_untracked(),
        )
    };
    let rows_keyboard = keyboard.clone();
    let (tree, tree_id) = navigation_tree(
        cx,
        &keyboard,
        config,
        accept.clone(),
        navigation::Navigation {
            page,
            displayed_page,
            history,
            index,
            expanded,
            matching,
            shown_pages,
            modifications,
        },
    );
    let search_clear = keyboard
        .icon(
            || SparrowIcons::SEARCH_CLEAR,
            move || {
                query.set(String::new());
                search_id.request_focus();
            },
            || false,
            // Hiding Clear removes it from the tab order. Disabling it in the click
            // handler would schedule a focus advance after the input regains focus.
            || false,
            || tokens::text("search", "clear_action_label"),
            config,
        )
        .style(move |s| {
            s.width(layout::search_control_height())
                .flex_shrink(0.0_f32)
                .apply_if(query.with(String::is_empty), |s| s.hide())
        });
    let icon_grid = tokens::size("search", "search_icon_grid");
    search_id.add_event_listener(
        EventListener::KeyDown,
        Box::new(move |event| {
            navigation::search_navigation(
                event,
                tree_id,
                shown_pages,
                page,
                history,
                index,
            )
        }),
    );
    let search = search_field_with_style(
        search,
        search_clear,
        config,
        SparrowIcons::SEARCH,
        icon_grid,
        layout::search_field,
        move |s| layout::search_focus(s, &config.get()),
    );
    let sidebar = stack((
        container(search).style(|s| {
            tokens::padding(s.width_full(), "navigation", "search_container_insets")
        }),
        tree.style(|s| s.size_full().min_height(0).flex_grow(1.0_f32).flex_basis(0)),
    ))
    .style(move |s| {
        s.flex_col()
            .background(layout::sidebar_background(&config.get()))
    });

    let breadcrumb = dyn_stack(
        move || {
            let page = displayed_page.get();
            if matching.with(|matches| {
                matches.as_ref().is_some_and(|matches| matches.len() != 1)
            }) {
                return vec![(None, true)];
            }
            let mut path: Vec<_> = page.ancestors().collect();
            path.reverse();
            path.push(page);
            path.into_iter()
                .enumerate()
                .map(|(index, page)| (Some(page), index == 0))
                .collect()
        },
        |part| *part,
        move |(page, first)| {
            breadcrumb_item(
                first,
                label(move || page.map(Page::title).unwrap_or("Search results"))
                    .style(|s| s.min_width(0).text_ellipsis()),
                config,
            )
        },
    );
    let history_toolbar = stack((
        keyboard.icon(
            || SparrowIcons::LOCATION_BACKWARD,
            move || {
                index.update(|i| *i = i.saturating_sub(1));
                page.set(history.with_untracked(|h| h[index.get_untracked()]));
                query.set(String::new());
            },
            || false,
            move || index.get() == 0,
            || "Back",
            config,
        ),
        keyboard.icon(
            || SparrowIcons::LOCATION_FORWARD,
            move || {
                index.update(|i| *i += 1);
                page.set(history.with_untracked(|h| h[index.get_untracked()]));
                query.set(String::new());
            },
            || false,
            move || index.get() + 1 >= history.with(Vec::len),
            || "Forward",
            config,
        ),
    ))
    .style(|s| tokens::padding(s, "navigation", "history_toolbar_insets"));
    let header = stack((
        breadcrumb.style(|s| {
            tokens::padding(s, "navigation", "banner_insets")
                .font_bold()
                .min_width(0)
                .flex_grow(1.0_f32)
                .items_center()
        }),
        history_toolbar,
    ))
    .style(|s| {
        s.width_full()
            .items_center()
            .min_height(32)
            .flex_shrink(0.0_f32)
    });

    let show_menus = move || {
        visible.get() && displayed_page.get() == Page::Menus && show_page_actions()
    };
    let menu_keyboard = keyboard.clone();
    let menu_editor = dyn_stack(
        move || show_menus().then_some(()).into_iter().collect::<Vec<_>>(),
        |_| (),
        move |_| {
            menu_editor::view(
                menu_keyboard.clone(),
                pending,
                settings_config.read_only(),
                config,
            )
        },
    )
    .style(|s| s.flex_col().width_full());
    let show_keymap = move || {
        visible.get() && displayed_page.get() == Page::Keymap && show_page_actions()
    };
    let keymap_common = window.common.clone();
    let keymap_keyboard = keyboard.clone();
    let keymap = dyn_stack(
        move || show_keymap().then_some(()).into_iter().collect::<Vec<_>>(),
        |_| (),
        move |_| {
            crate::keymap::settings_keymap_view(
                keymap_common.clone(),
                keymap_keyboard.clone(),
            )
        },
    )
    .style(move |s| {
        s.width_full()
            .flex_grow(1.0_f32)
            .flex_basis(0)
            .min_height(0)
            .apply_if(!show_keymap(), |s| s.hide())
    });
    let page_content = stack((
        header,
        scope_picker.style(move |s| {
            s.apply_if(
                !can_project
                    || show_keymap()
                    || (displayed_page.get() == Page::Debugger
                        && show_page_actions()),
                |s| s.hide(),
            )
        }),
        keymap,
        label(move || error.get()).style(move |s| {
            s.color(config.get().color(SparrowColor::SPARROW_ERROR))
                .text_ellipsis()
                .apply_if(error.with(String::is_empty), |s| s.hide())
        }),
        sparrow_scroll(
            stack((
                overview,
                external_buttons,
                account_settings,
                menu_editor,
                label(|| "No settings match your search.").style(move |s| {
                    s.margin_top(12.0)
                        .color(
                            config.get().color(SparrowColor::PANEL_FOREGROUND_DIM),
                        )
                        .apply_if(
                            matching.with(|pages| {
                                pages.as_ref().is_none_or(|pages| !pages.is_empty())
                            }),
                            |s| s.hide(),
                        )
                }),
                dyn_stack(
                    move || {
                        sections.with(|sections| {
                            sections
                                .iter()
                                .map(|(title, _)| *title)
                                .collect::<Vec<_>>()
                        })
                    },
                    |title| *title,
                    move |title| {
                        let keyboard = rows_keyboard.clone();
                        stack((
                            form::section_title(title, config),
                            dyn_stack(
                                move || {
                                    sections.with(|sections| {
                                        sections
                                            .iter()
                                            .find(|(section, _)| *section == title)
                                            .map(|(_, items)| items.clone())
                                            .unwrap_or_default()
                                    })
                                },
                                move |item| {
                                    (
                                        epoch.get(),
                                        item.kind.clone(),
                                        item.field.clone(),
                                        initial(&item.value).map(|v| v.0),
                                    )
                                },
                                move |item| {
                                    let debugger = item.kind == "debugger";
                                    setting_row(
                                        item,
                                        if debugger {
                                            debugger_pending
                                        } else {
                                            pending
                                        },
                                        keyboard.clone(),
                                        if debugger {
                                            no_reset
                                        } else {
                                            project_scope
                                        },
                                        highlight_matcher,
                                        config,
                                    )
                                },
                            )
                            .style(|s| s.flex_col().width_full()),
                        ))
                        .style(|s| s.flex_col().width_full())
                    },
                )
                .style(|s| s.flex_col().width_full()),
            ))
            .style(|s| s.flex_col().min_width_full()),
        )
        .scroll_to(move || {
            let _ = (displayed_page.get(), search_query.get());
            Some(floem::kurbo::Point::ORIGIN)
        })
        .style(move |s| {
            s.flex_grow(1.0_f32)
                .flex_basis(0)
                .min_height(0)
                .width_full()
                .apply_if(show_keymap(), |s| s.hide())
        }),
    ))
    .style(|s| s.flex_col().padding_horiz(16.0).padding_top(8.0));

    let project_name = window
        .workspace
        .path
        .as_deref()
        .and_then(std::path::Path::file_name)
        .map(|name| name.to_string_lossy().into_owned());
    let title = stack((
        label(move || {
            let caption = if cfg!(target_os = "macos") {
                tokens::text("dialog", "title_macos")
            } else {
                tokens::text("dialog", "title")
            };
            project_name.as_ref().map_or_else(
                || caption.to_owned(),
                |name| format!("{caption} – {name}"),
            )
        })
        .style(|s| s.flex_grow(1.0_f32).font_bold().text_ellipsis()),
        keyboard.icon(
            || SparrowIcons::CLOSE,
            close,
            || false,
            || false,
            || "Close Settings",
            config,
        ),
    ))
    .style(move |s| {
        s.height(36.0)
            .padding_horiz(16.0)
            .width_full()
            .items_center()
            .flex_shrink(0.0_f32)
            .background(config.get().color(SparrowColor::PANEL_CURRENT_BACKGROUND))
    });

    let footer = stack((
        Divider::new(config)
            .thickness(tokens::length("footer", "divider_width") as f32)
            .into_view(),
        label(|| "Search by setting name or description. Changes are saved only by Apply or OK.")
            .style(move |s| {
                tokens::padding(s, "footer", "compact_footer_insets")
                    .width_full()
                    .text_ellipsis()
                    .apply_if(!help_open.get(), |s| s.hide())
            }),
        stack((
            keyboard.control(
                crate::widgets::button(
                    || tokens::text("footer", "help_label"),
                    crate::widgets::ButtonKind::Plain,
                    config,
                )
                .on_click_stop(move |_| help_open.update(|open| *open = !*open)),
            ),
            empty().style(|s| s.flex_grow(1.0_f32)),
            button(tokens::text("footer", "ok_label")).on_click_stop(move |_| accept()),
            button(tokens::text("footer", "cancel_label")).on_click_stop(move |_| close()),
            button(tokens::text("footer", "apply_label"))
                .on_click_stop(move |_| { save(); })
                .disabled(move || !dirty.get()),
        ))
        .style(layout::footer_buttons),
    ))
    .style(|s| s.width_full().flex_col().flex_shrink(0.0_f32));

    let card = stack((
        title,
        layout::settings_split(sidebar, page_content, config),
        footer,
    ))
    .style(move |s| layout::dialog_card(s, &config.get()));
    let dialog = container(card)
        .on_event_stop(EventListener::PointerDown, |_| {})
        .style(move |s| {
            s.absolute()
                .size_full()
                .items_center()
                .justify_center()
                .background(floem::peniko::Color::from_rgba8(0, 0, 0, 70))
                .apply_if(!visible.get(), |s| s.hide())
        });
    keyboard
        .bind_root(dialog, visible, config)
        .style(|s| s.z_index(210))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn category_navigation_skips_collapsed_children_and_retains_search_results() {
        let collapsed = visible_pages(&BTreeSet::new(), None);
        assert!(!collapsed.contains(&Page::Appearance));
        assert!(!collapsed.contains(&Page::Flutter));
        let appearance = visible_pages(&BTreeSet::from([Page::Overview]), None);
        assert!(appearance.contains(&Page::Appearance));
        assert!(appearance.contains(&Page::System));
        assert!(!appearance.contains(&Page::Dart));
        let languages = visible_pages(&BTreeSet::from([Page::Languages]), None);
        assert!(languages.contains(&Page::Flutter));
        assert!(languages.contains(&Page::Dart));
        assert!(!languages.contains(&Page::Appearance));
        assert_eq!(
            visible_pages(&BTreeSet::new(), Some(&[Page::Dart])),
            vec![Page::Languages, Page::Dart]
        );
        assert!(visible_pages(&BTreeSet::new(), Some(&[])).is_empty());
    }
    #[test]
    fn nested_settings_keep_all_ancestors_and_follow_expansion() {
        assert_eq!(
            visible_pages(&BTreeSet::new(), Some(&[Page::Files])),
            [Page::Overview, Page::System, Page::Files],
        );
        assert!(
            !visible_pages(&BTreeSet::from([Page::Overview]), None)
                .contains(&Page::Files)
        );
        assert!(
            visible_pages(&BTreeSet::from([Page::Overview, Page::System]), None)
                .contains(&Page::Files)
        );
    }

    #[test]
    fn quick_actions_and_project_debugger_settings_round_trip() {
        let temp = tempfile::tempdir().unwrap();
        let ide = temp.path().join("settings.toml");
        let debugger = temp.path().join("debugger.toml");
        std::fs::write(&debugger, "# Debugger preferences\ndebug-external = true\n")
            .unwrap();
        let edit = Edit {
            raw: r#"["save_all","palette"]"#.into(),
            kind: Type::StringList,
        };
        SparrowConfig::update_scoped_settings(vec![
            (
                ide.clone(),
                vec![(
                    "ui".into(),
                    "quick-actions".into(),
                    Some(edit.value("ui").unwrap()),
                )],
            ),
            (
                debugger.clone(),
                vec![(String::new(), "inline-values".into(), Some(false.into()))],
            ),
        ])
        .unwrap();
        let ide: toml::Value =
            toml::from_str(&std::fs::read_to_string(ide).unwrap()).unwrap();
        let actions = ide["ui"]["quick-actions"].as_array().unwrap();
        assert_eq!(
            actions
                .iter()
                .map(|item| item.as_str().unwrap())
                .collect::<Vec<_>>(),
            ["save_all", "palette"]
        );
        let text = std::fs::read_to_string(debugger).unwrap();
        assert!(text.starts_with("# Debugger preferences"));
        let settings: sparrow_rpc::dap_types::DebuggerSettings =
            toml::from_str(&text).unwrap();
        assert!(settings.debug_external);
        assert!(!settings.inline_values);
        assert!(settings.preview_files);
    }

    fn setting(cx: Scope, field: &str, description: &str) -> SettingsItem {
        SettingsItem {
            kind: "core".into(),
            name: String::new(),
            field: field.into(),
            description: description.into(),
            filter_text: String::new(),
            value: SettingsValue::String(String::new()),
            serde_value: serde_json::Value::Null,
            pos: cx.create_rw_signal(floem::kurbo::Point::ORIGIN),
            size: cx.create_rw_signal(floem::kurbo::Size::ZERO),
            header: false,
        }
    }

    #[test]
    fn searches_setting_keys_descriptions_and_page_titles() {
        let cx = Scope::new();
        let items = [
            setting(cx, "flutter-sdk-path", "Local Flutter toolchain"),
            setting(cx, "dart-sdk-path", "Standalone Dart toolchain"),
        ];
        assert_eq!(matching_pages(&items, "path flutter"), [Page::Flutter]);
        assert_eq!(matching_pages(&items, "standalone toolchain"), [Page::Dart]);
        assert_eq!(matching_pages(&items, "keymap"), [Page::Keymap]);
        assert_eq!(matching_pages(&items, "keyboard shortcuts"), [Page::Keymap]);
        assert_eq!(matching_pages(&items, "quick actions"), [Page::Menus]);
        assert!(matching_pages(&items, "missing value").is_empty());
        cx.dispose();
    }

    #[test]
    fn previews_a_single_matching_page_without_changing_navigation() {
        let selected = Page::Appearance;
        assert_eq!(search_page(selected, Some(&[Page::Keymap])), Page::Keymap);
        assert_eq!(search_page(selected, Some(&[])), selected);
        assert_eq!(
            search_page(selected, Some(&[Page::Keymap, Page::Menus])),
            selected
        );
        assert_eq!(search_page(selected, None), selected);
        assert_eq!(
            search_page(Page::Overview, Some(&[Page::Menus])),
            Page::Overview
        );
    }

    #[test]
    fn marks_pending_changes_and_override_removals_in_both_scopes() {
        let dart_key = ("core".into(), "dart-sdk-path".into());
        let mut active = Pending::from([(
            dart_key,
            Edit {
                raw: "/dart".into(),
                kind: Type::String,
            },
        )]);
        let inactive = Pending::from([(
            ("core".into(), "flutter-sdk-path".into()),
            Edit {
                raw: String::new(),
                kind: Type::Reset,
            },
        )]);
        assert_eq!(
            modified_pages(&active, &inactive),
            [Page::Dart, Page::Flutter]
        );
        active.clear();
        assert_eq!(modified_pages(&active, &inactive), [Page::Flutter]);
        assert!(modified_pages(&active, &Pending::new()).is_empty());
    }

    #[test]
    fn validates_numbers_and_preserves_string_whitespace() {
        assert!(
            Edit {
                raw: "NaN".into(),
                kind: Type::Float
            }
            .value("editor")
            .is_err()
        );
        assert!(
            Edit {
                raw: "-1".into(),
                kind: Type::Integer
            }
            .value("ui")
            .is_err()
        );
        assert_eq!(
            Edit {
                raw: "  literal  ".into(),
                kind: Type::String
            }
            .value("plugin")
            .unwrap()
            .as_str(),
            Some("  literal  ")
        );
    }
}
