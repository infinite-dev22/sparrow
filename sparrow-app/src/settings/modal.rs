//! Reference-style modal settings with deferred, validated edits.
use super::{SettingsData, SettingsItem, SettingsValue};
use crate::modal_focus::{ModalChoice, ModalFocus, list_destination};
use crate::{
    app::{clickable_icon, tooltip_label},
    config::{SparrowConfig, color::SparrowColor, icon::SparrowIcons},
    panel::kind::PanelKind,
    window_tab::WindowTabData,
};
use floem::{
    IntoView, View,
    event::{Event, EventListener, EventPropagation},
    keyboard::{Key, NamedKey},
    reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith},
    views::{Decorators, container, dyn_stack, empty, label, scroll, stack, svg},
};
use std::{collections::BTreeMap, rc::Rc};
#[derive(Clone, Copy, PartialEq, Eq)]
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
}
fn visible_pages(appearance: bool, languages: bool, searching: bool) -> Vec<Page> {
    PAGES
        .iter()
        .filter(|(page, _, depth)| {
            *depth == 0
                || searching
                || if matches!(page, Page::Flutter | Page::Dart) {
                    languages
                } else {
                    appearance
                }
        })
        .map(|(page, _, _)| *page)
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
fn matches_setting(item: &SettingsItem, query: &str) -> bool {
    let haystack = format!(
        "{} {} {} {}",
        item.kind, item.field, item.name, item.description
    )
    .to_lowercase();
    query
        .to_lowercase()
        .split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .all(|token| haystack.contains(token))
}

fn navigate(
    page: Page,
    current: RwSignal<Page>,
    history: RwSignal<Vec<Page>>,
    index: RwSignal<usize>,
    query: RwSignal<String>,
) {
    if current.get_untracked() != page {
        history.update(|h| {
            h.truncate(index.get_untracked() + 1);
            h.push(page);
        });
        index.set(history.with_untracked(Vec::len) - 1);
        current.set(page);
    }
    query.set(String::new());
}
fn setting_row(
    item: SettingsItem,
    pending: RwSignal<Pending>,
    keyboard: ModalFocus,
    project_scope: RwSignal<bool>,
    query: RwSignal<String>,
    config: floem::reactive::ReadSignal<std::sync::Arc<SparrowConfig>>,
) -> impl View {
    let (original, kind) = initial(&item.value).unwrap();
    let key = (item.kind.clone(), item.field.clone());
    let raw = pending
        .with_untracked(|p| p.get(&key).map(|v| v.raw.clone()))
        .unwrap_or_else(|| original.clone());
    let reset_key = key.clone();
    let reset_value = original.clone();
    let reset_key_label = key.clone();
    let value = floem::reactive::create_rw_signal(raw);
    Scope::current().create_effect(move |last: Option<String>| {
        let raw = value.get();
        if last.is_some() {
            pending.update(|p| {
                if raw == original {
                    p.remove(&key);
                } else {
                    p.insert(
                        key.clone(),
                        Edit {
                            raw: raw.clone(),
                            kind: kind.clone(),
                        },
                    );
                }
            });
        }
        raw
    });
    let needs_control = matches!(&item.value, SettingsValue::Bool(_));
    let control = match item.value {
        SettingsValue::Bool(_) => label(move || {
            if value.get() == "true" {
                "☑ Enabled"
            } else {
                "☐ Disabled"
            }
        })
        .on_click_stop(move |_| {
            value.update(|v| *v = if v == "true" { "false" } else { "true" }.into())
        })
        .into_any(),
        SettingsValue::Dropdown(dropdown) => keyboard
            .menu(
                label(move || format!("{}  ⌄", value.get())).style(move |s| {
                    s.min_width(220.0)
                        .height(26.0)
                        .padding_horiz(8.0)
                        .items_center()
                        .border(1.0)
                        .border_radius(3.0)
                        .border_color(
                            config.get().color(SparrowColor::SPARROW_BORDER),
                        )
                }),
                move || {
                    dropdown
                        .items
                        .iter()
                        .map(|choice| {
                            let text = choice.clone();
                            ModalChoice::new(choice.clone(), move || {
                                value.set(text.clone())
                            })
                            .selected(value.get_untracked() == *choice)
                        })
                        .collect()
                },
            )
            .into_any(),
        SettingsValue::String(_)
            if item.kind == "core"
                && ["flutter-sdk-path", "dart-sdk-path"]
                    .contains(&item.field.as_str()) =>
        {
            let input = keyboard.input(value).style(move |s| {
                s.width(400.0)
                    .max_width_full()
                    .height(26.0)
                    .padding_horiz(8.0)
                    .border(1.0)
                    .border_radius(3.0)
                    .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
            });
            let input_id = input.id();
            let browser = keyboard.clone();
            stack((
                input,
                keyboard.icon(
                    || SparrowIcons::DIRECTORY,
                    move || {
                        let mut options = floem::file::FileDialogOptions::new()
                            .title("Choose SDK directory")
                            .select_directories();
                        let current =
                            std::path::PathBuf::from(value.get_untracked());
                        if current.is_dir() {
                            options = options.force_starting_directory(current);
                        }
                        browser.open_file(options, move |selection| {
                            if let Some(path) = selection.and_then(|selection| {
                                selection.path.first().cloned()
                            }) {
                                value.set(path.display().to_string());
                            }
                            if !input_id.is_hidden_recursive() {
                                input_id.request_focus();
                            }
                        })
                    },
                    || false,
                    || false,
                    || "Choose SDK directory",
                    config,
                ),
            ))
            .style(|s| s.items_center().gap(6.0))
            .into_any()
        }
        _ => keyboard
            .input(value)
            .style(move |s| {
                s.width(320.0)
                    .max_width_full()
                    .height(26.0)
                    .padding_horiz(8.0)
                    .border(1.0)
                    .border_radius(3.0)
                    .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
            })
            .into_any(),
    };
    let control = if needs_control {
        keyboard.control(control).into_any()
    } else {
        control
    };
    let reset = keyboard.control(
        label(move || {
            if pending.with(|pending| {
                pending
                    .get(&reset_key_label)
                    .is_some_and(|edit| matches!(edit.kind, Type::Reset))
            }) {
                "Will inherit IDE value on Apply"
            } else {
                "Use IDE value"
            }
        })
        .on_click_stop(move |_| {
            pending.update(|pending| {
                pending.insert(
                    reset_key.clone(),
                    Edit {
                        raw: reset_value.clone(),
                        kind: Type::Reset,
                    },
                );
            })
        })
        .style(move |s| {
            s.height(24.0)
                .items_center()
                .color(config.get().color(SparrowColor::EDITOR_LINK))
                .apply_if(!project_scope.get(), |s| s.hide())
        }),
    );
    let qualified_name = item.name.clone();
    let name = item
        .name
        .split_once(": ")
        .map(|(_, n)| n.to_string())
        .unwrap_or(item.name);
    stack((
        label(move || {
            if query.with(|query| query.is_empty()) {
                name.clone()
            } else {
                qualified_name.clone()
            }
        })
        .style(|s| s.font_bold().margin_bottom(6.0)),
        control,
        reset,
        label(move || item.description.clone()).style(move |s| {
            s.margin_top(6.0)
                .font_size(12.0)
                .max_width_full()
                .color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
        }),
    ))
    .style(|s| s.flex_col().width_full().padding_bottom(20.0))
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
    let query = cx.create_rw_signal(String::new());
    let page = cx.create_rw_signal(Page::Overview);
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
    let history = cx.create_rw_signal(vec![Page::Overview]);
    let index = cx.create_rw_signal(0usize);
    let epoch = cx.create_rw_signal(0usize);
    let error = cx.create_rw_signal(String::new());
    let search = keyboard.input(query);
    let search_id = search.id();
    let root_id = window.common.view_id;
    let close = move || {
        visible.set(false);
        pending.set(Pending::new());
        scope_drafts.set([Pending::new(), Pending::new()]);
        root_id.get_untracked().request_focus();
    };
    cx.create_effect(move |_| {
        if visible.get() {
            pending.set(Pending::new());
            scope_drafts.set([Pending::new(), Pending::new()]);
            query.set(String::new());
            error.set(String::new());
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
    let filtered = cx.create_rw_signal(Vec::<SettingsItem>::new());
    cx.create_effect(move |_| {
        let pattern = query.get().to_lowercase();
        filtered.set(all.with(|items| {
            items
                .iter()
                .filter(|item| {
                    if pattern.is_empty() {
                        belongs(item, page.get())
                    } else {
                        matches_setting(item, &pattern)
                    }
                })
                .cloned()
                .collect()
        }));
    });
    let dirty = cx.create_memo(move |_| {
        !pending.with(|pending| pending.is_empty())
            || scope_drafts
                .with(|drafts| drafts.iter().any(|draft| !draft.is_empty()))
    });
    let can_project =
        window.workspace.path.is_some() && window.workspace.kind.is_local();
    let scope_picker = keyboard.menu(
        label(move || {
            if project_scope.get() {
                "Settings for: This project  ⌄"
            } else {
                "Settings for: IDE  ⌄"
            }
        })
        .style(move |s| {
            s.height(28.0)
                .padding_horiz(8.0)
                .items_center()
                .border(1.0)
                .border_radius(3.0)
                .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
        }),
        move || {
            [false, true]
                .into_iter()
                .map(|project| {
                    ModalChoice::new(
                        if project { "This project" } else { "IDE" },
                        move || {
                            if project == project_scope.get_untracked() {
                                return;
                            }
                            scope_drafts.update(|drafts| {
                                drafts[project_scope.get_untracked() as usize] =
                                    pending.get_untracked()
                            });
                            pending.set(scope_drafts.with_untracked(|drafts| {
                                drafts[project as usize].clone()
                            }));
                            project_scope.set(project);
                            error.set(String::new());
                            epoch.update(|epoch| *epoch += 1);
                        },
                    )
                    .selected(project_scope.get_untracked() == project)
                    .enabled(!project || can_project)
                })
                .collect()
        },
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
    keyboard.set_submit({
        let apply = apply.clone();
        move || {
            if apply() {
                close();
            }
        }
    });
    let okay = apply.clone();
    let save = apply.clone();
    let external = window.clone();
    let overview_keyboard = keyboard.clone();
    let overview=stack((
        label(||"Customize IDE appearance and behavior: change themes and font size, tune the keymap, configure Dart/Flutter and system settings.").style(|s|s.width_full().margin_top(8.0).margin_bottom(10.0)),
        dyn_stack(move ||if page.get()==Page::Languages {vec![Page::Flutter,Page::Dart]}else{vec![Page::Appearance,Page::System,Page::Editor,Page::Terminal,Page::Keymap,Page::Plugins,Page::Flutter,Page::Dart]},|p|*p as u8,move |p|overview_keyboard.control(label(move ||p.title()).on_click_stop(move |_|navigate(p,page,history,index,query)).style(move |s|s.margin_left(24.0).height(22.0).color(config.get().color(SparrowColor::EDITOR_LINK)).hover(|s|s.cursor(floem::style::CursorStyle::Pointer))))).style(|s|s.flex_col().width_full()),
    )).style(move |s|s.flex_col().width_full().apply_if(!matches!(page.get(),Page::Overview|Page::Languages)||!query.with(|q|q.is_empty()),|s|s.hide()));
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
                !query.with(|q| q.is_empty())
                    || !matches!(
                        page.get(),
                        Page::Keymap | Page::Run | Page::Plugins
                    ),
                |s| s.hide(),
            )
    });
    let external_button = keyboard.control(external_button);
    let workspace = window.workspace.clone();
    let button_keyboard = keyboard.clone();
    let button = move |text: &'static str| {
        button_keyboard.control_if(
            label(move || text).style(move |s| {
                s.width(72.0)
                    .height(24.0)
                    .items_center()
                    .justify_center()
                    .border(1.0)
                    .border_radius(4.0)
                    .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
                    .hover(|s| {
                        s.cursor(floem::style::CursorStyle::Pointer).background(
                            config
                                .get()
                                .color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                        )
                    })
            }),
            move || text != "Apply" || dirty.get_untracked(),
        )
    };
    let rows_keyboard = keyboard.clone();
    let tree_focus = cx.create_rw_signal(None::<floem::ViewId>);
    let tree = dyn_stack(
        || PAGES.to_vec(),
        |(p, _, _)| *p as u8,
        move |(p, title, depth)| {
            stack((
                label(move || {
                    if p == Page::Overview {
                        if appearance_expanded.get() {
                            "⌄"
                        } else {
                            "›"
                        }
                    } else if p == Page::Languages {
                        if languages_expanded.get() {
                            "⌄"
                        } else {
                            "›"
                        }
                    } else {
                        ""
                    }
                })
                .on_click_stop(move |_| {
                    if p == Page::Overview {
                        let collapsing = appearance_expanded.get_untracked();
                        if collapsing
                            && matches!(
                                page.get_untracked(),
                                Page::Appearance | Page::System
                            )
                        {
                            navigate(p, page, history, index, query);
                        }
                        appearance_expanded.set(!collapsing);
                    } else if p == Page::Languages {
                        let collapsing = languages_expanded.get_untracked();
                        if collapsing
                            && matches!(
                                page.get_untracked(),
                                Page::Flutter | Page::Dart
                            )
                        {
                            navigate(p, page, history, index, query);
                        }
                        languages_expanded.set(!collapsing);
                    }
                    if let Some(id) = tree_focus.get_untracked() {
                        id.request_focus();
                    }
                })
                .style(|s| s.width(16.0)),
                label(move || title).style(|s| s.flex_grow(1.0_f32)),
            ))
            .on_click_stop(move |_| {
                navigate(p, page, history, index, query);
                if let Some(id) = tree_focus.get_untracked() {
                    id.request_focus();
                }
            })
            .style(move |s| {
                s.height(25.0)
                    .width_full()
                    .padding_left((18 + depth * 16) as f32)
                    .items_center()
                    .apply_if(
                        depth > 0
                            && !(if matches!(p, Page::Flutter | Page::Dart) {
                                languages_expanded.get()
                            } else {
                                appearance_expanded.get()
                            })
                            && query.with(|q| q.is_empty()),
                        |s| s.hide(),
                    )
                    .apply_if(page.get() == p, |s| {
                        s.background(
                            config
                                .get()
                                .color(SparrowColor::SPARROW_TAB_ACTIVE_BACKGROUND),
                        )
                    })
                    .hover(|s| s.cursor(floem::style::CursorStyle::Pointer))
            })
        },
    )
    .style(|s| s.flex_col().width_full());
    let tree_id = tree.id();
    tree_focus.set(Some(tree_id));
    let tree = keyboard
        .control(tree)
        .style(|s| s.focus(|s| s.background(floem::peniko::Color::TRANSPARENT)))
        .on_event(EventListener::KeyDown, move |event| {
            let Event::KeyDown(key) = event else {
                return EventPropagation::Continue;
            };
            if !key.modifiers.is_empty() {
                return EventPropagation::Continue;
            }
            let current = page.get_untracked();
            let pages = visible_pages(
                appearance_expanded.get_untracked(),
                languages_expanded.get_untracked(),
                !query.with_untracked(|q| q.is_empty()),
            );
            let position = pages.iter().position(|p| *p == current).unwrap_or(0);
            let destination =
                list_destination(&key.key.logical_key, position, pages.len())
                    .map(|i| pages[i]);
            let destination = match (&key.key.logical_key, current) {
                (Key::Named(NamedKey::ArrowRight), Page::Overview) => {
                    if appearance_expanded.get_untracked() {
                        Some(Page::Appearance)
                    } else {
                        appearance_expanded.set(true);
                        None
                    }
                }
                (Key::Named(NamedKey::ArrowRight), Page::Languages) => {
                    if languages_expanded.get_untracked() {
                        Some(Page::Flutter)
                    } else {
                        languages_expanded.set(true);
                        None
                    }
                }
                (Key::Named(NamedKey::ArrowLeft), Page::Overview) => {
                    appearance_expanded.set(false);
                    None
                }
                (Key::Named(NamedKey::ArrowLeft), Page::Languages) => {
                    languages_expanded.set(false);
                    None
                }
                (
                    Key::Named(NamedKey::ArrowLeft),
                    Page::Appearance | Page::System,
                ) => Some(Page::Overview),
                (Key::Named(NamedKey::ArrowLeft), Page::Flutter | Page::Dart) => {
                    Some(Page::Languages)
                }
                _ => destination,
            };
            if let Some(destination) = destination {
                navigate(destination, page, history, index, query);
            }
            if matches!(
                key.key.logical_key,
                Key::Named(
                    NamedKey::ArrowUp
                        | NamedKey::ArrowDown
                        | NamedKey::ArrowLeft
                        | NamedKey::ArrowRight
                        | NamedKey::Home
                        | NamedKey::End
                )
            ) {
                EventPropagation::Stop
            } else {
                EventPropagation::Continue
            }
        });
    let dialog = container(container(stack((
        stack((empty().style(|s|s.width(26.0)),label(move ||format!("Settings – {}",workspace.display().unwrap_or_else(||"Sparrow".into()))).style(|s|s.flex_grow(1.0_f32).items_center().justify_center().font_bold()),clickable_icon(||SparrowIcons::CLOSE,close,||false,||false,||"Close Settings",config)))
            .style(move |s|s.height(36.0).width_full().items_center().flex_shrink(0.0_f32).background(floem::peniko::Color::from_rgb8(40,40,40))),
        stack((
            stack((stack((
                svg(move ||config.get().ui_svg(SparrowIcons::SEARCH)).style(move |s|s.size(14.0,14.0).margin_left(6.0).color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))),
                search.style(|s|s.height_full().flex_grow(1.0_f32).min_width(0.0).padding_horiz(4.0)),
            )).style(move |s|s.margin_horiz(8.0).margin_top(11.0).margin_bottom(9.0).height(24.0).items_center().border(1.0).border_radius(3.0).border_color(config.get().color(SparrowColor::SPARROW_BORDER))),
                scope_picker.style(|s|s.margin_horiz(8.0).margin_bottom(8.0)),
                tree,
            )).style(move |s|s.width(256.0).height_full().flex_col().flex_shrink(0.0_f32).border_right(1.0).border_color(config.get().color(SparrowColor::SPARROW_BORDER))),
            stack((
                stack((label(move ||if query.with(|q|q.is_empty()){page.get().title()}else{"Search results"}).style(|s|s.font_bold().flex_grow(1.0_f32)),
                    keyboard.icon(||SparrowIcons::LOCATION_BACKWARD,move ||{index.update(|i|*i=i.saturating_sub(1));page.set(history.with_untracked(|h|h[index.get_untracked()]));query.set(String::new());},||false,move ||index.get()==0,||"Back",config),
                    keyboard.icon(||SparrowIcons::LOCATION_FORWARD,move ||{index.update(|i|*i+=1);page.set(history.with_untracked(|h|h[index.get_untracked()]));query.set(String::new());},||false,move ||index.get()+1>=history.with(Vec::len),||"Forward",config),
                )).style(|s|s.height(32.0).items_center().flex_shrink(0.0_f32)),
                label(move ||error.get()).style(move |s|s.color(config.get().color(SparrowColor::SPARROW_ERROR)).text_ellipsis().apply_if(error.with(|e|e.is_empty()),|s|s.hide())),
                scroll(stack((overview,external_button,
                    label(||"No settings match your search.").style(move |s|s.margin_top(12.0).color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM)).apply_if(query.with(|q|q.is_empty())||!filtered.with(|r|r.is_empty()),|s|s.hide())),
                    dyn_stack(move ||filtered.get(),move |item|(epoch.get(),item.kind.clone(),item.field.clone(),initial(&item.value).map(|v|v.0)),move |item|setting_row(item,pending,rows_keyboard.clone(),project_scope,query,config)).style(|s|s.flex_col().width_full()),
                )).style(|s|s.flex_col().min_width_full())).style(|s|s.flex_grow(1.0_f32).flex_basis(0.0).min_height(0.0).width_full()),
            )).style(|s|s.flex_col().height_full().flex_grow(1.0_f32).flex_basis(0.0).min_width(0.0).padding_horiz(16.0).padding_top(8.0)),
        )).style(|s|s.flex_grow(1.0_f32).flex_basis(0.0).min_height(0.0).width_full()),
        stack((tooltip_label(config,label(||"?").style(move |s|s.size(22.0,22.0).border_radius(11.0).border(1.0).items_center().justify_center().border_color(config.get().color(SparrowColor::SPARROW_BORDER))),||"Search by setting name or description. Changes are saved only by Apply or OK."),empty().style(|s|s.flex_grow(1.0_f32)),button("OK").on_click_stop(move |_|{if okay(){close();}}).style(move |s|s.background(config.get().color(SparrowColor::SPARROW_BUTTON_PRIMARY_BACKGROUND))),button("Cancel").on_click_stop(move |_|close()),button("Apply").on_click_stop(move |_|{save();}).disabled(move ||!dirty.get())))
            .style(move |s|s.height(46.0).width_full().items_center().gap(12.0).padding_horiz(16.0).flex_shrink(0.0_f32).border_top(1.0).border_color(config.get().color(SparrowColor::SPARROW_BORDER))),
    )).style(|s|s.flex_col().size_full())).style(move |s|s.font_family(config.get().ui.font_family.clone()).font_size(13.0).width(900.0).height(700.0).max_width_pct(96.0).max_height_pct(92.0).flex_col().border_radius(8.0).color(config.get().color(SparrowColor::EDITOR_FOREGROUND)).background(config.get().color(SparrowColor::PANEL_BACKGROUND))))
    .on_event_stop(EventListener::PointerDown,|_|{})
    .on_event(EventListener::KeyDown,move |event|{if let Event::KeyDown(key)=event{if key.key.logical_key==Key::Named(NamedKey::Escape){close();return EventPropagation::Stop;}}EventPropagation::Continue})
    .style(move |s|s.absolute().size_full().padding_bottom(100.0).items_center().justify_center().z_index(210).background(floem::peniko::Color::from_rgba8(0,0,0,70)).apply_if(!visible.get(),|s|s.hide()));
    keyboard.bind_root(dialog, visible, config)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn category_navigation_skips_collapsed_children_and_retains_search_results() {
        let collapsed = visible_pages(false, false, false);
        assert!(!collapsed.contains(&Page::Appearance));
        assert!(!collapsed.contains(&Page::Flutter));
        let appearance = visible_pages(true, false, false);
        assert!(appearance.contains(&Page::Appearance));
        assert!(appearance.contains(&Page::System));
        assert!(!appearance.contains(&Page::Dart));
        let languages = visible_pages(false, true, false);
        assert!(languages.contains(&Page::Flutter));
        assert!(languages.contains(&Page::Dart));
        assert!(!languages.contains(&Page::Appearance));
        assert_eq!(visible_pages(false, false, true).len(), PAGES.len());
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
