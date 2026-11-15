use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use include_dir::{Dir, include_dir};

use crate::config::LOGO;

const CODICONS_ICONS_DIR: Dir =
    include_dir!("$CARGO_MANIFEST_DIR/../icons/codicons");
const SPARROW_ICONS_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/../icons/sparrow");
const LUCIDE_ICONS_DIR:   Dir  = include_dir!("$CARGO_MANIFEST_DIR/../icons/lucide");
const TABLER_ICONS_DIR:   Dir  = include_dir!("$CARGO_MANIFEST_DIR/../icons/tabler");
const DART_ICONS_DIR:     Dir  = include_dir!("$CARGO_MANIFEST_DIR/../icons/dart-flutter");

/// Rendered when an icon key or file is missing. A visible dashed red
/// box, better than a panic and easy to spot.
pub const MISSING_ICON_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 16 16" fill="none" stroke="#c0392b" stroke-width="1.2" stroke-dasharray="2 2"><rect x="1" y="1" width="14" height="14" rx="2"/></svg>"##;

pub fn missing_icon_svg() -> String {
    MISSING_ICON_SVG.to_owned()
}

#[derive(Debug, Clone)]
pub struct SvgStore {
    svgs: HashMap<String, String>,
    svgs_on_disk: HashMap<PathBuf, Option<String>>,
}

impl Default for SvgStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SvgStore {
    fn new() -> Self {
        let mut svgs = HashMap::new();
        svgs.insert("sparrow_logo".to_string(), LOGO.to_string());

        Self {
            svgs,
            svgs_on_disk: HashMap::new(),
        }
    }

    pub fn logo_svg(&self) -> String {
        self.svgs.get("sparrow_logo").unwrap().clone()
    }

    pub fn get_default_svg(&mut self, name: &str) -> String {
        if !self.svgs.contains_key(name) {
            // Resolve in order of priority. Every directory listed here just
            // needs the physical `.svg` file to exist; the logical mapping
            // is done in icon-theme.toml.
            let dir = if name.starts_with("sparrow_") {
                &SPARROW_ICONS_DIR
            } else if let Some(_) = LUCIDE_ICONS_DIR.get_file(name) {
                &LUCIDE_ICONS_DIR
            } else if let Some(_) = TABLER_ICONS_DIR.get_file(name) {
                &TABLER_ICONS_DIR
            }  else if name.starts_with("dart_")
                || name.starts_with("flutter_")
                || name.starts_with("material_")
                || name.starts_with("cupertino_")
                || name.starts_with("pub_")
                || name.starts_with("widget_")
            {
                &DART_ICONS_DIR
            } else {
                &CODICONS_ICONS_DIR
            };

            let file = dir
                .get_file(name)
                .unwrap_or_else(|| panic!("Icon not found in any theme directory: {name}"));
            let content = file.contents_utf8().unwrap();
            self.svgs.insert(name.to_string(), content.to_string());
        }
        self.svgs.get(name).unwrap().clone()
    }

    pub fn get_svg_on_disk(&mut self, path: &Path) -> Option<String> {
        if !self.svgs_on_disk.contains_key(path) {
            let svg = fs::read_to_string(path).ok();
            self.svgs_on_disk.insert(path.to_path_buf(), svg);
        }

        self.svgs_on_disk.get(path).unwrap().clone()
    }
}

#[cfg(test)]
mod file_icon_tests {
    use super::*;
    #[test]
    fn flutter_project_file_types_resolve_to_distinct_embedded_icons() {
        let mut store=SvgStore::default();
        let mut rendered=std::collections::HashSet::new();
        for name in ["main.dart","pubspec.yaml","config.yaml","data.json","app.arb","README.md","image.png","drawing.svg","font.ttf","AndroidManifest.xml"] {
            let path=crate::config::DEFAULT_ICON_THEME_ICON_CONFIG.resolve_path_to_icon(&[Path::new(name)]).expect("file type must have an icon");
            let svg=store.get_default_svg(path.file_name().unwrap().to_str().unwrap());
            assert!(svg.contains("<svg"));
            rendered.insert(svg);
        }
        assert_eq!(rendered.len(),10);
    }
}

#[cfg(test)]
mod panel_icon_tests {
    use super::*;
    use strum::IntoEnumIterator;
    #[test]
    fn pinned_tools_have_distinct_readable_glyphs() {
        let mut store = SvgStore::default();
        let mut rendered = std::collections::HashSet::new();
        for kind in crate::panel::kind::PanelKind::iter().filter(|kind| kind.pinned()) {
            let path = crate::config::DEFAULT_ICON_THEME_ICON_CONFIG.ui.get(kind.svg_name()).unwrap();
            let svg = store.get_default_svg(path);
            assert!(svg.contains("<svg"));
            assert!(rendered.insert(svg), "duplicate glyph for {}", kind.title());
        }
    }
}

#[cfg(test)]
mod sdk_icon_tests {
    use super::*;
    #[test]
    fn sdk_toolbar_logos_resolve_to_colored_embedded_assets() {
        let mut store = SvgStore::default();
        for key in [crate::config::icon::SparrowIcons::DART, crate::config::icon::SparrowIcons::FLUTTER] {
            let path = crate::config::DEFAULT_ICON_THEME_ICON_CONFIG.ui.get(key).unwrap();
            let svg = store.get_default_svg(path);
            assert!(svg.contains("<svg") && svg.contains("fill=\"#"));
        }
    }
}

