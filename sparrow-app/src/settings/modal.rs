//! Reference-style modal settings with deferred, validated edits.
use super::{SettingsData, SettingsItem, SettingsValue};
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
    menu::{Menu, MenuItem},
    reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith},
    views::{
        Decorators, container, dyn_stack, empty, label, scroll, stack, svg, text_input,
    },
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
}
const PAGES: [(Page, &str, usize); 8] = [
    (Page::Overview, "Appearance & Behavior", 0),
    (Page::Appearance, "Appearance", 1),
    (Page::System, "System Settings", 1),
    (Page::Keymap, "Keymap", 0),
    (Page::Editor, "Editor", 0),
    (Page::Plugins, "Plugins", 0),
    (Page::Run, "Build, Execution, Deployment", 0),
    (Page::Terminal, "Tools › Terminal", 0),
];
impl Page {
    fn title(self) -> &'static str {
        PAGES.iter().find(|(p, _, _)| *p == self).unwrap().1
    }
}
#[derive(Clone)]
enum Type {
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
        Page::System => item.kind == "core" && !appearance,
        Page::Editor => item.kind == "editor",
        Page::Terminal => item.kind == "terminal",
        Page::Plugins => {
            !["core", "ui", "editor", "terminal"].contains(&item.kind.as_str())
        }
        _ => false,
    }
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
    config: floem::reactive::ReadSignal<std::sync::Arc<SparrowConfig>>,
) -> impl View {
    let (original, kind) = initial(&item.value).unwrap();
    let key = (item.kind.clone(), item.field.clone());
    let raw = pending
        .with_untracked(|p| p.get(&key).map(|v| v.raw.clone()))
        .unwrap_or_else(|| original.clone());
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
        SettingsValue::Dropdown(dropdown) => {
            label(move || format!("{}  ⌄", value.get()))
                .popout_menu(move || {
                    let mut menu = Menu::new("");
                    for choice in &dropdown.items {
                        let choice = choice.clone();
                        let text = choice.clone();
                        menu = menu.entry(
                            MenuItem::new(text)
                                .action(move || value.set(choice.clone())),
                        );
                    }
                    menu
                })
                .style(move |s| {
                    s.min_width(220.0)
                        .height(26.0)
                        .padding_horiz(8.0)
                        .items_center()
                        .border(1.0)
                        .border_radius(3.0)
                        .border_color(
                            config.get().color(SparrowColor::SPARROW_BORDER),
                        )
                })
                .into_any()
        }
        _ => text_input(value)
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
    let name = item
        .name
        .split_once(": ")
        .map(|(_, n)| n.to_string())
        .unwrap_or(item.name);
    stack((
        label(move || name.clone()).style(|s| s.font_bold().margin_bottom(6.0)),
        control,
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
    let data = SettingsData::new(cx, window.plugin.installed, window.common.clone());
    let items = data.items;
    let plugins = data.plugin_items;
    let pending = cx.create_rw_signal(Pending::new());
    let query = cx.create_rw_signal(String::new());
    let page = cx.create_rw_signal(Page::Overview);
    let appearance_expanded = cx.create_rw_signal(false);
    cx.create_effect(move |_| {
        if matches!(page.get(), Page::Appearance | Page::System) {
            appearance_expanded.set(true);
        }
    });
    let history = cx.create_rw_signal(vec![Page::Overview]);
    let index = cx.create_rw_signal(0usize);
    let epoch = cx.create_rw_signal(0usize);
    let error = cx.create_rw_signal(String::new());
    let search = text_input(query);
    let search_id = search.id();
    let root_id = window.common.view_id;
    let close = move || {
        visible.set(false);
        pending.set(Pending::new());
        root_id.get_untracked().request_focus();
    };
    cx.create_effect(move |_| {
        if visible.get() {
            pending.set(Pending::new());
            query.set(String::new());
            error.set(String::new());
            epoch.update(|n| *n += 1);
            search_id.request_focus();
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
                        item.filter_text.contains(&pattern)
                    }
                })
                .cloned()
                .collect()
        }));
    });
    let apply: Rc<dyn Fn() -> bool> = {
        let window = window.clone();
        Rc::new(move || {
            if pending.with_untracked(|p| p.is_empty()) {
                error.set(String::new());
                return true;
            }
            let edits = pending.get_untracked();
            let result = edits
                .into_iter()
                .map(|((section, key), edit)| {
                    edit.value(&section)
                        .map(|value| (section.clone(), key.clone(), value))
                        .map_err(|e| format!("{section}.{key}: {e}"))
                })
                .collect::<Result<Vec<_>, _>>()
                .and_then(SparrowConfig::update_settings_batch);
            match result {
                Ok(()) => {
                    pending.set(Pending::new());
                    error.set(String::new());
                    window.reload_config();
                    true
                }
                Err(e) => {
                    error.set(e);
                    false
                }
            }
        })
    };
    let okay = apply.clone();
    let save = apply.clone();
    let external = window.clone();
    let overview=stack((
        label(||"Customize IDE appearance and behavior: change themes and font size, tune the keymap, configure plugins and system settings.").style(|s|s.max_width_full().margin_bottom(10.0)),
        dyn_stack(||vec![Page::Appearance,Page::System,Page::Editor,Page::Terminal,Page::Keymap,Page::Plugins],|p|*p as u8,move |p|label(move ||p.title()).on_click_stop(move |_|navigate(p,page,history,index,query)).style(move |s|s.margin_left(24.0).height(22.0).color(config.get().color(SparrowColor::EDITOR_LINK)).hover(|s|s.cursor(floem::style::CursorStyle::Pointer)))),
    )).style(move |s|s.flex_col().width_full().apply_if(page.get()!=Page::Overview||!query.with(|q|q.is_empty()),|s|s.hide()));
    let launch_page = page;
    let external_button = label(move || match launch_page.get() {
        Page::Keymap => "Open Keymap Editor",
        Page::Run => "Edit Run/Debug Configurations…",
        _ => "Manage Plugins…",
    })
    .on_click_stop(move |_| {
        if !pending.with_untracked(|p| p.is_empty()) {
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
    let workspace = window.workspace.clone();
    let button = move |text: &'static str| {
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
                        config.get().color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                    )
                })
        })
    };
    container(container(stack((
        stack((empty().style(|s|s.width(26.0)),label(move ||format!("Settings – {}",workspace.display().unwrap_or_else(||"Sparrow".into()))).style(|s|s.flex_grow(1.0_f32).items_center().justify_center().font_bold()),clickable_icon(||SparrowIcons::CLOSE,close,||false,||false,||"Close Settings",config)))
            .style(move |s|s.height(36.0).items_center().flex_shrink(0.0_f32).background(config.get().color(SparrowColor::SPARROW_WINDOW_BACKGROUND))),
        stack((
            stack((stack((
                svg(move ||config.get().ui_svg(SparrowIcons::SEARCH)).style(move |s|s.size(14.0,14.0).margin_left(6.0).color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))),
                search.style(|s|s.height_full().flex_grow(1.0_f32).min_width(0.0).padding_horiz(4.0)),
            )).style(move |s|s.margin(8.0).height(24.0).items_center().border(1.0).border_radius(3.0).border_color(config.get().color(SparrowColor::SPARROW_BORDER))),
                dyn_stack(||PAGES.to_vec(),|(p,_,_)|*p as u8,move |(p,title,depth)|stack((
                    label(move ||if p==Page::Overview { if appearance_expanded.get(){"⌄"}else{"›"} }else{""})
                        .on_click_stop(move |_|{if p==Page::Overview {appearance_expanded.update(|v|*v=!*v);}}).style(|s|s.width(16.0)),
                    label(move ||title).style(|s|s.flex_grow(1.0_f32)),
                )).on_click_stop(move |_|navigate(p,page,history,index,query)).style(move |s|s.height(25.0).width_full().padding_left((18+depth*16) as f32).items_center()
                    .apply_if(depth>0&&!appearance_expanded.get()&&query.with(|q|q.is_empty()),|s|s.hide())
                    .apply_if(page.get()==p,|s|s.background(config.get().color(SparrowColor::SPARROW_TAB_ACTIVE_BACKGROUND))).hover(|s|s.cursor(floem::style::CursorStyle::Pointer)))),
            )).style(move |s|s.width(256.0).height_full().flex_col().flex_shrink(0.0_f32).border_right(1.0).border_color(config.get().color(SparrowColor::SPARROW_BORDER))),
            stack((
                stack((label(move ||if query.with(|q|q.is_empty()){page.get().title()}else{"Search results"}).style(|s|s.font_bold().flex_grow(1.0_f32)),
                    clickable_icon(||SparrowIcons::LOCATION_BACKWARD,move ||{index.update(|i|*i=i.saturating_sub(1));page.set(history.with_untracked(|h|h[index.get_untracked()]));query.set(String::new());},||false,move ||index.get()==0,||"Back",config),
                    clickable_icon(||SparrowIcons::LOCATION_FORWARD,move ||{index.update(|i|*i+=1);page.set(history.with_untracked(|h|h[index.get_untracked()]));query.set(String::new());},||false,move ||index.get()+1>=history.with(Vec::len),||"Forward",config),
                )).style(|s|s.height(32.0).items_center().flex_shrink(0.0_f32)),
                label(move ||error.get()).style(move |s|s.color(config.get().color(SparrowColor::SPARROW_ERROR)).text_ellipsis().apply_if(error.with(|e|e.is_empty()),|s|s.hide())),
                scroll(stack((overview,external_button,
                    label(||"No settings match your search.").style(move |s|s.margin_top(12.0).color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM)).apply_if(query.with(|q|q.is_empty())||!filtered.with(|r|r.is_empty()),|s|s.hide())),
                    dyn_stack(move ||filtered.get(),move |item|(epoch.get(),item.kind.clone(),item.field.clone(),initial(&item.value).map(|v|v.0)),move |item|setting_row(item,pending,config)).style(|s|s.flex_col().width_full()),
                )).style(|s|s.flex_col().min_width_full())).style(|s|s.flex_grow(1.0_f32).flex_basis(0.0).min_height(0.0).width_full()),
            )).style(|s|s.flex_col().height_full().flex_grow(1.0_f32).flex_basis(0.0).min_width(0.0).padding_horiz(16.0).padding_top(8.0)),
        )).style(|s|s.flex_grow(1.0_f32).flex_basis(0.0).min_height(0.0).width_full()),
        stack((tooltip_label(config,label(||"?").style(move |s|s.size(22.0,22.0).border_radius(11.0).border(1.0).items_center().justify_center().border_color(config.get().color(SparrowColor::SPARROW_BORDER))),||"Search by setting name or description. Changes are saved only by Apply or OK."),empty().style(|s|s.flex_grow(1.0_f32)),button("OK").on_click_stop(move |_|{if okay(){close();}}).style(move |s|s.background(config.get().color(SparrowColor::SPARROW_BUTTON_PRIMARY_BACKGROUND))),button("Cancel").on_click_stop(move |_|close()),button("Apply").on_click_stop(move |_|{save();}).disabled(move ||pending.with(|p|p.is_empty()))))
            .style(move |s|s.height(46.0).items_center().gap(12.0).padding_horiz(16.0).flex_shrink(0.0_f32).border_top(1.0).border_color(config.get().color(SparrowColor::SPARROW_BORDER))),
    ))).style(move |s|s.width(900.0).height(700.0).max_width_pct(96.0).max_height_pct(92.0).flex_col().border_radius(8.0).background(config.get().color(SparrowColor::PANEL_BACKGROUND))))
    .on_event_stop(EventListener::PointerDown,|_|{})
    .on_event(EventListener::KeyDown,move |event|{if let Event::KeyDown(key)=event{if key.key.logical_key==Key::Named(NamedKey::Escape){close();return EventPropagation::Stop;}}EventPropagation::Continue})
    .style(move |s|s.absolute().size_full().items_center().justify_center().z_index(210).background(floem::peniko::Color::from_rgba8(0,0,0,70)).apply_if(!visible.get(),|s|s.hide()))
}
#[cfg(test)]
mod tests {
    use super::*;
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
