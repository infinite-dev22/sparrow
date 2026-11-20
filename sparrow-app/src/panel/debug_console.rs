//! Linkification shared by adapter consoles. Plain text remains selectable.
use crate::{
    config::color::SparrowColor, editor::location::EditorLocation,
    window_tab::WindowTabData,
};
use floem::{
    IntoView, View,
    reactive::{SignalGet, SignalUpdate},
    style::CursorStyle,
    views::{Decorators, label, stack_from_iter},
};
use std::{path::Path, rc::Rc, sync::LazyLock};

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Target {
    File(EditorLocation),
    Web(String),
}
#[derive(Clone, Debug)]
pub(super) struct Part {
    pub text: String,
    pub target: Option<Target>,
}

pub(super) fn parts(line: &str, root: Option<&Path>) -> Vec<Part> {
    static LINKS: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(
        r"https?://[^\s<>]+|(?:file://|package:)?[^\s()<>\[\]]+\.dart(?::|\s+)\d+(?::\d+)?"
    ).unwrap()
    });
    let mut parts = Vec::new();
    let mut previous = 0;
    for found in LINKS.find_iter(line) {
        let mut token = found.as_str().trim_end_matches([',', ';', '.']);
        while token.ends_with(')')
            && token.matches(')').count() > token.matches('(').count()
        {
            token = &token[..token.len() - 1];
        }
        if found.start() > previous {
            parts.push(Part {
                text: line[previous..found.start()].into(),
                target: None,
            });
        }
        let target = if token.starts_with("http://") || token.starts_with("https://")
        {
            url::Url::parse(token)
                .ok()
                .filter(|u| matches!(u.scheme(), "http" | "https"))
                .map(|_| Target::Web(token.into()))
        } else {
            super::flutter_run_view::parse_stack_trace_link(token, root)
                .map(Target::File)
        };
        parts.push(Part {
            text: token.into(),
            target,
        });
        previous = found.start() + token.len();
    }
    if previous < line.len() || parts.is_empty() {
        parts.push(Part {
            text: line[previous..].into(),
            target: None,
        });
    }
    parts
}

pub(super) fn row(line: String, window: Rc<WindowTabData>) -> impl View {
    let config = window.common.config;
    let views = parts(&line, window.workspace.path.as_deref())
        .into_iter()
        .map(move |part| {
            let linked = part.target.is_some();
            let window = window.clone();
            label(move || part.text.clone())
                .on_click_stop(move |_| match &part.target {
                    Some(Target::File(location)) => {
                        window.main_split.jump_to_location(location.clone(), None);
                        if !window
                            .common
                            .debugger_settings
                            .get_untracked()
                            .preview_files
                        {
                            if let Some(editor) =
                                window.main_split.active_editor.get_untracked()
                            {
                                editor.confirmed.set(true);
                            }
                        }
                    }
                    Some(Target::Web(uri)) => {
                        if let Err(error) = open::that(uri) {
                            window.show_alert(
                                "Cannot open console link".into(),
                                error.to_string(),
                                Vec::new(),
                            );
                        }
                    }
                    None => {}
                })
                .style(move |s| {
                    s.selectable(true)
                        .flex_shrink(0.0_f32)
                        .apply_if(linked, |s| {
                            s.color(config.get().color(SparrowColor::EDITOR_LINK))
                                .cursor(CursorStyle::Pointer)
                                .hover(|s| {
                                    s.background(config.get().color(
                                        SparrowColor::PANEL_HOVERED_BACKGROUND,
                                    ))
                                })
                        })
                })
                .into_any()
        });
    stack_from_iter(views).style(|s| s.items_center().min_width_full())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_unicode_punctuation_and_multiple_links() {
        let text = "✓ Visit (https://example.org/help), then http://localhost:8123/path?q=1.";
        let parts = parts(text, None);
        assert_eq!(
            parts.iter().map(|p| p.text.as_str()).collect::<String>(),
            text
        );
        let targets = parts
            .iter()
            .filter_map(|p| p.target.as_ref())
            .collect::<Vec<_>>();
        assert_eq!(
            targets,
            vec![
                &Target::Web("https://example.org/help".into()),
                &Target::Web("http://localhost:8123/path?q=1".into())
            ]
        );
    }
    #[test]
    fn file_links_keep_line_and_column_and_reject_missing_files() {
        let root = std::env::temp_dir()
            .join(format!("sparrow-console-link-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("sample.dart"), "void main() {}\n").unwrap();
        let parts = parts("sample.dart:3:4 missing.dart:8:1", Some(&root));
        let file = parts
            .iter()
            .filter_map(|p| p.target.as_ref())
            .find_map(|target| match target {
                Target::File(location) => Some(location),
                _ => None,
            })
            .unwrap();
        assert_eq!(file.path, root.join("sample.dart"));
        assert_eq!(parts.iter().filter(|p| p.target.is_some()).count(), 1);
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            file.position,
            Some(crate::editor::location::EditorPosition::Position(
                lsp_types::Position::new(2, 3)
            ))
        );
    }
    #[test]
    fn arbitrary_schemes_are_not_opened() {
        assert!(
            parts(
                "javascript:alert(1) command:rm file:///missing.dart:1",
                None
            )
            .iter()
            .all(|p| p.target.is_none())
        );
    }
}
