use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use ::core::slice;
use floem::{peniko::Color, prelude::palette::css};
use itertools::Itertools;
use lsp_types::{CompletionItemKind, SymbolKind};
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use serde::Deserialize;
use sparrow_core::directory::Directory;
use sparrow_proxy::plugin::themes::find_themes;
use sparrow_rpc::plugin::VoltID;
use strum::VariantNames;
use tracing::error;

use self::svg::missing_icon_svg;
use self::{
    color::SparrowColor,
    color_theme::{ColorThemeConfig, ThemeColor, ThemeColorPreference},
    core::CoreConfig,
    editor::{EditorConfig, SCALE_OR_SIZE_LIMIT, WrapStyle},
    icon::SparrowIcons,
    icon_theme::IconThemeConfig,
    svg::SvgStore,
    terminal::TerminalConfig,
    ui::UIConfig,
};
use crate::workspace::{SparrowWorkspace, SparrowWorkspaceType};

pub mod ai_commit;
pub mod color;
pub mod color_theme;
pub mod core;
pub mod editor;
pub mod icon;
pub mod icon_theme;
pub mod svg;
pub mod terminal;
pub mod ui;
pub mod watcher;

pub const LOGO: &str = include_str!("../../extra/images/logo.svg");
const DEFAULT_SETTINGS: &str = include_str!("../../defaults/settings.toml");
const DEFAULT_LIGHT_THEME: &str = include_str!("../../defaults/light-theme.toml");
const DEFAULT_DARK_THEME: &str = include_str!("../../defaults/dark-theme.toml");
const DEFAULT_ISLANDS_DARK_THEME: &str =
    include_str!("../../defaults/islands-dark-theme.toml");
const DEFAULT_ICON_THEME: &str = include_str!("../../defaults/icon-theme.toml");

static DEFAULT_CONFIG: Lazy<config::Config> =
    Lazy::new(SparrowConfig::default_config);
static DEFAULT_SPARROW_CONFIG: Lazy<SparrowConfig> =
    Lazy::new(SparrowConfig::default_sparrow_config);

static DEFAULT_DARK_THEME_CONFIG: Lazy<config::Config> = Lazy::new(|| {
    config::Config::builder()
        .add_source(config::File::from_str(
            DEFAULT_DARK_THEME,
            config::FileFormat::Toml,
        ))
        .build()
        .unwrap()
});

/// The default theme is the dark theme.
static DEFAULT_DARK_THEME_COLOR_CONFIG: Lazy<ColorThemeConfig> = Lazy::new(|| {
    let (_, theme) =
        SparrowConfig::load_color_theme_from_str(DEFAULT_DARK_THEME).unwrap();
    theme.get::<ColorThemeConfig>("color-theme")
    .expect("Failed to load default dark theme. This is likely due to a missing or misnamed field in dark-theme.toml")
});

static DEFAULT_ICON_THEME_CONFIG: Lazy<config::Config> = Lazy::new(|| {
    config::Config::builder()
        .add_source(config::File::from_str(
            DEFAULT_ICON_THEME,
            config::FileFormat::Toml,
        ))
        .build()
        .unwrap()
});
static DEFAULT_ICON_THEME_ICON_CONFIG: Lazy<IconThemeConfig> = Lazy::new(|| {
    DEFAULT_ICON_THEME_CONFIG.get::<IconThemeConfig>("icon-theme")
    .expect("Failed to load default icon theme. This is likely due to a missing or misnamed field in icon-theme.toml")
});

