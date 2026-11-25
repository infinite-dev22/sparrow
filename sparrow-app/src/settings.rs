use std::rc::Rc;

use floem::{
    keyboard::Modifiers,
    peniko::kurbo::{Point, Size},
    reactive::{RwSignal, Scope, SignalGet, SignalUpdate},
    views::VirtualVector,
};
use indexmap::IndexMap;
use inflector::Inflector;
use serde::Serialize;
use serde_json::Value;
use sparrow_core::mode::Mode;
use sparrow_rpc::plugin::VoltID;

use crate::{
    command::CommandExecuted,
    config::{
        DropdownInfo, ai_commit::AiCommitConfig, core::CoreConfig,
        editor::EditorConfig, terminal::TerminalConfig, ui::UIConfig,
    },
    keypress::KeyPressFocus,
    plugin::InstalledVoltData,
    window_tab::CommonData,
};

#[derive(Debug, Clone)]
pub enum SettingsValue {
    Float(f64),
    Integer(i64),
    String(String),
    Bool(bool),
    Dropdown(DropdownInfo),
    Empty,
}

impl From<serde_json::Value> for SettingsValue {
    fn from(v: serde_json::Value) -> Self {
        match v {
            serde_json::Value::Number(n) => {
                if n.is_f64() {
                    SettingsValue::Float(n.as_f64().unwrap())
                } else {
                    SettingsValue::Integer(n.as_i64().unwrap())
                }
            }
            serde_json::Value::String(s) => SettingsValue::String(s),
            serde_json::Value::Bool(b) => SettingsValue::Bool(b),
            _ => SettingsValue::Empty,
        }
    }
}

#[derive(Clone, Debug)]
struct SettingsItem {
    kind: String,
    name: String,
    field: String,
    description: String,
    filter_text: String,
    value: SettingsValue,
    serde_value: Value,
    pos: RwSignal<Point>,
    size: RwSignal<Size>,
    // this is only the header that give an visual separation between different type of settings
    header: bool,
}

#[derive(Clone, Debug)]
struct SettingsData {
    items: RwSignal<im::Vector<SettingsItem>>,
    kinds: RwSignal<im::Vector<(String, RwSignal<Point>)>>,
    plugin_items: RwSignal<im::Vector<SettingsItem>>,
    plugin_kinds: RwSignal<im::Vector<(String, RwSignal<Point>)>>,
    filtered_items: RwSignal<im::Vector<SettingsItem>>,
    common: Rc<CommonData>,
}

impl KeyPressFocus for SettingsData {
    fn get_mode(&self) -> sparrow_core::mode::Mode {
        Mode::Insert
    }

    fn check_condition(
        &self,
        _condition: crate::keypress::condition::Condition,
    ) -> bool {
        false
    }

    fn run_command(
        &self,
        _command: &crate::command::SparrowCommand,
        _count: Option<usize>,
        _mods: Modifiers,
    ) -> crate::command::CommandExecuted {
        CommandExecuted::No
    }

    fn receive_char(&self, _c: &str) {}
}

impl VirtualVector<SettingsItem> for SettingsData {
    fn total_len(&self) -> usize {
        self.filtered_items.get_untracked().len()
    }

    fn slice(
        &mut self,
        _range: std::ops::Range<usize>,
    ) -> impl Iterator<Item = SettingsItem> {
        Box::new(self.filtered_items.get().into_iter())
    }
}

