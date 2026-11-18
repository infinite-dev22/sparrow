//! Local color and icon theme metadata. No executable extension runtime.
use anyhow::Result;
use sparrow_core::directory::Directory;
use sparrow_rpc::plugin::VoltMetadata;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};
pub fn find_themes(extra_theme_paths: &[PathBuf]) -> Vec<VoltMetadata> {
    let Some(plugin_dir) = Directory::plugins_directory() else {
        return Vec::new();
    };

    let mut themes: Vec<VoltMetadata> = plugin_dir
        .read_dir()
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|result| {
            let entry = result.ok()?;
            let metadata = entry.metadata().ok()?;

            // Ignore any loose files or '.' prefixed hidden directories
            if metadata.is_file() || entry.file_name().to_str()?.starts_with('.') {
                return None;
            }

            Some(entry.path())
        })
        .filter_map(|path| match load_theme(&path) {
            Ok(metadata) => Some(metadata),
            Err(e) => {
                tracing::error!("Failed to load theme: {:?}", e);
                None
            }
        })
        .collect();

    for plugin_path in extra_theme_paths {
        let mut metadata = match load_theme(plugin_path) {
            Ok(metadata) => metadata,
            Err(e) => {
                tracing::error!("Failed to load extra theme: {:?}", e);
                continue;
            }
        };

        let pos = themes.iter().position(|meta| {
            meta.name == metadata.name && meta.author == metadata.author
        });

        if let Some(pos) = pos {
            std::mem::swap(&mut themes[pos], &mut metadata);
        } else {
            themes.push(metadata);
        }
    }

    themes
}
pub fn load_theme(path: &Path) -> Result<VoltMetadata> {
    let path = path.canonicalize()?;
    let mut file = fs::File::open(path.join("volt.toml"))?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let mut meta: VoltMetadata = toml::from_str(&contents)?;

    if meta.wasm.is_some() {
        anyhow::bail!(
            "Executable extensions are not supported; Sparrow uses the Dart SDK directly."
        );
    }
    if meta.color_themes.as_ref().is_none_or(|v| v.is_empty())
        && meta.icon_themes.as_ref().is_none_or(|v| v.is_empty())
    {
        anyhow::bail!("Expected a color or icon theme package");
    }
    meta.config = None;
    meta.activation = None;
    meta.dir = Some(path.clone());
    let resolve = |assets: Option<Vec<String>>| -> Option<Vec<String>> {
        let paths = assets?
            .into_iter()
            .filter_map(|asset| {
                let p = path.join(asset).canonicalize().ok()?;
                p.is_file()
                    .then(|| p.to_str().map(str::to_string))
                    .flatten()
            })
            .collect::<Vec<_>>();
        (!paths.is_empty()).then_some(paths)
    };
    meta.color_themes = resolve(meta.color_themes.take());
    meta.icon_themes = resolve(meta.icon_themes.take());
    if meta.color_themes.is_none() && meta.icon_themes.is_none() {
        anyhow::bail!("Theme package contains no readable appearance assets");
    }

    Ok(meta)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn metadata(extra: &str) -> String {
        format!(
            "name = 'theme'\nversion = '1.0.0'\ndisplay-name = 'Theme'\nauthor = 'test'\ndescription = 'Local appearance'\n{extra}"
        )
    }
    #[test]
    fn executable_packages_cannot_be_loaded_as_themes() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("volt.toml"),
            metadata("wasm = 'plugin.wasm'\ncolor-themes = ['theme.toml']"),
        )
        .unwrap();
        fs::write(dir.path().join("theme.toml"), "").unwrap();
        assert!(load_theme(dir.path()).is_err());
    }
    #[test]
    fn missing_assets_are_not_advertised_as_installed_themes() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("volt.toml"),
            metadata("color-themes = ['missing.toml']"),
        )
        .unwrap();
        assert!(load_theme(dir.path()).is_err());
    }
    #[test]
    fn local_color_theme_metadata_is_retained() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("volt.toml"),
            metadata("color-themes = ['theme.toml']"),
        )
        .unwrap();
        fs::write(
            dir.path().join("theme.toml"),
            "[color-theme]\nname='Theme'\n",
        )
        .unwrap();
        let meta = load_theme(dir.path()).unwrap();
        assert!(meta.wasm.is_none());
        assert_eq!(meta.color_themes.unwrap().len(), 1);
    }
}
