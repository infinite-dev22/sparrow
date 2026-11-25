//! Published panel resolutions and the corresponding preview viewports.
use std::collections::HashSet;

use once_cell::sync::Lazy;
use serde::Deserialize;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Display {
    pub pixels: (u32, u32),
    pub logical: (u32, u32),
    pub scale: f64,
    pub derived: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum DevicePlatform {
    Android,
    #[serde(rename = "ios")]
    Ios,
    #[serde(rename = "ipad-os")]
    IpadOs,
    Generic,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum DeviceCategory {
    Phone,
    Tablet,
    Generic,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum CutoutShape {
    None,
    Notch,
    Pill,
    PunchHole,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum NavigationIndicator {
    HomeIndicator,
    GesturePill,
    NavigationBar,
}

#[derive(Clone, Debug)]
pub(super) struct Device {
    pub id: String,
    pub name: String,
    pub platform: DevicePlatform,
    pub category: DeviceCategory,
    pub display: Display,
    pub cover: Option<Display>,
    pub ios: bool,
    pub corner_radius: f64,
    pub status_bar_height: f64,
    pub navigation_height: f64,
    pub cutout: CutoutShape,
    pub navigation: NavigationIndicator,
    pub frame_color: (u8, u8, u8),
    pub source_url: String,
    pub source_date: String,
    pub frame_source: String,
    pub frame_source_date: String,
}

#[derive(Deserialize)]
struct DeviceCatalog {
    devices: Vec<DeviceDefinition>,
}

#[derive(Deserialize)]
struct DeviceDefinition {
    id: String,
    name: String,
    ios: bool,
    platform: DevicePlatform,
    category: DeviceCategory,
    display: DisplayDefinition,
    #[serde(default)]
    cover: Option<DisplayDefinition>,
    corner_radius: f64,
    status_bar_height: f64,
    navigation_height: f64,
    cutout: CutoutShape,
    navigation: NavigationIndicator,
    frame_color: (u8, u8, u8),
    source_url: String,
    source_date: String,
    frame_source: String,
    frame_source_date: String,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum DisplayDefinition {
    Android {
        pixels: (u32, u32),
        ppi: f64,
    },
    Retina {
        pixels: (u32, u32),
        logical: (u32, u32),
        scale: f64,
    },
}

impl DisplayDefinition {
    fn into_display(self) -> Result<Display, String> {
        let display = match self {
            Self::Android { pixels, ppi } => {
                if !ppi.is_finite() || ppi <= 0.0 {
                    return Err(
                        "Android display density must be finite and positive."
                            .into(),
                    );
                }
                let scale = ppi / 160.0;
                Display {
                    pixels,
                    logical: (
                        (pixels.0 as f64 / scale + 0.5) as u32,
                        (pixels.1 as f64 / scale + 0.5) as u32,
                    ),
                    scale,
                    derived: true,
                }
            }
            Self::Retina {
                pixels,
                logical,
                scale,
            } => Display {
                pixels,
                logical,
                scale,
                derived: false,
            },
        };
        if display.pixels.0 == 0
            || display.pixels.1 == 0
            || display.logical.0 == 0
            || display.logical.1 == 0
            || !display.scale.is_finite()
            || display.scale <= 0.0
        {
            return Err(
                "Device display dimensions and scale must be positive.".into()
            );
        }
        Ok(display)
    }
}

fn parse_device_catalog(source: &str) -> Result<Vec<Device>, String> {
    let catalog: DeviceCatalog = toml::from_str(source)
        .map_err(|error| format!("Invalid device catalog: {error}"))?;
    let mut ids = HashSet::with_capacity(catalog.devices.len());
    catalog
        .devices
        .into_iter()
        .map(|definition| {
            if definition.id.trim().is_empty() || definition.name.trim().is_empty() {
                return Err("Device catalog ids and names must not be empty.".into());
            }
            if !ids.insert(definition.id.clone()) {
                return Err(format!("Duplicate device id: {}", definition.id));
            }
            if definition.source_url.trim().is_empty()
                || definition.source_date.trim().is_empty()
                || definition.frame_source.trim().is_empty()
                || definition.frame_source_date.trim().is_empty()
            {
                return Err(format!(
                    "Device {} is missing spec or frame provenance.",
                    definition.id
                ));
            }
            if !definition.corner_radius.is_finite()
                || definition.corner_radius < 0.0
                || !definition.status_bar_height.is_finite()
                || definition.status_bar_height < 0.0
                || !definition.navigation_height.is_finite()
                || definition.navigation_height < 0.0
            {
                return Err(format!(
                    "Device {} has invalid frame dimensions.",
                    definition.id
                ));
            }
            Ok(Device {
                id: definition.id,
                name: definition.name,
                platform: definition.platform,
                category: definition.category,
                display: definition.display.into_display()?,
                cover: definition
                    .cover
                    .map(DisplayDefinition::into_display)
                    .transpose()?,
                ios: definition.ios,
                corner_radius: definition.corner_radius,
                status_bar_height: definition.status_bar_height,
                navigation_height: definition.navigation_height,
                cutout: definition.cutout,
                navigation: definition.navigation,
                frame_color: definition.frame_color,
                source_url: definition.source_url,
                source_date: definition.source_date,
                frame_source: definition.frame_source,
                frame_source_date: definition.frame_source_date,
            })
        })
        .collect()
}

static DEVICES: Lazy<Vec<Device>> = Lazy::new(|| {
    parse_device_catalog(include_str!("device-presets.toml"))
        .expect("Invalid built-in mobile preview device catalog")
});

pub(super) fn devices() -> &'static [Device] {
    DEVICES.as_slice()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum DeviceChoice {
    Preset(String),
    Flexible,
}

impl DeviceChoice {
    pub fn device(&self) -> Option<&'static Device> {
        match self {
            Self::Preset(id) => devices()
                .iter()
                .find(|device| device.id.as_str() == id.as_str()),
            Self::Flexible => None,
        }
    }

    pub fn is_flexible(&self) -> bool {
        matches!(self, Self::Flexible)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum PreviewLocale {
    EnglishUnitedStates,
    EnglishUnitedKingdom,
    FrenchFrance,
    GermanGermany,
    JapaneseJapan,
    ArabicSaudiArabia,
}

impl PreviewLocale {
    pub const ALL: [Self; 6] = [
        Self::EnglishUnitedStates,
        Self::EnglishUnitedKingdom,
        Self::FrenchFrance,
        Self::GermanGermany,
        Self::JapaneseJapan,
        Self::ArabicSaudiArabia,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Self::EnglishUnitedStates => "en-US",
            Self::EnglishUnitedKingdom => "en-GB",
            Self::FrenchFrance => "fr-FR",
            Self::GermanGermany => "de-DE",
            Self::JapaneseJapan => "ja-JP",
            Self::ArabicSaudiArabia => "ar-SA",
        }
    }
}

impl fmt::Display for PreviewLocale {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Viewport {
    pub size: (u32, u32),
    pub scale: f64,
    pub landscape: bool,
    pub dark: bool,
    pub ios: bool,
    pub mobile: bool,
    pub locale: PreviewLocale,
}

impl Viewport {
    pub fn new(
        display: Display,
        landscape: bool,
        dark: bool,
        ios: bool,
        locale: PreviewLocale,
    ) -> Self {
        let (w, h) = display.logical;
        Self {
            size: if landscape { (h, w) } else { (w, h) },
            scale: display.scale,
            landscape,
            dark,
            ios,
            mobile: true,
            locale,
        }
    }

    pub fn custom(width: &str, height: &str, scale: &str) -> Option<Display> {
        let width: u32 = width.trim().parse().ok()?;
        let height: u32 = height.trim().parse().ok()?;
        let scale: f64 = scale.trim().parse().ok()?;
        if !(200..=2000).contains(&width)
            || !(200..=2500).contains(&height)
            || !(0.5..=4.0).contains(&scale)
        {
            return None;
        }
        Some(Display {
            pixels: (
                (width as f64 * scale).round() as u32,
                (height as f64 * scale).round() as u32,
            ),
            logical: (width, height),
            scale,
            derived: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_preserves_device_scale_and_fold_displays_are_distinct() {
        let device = devices()
            .iter()
            .find(|d| d.name == "Pixel 10 Pro Fold")
            .unwrap();
        let inner = Viewport::new(
            device.display,
            true,
            true,
            false,
            PreviewLocale::EnglishUnitedStates,
        );
        let outer = Viewport::new(
            device.cover.unwrap(),
            false,
            false,
            false,
            PreviewLocale::EnglishUnitedStates,
        );
        assert_eq!(device.display.pixels, (2076, 2152));
        assert_eq!(inner.size.0, device.display.logical.1);
        assert_eq!(inner.scale, device.display.scale);
        assert_ne!(inner.size, outer.size);
    }

    #[test]
    fn tablet_profiles_keep_logical_size_scale_and_platform_identity() {
        let ipad = devices()
            .iter()
            .find(|device| device.id == "ipad-pro-11-m4")
            .unwrap();
        let galaxy_tab = devices()
            .iter()
            .find(|device| device.id == "galaxy-tab-s9")
            .unwrap();
        let viewport = Viewport::new(
            ipad.display,
            false,
            false,
            ipad.ios,
            PreviewLocale::JapaneseJapan,
        );

        assert_eq!(viewport.size, (834, 1210));
        assert_eq!(viewport.scale, 2.0);
        assert!(viewport.ios);
        assert_eq!(viewport.locale, PreviewLocale::JapaneseJapan);
        assert_eq!(galaxy_tab.display.pixels, (1600, 2560));
        assert!(!galaxy_tab.ios);
    }

    #[test]
    fn flexible_dimensions_are_bounded_and_require_a_finite_scale() {
        assert_eq!(
            Viewport::custom("400", "800", "2.5").unwrap().pixels,
            (1000, 2000)
        );
        assert!(Viewport::custom("0", "800", "2").is_none());
        assert!(Viewport::custom("400", "800", "NaN").is_none());
    }

    #[test]
    fn device_choices_resolve_by_unique_stable_ids() {
        let mut ids = devices()
            .iter()
            .map(|device| device.id.as_str())
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids.dedup();

        assert_eq!(ids.len(), devices().len());
        assert_eq!(
            devices()
                .iter()
                .filter(|device| {
                    device.category == DeviceCategory::Phone
                        && device.platform == DevicePlatform::Android
                })
                .count(),
            6
        );
        assert_eq!(
            devices()
                .iter()
                .filter(|device| {
                    device.category == DeviceCategory::Phone
                        && device.platform == DevicePlatform::Ios
                })
                .count(),
            6
        );
        assert_eq!(
            devices()
                .iter()
                .filter(|device| device.category == DeviceCategory::Tablet)
                .count(),
            2
        );
        assert_eq!(
            devices()
                .iter()
                .filter(|device| device.category == DeviceCategory::Generic)
                .count(),
            3
        );
        assert!(devices().iter().all(|device| {
            !device.source_url.is_empty()
                && device.source_date == "2026-11-25"
                && !device.frame_source.is_empty()
                && device.frame_source_date == "2026-11-25"
        }));
        assert_eq!(
            DeviceChoice::Preset("iphone-12-pro".into())
                .device()
                .map(|device| device.name.as_str()),
            Some("iPhone 12 Pro")
        );
        assert!(DeviceChoice::Flexible.is_flexible());
        assert!(DeviceChoice::Flexible.device().is_none());
    }

    #[test]
    fn device_catalog_rejects_duplicate_ids_and_invalid_dimensions() {
        let duplicate = r#"
            [[devices]]
            id = "same"
            name = "One"
            ios = false
            platform = "android"
            category = "phone"
            display = { kind = "android", pixels = [100, 200], ppi = 160.0 }
            corner_radius = 10.0
            status_bar_height = 24.0
            navigation_height = 24.0
            cutout = "none"
            navigation = "gesture-pill"
            frame_color = [1, 2, 3]
            source_url = "https://example.test/one"
            source_date = "2026-11-25"
            frame_source = "Test approximation"
            frame_source_date = "2026-11-25"

            [[devices]]
            id = "same"
            name = "Two"
            ios = false
            platform = "android"
            category = "phone"
            display = { kind = "android", pixels = [100, 200], ppi = 160.0 }
            corner_radius = 10.0
            status_bar_height = 24.0
            navigation_height = 24.0
            cutout = "none"
            navigation = "gesture-pill"
            frame_color = [1, 2, 3]
            source_url = "https://example.test/two"
            source_date = "2026-11-25"
            frame_source = "Test approximation"
            frame_source_date = "2026-11-25"
        "#;
        let invalid = r#"
            [[devices]]
            id = "zero-size"
            name = "Zero"
            ios = false
            platform = "android"
            category = "phone"
            display = { kind = "retina", pixels = [0, 200], logical = [100, 200], scale = 2.0 }
            corner_radius = 10.0
            status_bar_height = 24.0
            navigation_height = 24.0
            cutout = "none"
            navigation = "gesture-pill"
            frame_color = [1, 2, 3]
            source_url = "https://example.test/zero"
            source_date = "2026-11-25"
            frame_source = "Test approximation"
            frame_source_date = "2026-11-25"
        "#;

        assert!(
            parse_device_catalog(duplicate)
                .unwrap_err()
                .contains("Duplicate")
        );
        assert!(
            parse_device_catalog(invalid)
                .unwrap_err()
                .contains("positive")
        );
    }
}