impl SettingsData {
    pub fn new(
        cx: Scope,
        installed_plugin: RwSignal<IndexMap<VoltID, InstalledVoltData>>,
        common: Rc<CommonData>,
    ) -> Self {
        fn into_settings_map(
            data: &impl Serialize,
        ) -> serde_json::Map<String, serde_json::Value> {
            match serde_json::to_value(data).unwrap() {
                serde_json::Value::Object(h) => h,
                _ => serde_json::Map::default(),
            }
        }

        let config = common.config;
        let plugin_items = cx.create_rw_signal(im::Vector::new());
        let plugin_kinds = cx.create_rw_signal(im::Vector::new());
        let filtered_items = cx.create_rw_signal(im::Vector::new());
        let items = cx.create_rw_signal(im::Vector::new());
        let kinds = cx.create_rw_signal(im::Vector::new());
        cx.create_effect(move |_| {
            let config = config.get();

            let mut data_items = im::Vector::new();
            let mut data_kinds = im::Vector::new();
            let mut item_height_accum = 0.0;
            for (kind, fields, descs, mut settings_map) in [
                (
                    "Core",
                    &CoreConfig::FIELDS[..],
                    &CoreConfig::DESCS[..],
                    into_settings_map(&config.core),
                ),
                (
                    "AI-Commit",
                    &AiCommitConfig::FIELDS[..],
                    &AiCommitConfig::DESCS[..],
                    into_settings_map(&config.ai_commit),
                ),
                (
                    "Editor",
                    &EditorConfig::FIELDS[..],
                    &EditorConfig::DESCS[..],
                    into_settings_map(&config.editor),
                ),
                (
                    "UI",
                    &UIConfig::FIELDS[..],
                    &UIConfig::DESCS[..],
                    into_settings_map(&config.ui),
                ),
                (
                    "Terminal",
                    &TerminalConfig::FIELDS[..],
                    &TerminalConfig::DESCS[..],
                    into_settings_map(&config.terminal),
                ),
            ] {
                let pos = cx.create_rw_signal(Point::new(0.0, item_height_accum));
                data_items.push_back(SettingsItem {
                    kind: kind.to_string(),
                    name: "".to_string(),
                    field: "".to_string(),
                    filter_text: "".to_string(),
                    description: "".to_string(),
                    value: SettingsValue::Empty,
                    serde_value: Value::Null,
                    pos,
                    size: cx.create_rw_signal(Size::ZERO),
                    header: true,
                });
                data_kinds.push_back((kind.to_string(), pos));
                for (name, desc) in fields.iter().zip(descs.iter()) {
                    let field = name.replace('_', "-");

                    let (value, serde_value) = if let Some(dropdown) =
                        config.get_dropdown_info(&kind.to_lowercase(), &field)
                    {
                        let index = dropdown.active_index;
                        (
                            SettingsValue::Dropdown(dropdown),
                            Value::Number(index.into()),
                        )
                    } else {
                        let value = settings_map.remove(&field).unwrap();
                        (SettingsValue::from(value.clone()), value)
                    };

                    let name = format!(
                        "{kind}: {}",
                        name.replace('_', " ").to_title_case()
                    );
                    let kind = kind.to_lowercase();
                    let filter_text = format!("{kind} {name} {desc}").to_lowercase();
                    let filter_text =
                        format!("{filter_text}{}", filter_text.replace(' ', ""));
                    data_items.push_back(SettingsItem {
                        kind,
                        name,
                        field,
                        filter_text,
                        description: desc.to_string(),
                        value,
                        pos: cx.create_rw_signal(Point::ZERO),
                        size: cx.create_rw_signal(Size::ZERO),
                        serde_value,
                        header: false,
                    });
                    item_height_accum += 50.0;
                }
            }

            filtered_items.set(data_items.clone());
            items.set(data_items);

            let plugins = installed_plugin.get();
            let mut setting_items = im::Vector::new();
            let mut plugin_kinds_tmp = im::Vector::new();
            for (_, volt) in plugins {
                let meta = volt.meta.get();
                let kind = meta.name;
                let plugin_config = config.plugins.get(&kind);
                if let Some(config) = meta.config {
                    let pos =
                        cx.create_rw_signal(Point::new(0.0, item_height_accum));
                    setting_items.push_back(SettingsItem {
                        kind: meta.display_name.clone(),
                        name: "".to_string(),
                        field: "".to_string(),
                        filter_text: "".to_string(),
                        description: "".to_string(),
                        value: SettingsValue::Empty,
                        serde_value: Value::Null,
                        pos,
                        size: cx.create_rw_signal(Size::ZERO),
                        header: true,
                    });
                    plugin_kinds_tmp.push_back((meta.display_name.clone(), pos));

                    {
                        let mut local_items = Vec::new();
                        for (name, config) in config {
                            let field = name.clone();

                            let name = format!(
                                "{}: {}",
                                meta.display_name,
                                name.replace('_', " ").to_title_case()
                            );
                            let desc = config.description;
                            let filter_text =
                                format!("{kind} {name} {desc}").to_lowercase();
                            let filter_text = format!(
                                "{filter_text}{}",
                                filter_text.replace(' ', "")
                            );

                            let value = plugin_config
                                .and_then(|config| config.get(&field).cloned())
                                .unwrap_or(config.default);
                            let value = SettingsValue::from(value);

                            let item = SettingsItem {
                                kind: kind.clone(),
                                name,
                                field,
                                filter_text,
                                description: desc.to_string(),
                                value,
                                pos: cx.create_rw_signal(Point::ZERO),
                                size: cx.create_rw_signal(Size::ZERO),
                                serde_value: Value::Null,
                                header: false,
                            };
                            local_items.push(item);
                            item_height_accum += 50.0;
                        }
                        local_items.sort_by_key(|i| i.name.clone());
                        setting_items.extend(local_items.into_iter());
                    }
                }
            }
            plugin_items.set(setting_items);
            plugin_kinds.set(plugin_kinds_tmp);
            kinds.set(data_kinds);
        });

        Self {
            filtered_items,
            plugin_items,
            plugin_kinds,
            items,
            kinds,
            common,
        }
    }
}

pub mod modal;
mod setting_row;
mod theme_colors;
mod view;

pub use theme_colors::theme_color_settings_view;
pub use view::settings_view;