#[cfg(test)]
mod flutter_file_catalog_tests {
    use super::*;
    const FILES: &str = "main.dart model.g.dart model.freezed.dart service.mocks.dart pubspec.yaml analysis_options.yaml build.yaml pubspec.lock app.arb .metadata .packages package_config.json a.png a.jpg a.jpeg a.webp a.gif a.bmp a.svg a.ttf a.otf a.json a.riv a.frag a.db a.sqlite a.sqlite3 a.mp3 a.wav a.ogg a.mp4 a.glb a.gltf a.obj a.txt a.csv a.kt a.java build.gradle build.gradle.kts AndroidManifest.xml local.properties rules.pro signing.jks signing.keystore library.jar library.aar a.swift a.m a.mm a.h Info.plist a.entitlements a.xcconfig Podfile Podfile.lock LaunchScreen.storyboard a.xib Assets.xcassets Runner.xcodeproj Runner.xcworkspace a.framework a.xcframework index.html a.css a.js a.mjs a.wasm a.cpp a.c a.hpp CMakeLists.txt Runner.rc a.ico a.dll a.lib a.cc a.desktop a.so README.md .env .env.local .gitignore .gitattributes a.iml a.sh a.bat a.ps1";
    #[test]
    fn every_requested_file_type_loads_an_embedded_icon() {
        let theme = &crate::config::DEFAULT_ICON_THEME_ICON_CONFIG;
        let mut store = SvgStore::default();
        for name in FILES.split_whitespace() {
            let icon = theme.resolve_path_to_icon(&[Path::new(name)]).unwrap_or_else(|| panic!("no icon for {name}"));
            assert!(store.get_default_svg(icon.file_name().unwrap().to_str().unwrap()).contains("<svg"), "{name}");
        }
        for name in ["Assets.xcassets", "Runner.xcodeproj", "Runner.xcworkspace", "Flutter.framework", "Flutter.xcframework"] {
            let icon = theme.resolve_folder_to_icon(Path::new(name)).unwrap();
            assert!(store.get_default_svg(icon.file_name().unwrap().to_str().unwrap()).contains("<svg"));
        }
    }
    #[test]
    fn docker_names_override_yaml_without_affecting_other_configs() {
        let theme = &crate::config::DEFAULT_ICON_THEME_ICON_CONFIG;
        let icon = |name| theme.resolve_path_to_icon(&[Path::new(name)]).unwrap();
        let docker = icon("Dockerfile");
        for name in ["Dockerfile.dev", "Dockerfile.release", "dockerfile", "dockerfile.dev",
            "app.Dockerfile", ".dockerignore", "compose.yaml", "compose.yml",
            "compose.override.yaml", "compose.override.yml", "docker-compose.yaml",
            "docker-compose.yml", "docker-compose.override.yaml", "docker-compose.override.yml"] {
            assert_eq!(icon(name), docker, "{name}");
        }
        assert_ne!(icon("config.yaml"), docker);
        assert_ne!(icon("pubspec.yaml"), docker);
        assert_eq!(docker.file_name().unwrap(), "filetype-docker.svg");
    }

    #[test]
    fn all_bundled_filetype_assets_are_mapped_and_ui_assets_load() {
        let theme = &crate::config::DEFAULT_ICON_THEME_ICON_CONFIG;
        let mapped: std::collections::HashSet<_> = theme.filename.values()
            .chain(theme.extension.values()).chain(theme.foldername.values()).collect();
        let mut store = SvgStore::default();
        for file in LUCIDE_ICONS_DIR.files() {
            let name = file.path().to_str().unwrap();
            if name.starts_with("filetype-") && name.ends_with(".svg") {
                assert!(mapped.iter().any(|mapped| mapped.as_str() == name), "unmapped {name}");
                assert!(store.get_default_svg(name).contains("<svg"));
            }
        }
        for asset in theme.ui.values() {
            assert!(store.get_default_svg(asset).contains("<svg"), "{asset}");
        }
    }

    #[test]
    fn compound_generated_and_named_icons_override_generic_suffixes() {
        let theme = &crate::config::DEFAULT_ICON_THEME_ICON_CONFIG;
        let icon = |name| theme.resolve_path_to_icon(&[Path::new(name)]).unwrap();
        let generated: std::collections::HashSet<_> = ["a.dart", "a.g.dart", "a.freezed.dart", "a.mocks.dart"].map(icon).into_iter().collect();
        assert_eq!(generated.len(), 4);
        assert_ne!(icon("build.gradle.kts"), icon("script.kts"));
        assert_ne!(icon("Podfile.lock"), icon("pubspec.lock"));
        assert_eq!(icon("PHOTO.PNG"), icon("photo.png"));
        assert_eq!(icon(".env.local"), icon(".env"));
        assert!(theme.resolve_path_to_icon(&[Path::new("unknown.zzz")]).is_none());
    }
}
