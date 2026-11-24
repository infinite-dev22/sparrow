//! Working-copy changes against HEAD, shared by the editor gutter and overview.

use std::ops::Range;

use floem::{
    reactive::{SignalGet, SignalWith},
    views::editor::{core::register::Clipboard, text::SystemClipboard},
};
use sparrow_core::{
    buffer::{diff::DiffLines, rope_text::RopeText},
    editor::EditType,
    selection::Selection,
};

use crate::{
    config::color::SparrowColor,
    doc::Doc,
    editor::{EditorData, location::EditorPosition},
    widgets::popup_menu::{Menu, MenuItem, show_context_menu},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Hunk {
    pub lines: Range<usize>,
    original: Range<usize>,
    pub color: &'static str,
    pub description: &'static str,
}

pub(crate) fn hunks(doc: &Doc, changes: &im::Vector<DiffLines>) -> Vec<Hunk> {
    let head = doc.head_text();
    let buffer = doc.buffer.with_untracked(|buffer| buffer.text().clone());
    let mut old = 0;
    let mut new = 0;
    let mut result = Vec::new();
    let mut changes = changes.iter().peekable();
    while let Some(change) = changes.next() {
        let mut removed = 0;
        let mut added = 0;
        match change {
            DiffLines::Both(info) => {
                old = info.left.end;
                new = info.right.end;
                continue;
            }
            DiffLines::Left(range) => {
                removed = range.len();
                if let Some(DiffLines::Right(range)) = changes.peek() {
                    added = range.len();
                    changes.next();
                }
            }
            DiffLines::Right(range) => added = range.len(),
        }
        let original = old..old + removed;
        let lines = new..new + added;
        let (color, description) = if added == 0 {
            (
                SparrowColor::SOURCE_CONTROL_DELETION_MARKER,
                "Deleted lines",
            )
        } else if removed == 0 {
            let inserted = buffer.slice_to_cow(byte_range(&buffer, &lines));
            if inserted.contains([' ', '\t']) && inserted.trim().is_empty() {
                (SparrowColor::SOURCE_CONTROL_WHITESPACE, "Whitespace change")
            } else {
                (SparrowColor::SOURCE_CONTROL_ADDED, "Added lines")
            }
        } else if head.as_ref().is_some_and(|head| {
            let before = head.slice_to_cow(byte_range(head, &original));
            let after = buffer.slice_to_cow(byte_range(&buffer, &lines));
            before
                .chars()
                .filter(|c| !c.is_whitespace())
                .eq(after.chars().filter(|c| !c.is_whitespace()))
        }) {
            (SparrowColor::SOURCE_CONTROL_WHITESPACE, "Whitespace change")
        } else {
            (SparrowColor::SOURCE_CONTROL_MODIFIED, "Modified lines")
        };
        result.push(Hunk {
            lines,
            original,
            color,
            description,
        });
        old += removed;
        new += added;
    }
    result
}

fn byte_range(buffer: &impl RopeText, lines: &Range<usize>) -> Range<usize> {
    let offset = |line| {
        if line > buffer.last_line() {
            buffer.len()
        } else {
            buffer.offset_of_line(line)
        }
    };
    offset(lines.start)..offset(lines.end)
}

pub(crate) fn gutter_span(editor: &EditorData, hunk: &Hunk) -> (f64, f64) {
    let line_height =
        editor.common.config.get_untracked().editor.line_height() as f64;
    let last_line = editor
        .doc()
        .buffer
        .with_untracked(|buffer| buffer.last_line());
    let start = editor
        .editor
        .vline_of_line(hunk.lines.start.min(last_line))
        .0 as f64
        * line_height;
    if hunk.lines.is_empty() {
        let boundary = start
            + if hunk.lines.start > last_line {
                line_height
            } else {
                0.0
            };
        (boundary - 3.0, boundary + 3.0)
    } else {
        let end = if hunk.lines.end <= last_line {
            editor.editor.vline_of_line(hunk.lines.end).0 as f64 * line_height
        } else {
            editor.editor.vline_of_line(last_line).0 as f64 * line_height
                + line_height
        };
        (start, end.max(start + line_height))
    }
}

pub(crate) fn go_to_line(editor: &EditorData, line: usize) {
    let line = editor
        .doc()
        .buffer
        .with_untracked(|buffer| line.min(buffer.last_line()));
    editor.go_to_position(EditorPosition::Line(line), None, None);
    if let Some(view) = editor.editor.editor_view_id.get_untracked() {
        view.request_focus();
    }
}

pub(crate) fn show_hunk_menu(editor: &EditorData, items: &[Hunk], index: usize) {
    let hunk = &items[index];
    let doc = editor.doc();
    let revision = doc.rev();
    let head = doc.head_text();
    let original = head.as_ref().map(|text| {
        text.slice_to_cow(byte_range(text, &hunk.original))
            .into_owned()
    });
    let menu = Menu::new(hunk.description);
    let navigation = |name, line: Option<usize>| {
        let editor = editor.clone();
        MenuItem::new(name).enabled(line.is_some()).action(move || {
            if let Some(line) = line {
                go_to_line(&editor, line);
            }
        })
    };
    let mut menu = menu
        .entry(navigation(
            "Previous Change",
            index.checked_sub(1).map(|i| items[i].lines.start),
        ))
        .entry(navigation(
            "Next Change",
            items.get(index + 1).map(|h| h.lines.start),
        ));
    if let Some(path) = doc
        .content
        .with_untracked(|content| content.path().cloned())
    {
        let command = editor.common.internal_command;
        menu =
            menu.entry(MenuItem::new("Compare File with HEAD").action(move || {
                command.send(crate::command::InternalCommand::OpenFileChanges {
                    path: path.clone(),
                });
            }));
    }
    if let Some(original) = original {
        let copy = original.clone();
        let range = doc
            .buffer
            .with_untracked(|buffer| byte_range(buffer, &hunk.lines));
        menu =
            menu.separator()
                .entry(
                    MenuItem::new("Copy Original Lines")
                        .enabled(!copy.is_empty())
                        .action(move || {
                            SystemClipboard::new().put_string(&copy);
                        }),
                )
                .entry(MenuItem::new("Revert This Change").action(move || {
                    if doc.rev() == revision
                        && doc.head_text().zip(head.clone()).is_some_and(
                            |(current, original)| current.ptr_eq(&original),
                        )
                    {
                        doc.do_raw_edit(
                            &[(
                                Selection::region(range.start, range.end),
                                original.as_str(),
                            )],
                            EditType::Other,
                        );
                    }
                }));
    }
    show_context_menu(menu, None);
}
