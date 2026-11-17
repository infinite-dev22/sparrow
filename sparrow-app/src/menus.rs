//! Shared context-menu behavior for file, editor and tab surfaces.
use crate::popup_menu::{Menu, MenuItem};
use floem::{
    views::editor::{core::register::Clipboard, text::SystemClipboard},
};
use std::path::{Path, PathBuf};

/// Snapshot rebuilt when a native menu opens (and when observed state changes).
pub struct MenuContext {
    pub run_label: String,
    pub debug_label: String,
    pub can_run: bool,
    pub can_stop: bool,
    pub can_reload: bool,
    pub dart_editor: bool,
}
impl MenuContext {
    pub fn from_window(tab: &crate::window_tab::WindowTabData) -> Self {
        use floem::reactive::{SignalGet, SignalWith};
        let selected = tab.run_configs.with(|configs| {
            configs
                .get(tab.active_run_config_idx.get())
                .map(|config| config.name.clone())
        });
        let processes = tab.terminal.run_debug_process(true);
        let can_stop = processes.iter().any(|(_, process)| !process.stopped);
        let can_reload = processes.iter().any(|(_, process)| {
            !process.stopped
                && crate::dart::run_config::is_flutter_run(&process.config)
        });
        let dart_editor = tab.main_split.active_editor.get().is_some_and(|editor| {
            editor.doc().content.with(|content| {
                content.path().is_some_and(|path| {
                    path.extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("dart"))
                })
            })
        });
        Self {
            run_label: selected
                .as_ref()
                .map(|name| format!("Run '{name}'"))
                .unwrap_or_else(|| "Run".into()),
            debug_label: selected
                .as_ref()
                .map(|name| format!("Debug '{name}'"))
                .unwrap_or_else(|| "Debug".into()),
            can_run: selected.is_some(),
            can_stop,
            can_reload,
            dart_editor,
        }
    }
}

pub fn path_references(
    path: &Path,
    workspace: Option<&Path>,
) -> Vec<(&'static str, String)> {
    let mut entries = vec![("Absolute Path", path.to_string_lossy().into_owned())];
    if let Some(name) = path.file_name() {
        entries.push(("File Name", name.to_string_lossy().into_owned()));
    }
    if let Some(relative) = workspace.and_then(|root| path.strip_prefix(root).ok()) {
        entries.push((
            "Path From Content Root",
            relative.to_string_lossy().replace('\\', "/"),
        ));
        for source in ["lib", "test", "bin"] {
            if let Ok(relative) = relative.strip_prefix(source) {
                entries.push((
                    "Path From Source Root",
                    relative.to_string_lossy().replace('\\', "/"),
                ));
                break;
            }
        }
    }
    if let Some(root) = path
        .ancestors()
        .skip(1)
        .find(|root| root.join(".git").exists())
    {
        if let Ok(relative) = path.strip_prefix(root) {
            entries.push((
                "Path From Repository Root",
                relative.to_string_lossy().replace('\\', "/"),
            ));
        }
    }
    entries
}

pub fn copy_path_menu(path: PathBuf, workspace: Option<PathBuf>) -> Menu {
    let mut menu = Menu::new("Copy Path/Reference");
    for (label, value) in path_references(&path, workspace.as_deref()) {
        menu = menu.entry(
            MenuItem::new(label)
                .action(move || SystemClipboard::new().put_string(value.clone())),
        );
    }
    menu
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn flutter_paths_distinguish_content_and_source_roots() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join(".git")).unwrap();
        let path = root.path().join("lib/src/a.dart");
        let refs = path_references(&path, Some(root.path()));
        let get = |label| {
            refs.iter()
                .find(|(name, _)| *name == label)
                .unwrap()
                .1
                .as_str()
        };
        assert_eq!(get("File Name"), "a.dart");
        assert_eq!(get("Path From Content Root"), "lib/src/a.dart");
        assert_eq!(get("Path From Source Root"), "src/a.dart");
        assert_eq!(get("Path From Repository Root"), "lib/src/a.dart");
    }
    #[test]
    fn outside_workspace_has_no_misleading_relative_reference() {
        let root = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap().path().join("a.dart");
        let refs = path_references(&external, Some(root.path()));
        assert!(!refs.iter().any(|(name, _)| name.contains("Content") || name.contains("Source")));
    }
}

pub fn copy_text(value:&str) {SystemClipboard::new().put_string(value.to_string());}
