//! Reference-style modal settings with deferred, validated edits.
use super::{SettingsData, SettingsItem, SettingsValue};
use crate::widgets::modal_focus::ModalFocus;
use crate::{
    config::{SparrowConfig, color::SparrowColor, icon::SparrowIcons},
    panel::kind::PanelKind,
    widgets::{
        Divider, breadcrumb_item, dialog_surface,
        floating_search::search_field_with_style,
    },
    window_tab::WindowTabData,
};
use floem::{
    View,
    event::{Event, EventListener, EventPropagation},
    keyboard::{Key, NamedKey},
    reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith},
    views::{Decorators, container, dyn_stack, empty, label, stack},
};
use std::{collections::BTreeMap, rc::Rc};
mod layout;
mod navigation;
mod setting_row;
mod tokens;

use crate::widgets::sparrow_scroll;
use navigation::navigation_tree;
use setting_row::setting_row;

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
    let page = cx.create_rw_signal(Page::Appearance);
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
    let history = cx.create_rw_signal(vec![Page::Appearance]);
    let index = cx.create_rw_signal(0usize);
    let epoch = cx.create_rw_signal(0usize);
    let error = cx.create_rw_signal(String::new());
    let help_open = cx.create_rw_signal(false);
    let search = keyboard.input(query, config);
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
            help_open.set(false);
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
                scope_drafts.update(|drafts| {
                    drafts[project_scope.get_untracked() as usize] =
                        pending.get_untracked();
                });
                pending.set(
                    scope_drafts
                        .with_untracked(|drafts| drafts[project as usize].clone()),
                );
                project_scope.set(project);
                error.set(String::new());
                epoch.update(|epoch| *epoch += 1);
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
    let [button_width, button_height] =
        tokens::control_minimum_size("Button.minimumSize");
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
            .style(move |s| s.min_width(button_width).min_height(button_height)),
            move || text != "Apply" || dirty.get_untracked(),
        )
    };
    let rows_keyboard = keyboard.clone();
    let tree = navigation_tree(
        cx,
        &keyboard,
        config,
        page,
        history,
        index,
        query,
        appearance_expanded,
        languages_expanded,
    );
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
    let gap = tokens::length("search", "icon_gap");
    let [_, search_height] = tokens::control_minimum_size("TextField.minimumSize");
    let focus_width = tokens::length("dark", "Component.focusWidth");
    let focus_color = tokens::color("dark", "Component.focusColor");
    let search = search_field_with_style(
        search,
        search_clear,
        config,
        SparrowIcons::SEARCH,
        icon_grid,
        move |s| {
            s.height(search_height)
                .padding_horiz(gap)
                .gap(gap)
                .border_radius(tokens::length(
                    "search",
                    "inner_outline_radius_at_1x",
                ))
        },
        move |s| s.outline(focus_width).outline_color(focus_color),
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
            !query.with(String::is_empty)
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
                query.with(String::is_empty)
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
                    if query.with(String::is_empty) {
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
                            query.with(String::is_empty)
                                || !filtered.with(Vec::is_empty),
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
                            query,
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

    let footer_gap = tokens::length("footer", "button_base_gap");
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
            button(tokens::text("footer", "ok_label")).on_click_stop(move |_| {
                if okay() {
                    close();
                }
            }),
            button(tokens::text("footer", "cancel_label")).on_click_stop(move |_| close()),
            button(tokens::text("footer", "apply_label"))
                .on_click_stop(move |_| { save(); })
                .disabled(move || !dirty.get()),
        ))
        .style(move |s| {
            tokens::padding(s, "footer", "compact_footer_insets")
                .width_full()
                .items_center()
                .gap(footer_gap)
                .flex_wrap(floem::style::FlexWrap::Wrap)
        }),
    ))
    .style(|s| s.width_full().flex_col().flex_shrink(0.0_f32));

    let [preferred_width, preferred_height] =
        tokens::size("dialog", "root_minimum_size");
    let card = stack((
        title,
        layout::settings_split(sidebar, page_content, config),
        footer,
    ))
    .style(move |s| {
        let config = config.get();
        // In a workbench overlay, clamp the requested size to the available viewport.
        dialog_surface(s, &config)
            .font_family(config.ui.font_family.clone())
            .font_size(13.0)
            .width(preferred_width)
            .height(preferred_height)
            .max_width_pct(96.0)
            .max_height_pct(92.0)
            .min_height(0.0)
            .flex_col()
            .border_radius(8.0)
    });
    let dialog = container(card)
        .on_event_stop(EventListener::PointerDown, |_| {})
        .on_event(EventListener::KeyDown, move |event| {
            if let Event::KeyDown(key) = event
                && key.key.logical_key == Key::Named(NamedKey::Escape)
            {
                close();
                return EventPropagation::Stop;
            }
            EventPropagation::Continue
        })
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
