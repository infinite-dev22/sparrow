//! Workspace path navigation in the status bar.

use std::{rc::Rc, sync::Arc};

use floem::{
    View,
    reactive::{Memo, ReadSignal, SignalGet, SignalWith},
    style::CursorStyle,
    views::{Decorators, dyn_stack, label, stack, svg},
};

use crate::{
    config::{SparrowConfig, color::SparrowColor},
    editor::EditorData,
    panel::kind::PanelKind,
    window_tab::WindowTabData,
};

pub(super) fn status_breadcrumbs(
    window: Rc<WindowTabData>,
    editor: Memo<Option<EditorData>>,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    let workspace = window.workspace.clone();
    dyn_stack(
        move || {
            let Some(editor) = editor.get() else {
                return Vec::new().into_iter().enumerate();
            };
            // Preview tabs can change documents while retaining the editor ID.
            let doc = editor.doc_signal().get();
            let path = doc.content.with(|content| match content {
                crate::doc::DocContent::History(history) => {
                    Some(history.path.clone())
                }
                content => content.path().cloned(),
            });
            path.map(|path| breadcrumb_paths(workspace.path.as_deref(), &path))
                .unwrap_or_default()
                .into_iter()
                .enumerate()
        },
        |(i, path)| (*i, path.clone()),
        move |(i, path)| {
            let is_file = editor.get_untracked().is_some_and(|editor| {
                editor
                    .doc()
                    .content
                    .with_untracked(|content| content.path() == Some(&path))
            });
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let icon_path = path.clone();
            let icon_color_path = path.clone();
            let window = window.clone();
            crate::widgets::breadcrumb_item(
                i == 0,
                stack((
                    svg(move || config.get().files_svg(&[&icon_path]).0).style(
                        move |s| {
                            s.size(14.0, 14.0)
                                .margin_right(4.0)
                                .apply_if(!is_file, |s| s.hide())
                                .apply_opt(
                                    config.get().files_svg(&[&icon_color_path]).1,
                                    |s, color| s.color(color),
                                )
                        },
                    ),
                    label(move || name.clone())
                        .style(|s| s.selectable(false).margin_horiz(2.0)),
                ))
                .style(|s| s.items_center()),
                config,
            )
            .on_click_stop(move |_| {
                window.file_explorer.reveal_in_file_tree(path.clone());
                window.show_panel(PanelKind::FileExplorer);
            })
            .style(move |s| {
                s.items_center().cursor(CursorStyle::Pointer).hover(|s| {
                    s.background(
                        config.get().color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                    )
                })
            })
        },
    )
    .style(|s| s.padding_left(10.0).items_center().min_width(0.0))
}

fn breadcrumb_paths(
    root: Option<&std::path::Path>,
    file: &std::path::Path,
) -> Vec<std::path::PathBuf> {
    let mut paths: Vec<_> = file
        .ancestors()
        .take_while(|path| Some(*path) != root)
        .filter(|path| path.file_name().is_some())
        .map(std::path::Path::to_path_buf)
        .collect();
    if let Some(root) = root.filter(|root| file.starts_with(root)) {
        paths.push(root.to_path_buf());
    }
    paths.reverse();
    paths
}

#[cfg(test)]
mod breadcrumb_tests {
    use super::*;
    use std::path::{Path, PathBuf};
    #[test]
    fn workspace_crumbs_have_real_navigation_paths() {
        let paths = breadcrumb_paths(
            Some(Path::new("/work/app")),
            Path::new("/work/app/lib/my file.dart"),
        );
        assert_eq!(
            paths,
            ["/work/app", "/work/app/lib", "/work/app/lib/my file.dart"]
                .map(PathBuf::from)
        );
        let outside = breadcrumb_paths(
            Some(Path::new("/work/app")),
            Path::new("/work/app_other/main.dart"),
        );
        assert!(!outside.contains(&PathBuf::from("/work/app")));
    }
}
