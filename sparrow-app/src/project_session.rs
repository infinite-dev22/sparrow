//! Personal project state, separate from shareable settings and run configurations.
use crate::{
    doc::DocContent,
    editor_tab::EditorTabChildInfo,
    main_split::{SplitContentInfo, SplitInfo},
    workspace::WorkspaceInfo,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
#[derive(Serialize, Deserialize)]
struct Session {
    version: u32,
    workspace: WorkspaceInfo,
}
fn content_paths(content: &mut DocContent, visit: &mut impl FnMut(&mut PathBuf)) {
    match content {
        DocContent::File { path, .. } => visit(path),
        DocContent::History(history) => visit(&mut history.path),
        _ => {}
    }
}
fn split_paths(split: &mut SplitInfo, visit: &mut impl FnMut(&mut PathBuf)) {
    for child in &mut split.children {
        match child {
            SplitContentInfo::Split(split) => split_paths(split, visit),
            SplitContentInfo::EditorTab(tab) => {
                for child in &mut tab.children {
                    match child {
                        EditorTabChildInfo::Editor(editor) => {
                            content_paths(&mut editor.content, visit)
                        }
                        EditorTabChildInfo::DiffEditor(editor) => {
                            content_paths(&mut editor.left_content, visit);
                            content_paths(&mut editor.right_content, visit);
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}
fn paths(info: &mut WorkspaceInfo, mut visit: impl FnMut(&mut PathBuf)) {
    split_paths(&mut info.split, &mut visit);
    info.breakpoints = std::mem::take(&mut info.breakpoints)
        .into_iter()
        .map(|(mut path, points)| {
            visit(&mut path);
            (path, points)
        })
        .collect();
    info.bookmarks = std::mem::take(&mut info.bookmarks)
        .into_iter()
        .map(|(mut path, lines)| {
            visit(&mut path);
            (path, lines)
        })
        .collect();
    info.document_views = std::mem::take(&mut info.document_views)
        .into_iter()
        .map(|(mut path, mode)| {
            visit(&mut path);
            (path, mode)
        })
        .collect();
}
pub fn read(root: &Path) -> Result<Option<WorkspaceInfo>, String> {
    let path = root.join(".sparrow/workspace.json");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(None);
        }
        Err(error) => return Err(error.to_string()),
    };
    let mut session: Session = serde_json::from_str(&text)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if session.version != 1 {
        return Err(format!(
            "Unsupported project workspace version {}",
            session.version
        ));
    }
    paths(&mut session.workspace, |path| {
        if path.is_relative() {
            *path = root.join(&*path);
        }
    });
    Ok(Some(session.workspace))
}
pub fn write(root: &Path, info: &WorkspaceInfo) -> Result<(), String> {
    let dir = root.join(".sparrow");
    if dir.is_symlink() {
        return Err("Project session directory must not be a symbolic link.".into());
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    if let Ok(existing) = std::fs::read_to_string(dir.join("workspace.json")) {
        match serde_json::from_str::<serde_json::Value>(&existing) {
            Ok(value)
                if value["version"].as_u64().is_some_and(|version| version > 1) =>
            {
                return Err(
                    "A newer IDE wrote this project session; keeping it intact."
                        .into(),
                );
            }
            Err(_) => {
                crate::persistence::write_files(vec![(
                    dir.join("workspace.json.backup"),
                    existing,
                )])?;
            }
            _ => {}
        }
    }
    // This file can contain unsaved editor text and must remain personal.
    let ignore = dir.join(".gitignore");
    let mut ignored = std::fs::read_to_string(&ignore).unwrap_or_default();
    if !ignored
        .lines()
        .any(|line| line.trim() == "/workspace.json*")
    {
        if !ignored.is_empty() && !ignored.ends_with('\n') {
            ignored.push('\n');
        }
        ignored.push_str("/workspace.json*\n");
        crate::persistence::write_files(vec![(ignore, ignored)])?;
    }
    let mut info = info.clone();
    paths(&mut info, |path| {
        if let Ok(relative) = path.strip_prefix(root) {
            *path = relative.to_path_buf();
        }
    });
    let text = serde_json::to_string_pretty(&Session {
        version: 1,
        workspace: info,
    })
    .map_err(|e| e.to_string())?;
    crate::persistence::write_files(vec![(dir.join("workspace.json"), text)])
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        main_split::SplitDirection,
        panel::data::{PanelInfo, PanelSize, default_panel_order},
    };
    fn info(root: &Path) -> WorkspaceInfo {
        WorkspaceInfo {
            commit_message: "Draft commit".into(),
            amend_commit: true,
            selected_run_configuration: Some("main.dart".into()),
            split: SplitInfo {
                children: vec![SplitContentInfo::EditorTab(
                    crate::editor_tab::EditorTabInfo {
                        active: 0,
                        is_focus: true,
                        children: vec![EditorTabChildInfo::Editor(
                            crate::editor::EditorInfo {
                                content: DocContent::File {
                                    path: root.join("lib/main.dart"),
                                    read_only: false,
                                },
                                unsaved: Some("void main() {}".into()),
                                offset: 5,
                                scroll_offset: (0.0, 22.0),
                                confirmed: Some(true),
                            },
                        )],
                    },
                )],
                direction: SplitDirection::Vertical,
            },
            panel: PanelInfo {
                panels: default_panel_order(),
                styles: Default::default(),
                sections: Default::default(),
                size: PanelSize {
                    left: 300.0,
                    left_split: 0.5,
                    right: 300.0,
                    right_split: 0.5,
                    bottom: 300.0,
                    bottom_split: 0.5,
                },
            },
            breakpoints: Default::default(),
            bookmarks: std::collections::BTreeMap::from([(
                root.join("lib/main.dart"),
                [12usize].into(),
            )]),
            document_views: std::collections::BTreeMap::from([(
                root.join("README.md"),
                "split".into(),
            )]),
        }
    }
    #[test]
    fn personal_state_roundtrips_and_relative_paths_survive_project_move() {
        let root = tempfile::tempdir().unwrap();
        write(root.path(), &info(root.path())).unwrap();
        let text =
            std::fs::read_to_string(root.path().join(".sparrow/workspace.json"))
                .unwrap();
        assert!(!text.contains(root.path().to_str().unwrap()));
        let other = tempfile::tempdir().unwrap();
        std::fs::create_dir(other.path().join(".sparrow")).unwrap();
        std::fs::write(other.path().join(".sparrow/workspace.json"), text).unwrap();
        let restored = read(other.path()).unwrap().unwrap();
        assert!(
            restored.bookmarks[&other.path().join("lib/main.dart")].contains(&12)
        );
        assert_eq!(
            restored.document_views[&other.path().join("README.md")],
            "split"
        );
        let SplitContentInfo::EditorTab(tab) = &restored.split.children[0] else {
            panic!("Missing editor tab")
        };
        let EditorTabChildInfo::Editor(editor) = &tab.children[0] else {
            panic!("Missing editor")
        };
        assert_eq!(
            editor.content.path(),
            Some(&other.path().join("lib/main.dart"))
        );
        assert_eq!(editor.unsaved.as_deref(), Some("void main() {}"));
        assert_eq!(editor.offset, 5);
        assert!(
            std::fs::read_to_string(root.path().join(".sparrow/.gitignore"))
                .unwrap()
                .contains("/workspace.json")
        );
    }
    #[test]
    fn malformed_and_newer_sessions_report_failure_without_overwriting_them() {
        let root = tempfile::tempdir().unwrap();
        assert!(read(root.path()).unwrap().is_none());
        write(root.path(), &info(root.path())).unwrap();
        let path = root.path().join(".sparrow/workspace.json");
        let text = std::fs::read_to_string(&path)
            .unwrap()
            .replace("\"version\": 1", "\"version\": 99");
        std::fs::write(&path, &text).unwrap();
        assert!(read(root.path()).err().unwrap().contains("Unsupported"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        assert!(write(root.path(), &info(root.path())).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), text);
    }
}
