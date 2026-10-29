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
            } else if name.starts_with("dart_")
                || name.starts_with("flutter_")
                || name.starts_with("material_")
                || name.starts_with("cupertino_")
                || name.starts_with("pub_")
                || name.starts_with("widget_")
            {
                &DART_ICONS_DIR
            } else if let Some(_) = LUCIDE_ICONS_DIR.get_file(name) {
                &LUCIDE_ICONS_DIR
            } else if let Some(_) = TABLER_ICONS_DIR.get_file(name) {
                &TABLER_ICONS_DIR
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
