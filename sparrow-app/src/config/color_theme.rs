use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    str::FromStr,
};

use floem::{peniko::Color, prelude::palette::css};
use serde::{Deserialize, Serialize};

use super::color::LoadThemeError;

#[derive(Debug, Clone, Default)]
pub enum ThemeColorPreference {
    #[default]
    Light,
    Dark,
    HighContrastDark,
    HighContrastLight,
}

/// Holds all the resolved theme variables
#[derive(Debug, Clone, Default)]
pub struct ThemeBaseColor(HashMap<String, Color>);
impl ThemeBaseColor {
    pub fn get(&self, name: &str) -> Option<Color> {
        self.0.get(name).map(ToOwned::to_owned)
    }
}

pub const THEME_RECURSION_LIMIT: usize = 6;

#[derive(Debug, Clone, Default)]
pub struct ThemeColor {
    pub color_preference: ThemeColorPreference,
    pub base: ThemeBaseColor,
    pub syntax: HashMap<String, Color>,
    pub ui: HashMap<String, Color>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ThemeBaseConfig(pub BTreeMap<String, String>);

impl ThemeBaseConfig {
    /// Resolve the variables in this theme base config into the actual colors.  
    /// The basic idea is just: `"field`: some value` does:
    /// - If the value does not start with `$`, then it is a color and we return it
    /// - If the value starts with `$` then it is a variable
    ///   - Look it up in the current theme
    ///   - If not found, look it up in the default theme
    ///   - If not found, return `Color::HOT_PINK` as a fallback
    ///
    /// Note that this applies even if the default theme colors have a variable.  
    /// This allows the default theme to have, for example, a `$uibg` variable that the current
    /// them can override so that if there's ever a new ui element using that variable, the theme
    /// does not have to be updated.
    pub fn resolve(&self, default: Option<&ThemeBaseConfig>) -> ThemeBaseColor {
        let default = default.cloned().unwrap_or_default();

        let mut base = ThemeBaseColor(HashMap::new());

        // We resolve all the variables to their values
        for (key, value) in self.0.iter() {
            match self.resolve_variable(&default, key, value, 0) {
                Ok(Some(color)) => {
                    let color = Color::from_str(color)
                        .unwrap_or_else(|_| {
                            tracing::warn!(
                                "Failed to parse color theme variable for ({key}: {value})"
                            );
                            css::HOT_PINK
                        });
                    base.0.insert(key.to_string(), color);
                }
                Ok(None) => {
                    tracing::warn!(
                        "Failed to resolve color theme variable for ({key}: {value})"
                    );
                }
                Err(err) => {
                    tracing::error!(
                        "Failed to resolve color theme variable ({key}: {value}): {err}"
                    );
                }
            }
        }

        base
    }

    fn resolve_variable<'a>(
        &'a self,
        defaults: &'a ThemeBaseConfig,
        key: &str,
        value: &'a str,
        i: usize,
    ) -> Result<Option<&'a str>, LoadThemeError> {
        let Some(value) = value.strip_prefix('$') else {
            return Ok(Some(value));
        };

        if i > THEME_RECURSION_LIMIT {
            return Err(LoadThemeError::RecursionLimitReached {
                variable_name: key.to_string(),
            });
        }

        let target =
            self.get(value)
                .or_else(|| defaults.get(value))
                .ok_or_else(|| LoadThemeError::VariableNotFound {
                    variable_name: key.to_string(),
                })?;

        self.resolve_variable(defaults, value, target, i + 1)
    }

    // Note: this returns an `&String` just to make it consistent with hashmap lookups that are
    // also used via ui/syntax
    pub fn get(&self, name: &str) -> Option<&String> {
        self.0.get(name)
    }

    pub fn key_values(&self) -> BTreeMap<String, String> {
        self.0.clone()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "kebab-case", default)]
pub struct ColorThemeConfig {
    #[serde(skip)]
    pub path: PathBuf,
    pub name: String,
    pub high_contrast: Option<bool>,
    pub base: ThemeBaseConfig,
    pub syntax: BTreeMap<String, String>,
    pub ui: BTreeMap<String, String>,
}