/// Used for creating a `DropdownData` for a setting
#[derive(Debug, Clone)]
pub struct DropdownInfo {
    /// The currently selected item.
    pub active_index: usize,
    pub items: im::Vector<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct MobilePreviewConfig {
    pub auto_reload_on_save: bool,
    pub approximation_notice_dismissed: bool,
}

impl Default for MobilePreviewConfig {
    fn default() -> Self {
        Self {
            auto_reload_on_save: true,
            approximation_notice_dismissed: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct SparrowConfig {
    #[serde(skip)]
    pub id: u64,
    pub core: CoreConfig,
    pub ui: UIConfig,
    pub editor: EditorConfig,
    pub terminal: TerminalConfig,
    #[serde(default)]
    pub ai_commit: ai_commit::AiCommitConfig,
    #[serde(default)]
    pub color_theme: ColorThemeConfig,
    #[serde(default)]
    pub icon_theme: IconThemeConfig,
    #[serde(default)]
    pub mobile_preview: MobilePreviewConfig,
    #[serde(flatten)]
    pub plugins: HashMap<String, HashMap<String, serde_json::Value>>,
    #[serde(skip)]
    pub color: ThemeColor,
    #[serde(skip)]
    pub available_color_themes: HashMap<String, (String, config::Config)>,
    #[serde(skip)]
    pub available_icon_themes:
        HashMap<String, (String, config::Config, Option<PathBuf>)>,
    // #[serde(skip)]
    // tab_layout_info: Arc<RwLock<HashMap<(FontFamily, usize), f64>>>,
    #[serde(skip)]
    svg_store: Arc<RwLock<SvgStore>>,
    /// A list of the themes that are available. This is primarily for populating
    /// the theme picker, and serves as a cache.
    #[serde(skip)]
    color_theme_list: im::Vector<String>,
    #[serde(skip)]
    icon_theme_list: im::Vector<String>,
    /// The couple names for the wrap style
    #[serde(skip)]
    wrap_style_list: im::Vector<String>,
}

impl SparrowConfig {
    pub fn load(
        workspace: &SparrowWorkspace,
        disabled_volts: &[VoltID],
        extra_plugin_paths: &[PathBuf],
    ) -> Self {
        let config = Self::merge_config(workspace, None, None);
        let mut sparrow_config: SparrowConfig = match config.try_deserialize() {
            Ok(config) => config,
            Err(error) => {
                error!("Failed to deserialize configuration file: {error}");
                DEFAULT_SPARROW_CONFIG.clone()
            }
        };

        sparrow_config.available_color_themes =
            Self::load_color_themes(disabled_volts, extra_plugin_paths);
        sparrow_config.available_icon_themes =
            Self::load_icon_themes(disabled_volts, extra_plugin_paths);
        sparrow_config.resolve_theme(workspace);

        sparrow_config.color_theme_list = sparrow_config
            .available_color_themes
            .values()
            .map(|(name, _)| name.clone())
            .sorted()
            .collect();
        sparrow_config.color_theme_list.sort();

        sparrow_config.icon_theme_list = sparrow_config
            .available_icon_themes
            .values()
            .map(|(name, _, _)| name.clone())
            .sorted()
            .collect();
        sparrow_config.icon_theme_list.sort();

        sparrow_config.wrap_style_list = im::vector![
            WrapStyle::None.to_string(),
            WrapStyle::EditorWidth.to_string(),
            // TODO: WrapStyle::WrapColumn.to_string(),
            WrapStyle::WrapWidth.to_string()
        ];

        sparrow_config.terminal.get_indexed_colors();

        sparrow_config
    }

    fn merge_config(
        workspace: &SparrowWorkspace,
        color_theme_config: Option<config::Config>,
        icon_theme_config: Option<config::Config>,
    ) -> config::Config {
        let mut config = DEFAULT_CONFIG.clone();

        if let Some(theme) = color_theme_config {
            // TODO: use different color theme basis if the theme declares its color preference
            // differently
            config = config::Config::builder()
                .add_source(config.clone())
                .add_source(DEFAULT_DARK_THEME_CONFIG.clone())
                .add_source(theme)
                .build()
                .unwrap_or_else(|_| config.clone());
        }

        if let Some(theme) = icon_theme_config {
            config = config::Config::builder()
                .add_source(config.clone())
                .add_source(theme)
                .build()
                .unwrap_or_else(|_| config.clone());
        }

        if let Some(path) = Self::settings_file() {
            config = config::Config::builder()
                .add_source(config.clone())
                .add_source(config::File::from(path.as_path()).required(false))
                .build()
                .unwrap_or_else(|_| config.clone());
        }

        match workspace.kind {
            SparrowWorkspaceType::Local => {
                if let Some(path) = workspace.path.as_ref() {
                    let path = path.join("./.sparrow/settings.toml");
                    config = config::Config::builder()
                        .add_source(config.clone())
                        .add_source(
                            config::File::from(path.as_path()).required(false),
                        )
                        .build()
                        .unwrap_or_else(|_| config.clone());
                }
            }
            SparrowWorkspaceType::RemoteSSH(_) => {}
            #[cfg(windows)]
            SparrowWorkspaceType::RemoteWSL(_) => {}
        }

        config
    }

    fn update_id(&mut self) {
        self.id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
    }

    fn default_config() -> config::Config {
        config::Config::builder()
            .add_source(config::File::from_str(
                DEFAULT_SETTINGS,
                config::FileFormat::Toml,
            ))
            .build()
            .unwrap()
    }

    fn default_sparrow_config() -> SparrowConfig {
        let mut default_sparrow_config: SparrowConfig =
            DEFAULT_CONFIG.clone().try_deserialize().expect("Failed to deserialize default config, this likely indicates a missing or misnamed field in settings.toml");
        default_sparrow_config.color_theme = DEFAULT_DARK_THEME_COLOR_CONFIG.clone();
        default_sparrow_config.icon_theme = DEFAULT_ICON_THEME_ICON_CONFIG.clone();
        default_sparrow_config.resolve_colors(None);
        default_sparrow_config
    }

    fn resolve_theme(&mut self, workspace: &SparrowWorkspace) {
        let default_sparrow_config = DEFAULT_SPARROW_CONFIG.clone();

        let color_theme_config = self
            .available_color_themes
            .get(&self.core.color_theme.to_lowercase())
            .map(|(_, config)| config)
            .unwrap_or(&DEFAULT_DARK_THEME_CONFIG);

        let icon_theme_config = self
            .available_icon_themes
            .get(&self.core.icon_theme.to_lowercase())
            .map(|(_, config, _)| config)
            .unwrap_or(&DEFAULT_ICON_THEME_CONFIG);

        let icon_theme_path = self
            .available_icon_themes
            .get(&self.core.icon_theme.to_lowercase())
            .map(|(_, _, path)| path);

        if let Ok(new) = Self::merge_config(
            workspace,
            Some(color_theme_config.clone()),
            Some(icon_theme_config.clone()),
        )
        .try_deserialize::<SparrowConfig>()
        {
            self.core = new.core;
            self.ui = new.ui;
            self.editor = new.editor;
            self.terminal = new.terminal;
            self.terminal.get_indexed_colors();

            self.color_theme = new.color_theme;
            self.icon_theme = new.icon_theme;
            if let Some(icon_theme_path) = icon_theme_path {
                self.icon_theme.path = icon_theme_path.clone().unwrap_or_default();
            }
            self.plugins = new.plugins;
        }
        self.resolve_colors(Some(&default_sparrow_config));
        self.update_id();
    }

    fn load_color_themes(
        disabled_volts: &[VoltID],
        extra_plugin_paths: &[PathBuf],
    ) -> HashMap<String, (String, config::Config)> {
        let mut themes = Self::load_local_themes().unwrap_or_default();

        for (key, theme) in
            Self::load_plugin_color_themes(disabled_volts, extra_plugin_paths)
        {
            themes.insert(key, theme);
        }

        let (name, theme) =
            Self::load_color_theme_from_str(DEFAULT_LIGHT_THEME).unwrap();
        themes.insert(name.to_lowercase(), (name, theme));
        let (name, theme) =
            Self::load_color_theme_from_str(DEFAULT_DARK_THEME).unwrap();
        themes.insert(name.to_lowercase(), (name, theme));
        let (name, theme) =
            Self::load_color_theme_from_str(DEFAULT_ISLANDS_DARK_THEME).unwrap();
        themes.insert(name.to_lowercase(), (name, theme));

        themes
    }

    pub fn default_color_theme(&self) -> &ColorThemeConfig {
        &DEFAULT_DARK_THEME_COLOR_CONFIG
    }

    /// Set the active color theme.
    /// Note that this does not save the config.
    pub fn set_color_theme(&mut self, workspace: &SparrowWorkspace, theme: &str) {
        self.core.color_theme = theme.to_string();
        self.resolve_theme(workspace);
    }

    /// Set the active icon theme.
    /// Note that this does not save the config.
    pub fn set_icon_theme(&mut self, workspace: &SparrowWorkspace, theme: &str) {
        self.core.icon_theme = theme.to_string();
        self.resolve_theme(workspace);
    }

    pub fn set_modal(&mut self, _workspace: &SparrowWorkspace, modal: bool) {
        self.core.modal = modal;
    }

    /// Get the color by the name from the current theme if it exists
    /// Otherwise, get the color from the base them
    /// # Panics
    /// If the color was not able to be found in either theme, which may be indicative that
    /// it is misspelled or needs to be added to the base-theme.
    pub fn color(&self, name: &str) -> Color {
        match self.color.ui.get(name) {
            Some(c) => *c,
            None => {
                error!("Failed to find key: {name}");
                css::HOT_PINK
            }
        }
    }

    /// Retrieve a color value whose key starts with "style."
    pub fn style_color(&self, name: &str) -> Option<Color> {
        self.color.syntax.get(name).copied()
    }

    pub fn completion_color(
        &self,
        kind: Option<CompletionItemKind>,
    ) -> Option<Color> {
        let kind = kind?;
        let theme_str = match kind {
            CompletionItemKind::METHOD => "method",
            CompletionItemKind::FUNCTION => "method",
            CompletionItemKind::ENUM => "enum",
            CompletionItemKind::ENUM_MEMBER => "enum-member",
            CompletionItemKind::CLASS => "class",
            CompletionItemKind::VARIABLE => "field",
            CompletionItemKind::STRUCT => "structure",
            CompletionItemKind::KEYWORD => "keyword",
            CompletionItemKind::CONSTANT => "constant",
            CompletionItemKind::PROPERTY => "property",
            CompletionItemKind::FIELD => "field",
            CompletionItemKind::INTERFACE => "interface",
            CompletionItemKind::SNIPPET => "snippet",
            CompletionItemKind::MODULE => "builtinType",
            _ => "string",
        };

        self.style_color(theme_str)
    }

    fn resolve_colors(&mut self, default_config: Option<&SparrowConfig>) {
        self.color.base = self
            .color_theme
            .base
            .resolve(default_config.map(|c| &c.color_theme.base));
        self.color.ui = self
            .color_theme
            .resolve_ui_color(&self.color.base, default_config.map(|c| &c.color.ui));
        self.color.syntax = self.color_theme.resolve_syntax_color(
            &self.color.base,
            default_config.map(|c| &c.color.syntax),
        );

        let fg = self.color(SparrowColor::EDITOR_FOREGROUND).to_rgba8();
        let bg = self.color(SparrowColor::EDITOR_BACKGROUND).to_rgba8();
        let is_light = bg.r as u32 + bg.g as u32 + bg.b as u32
            > fg.r as u32 + fg.g as u32 + fg.b as u32;
        let high_contrast = self.color_theme.high_contrast.unwrap_or(false);
        self.color.color_preference = match (is_light, high_contrast) {
            (true, true) => ThemeColorPreference::HighContrastLight,
            (false, true) => ThemeColorPreference::HighContrastDark,
            (true, false) => ThemeColorPreference::Light,
            (false, false) => ThemeColorPreference::Dark,
        };
    }

    fn load_local_themes() -> Option<HashMap<String, (String, config::Config)>> {
        let themes_folder = Directory::themes_directory()?;
        let themes: HashMap<String, (String, config::Config)> =
            std::fs::read_dir(themes_folder)
                .ok()?
                .filter_map(|entry| {
                    entry
                        .ok()
                        .and_then(|entry| Self::load_color_theme(&entry.path()))
                })
                .collect();
        Some(themes)
    }

    fn load_color_theme(path: &Path) -> Option<(String, (String, config::Config))> {
        if !path.is_file() {
            return None;
        }
        let config = config::Config::builder()
            .add_source(config::File::from(path))
            .build()
            .ok()?;
        let table = config.get_table("color-theme").ok()?;
        let name = table.get("name")?.to_string();
        Some((name.to_lowercase(), (name, config)))
    }

    /// Load the given theme by its contents.
    /// Returns `(name, theme fields)`
    fn load_color_theme_from_str(s: &str) -> Option<(String, config::Config)> {
        let config = config::Config::builder()
            .add_source(config::File::from_str(s, config::FileFormat::Toml))
            .build()
            .ok()?;
        let table = config.get_table("color-theme").ok()?;
        let name = table.get("name")?.to_string();
        Some((name, config))
    }

    fn load_icon_themes(
        disabled_volts: &[VoltID],
        extra_plugin_paths: &[PathBuf],
    ) -> HashMap<String, (String, config::Config, Option<PathBuf>)> {
        let mut themes = HashMap::new();

        for (key, (name, theme, path)) in
            Self::load_plugin_icon_themes(disabled_volts, extra_plugin_paths)
        {
            themes.insert(key, (name, theme, Some(path)));
        }

        let (name, theme) =
            Self::load_icon_theme_from_str(DEFAULT_ICON_THEME).unwrap();
        themes.insert(name.to_lowercase(), (name, theme, None));

        themes
    }

    fn load_icon_theme_from_str(s: &str) -> Option<(String, config::Config)> {
        let config = config::Config::builder()
            .add_source(config::File::from_str(s, config::FileFormat::Toml))
            .build()
            .ok()?;
        let table = config.get_table("icon-theme").ok()?;
        let name = table.get("name")?.to_string();
        Some((name, config))
    }

    fn load_plugin_color_themes(
        disabled_volts: &[VoltID],
        extra_plugin_paths: &[PathBuf],
    ) -> HashMap<String, (String, config::Config)> {
        let mut themes: HashMap<String, (String, config::Config)> = HashMap::new();
        for meta in find_themes(extra_plugin_paths) {
            if disabled_volts.contains(&meta.id()) {
                continue;
            }
            if let Some(plugin_themes) = meta.color_themes.as_ref() {
                for theme_path in plugin_themes {
                    if let Some((key, theme)) =
                        Self::load_color_theme(&PathBuf::from(theme_path))
                    {
                        themes.insert(key, theme);
                    }
                }
            }
        }
        themes
    }

    fn load_plugin_icon_themes(
        disabled_volts: &[VoltID],
        extra_plugin_paths: &[PathBuf],
    ) -> HashMap<String, (String, config::Config, PathBuf)> {
        let mut themes: HashMap<String, (String, config::Config, PathBuf)> =
            HashMap::new();
        for meta in find_themes(extra_plugin_paths) {
            if disabled_volts.contains(&meta.id()) {
                continue;
            }
            if let Some(plugin_themes) = meta.icon_themes.as_ref() {
                for theme_path in plugin_themes {
                    if let Some((key, theme)) =
                        Self::load_icon_theme(&PathBuf::from(theme_path))
                    {
                        themes.insert(key, theme);
                    }
                }
            }
        }
        themes
    }

    fn load_icon_theme(
        path: &Path,
    ) -> Option<(String, (String, config::Config, PathBuf))> {
        if !path.is_file() {
            return None;
        }
        let config = config::Config::builder()
            .add_source(config::File::from(path))
            .build()
            .ok()?;
        let table = config.get_table("icon-theme").ok()?;
        let name = table.get("name")?.to_string();
        Some((
            name.to_lowercase(),
            (name, config, path.parent().unwrap().to_path_buf()),
        ))
    }

    pub fn export_theme(&self) -> String {
        let mut table = toml::value::Table::new();
        let mut theme = self.color_theme.clone();
        theme.name = "".to_string();
        table.insert(
            "color-theme".to_string(),
            toml::Value::try_from(&theme).unwrap(),
        );
        table.insert("ui".to_string(), toml::Value::try_from(&self.ui).unwrap());
        let value = toml::Value::Table(table);
        toml::to_string_pretty(&value).unwrap()
    }

    pub fn settings_file() -> Option<PathBuf> {
        let path = Directory::config_directory()?.join("settings.toml");

        if !path.exists() {
            if let Err(err) = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path)
            {
                tracing::error!("{:?}", err);
            }
        }

        Some(path)
    }

    pub fn keymaps_file() -> Option<PathBuf> {
        let path = Directory::config_directory()?.join("keymaps.toml");

        if !path.exists() {
            if let Err(err) = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path)
            {
                tracing::error!("{:?}", err);
            }
        }

        Some(path)
    }

    pub fn ui_svg(&self, icon: &'static str) -> String {
        let svg = self.icon_theme.ui.get(icon).and_then(|path| {
            let path = self.icon_theme.path.join(path);
            self.svg_store.write().get_svg_on_disk(&path)
        });

        svg.unwrap_or_else(|| {
            let name = match DEFAULT_ICON_THEME_ICON_CONFIG.ui.get(icon) {
                Some(name) => name.clone(),
                None => {
                    tracing::warn!(
                        "ui_svg: `{icon}` not found in the active icon theme or \
                         defaults/icon-theme.toml. Add this line under \
                         [icon-theme.ui] in defaults/icon-theme.toml:\n    \
                         \"{icon}\" = \"<file>.svg\""
                    );
                    return missing_icon_svg();
                }
            };
            self.svg_store.write().get_default_svg(&name)
        })
    }

    pub fn files_svg(&self, paths: &[&Path]) -> (String, Option<Color>) {
        let svg = self
            .icon_theme
            .resolve_path_to_icon(paths)
            .and_then(|p| self.svg_store.write().get_svg_on_disk(&p));

        let svg = svg.or_else(|| {
            let path = DEFAULT_ICON_THEME_ICON_CONFIG.resolve_path_to_icon(paths)?;
            let name = path.file_name()?.to_str()?;
            Some(self.svg_store.write().get_default_svg(name))
        });
        if let Some(svg) = svg {
            let color = if self.icon_theme.use_editor_color.unwrap_or(false) {
                Some(self.color(SparrowColor::SPARROW_ICON_ACTIVE))
            } else {
                None
            };
            (svg, color)
        } else {
            (
                self.ui_svg(SparrowIcons::FILE),
                Some(self.color(SparrowColor::SPARROW_ICON_ACTIVE)),
            )
        }
    }

    pub fn file_svg(&self, path: &Path) -> (String, Option<Color>) {
        self.files_svg(slice::from_ref(&path))
    }

    pub fn folder_svg(&self, path: &Path) -> (String, Option<Color>) {
        let svg = self
            .icon_theme
            .resolve_folder_to_icon(path)
            .and_then(|path| self.svg_store.write().get_svg_on_disk(&path))
            .or_else(|| {
                let path =
                    DEFAULT_ICON_THEME_ICON_CONFIG.resolve_folder_to_icon(path)?;
                Some(
                    self.svg_store
                        .write()
                        .get_default_svg(path.file_name()?.to_str()?),
                )
            });
        match svg {
            Some(svg) => {
                let color = self
                    .icon_theme
                    .use_editor_color
                    .unwrap_or_else(|| svg.contains("currentColor"))
                    .then(|| self.color(SparrowColor::SPARROW_ICON_ACTIVE));
                (svg, color)
            }
            None => (
                self.ui_svg(SparrowIcons::DIRECTORY),
                Some(self.color(SparrowColor::SPARROW_ICON_ACTIVE)),
            ),
        }
    }

    pub fn symbol_svg(&self, kind: &SymbolKind) -> Option<String> {
        let kind_str = match *kind {
            SymbolKind::ARRAY => SparrowIcons::SYMBOL_KIND_ARRAY,
            SymbolKind::BOOLEAN => SparrowIcons::SYMBOL_KIND_BOOLEAN,
            SymbolKind::CLASS => SparrowIcons::SYMBOL_KIND_CLASS,
            SymbolKind::CONSTANT => SparrowIcons::SYMBOL_KIND_CONSTANT,
            SymbolKind::ENUM_MEMBER => SparrowIcons::SYMBOL_KIND_ENUM_MEMBER,
            SymbolKind::ENUM => SparrowIcons::SYMBOL_KIND_ENUM,
            SymbolKind::EVENT => SparrowIcons::SYMBOL_KIND_EVENT,
            SymbolKind::FIELD => SparrowIcons::SYMBOL_KIND_FIELD,
            SymbolKind::FILE => SparrowIcons::SYMBOL_KIND_FILE,
            SymbolKind::INTERFACE => SparrowIcons::SYMBOL_KIND_INTERFACE,
            SymbolKind::KEY => SparrowIcons::SYMBOL_KIND_KEY,
            SymbolKind::FUNCTION => SparrowIcons::SYMBOL_KIND_FUNCTION,
            SymbolKind::METHOD => SparrowIcons::SYMBOL_KIND_METHOD,
            SymbolKind::OBJECT => SparrowIcons::SYMBOL_KIND_OBJECT,
            SymbolKind::NAMESPACE => SparrowIcons::SYMBOL_KIND_NAMESPACE,
            SymbolKind::NUMBER => SparrowIcons::SYMBOL_KIND_NUMBER,
            SymbolKind::OPERATOR => SparrowIcons::SYMBOL_KIND_OPERATOR,
            SymbolKind::TYPE_PARAMETER => SparrowIcons::SYMBOL_KIND_TYPE_PARAMETER,
            SymbolKind::PROPERTY => SparrowIcons::SYMBOL_KIND_PROPERTY,
            SymbolKind::STRING => SparrowIcons::SYMBOL_KIND_STRING,
            SymbolKind::STRUCT => SparrowIcons::SYMBOL_KIND_STRUCT,
            SymbolKind::VARIABLE => SparrowIcons::SYMBOL_KIND_VARIABLE,
            _ => return None,
        };

        Some(self.ui_svg(kind_str))
    }

    pub fn symbol_color(&self, kind: &SymbolKind) -> Option<Color> {
        let theme_str = match *kind {
            SymbolKind::METHOD => "method",
            SymbolKind::FUNCTION => "method",
            SymbolKind::ENUM => "enum",
            SymbolKind::ENUM_MEMBER => "enum-member",
            SymbolKind::CLASS => "class",
            SymbolKind::VARIABLE => "field",
            SymbolKind::STRUCT => "structure",
            SymbolKind::CONSTANT => "constant",
            SymbolKind::PROPERTY => "property",
            SymbolKind::FIELD => "field",
            SymbolKind::INTERFACE => "interface",
            SymbolKind::ARRAY => "",
            SymbolKind::BOOLEAN => "",
            SymbolKind::EVENT => "",
            SymbolKind::FILE => "",
            SymbolKind::KEY => "",
            SymbolKind::OBJECT => "",
            SymbolKind::NAMESPACE => "",
            SymbolKind::NUMBER => "number",
            SymbolKind::OPERATOR => "",
            SymbolKind::TYPE_PARAMETER => "",
            SymbolKind::STRING => "string",
            _ => return None,
        };

        self.style_color(theme_str)
    }

    pub fn logo_svg(&self) -> String {
        self.svg_store.read().logo_svg()
    }

    /// List of the color themes that are available by their display names.
    pub fn color_theme_list(&self) -> im::Vector<String> {
        self.color_theme_list.clone()
    }

    /// List of the icon themes that are available by their display names.
    pub fn icon_theme_list(&self) -> im::Vector<String> {
        self.icon_theme_list.clone()
    }

    pub fn terminal_font_family(&self) -> &str {
        if self.terminal.font_family.trim().is_empty() {
            self.editor.font_family.as_str()
        } else {
            self.terminal.font_family.as_str()
        }
    }

    pub fn terminal_font_size(&self) -> usize {
        if self.terminal.font_size > 0 {
            self.terminal.font_size
        } else {
            self.editor.font_size()
        }
    }

    pub fn terminal_line_height(&self) -> usize {
        let font_size = self.terminal_font_size();

        if self.terminal.line_height > 0.0 {
            let line_height = if self.terminal.line_height < SCALE_OR_SIZE_LIMIT {
                self.terminal.line_height * font_size as f64
            } else {
                self.terminal.line_height
            };

            // Prevent overlapping lines
            (line_height.round() as usize).max(font_size)
        } else {
            self.editor.line_height().max(font_size)
        }
    }

    pub fn terminal_get_color(
        &self,
        color: &alacritty_terminal::vte::ansi::Color,
        colors: &alacritty_terminal::term::color::Colors,
    ) -> Color {
        match color {
            alacritty_terminal::vte::ansi::Color::Named(color) => {
                self.terminal_get_named_color(color)
            }
            alacritty_terminal::vte::ansi::Color::Spec(rgb) => {
                Color::from_rgb8(rgb.r, rgb.g, rgb.b)
            }
            alacritty_terminal::vte::ansi::Color::Indexed(index) => {
                if let Some(rgb) = colors[*index as usize] {
                    return Color::from_rgb8(rgb.r, rgb.g, rgb.b);
                }
                const NAMED_COLORS: [alacritty_terminal::vte::ansi::NamedColor; 16] = [
                    alacritty_terminal::vte::ansi::NamedColor::Black,
                    alacritty_terminal::vte::ansi::NamedColor::Red,
                    alacritty_terminal::vte::ansi::NamedColor::Green,
                    alacritty_terminal::vte::ansi::NamedColor::Yellow,
                    alacritty_terminal::vte::ansi::NamedColor::Blue,
                    alacritty_terminal::vte::ansi::NamedColor::Magenta,
                    alacritty_terminal::vte::ansi::NamedColor::Cyan,
                    alacritty_terminal::vte::ansi::NamedColor::White,
                    alacritty_terminal::vte::ansi::NamedColor::BrightBlack,
                    alacritty_terminal::vte::ansi::NamedColor::BrightRed,
                    alacritty_terminal::vte::ansi::NamedColor::BrightGreen,
                    alacritty_terminal::vte::ansi::NamedColor::BrightYellow,
                    alacritty_terminal::vte::ansi::NamedColor::BrightBlue,
                    alacritty_terminal::vte::ansi::NamedColor::BrightMagenta,
                    alacritty_terminal::vte::ansi::NamedColor::BrightCyan,
                    alacritty_terminal::vte::ansi::NamedColor::BrightWhite,
                ];
                if (*index as usize) < NAMED_COLORS.len() {
                    self.terminal_get_named_color(&NAMED_COLORS[*index as usize])
                } else {
                    self.terminal.indexed_colors.get(index).cloned().unwrap()
                }
            }
        }
    }

    fn terminal_get_named_color(
        &self,
        color: &alacritty_terminal::vte::ansi::NamedColor,
    ) -> Color {
        let (color, alpha) = match color {
            alacritty_terminal::vte::ansi::NamedColor::Cursor => {
                (SparrowColor::TERMINAL_CURSOR, 1.0)
            }
            alacritty_terminal::vte::ansi::NamedColor::Foreground => {
                (SparrowColor::TERMINAL_FOREGROUND, 1.0)
            }
            alacritty_terminal::vte::ansi::NamedColor::Background => {
                (SparrowColor::TERMINAL_BACKGROUND, 1.0)
            }
            alacritty_terminal::vte::ansi::NamedColor::Blue => {
                (SparrowColor::TERMINAL_BLUE, 1.0)
            }
            alacritty_terminal::vte::ansi::NamedColor::Green => {
                (SparrowColor::TERMINAL_GREEN, 1.0)
            }
            alacritty_terminal::vte::ansi::NamedColor::Yellow => {
                (SparrowColor::TERMINAL_YELLOW, 1.0)
            }
            alacritty_terminal::vte::ansi::NamedColor::Red => {
                (SparrowColor::TERMINAL_RED, 1.0)
            }
            alacritty_terminal::vte::ansi::NamedColor::White => {
                (SparrowColor::TERMINAL_WHITE, 1.0)
            }
            alacritty_terminal::vte::ansi::NamedColor::Black => {
                (SparrowColor::TERMINAL_BLACK, 1.0)
            }
            alacritty_terminal::vte::ansi::NamedColor::Cyan => {
                (SparrowColor::TERMINAL_CYAN, 1.0)
            }
            alacritty_terminal::vte::ansi::NamedColor::Magenta => {
                (SparrowColor::TERMINAL_MAGENTA, 1.0)
            }
            alacritty_terminal::vte::ansi::NamedColor::BrightBlue => {
                (SparrowColor::TERMINAL_BRIGHT_BLUE, 1.0)
            }
            alacritty_terminal::vte::ansi::NamedColor::BrightGreen => {
                (SparrowColor::TERMINAL_BRIGHT_GREEN, 1.0)
            }
            alacritty_terminal::vte::ansi::NamedColor::BrightYellow => {
                (SparrowColor::TERMINAL_BRIGHT_YELLOW, 1.0)
            }
            alacritty_terminal::vte::ansi::NamedColor::BrightRed => {
                (SparrowColor::TERMINAL_BRIGHT_RED, 1.0)
            }
            alacritty_terminal::vte::ansi::NamedColor::BrightWhite => {
                (SparrowColor::TERMINAL_BRIGHT_WHITE, 1.0)
            }
            alacritty_terminal::vte::ansi::NamedColor::BrightBlack => {
                (SparrowColor::TERMINAL_BRIGHT_BLACK, 1.0)
            }
            alacritty_terminal::vte::ansi::NamedColor::BrightCyan => {
                (SparrowColor::TERMINAL_BRIGHT_CYAN, 1.0)
            }
            alacritty_terminal::vte::ansi::NamedColor::BrightMagenta => {
                (SparrowColor::TERMINAL_BRIGHT_MAGENTA, 1.0)
            }
            alacritty_terminal::vte::ansi::NamedColor::BrightForeground => {
                (SparrowColor::TERMINAL_FOREGROUND, 1.0)
            }
            alacritty_terminal::vte::ansi::NamedColor::DimBlack => {
                (SparrowColor::TERMINAL_BLACK, 0.66)
            }
            alacritty_terminal::vte::ansi::NamedColor::DimRed => {
                (SparrowColor::TERMINAL_RED, 0.66)
            }
            alacritty_terminal::vte::ansi::NamedColor::DimGreen => {
                (SparrowColor::TERMINAL_GREEN, 0.66)
            }
            alacritty_terminal::vte::ansi::NamedColor::DimYellow => {
                (SparrowColor::TERMINAL_YELLOW, 0.66)
            }
            alacritty_terminal::vte::ansi::NamedColor::DimBlue => {
                (SparrowColor::TERMINAL_BLUE, 0.66)
            }
            alacritty_terminal::vte::ansi::NamedColor::DimMagenta => {
                (SparrowColor::TERMINAL_MAGENTA, 0.66)
            }
            alacritty_terminal::vte::ansi::NamedColor::DimCyan => {
                (SparrowColor::TERMINAL_CYAN, 0.66)
            }
            alacritty_terminal::vte::ansi::NamedColor::DimWhite => {
                (SparrowColor::TERMINAL_WHITE, 0.66)
            }
            alacritty_terminal::vte::ansi::NamedColor::DimForeground => {
                (SparrowColor::TERMINAL_FOREGROUND, 0.66)
            }
        };
        self.color(color).multiply_alpha(alpha)
    }

    /// Get the dropdown information for the specific setting, used for the settings UI.
    /// This should aim to efficiently return the data, because it is used to determine whether to
    /// update the dropdown items.
    pub fn get_dropdown_info(&self, kind: &str, key: &str) -> Option<DropdownInfo> {
        match (kind, key) {
            ("core", "color-theme") => Some(DropdownInfo {
                active_index: self
                    .color_theme_list
                    .iter()
                    .position(|s| s == &self.color_theme.name)
                    .unwrap_or(0),
                items: self.color_theme_list.clone(),
            }),
            ("core", "icon-theme") => Some(DropdownInfo {
                active_index: self
                    .icon_theme_list
                    .iter()
                    .position(|s| s == &self.icon_theme.name)
                    .unwrap_or(0),
                items: self.icon_theme_list.clone(),
            }),
            ("editor", "wrap-style") => Some(DropdownInfo {
                // TODO: it would be better to have the text not be the default kebab-case when
                // displayed in settings, but we would need to map back from the dropdown's value
                // or index.
                active_index: self
                    .wrap_style_list
                    .iter()
                    .flat_map(|w| WrapStyle::try_from_str(w))
                    .position(|w| w == self.editor.wrap_style)
                    .unwrap_or(0),
                items: self.wrap_style_list.clone(),
            }),
            ("ui", "tab-close-button") => Some(DropdownInfo {
                active_index: self.ui.tab_close_button as usize,
                items: ui::TabCloseButton::VARIANTS
                    .iter()
                    .map(|s| s.to_string())
                    .sorted()
                    .collect(),
            }),
            ("ui", "tab-separator-height") => Some(DropdownInfo {
                active_index: self.ui.tab_separator_height as usize,
                items: ui::TabSeparatorHeight::VARIANTS
                    .iter()
                    .map(|s| s.to_string())
                    .sorted()
                    .collect(),
            }),
            ("terminal", "default-profile") => Some(DropdownInfo {
                active_index: self
                    .terminal
                    .profiles
                    .iter()
                    .position(|(profile_name, _)| {
                        profile_name
                            == self
                                .terminal
                                .default_profile
                                .get(std::env::consts::OS)
                                .unwrap_or(&String::from("default"))
                    })
                    .unwrap_or(0),
                items: self.terminal.profiles.clone().into_keys().collect(),
            }),
            _ => None,
        }
    }

    fn get_file_table() -> Option<toml_edit::Document> {
        let path = Self::settings_file()?;
        let content = std::fs::read_to_string(path).ok()?;
        let document: toml_edit::Document = content.parse().ok()?;
        Some(document)
    }

    pub fn reset_setting(parent: &str, key: &str) -> Option<()> {
        let mut main_table = Self::get_file_table().unwrap_or_default();

        // Find the container table
        let mut table = main_table.as_table_mut();
        for key in parent.split('.') {
            if !table.contains_key(key) {
                table.insert(
                    key,
                    toml_edit::Item::Table(toml_edit::Table::default()),
                );
            }
            table = table.get_mut(key)?.as_table_mut()?;
        }

        table.remove(key);

        // Store
        let path = Self::settings_file()?;
        std::fs::write(path, main_table.to_string().as_bytes()).ok()?;

        Some(())
    }

    /// Apply a validated settings draft in one atomic write.
    pub fn update_settings_batch(
        changes: Vec<(String, String, toml_edit::Value)>,
    ) -> Result<(), String> {
        let path =
            Self::settings_file().ok_or("Cannot locate the settings file.")?;
        let contents = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let updated = settings_transaction(&contents, changes)?;
        let temporary = tempfile::NamedTempFile::new_in(path.parent().unwrap())
            .map_err(|e| e.to_string())?;
        std::fs::write(temporary.path(), updated).map_err(|e| e.to_string())?;
        temporary.persist(&path).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn scoped_settings_file(
        workspace: Option<&std::path::Path>,
        project: bool,
    ) -> Result<PathBuf, String> {
        if project {
            Ok(workspace
                .ok_or("Open a local project to edit project settings.")?
                .join(".sparrow/settings.toml"))
        } else {
            Self::settings_file().ok_or("Cannot locate IDE settings".into())
        }
    }
    pub fn update_scoped_settings(
        changes: Vec<(PathBuf, Vec<(String, String, Option<toml_edit::Value>)>)>,
    ) -> Result<(), String> {
        let files = changes
            .into_iter()
            .map(|(path, changes)| {
                let text = match std::fs::read_to_string(&path) {
                    Ok(text) => text,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        String::new()
                    }
                    Err(error) => return Err(error.to_string()),
                };
                Ok((path, settings_scope_transaction(&text, changes)?))
            })
            .collect::<Result<Vec<_>, String>>()?;
        crate::persistence::write_files(files)
    }

    /// Update the config file with the given edit.
    /// This should be called whenever the configuration is changed, so that it is persisted.
    pub fn update_file(
        parent: &str,
        key: &str,
        value: toml_edit::Value,
    ) -> Option<()> {
        // TODO: This is a hack to fix the fact that terminal default profile is saved in a
        // different manner than other fields. As it is per-operating-system.
        // Thus we have to instead set the terminal.default-profile.{OS}
        // It would be better to not need a special hack.
        let (parent, key) = if parent == "terminal" && key == "default-profile" {
            ("terminal.default-profile", std::env::consts::OS)
        } else {
            (parent, key)
        };

        let mut main_table = Self::get_file_table().unwrap_or_default();

        // Find the container table
        let mut table = main_table.as_table_mut();
        for key in parent.split('.') {
            if !table.contains_key(key) {
                table.insert(
                    key,
                    toml_edit::Item::Table(toml_edit::Table::default()),
                );
            }
            table = table.get_mut(key)?.as_table_mut()?;
        }

        // Update key
        table.insert(key, toml_edit::Item::Value(value));

        // Store
        let path = Self::settings_file()?;
        std::fs::write(path, main_table.to_string().as_bytes()).ok()?;

        Some(())
    }
}
fn settings_scope_transaction(
    contents: &str,
    changes: Vec<(String, String, Option<toml_edit::Value>)>,
) -> Result<String, String> {
    let mut text = contents.to_string();
    for (section, key, value) in changes {
        if let Some(value) = value {
            text = settings_transaction(&text, vec![(section, key, value)])?;
        } else {
            let mut document = text
                .parse::<toml_edit::Document>()
                .map_err(|error| error.to_string())?;
            let (section, key) = if section == "terminal" && key == "default-profile"
            {
                (
                    "terminal.default-profile".to_owned(),
                    std::env::consts::OS.to_owned(),
                )
            } else {
                (section, key)
            };
            let mut table = Some(document.as_table_mut());
            for segment in section.split('.').filter(|_| !section.is_empty()) {
                table = table
                    .and_then(|table| table.get_mut(segment))
                    .and_then(toml_edit::Item::as_table_mut);
            }
            if let Some(table) = table {
                table.remove(&key);
            }
            text = document.to_string();
        }
    }
    Ok(text)
}

fn settings_transaction(
    contents: &str,
    changes: Vec<(String, String, toml_edit::Value)>,
) -> Result<String, String> {
    let mut document = contents
        .parse::<toml_edit::Document>()
        .map_err(|e| e.to_string())?;
    for (parent, key, value) in changes {
        let parent = if parent == "terminal" && key == "default-profile" {
            "terminal.default-profile".to_string()
        } else {
            parent
        };
        let key = if parent == "terminal.default-profile" && key == "default-profile"
        {
            std::env::consts::OS.to_string()
        } else {
            key
        };
        let mut table = document.as_table_mut();
        for segment in parent.split('.').filter(|_| !parent.is_empty()) {
            if !table.contains_key(segment) {
                table.insert(
                    segment,
                    toml_edit::Item::Table(toml_edit::Table::new()),
                );
            }
            table = table
                .get_mut(segment)
                .and_then(toml_edit::Item::as_table_mut)
                .ok_or_else(|| {
                    format!("Settings section '{parent}' is not a table.")
                })?;
        }
        table.insert(&key, toml_edit::Item::Value(value));
    }
    Ok(document.to_string())
}

#[cfg(test)]
mod theme_tests {
    use super::*;

