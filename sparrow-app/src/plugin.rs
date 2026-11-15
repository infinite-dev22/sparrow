//! Local appearance themes. Executable language plugins and the old marketplace
//! are deliberately absent; Dart services come from the configured SDK.
use crate::{
    command::{
        CommandExecuted, InternalCommand, SparrowCommand, SparrowWorkbenchCommand,
    },
    keypress::{KeyPressFocus, condition::Condition},
    main_split::Editors,
    window_tab::CommonData,
};
use floem::{
    View,
    keyboard::Modifiers,
    reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith},
    views::{Decorators, label, stack},
};
use indexmap::IndexMap;
use sparrow_core::mode::Mode;
use sparrow_rpc::{
    core::CoreRpcHandler,
    plugin::{VoltID, VoltInfo, VoltMetadata},
};
use std::{collections::HashSet, rc::Rc};
#[derive(Clone, Debug)]
pub struct InstalledVoltData {
    pub meta: RwSignal<VoltMetadata>,
}
/// The historic name/metadata IDs are retained for saved theme tabs and settings.
#[derive(Clone, Debug)]
pub struct PluginData {
    pub installed: RwSignal<IndexMap<VoltID, InstalledVoltData>>,
    pub common: Rc<CommonData>,
}
impl PluginData {
    pub fn new(
        cx: Scope,
        _disabled: HashSet<VoltID>,
        _workspace_disabled: HashSet<VoltID>,
        _editors: Editors,
        common: Rc<CommonData>,
        _core_rpc: CoreRpcHandler,
    ) -> Self {
        let data = Self {
            installed: cx.create_rw_signal(IndexMap::new()),
            common,
        };
        data.reload_themes();
        data
    }
    pub fn reload_themes(&self) {
        let themes = sparrow_proxy::plugin::themes::find_themes(
            &self.common.window_common.extra_plugin_paths,
        );
        self.installed.set(IndexMap::new());
        for theme in themes {
            self.volt_installed(&theme, &None);
        }
        self.common
            .internal_command
            .send(InternalCommand::ReloadConfig);
    }
    pub fn volt_installed(&self, meta: &VoltMetadata, _icon: &Option<Vec<u8>>) {
        if meta.wasm.is_some()
            || (meta.color_themes.as_ref().is_none_or(Vec::is_empty)
                && meta.icon_themes.as_ref().is_none_or(Vec::is_empty))
        {
            return;
        }
        self.installed.update(|items| {
            items.insert(
                meta.id(),
                InstalledVoltData {
                    meta: self.common.scope.create_rw_signal(meta.clone()),
                },
            );
        });
    }
    pub fn volt_removed(&self, meta: &VoltInfo) {
        self.installed.update(|items| {
            items.shift_remove(&meta.id());
        });
    }
}
impl KeyPressFocus for PluginData {
    fn get_mode(&self) -> Mode {
        Mode::Insert
    }
    fn check_condition(&self, condition: Condition) -> bool {
        matches!(condition, Condition::PanelFocus)
    }
    fn run_command(
        &self,
        _command: &SparrowCommand,
        _count: Option<usize>,
        _mods: Modifiers,
    ) -> CommandExecuted {
        CommandExecuted::No
    }
    fn receive_char(&self, _text: &str) {}
}
pub fn plugin_info_view(plugin: PluginData, id: VoltID) -> impl View {
    let installed = plugin.installed;
    let common = plugin.common;
    stack((label(move ||installed.with(|items|items.get(&id).map(|theme|theme.meta.get().display_name).unwrap_or_else(||"Theme no longer installed".into()))).style(|s|s.font_bold().margin_bottom(12.0)),label(||"Sparrow supports local color and icon themes. Dart and Flutter language services are built in and use the Dart SDK."),label(||"Choose color theme").on_click_stop(move |_|common.workbench_command.send(SparrowWorkbenchCommand::ChangeColorTheme)))).style(|s|s.size_full().flex_col().padding(24.0))
}
