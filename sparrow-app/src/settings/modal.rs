//! Reference-style modal settings with deferred, validated edits.
use super::{SettingsData, SettingsItem, SettingsValue};
use crate::widgets::modal_focus::ModalFocus;
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
use std::{collections::BTreeMap, rc::Rc, sync::Arc};
mod layout;
mod navigation;
mod setting_row;
mod tokens;

use crate::widgets::sparrow_scroll;
use navigation::navigation_tree;
use setting_row::setting_row;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    Overview,
    Appearance,
    System,
    Editor,
    Plugins,
    Keymap,
    Run,
    Terminal,
    Languages,
    Flutter,
    Dart,
}
const PAGES: [(Page, &str, usize); 11] = [
    (Page::Overview, "Appearance & Behavior", 0),
    (Page::Appearance, "Appearance", 1),
    (Page::System, "System Settings", 1),
    (Page::Keymap, "Keymap", 0),
    (Page::Editor, "Editor", 0),
    (Page::Plugins, "Themes", 0),
    (Page::Run, "Build, Execution, Deployment", 0),
    (Page::Terminal, "Tools › Terminal", 0),
    (Page::Languages, "Languages & Frameworks", 0),
    (Page::Flutter, "Flutter", 1),
    (Page::Dart, "Dart", 1),
];
impl Page {
    fn title(self) -> &'static str {
        PAGES.iter().find(|(p, _, _)| *p == self).unwrap().1
    }

    fn parent(self) -> Option<Self> {
        match self {
            Self::Appearance | Self::System => Some(Self::Overview),
            Self::Flutter | Self::Dart => Some(Self::Languages),
            _ => None,
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::Overview => "appearance-and-behavior",
            Self::Appearance => "appearance",
            Self::System => "system",
            Self::Editor => "editor",
            Self::Plugins => "themes",
            Self::Keymap => "keymap",
            Self::Run => "run",
            Self::Terminal => "terminal",
            Self::Languages => "languages",
            Self::Flutter => "flutter",
            Self::Dart => "dart",
        }
    }
}

