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