    #[test]
    fn settings_theme_preference_matches_the_resolved_palette() {
        use ThemeColorPreference::*;

        for (source, light) in [
            (DEFAULT_LIGHT_THEME, true),
            (DEFAULT_DARK_THEME, false),
            (DEFAULT_ISLANDS_DARK_THEME, false),
        ] {
            let (name, theme) =
                SparrowConfig::load_color_theme_from_str(source).unwrap();
            let mut config = DEFAULT_SPARROW_CONFIG.clone();
            config.color_theme = theme.get("color-theme").unwrap();
            for high_contrast in [false, true] {
                config.color_theme.high_contrast = Some(high_contrast);
                config.resolve_colors(Some(&DEFAULT_SPARROW_CONFIG));
                assert!(
                    matches!(
                        (light, high_contrast, &config.color.color_preference),
                        (true, false, Light)
                            | (false, false, Dark)
                            | (true, true, HighContrastLight)
                            | (false, true, HighContrastDark)
                    ),
                    "{name}, high contrast {high_contrast}: {:?}",
                    config.color.color_preference,
                );
            }
        }
    }
}

#[cfg(test)]
mod mobile_preview_config_tests {
    use super::MobilePreviewConfig;

    #[test]
    fn preview_preferences_default_safely_and_allow_overrides() {
        let defaults: MobilePreviewConfig = toml::from_str("").unwrap();
        assert!(defaults.auto_reload_on_save);
        assert!(!defaults.approximation_notice_dismissed);

        let override_value: MobilePreviewConfig =
            toml::from_str("auto-reload-on-save = false").unwrap();
        assert!(!override_value.auto_reload_on_save);
        assert!(!override_value.approximation_notice_dismissed);
    }
}

#[cfg(test)]
mod autosave_config_tests {
    use super::DEFAULT_CONFIG;