fn visible_pages(
    appearance: bool,
    languages: bool,
    matching: Option<&[Page]>,
) -> Vec<Page> {
    PAGES
        .iter()
        .map(|(page, _, _)| *page)
        .filter(|page| {
            if let Some(matching) = matching {
                return matching.contains(page)
                    || matching.iter().any(|item| item.parent() == Some(*page));
            }
            match page.parent() {
                Some(Page::Overview) => appearance,
                Some(Page::Languages) => languages,
                _ => true,
            }
        })
        .collect()
}
#[derive(Clone)]
enum Type {
    Reset,
    String,
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
fn belongs(item: &SettingsItem, page: Page) -> bool {
    let appearance = item.kind == "ui"
        || (item.kind == "core"
            && ["color-theme", "icon-theme", "custom-titlebar"]
                .contains(&item.field.as_str()));
    match page {
        Page::Appearance => appearance,
        Page::System => {
            item.kind == "core"
                && !appearance
                && !["flutter-sdk-path", "dart-sdk-path"]
                    .contains(&item.field.as_str())
        }
        Page::Flutter => item.kind == "core" && item.field == "flutter-sdk-path",
        Page::Dart => item.kind == "core" && item.field == "dart-sdk-path",
        Page::Editor => item.kind == "editor",
        Page::Terminal => item.kind == "terminal",
        Page::Plugins => {
            !["core", "ui", "editor", "terminal"].contains(&item.kind.as_str())
        }
        _ => false,
    }
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
        .map(|(page, _, _)| *page)
        .filter(|&page| {
            matches_query(page.title(), query)
                || items
                    .iter()
                    .any(|item| belongs(item, page) && matches_setting(item, query))
        })
        .collect()
}

fn modified_pages(
    items: &[SettingsItem],
    active: &Pending,
    inactive: &Pending,
) -> Vec<Page> {
    PAGES
        .iter()
        .map(|(page, _, _)| *page)
        .filter(|&page| {
            active.keys().chain(inactive.keys()).any(|(kind, field)| {
                items.iter().any(|item| {
                    &item.kind == kind && &item.field == field && belongs(item, page)
                })
            })
        })
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
    let project_scope = cx.create_rw_signal(false);
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
        .and_then(|id| PAGES.iter().find(|(page, _, _)| page.id() == id))
        .map(|(page, _, _)| *page)
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
    let appearance_expanded = cx.create_rw_signal(false);
    let languages_expanded = cx.create_rw_signal(false);
    cx.create_effect(move |_| {
        if matches!(page.get(), Page::Flutter | Page::Dart) {
            languages_expanded.set(true);
        }
    });
    cx.create_effect(move |_| {
        if matches!(page.get(), Page::Appearance | Page::System) {
            appearance_expanded.set(true);
        }
    });
    let history = cx.create_rw_signal(vec![initial_page]);
    let index = cx.create_rw_signal(0usize);
    let epoch = cx.create_rw_signal(0usize);
    let error = cx.create_rw_signal(String::new());
    let help_open = cx.create_rw_signal(false);
    let search = keyboard.input(query, config);
    let search_id = search.id();
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
    cx.create_effect(move |_| {
        let mut combined = items.get();
        combined.extend(plugins.get());
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
    let shown_pages = cx.create_memo(move |_| {
        matching.with(|matching| {
            visible_pages(
                appearance_expanded.get(),
                languages_expanded.get(),
                matching.as_deref(),
            )
        })
    });
    let modifications = cx.create_memo(move |_| {
        all.with(|items| {
            pending.with(|active| {
                scope_drafts.with(|drafts| {
                    modified_pages(
                        items,
                        active,
                        &drafts[usize::from(!project_scope.get())],
                    )
                })
            })
        })
    });
    let filtered = cx.create_rw_signal(Vec::<SettingsItem>::new());
    cx.create_effect(move |_| {
        if !visible.get() {
            filtered.set(Vec::new());
            return;
        }
        let selected = page.get();
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
                                && (matches_query(selected.title(), pattern)
                                    || matches_setting(item, pattern))
                        }
                        Some(pattern) => matches_setting(item, pattern),
                    })
                    .cloned()
                    .collect()
            })
        });
        filtered.set(items);
    });
    let dirty = cx.create_memo(move |_| {
        !pending.with(|pending| pending.is_empty())
            || scope_drafts
                .with(|drafts| !drafts[usize::from(!project_scope.get())].is_empty())
    });
    let can_project =
        window.workspace.path.is_some() && window.workspace.kind.is_local();
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
            let result = drafts.into_iter().enumerate().filter(|(_,edits)|!edits.is_empty()).map(|(scope,edits)| {
                let changes = edits.into_iter().map(|((section,key),edit)| {
                    let value = if matches!(edit.kind,Type::Reset) { Ok(None) } else {
                        edit.value(&section).and_then(|value| {
                            if section=="core" && ["flutter-sdk-path","dart-sdk-path"].contains(&key.as_str()) && !edit.raw.trim().is_empty() {
                                let path=std::path::Path::new(edit.raw.trim());
                                if !path.is_absolute() { return Err("Use an absolute SDK directory path.".into()); }
                                crate::dart::sdk::validate_sdk_root(path,key=="flutter-sdk-path")?;
                            }
                            Ok(Some(value))
                        })
                    };
                    value.map(|value|(section.clone(),key.clone(),value)).map_err(|error|format!("{} / {section}.{key}: {error}",if scope==0 {"IDE"}else{"Project"}))
                }).collect::<Result<Vec<_>,String>>()?;
                Ok((SparrowConfig::scoped_settings_file(window.workspace.path.as_deref(),scope==1)?,changes))
            }).collect::<Result<Vec<_>,String>>().and_then(SparrowConfig::update_scoped_settings);
            match result {
                Ok(()) => {
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
    let external = window.clone();
    let show_page_actions = move || {
        search_query.with(|query| {
            query
                .as_ref()
                .is_none_or(|query| matches_query(page.get().title(), query))
        })
    };
    let overview_keyboard = keyboard.clone();
    let overview=stack((
        label(||"Customize IDE appearance and behavior: change themes and font size, tune the keymap, configure Dart/Flutter and system settings.").style(|s|s.width_full().margin_top(8.0).margin_bottom(10.0)),
        dyn_stack(move ||if page.get()==Page::Languages {vec![Page::Flutter,Page::Dart]}else{vec![Page::Appearance,Page::System,Page::Editor,Page::Terminal,Page::Keymap,Page::Plugins,Page::Flutter,Page::Dart]},|p|*p as u8,move |p|overview_keyboard.control(label(move ||p.title()).on_click_stop(move |_|navigate(p,page,history,index,query)).style(move |s|s.margin_left(24.0).height(22.0).color(config.get().color(SparrowColor::EDITOR_LINK)).hover(|s|s.cursor(floem::style::CursorStyle::Pointer))))).style(|s|s.flex_col().width_full()),
    )).style(move |s|s.flex_col().width_full().apply_if(!matches!(page.get(),Page::Overview|Page::Languages)||!show_page_actions(),|s|s.hide()));
    let launch_page = page;
    let external_button = label(move || match launch_page.get() {
        Page::Keymap => "Open Keymap Editor",
        Page::Run => "Edit Run/Debug Configurations…",
        _ => "Manage Themes…",
    })
    .on_click_stop(move |_| {
        if dirty.get_untracked() {
            error.set(
                "Apply or cancel your settings changes before opening this editor."
                    .into(),
            );
            return;
        }
        visible.set(false);
        match launch_page.get() {
            Page::Keymap => external.main_split.open_keymap(),
            Page::Run => external.run_config_modal_visible.set(true),
            _ => external.show_panel(PanelKind::Plugin),
        }
    })
    .style(move |s| {
        s.height(28.0)
            .items_center()
            .color(config.get().color(SparrowColor::EDITOR_LINK))
            .apply_if(
                !show_page_actions()
                    || !matches!(
                        page.get(),
                        Page::Keymap | Page::Run | Page::Plugins
                    ),
                |s| s.hide(),
            )
    });
    let external_button = keyboard.control(external_button);
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
    let tree = navigation_tree(
        cx,
        &keyboard,
        config,
        accept.clone(),
        navigation::Navigation {
            page,
            history,
            index,
            appearance_expanded,
            languages_expanded,
            matching,
            shown_pages,
            modifications,
        },
    );
    let tree_id = tree.id();
    let search_clear = keyboard
        .icon(
            || SparrowIcons::SEARCH_CLEAR,
            move || {
                query.set(String::new());
                search_id.request_focus();
            },
            || false,
            move || query.with(String::is_empty),
            || tokens::text("search", "clear_action_label"),
            config,
        )
        .style(move |s| s.apply_if(query.with(String::is_empty), |s| s.hide()));
    let icon_grid = tokens::size("search", "search_icon_grid");
    let search = search_field_with_style(
        search.on_event(EventListener::KeyDown, move |event| {
            navigation::search_navigation(
                event,
                tree_id,
                shown_pages,
                page,
                history,
                index,
            )
        }),
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
        sparrow_scroll(tree)
            .style(|s| s.size_full().min_height(0).flex_grow(1.0_f32).flex_basis(0)),
    ))
    .style(move |s| {
        s.flex_col()
            .background(layout::sidebar_background(&config.get()))
    });

    let parent = breadcrumb_item(
        true,
        label(move || match page.get() {
            Page::Appearance | Page::System => Page::Overview.title(),
            Page::Flutter | Page::Dart => Page::Languages.title(),
            _ => "",
        }),
        config,
    )
    .style(move |s| {
        s.apply_if(
            search_query.with(Option::is_some)
                || !matches!(
                    page.get(),
                    Page::Appearance | Page::System | Page::Flutter | Page::Dart
                ),
            |s| s.hide(),
        )
    });
    let breadcrumb = dyn_stack(
        move || {
            vec![
                search_query.with(Option::is_none)
                    && matches!(
                        page.get(),
                        Page::Appearance | Page::System | Page::Flutter | Page::Dart
                    ),
            ]
        },
        |nested| *nested,
        move |nested| {
            breadcrumb_item(
                !nested,
                label(move || {
                    if search_query.with(Option::is_none) {
                        page.get().title()
                    } else {
                        "Search results"
                    }
                })
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
        stack((parent, breadcrumb)).style(|s| {
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

    let page_content = stack((
        header,
        scope_picker.style(move |s| s.apply_if(!can_project, |s| s.hide())),
        label(move || error.get()).style(move |s| {
            s.color(config.get().color(SparrowColor::SPARROW_ERROR))
                .text_ellipsis()
                .apply_if(error.with(String::is_empty), |s| s.hide())
        }),
        sparrow_scroll(
            stack((
                overview,
                external_button,
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
                    move || filtered.get(),
                    move |item| {
                        (
                            epoch.get(),
                            item.kind.clone(),
                            item.field.clone(),
                            initial(&item.value).map(|v| v.0),
                        )
                    },
                    move |item| {
                        setting_row(
                            item,
                            pending,
                            rows_keyboard.clone(),
                            project_scope,
                            highlight_matcher,
                            config,
                        )
                    },
                )
                .style(|s| s.flex_col().width_full()),
            ))
            .style(|s| s.flex_col().min_width_full()),
        )
        .style(|s| {
            s.flex_grow(1.0_f32)
                .flex_basis(0)
                .min_height(0)
                .width_full()
        }),
    ))
    .style(|s| s.flex_col().padding_horiz(16.0).padding_top(8.0));

    let title = stack((
        label(|| {
            if cfg!(target_os = "macos") {
                tokens::text("dialog", "title_macos")
            } else {
                tokens::text("dialog", "title")
            }
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
        let collapsed = visible_pages(false, false, None);
        assert!(!collapsed.contains(&Page::Appearance));
        assert!(!collapsed.contains(&Page::Flutter));
        let appearance = visible_pages(true, false, None);
        assert!(appearance.contains(&Page::Appearance));
        assert!(appearance.contains(&Page::System));
        assert!(!appearance.contains(&Page::Dart));
        let languages = visible_pages(false, true, None);
        assert!(languages.contains(&Page::Flutter));
        assert!(languages.contains(&Page::Dart));
        assert!(!languages.contains(&Page::Appearance));
        assert_eq!(
            visible_pages(false, false, Some(&[Page::Dart])),
            vec![Page::Languages, Page::Dart]
        );
        assert!(visible_pages(false, false, Some(&[])).is_empty());
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
        assert!(matching_pages(&items, "missing value").is_empty());
        cx.dispose();
    }

    #[test]
    fn marks_pending_changes_and_override_removals_in_both_scopes() {
        let cx = Scope::new();
        let items = [
            setting(cx, "flutter-sdk-path", ""),
            setting(cx, "dart-sdk-path", ""),
        ];
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
            modified_pages(&items, &active, &inactive),
            [Page::Flutter, Page::Dart]
        );
        active.clear();
        assert_eq!(modified_pages(&items, &active, &inactive), [Page::Flutter]);
        assert!(modified_pages(&items, &active, &Pending::new()).is_empty());
        cx.dispose();
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