impl ColorThemeConfig {
    fn resolve_color(
        colors: &BTreeMap<String, String>,
        base: &ThemeBaseColor,
        default: Option<&HashMap<String, Color>>,
    ) -> HashMap<String, Color> {
        colors
            .iter()
            .map(|(name, hex)| {
                let color = if let Some(stripped) = hex.strip_prefix('$') {
                    base.get(stripped)
                } else {
                    Color::from_str(hex).ok()
                };

                let color = color
                    .or_else(|| {
                        default.and_then(|default| default.get(name).cloned())
                    })
                    .unwrap_or(Color::from_rgb8(0, 0, 0));

                (name.to_string(), color)
            })
            .collect()
    }

    pub(super) fn resolve_ui_color(
        &self,
        base: &ThemeBaseColor,
        default: Option<&HashMap<String, Color>>,
    ) -> HashMap<String, Color> {
        Self::resolve_color(&self.ui, base, default)
    }

    pub(super) fn resolve_syntax_color(
        &self,
        base: &ThemeBaseColor,
        default: Option<&HashMap<String, Color>>,
    ) -> HashMap<String, Color> {
        Self::resolve_color(&self.syntax, base, default)
    }
}

#[cfg(test)]
mod tests {
    use config::Config;
    use floem::peniko::Color;

    use crate::config::{
        DEFAULT_DARK_THEME_COLOR_CONFIG, DEFAULT_SPARROW_CONFIG,
        color_theme::ColorThemeConfig,
    };

    #[test]
    fn test_resolve() {
        // Hermetic: no filesystem access. This test used to route through
        // `SparrowConfig::resolve_theme`, which reads
        // `~/.config/sparrow/settings.toml` and merges it on top of the
        // test theme — meaning the test's result depended on whatever the
        // user had last saved from the theme color settings view. We now
        // exercise the pure resolution functions directly.
        let test_theme_str = r##"
[color-theme]
name = "test"
color-preference = "dark"

[ui]

[color-theme.base]
"blah" = "#ff00ff"
"text" = "#000000"

[color-theme.syntax]

[color-theme.ui]
"sparrow.error" = "#ffffff"
"editor.background" = "$blah"
"editor.foreground" = "$text"
"##;

        let cfg = Config::builder()
            .add_source(config::File::from_str(
                test_theme_str,
                config::FileFormat::Toml,
            ))
            .build()
            .unwrap();
        let test_theme: ColorThemeConfig = cfg.get("color-theme").unwrap();

        // The test theme intentionally overrides only a few keys. Seed its
        // UI map with the default dark theme's entries so that
        // `sparrow.warn` (which we assert falls through to the default) is
        // present in the merged input — this mirrors what
        // `merge_config` produces in the real loader.
        let test_ui = test_theme.ui.clone();
        let mut merged_ui = DEFAULT_DARK_THEME_COLOR_CONFIG.ui.clone();
        merged_ui.extend(test_ui);
        let theme = ColorThemeConfig {
            ui: merged_ui,
            ..test_theme
        };

        // Resolve with the same defaults `SparrowConfig::load` uses.
        let base = theme
            .base
            .resolve(Some(&DEFAULT_DARK_THEME_COLOR_CONFIG.base));
        let ui = theme
            .resolve_ui_color(&base, Some(&DEFAULT_SPARROW_CONFIG.color.ui));

        // Basic override from the test theme.
        assert_eq!(
            ui.get("sparrow.error"),
            Some(&Color::WHITE),
            "Failed to get basic theme override"
        );
        // Fall through to the default dark theme for an unspecified color.
        assert_eq!(
            ui.get("sparrow.warn"),
            Some(&Color::from_rgb8(0xE5, 0xC0, 0x7B)),
            "Failed to get from fallback dark theme"
        );
        // Custom variable `$blah` resolved correctly.
        assert_eq!(
            ui.get("editor.background"),
            Some(&Color::from_rgb8(0xFF, 0x00, 0xFF)),
            "Failed to get from custom variable"
        );
        // Custom variable `$text` resolved correctly, overriding the value
        // it inherits from the dark theme's `editor.foreground = "$text"`.
        assert_eq!(
            ui.get("editor.foreground"),
            Some(&Color::BLACK),
            "Failed to get from custom variable circle back around"
        );
    }
}