    #[test]
    fn autosave_is_enabled_by_default_after_a_short_idle_delay() {
        assert_eq!(
            DEFAULT_CONFIG
                .get::<u64>("editor.autosave-interval")
                .unwrap(),
            1_000
        );
    }
}

#[cfg(test)]
mod settings_transaction_tests {
    use super::*;
    #[test]
    fn preserves_comments_and_unrelated_values() {
        let output = settings_transaction(
            "# user settings\n[ui]\nfont-size = 13\n[editor]\nfont-size = 14\n",
            vec![
                ("ui".into(), "font-size".into(), 16.into()),
                ("core".into(), "modal".into(), false.into()),
            ],
        )
        .unwrap();
        assert!(output.contains("# user settings"));
        let parsed = output.parse::<toml_edit::Document>().unwrap();
        assert_eq!(parsed["ui"]["font-size"].as_integer(), Some(16));
        assert_eq!(parsed["editor"]["font-size"].as_integer(), Some(14));
    }
    #[test]
    fn targets_terminal_profile_for_the_current_os() {
        let output = settings_transaction(
            "",
            vec![("terminal".into(), "default-profile".into(), "zsh".into())],
        )
        .unwrap();
        let parsed = output.parse::<toml_edit::Document>().unwrap();
        assert_eq!(
            parsed["terminal"]["default-profile"][std::env::consts::OS].as_str(),
            Some("zsh")
        );
    }
    #[test]
    fn rejects_malformed_input_and_section_collisions() {
        assert!(settings_transaction("[broken", vec![]).is_err());
        assert!(
            settings_transaction(
                "ui = 42",
                vec![("ui".into(), "font-size".into(), 13.into())]
            )
            .is_err()
        );
    }
}

#[cfg(test)]
mod scoped_settings_tests {
    use super::*;
    #[test]
    fn ide_and_project_values_are_saved_independently_and_override_can_be_removed() {
        let root = tempfile::tempdir().unwrap();
        let ide = root.path().join("ide.toml");
        let project = root.path().join("project/.sparrow/settings.toml");
        std::fs::write(&ide, "# keep comment\n[editor]\nfont-size = 13\n").unwrap();
        SparrowConfig::update_scoped_settings(vec![
            (
                ide.clone(),
                vec![("editor".into(), "font-size".into(), Some(15.into()))],
            ),
            (
                project.clone(),
                vec![("editor".into(), "font-size".into(), Some(19.into()))],
            ),
        ])
        .unwrap();
        assert!(
            std::fs::read_to_string(&ide)
                .unwrap()
                .contains("font-size = 15")
        );
        assert!(
            std::fs::read_to_string(&project)
                .unwrap()
                .contains("font-size = 19")
        );
        SparrowConfig::update_scoped_settings(vec![(
            project.clone(),
            vec![("editor".into(), "font-size".into(), None)],
        )])
        .unwrap();
        assert!(
            !std::fs::read_to_string(project)
                .unwrap()
                .contains("font-size")
        );
        assert!(
            std::fs::read_to_string(ide)
                .unwrap()
                .starts_with("# keep comment")
        );
    }
    #[test]
    fn invalid_project_document_does_not_partially_apply_ide_changes() {
        let root = tempfile::tempdir().unwrap();
        let ide = root.path().join("ide.toml");
        let project = root.path().join("project.toml");
        std::fs::write(&ide, "[editor]\nfont-size=13\n").unwrap();
        std::fs::write(&project, "[broken").unwrap();
        assert!(
            SparrowConfig::update_scoped_settings(vec![
                (
                    ide.clone(),
                    vec![("editor".into(), "font-size".into(), Some(15.into()))]
                ),
                (
                    project.clone(),
                    vec![("editor".into(), "font-size".into(), Some(19.into()))]
                )
            ])
            .is_err()
        );
        assert!(
            std::fs::read_to_string(ide)
                .unwrap()
                .contains("font-size=13")
        );
        assert_eq!(std::fs::read_to_string(project).unwrap(), "[broken");
    }
    #[test]
    fn reset_terminal_profile_uses_platform_key_and_preserves_other_platforms() {
        let source = format!(
            "[terminal.default-profile]\n{}='current'\nother='keep'\n",
            std::env::consts::OS
        );
        let result = settings_scope_transaction(
            &source,
            vec![("terminal".into(), "default-profile".into(), None)],
        )
        .unwrap();
        assert!(!result.contains("'current'"));
        assert!(result.contains("other='keep'"));
    }
}
