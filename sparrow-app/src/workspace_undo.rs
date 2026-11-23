//! Link native document undo groups for validated multi-file workspace edits.
use crate::{command::InternalCommand, doc::Doc, editor::EditorData};
use floem::reactive::{SignalGet, SignalUpdate, SignalWith, batch};
use lapce_xi_rope::Rope;
use lsp_types::TextEdit;
use sparrow_core::{
    buffer::rope_text::RopeText,
    command::EditCommand,
    cursor::{Cursor, CursorMode},
    editor::EditType,
    line_ending::LineEnding,
    rope_text_pos::RopeTextPosition,
    selection::Selection,
};
use sparrow_rpc::buffer::BufferId;
use std::{
    cell::RefCell,
    collections::HashMap,
    path::PathBuf,
    rc::{Rc, Weak},
};

/// Only insertions and character deletions coalesce in the native engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    Insert,
    Delete,
    Other,
    Undo,
    Redo,
}
impl ChangeKind {
    pub fn from_edit_type(kind: EditType) -> Self {
        match kind {
            EditType::InsertChars => Self::Insert,
            EditType::Delete => Self::Delete,
            EditType::Undo => Self::Undo,
            EditType::Redo => Self::Redo,
            _ => Self::Other,
        }
    }
    pub fn command(command: &EditCommand, cursor: &Cursor) -> Self {
        match command {
            EditCommand::Undo => Self::Undo,
            EditCommand::Redo => Self::Redo,
            EditCommand::InsertTab
            | EditCommand::DuplicateLineUp
            | EditCommand::DuplicateLineDown => Self::Insert,
            EditCommand::DeleteBackward | EditCommand::DeleteForward => {
                match &cursor.mode {
                    CursorMode::Normal(_) => Self::Delete,
                    CursorMode::Insert(selection) if selection.is_caret() => {
                        Self::Delete
                    }
                    _ => Self::Other,
                }
            }
            EditCommand::DeleteLine
            | EditCommand::DeleteForwardAndInsert
            | EditCommand::DeleteLineAndInsert
            | EditCommand::DeleteToEndOfLineAndInsert => Self::Delete,
            _ => Self::Other,
        }
    }
}
#[derive(Default)]
struct NativeHistory {
    undo: Vec<u64>,
    redo: Vec<u64>,
    last: Option<ChangeKind>,
    revision: u64,
}
impl NativeHistory {
    fn top(&self, redo: bool) -> Option<u64> {
        if redo {
            self.redo.last().copied()
        } else {
            self.undo.last().copied()
        }
    }
    fn observe(&mut self, kind: ChangeKind, count: usize, next: &mut u64) {
        for _ in 0..count {
            match kind {
                ChangeKind::Undo => {
                    if let Some(marker) = self.undo.pop() {
                        self.redo.push(marker);
                    }
                }
                ChangeKind::Redo => {
                    if let Some(marker) = self.redo.pop() {
                        self.undo.push(marker);
                    }
                }
                _ => {
                    let coalesces =
                        matches!(kind, ChangeKind::Insert | ChangeKind::Delete)
                            && self.last == Some(kind)
                            && !self.undo.is_empty();
                    if !coalesces {
                        *next += 1;
                        self.undo.push(*next);
                        self.redo.clear();
                    }
                }
            }
            self.last = Some(kind);
        }
    }
}
struct Entry {
    doc: Weak<Doc>,
    id: BufferId,
    path: PathBuf,
    marker: u64,
    previous: Option<u64>,
    detached_redo: bool,
    edits: Vec<TextEdit>,
    line_ending: LineEnding,
    before: Rope,
    after: Rope,
}
impl Entry {
    fn can_restore_redo(&self, native: &NativeHistory) -> bool {
        self.detached_redo && native.top(false) == self.previous
    }
}
struct Group {
    label: String,
    entries: Vec<Entry>,
}
#[derive(Default)]
pub struct History {
    native: HashMap<BufferId, NativeHistory>,
    groups: Vec<Group>,
    next: u64,
    blocked: std::collections::HashSet<BufferId>,
}
pub type SharedHistory = Rc<RefCell<History>>;
impl History {
    pub fn observe(
        &mut self,
        id: BufferId,
        kind: ChangeKind,
        count: usize,
        revision: u64,
    ) {
        if count == 0 {
            return;
        }
        let before = revision - count as u64;
        let linked = self
            .groups
            .iter()
            .any(|group| group.entries.iter().any(|e| e.id == id));
        let native = self.native.entry(id).or_insert_with(|| NativeHistory {
            revision: before,
            ..Default::default()
        });
        if native.revision != before {
            if linked {
                self.blocked.insert(id);
            }
            native.undo.clear();
            native.redo.clear();
            native.last = None;
        }
        let previous_redo = native.top(true);
        native.observe(kind, count, &mut self.next);
        native.revision = revision;
        if let Some(marker) = previous_redo
            && !matches!(kind, ChangeKind::Undo | ChangeKind::Redo)
            && native.redo.is_empty()
        {
            // A new native edit drops the redo stack. Retain the next linked
            // redo so it can be rebuilt after the new edits are undone.
            for entry in self.groups.iter_mut().flat_map(|group| &mut group.entries)
            {
                if entry.id == id && entry.marker == marker {
                    entry.detached_redo = true;
                }
            }
        }
    }
}

pub struct Snapshot {
    doc: Rc<Doc>,
    text: Rope,
    depth: usize,
    top: Option<u64>,
    edits: Vec<TextEdit>,
    line_ending: LineEnding,
}
pub fn snapshot(doc: Rc<Doc>, edits: &[TextEdit]) -> Snapshot {
    let history = doc.common.workspace_undo.borrow();
    let native = history.native.get(&doc.buffer_id);
    let depth = native.map_or(0, |n| n.undo.len());
    let top = native.and_then(|n| n.top(false));
    let (text, line_ending) = doc
        .buffer
        .with_untracked(|buffer| (buffer.text().clone(), buffer.line_ending()));
    drop(history);
    Snapshot {
        doc,
        text,
        depth,
        top,
        edits: edits.to_vec(),
        line_ending,
    }
}
pub fn record(history: &SharedHistory, label: String, snapshots: Vec<Snapshot>) {
    let mut history = history.borrow_mut();
    let mut entries = Vec::new();
    for snapshot in snapshots {
        let doc = snapshot.doc;
        let after = doc.buffer.with_untracked(|buffer| buffer.text().clone());
        if same_text(&after, &snapshot.text) {
            continue;
        }
        let Some(native) = history.native.get(&doc.buffer_id) else {
            return;
        };
        // Each prepared workspace edit must contribute exactly one native group.
        if native.undo.len() != snapshot.depth + 1
            || native.undo.get(snapshot.depth.wrapping_sub(1)).copied()
                != snapshot.top
            || native.revision != doc.rev()
        {
            return;
        }
        let Some(path) = doc.content.with_untracked(|c| c.path().cloned()) else {
            return;
        };
        entries.push(Entry {
            doc: Rc::downgrade(&doc),
            id: doc.buffer_id,
            path,
            marker: native.top(false).unwrap(),
            previous: snapshot.top,
            detached_redo: false,
            edits: snapshot.edits,
            line_ending: snapshot.line_ending,
            before: snapshot.text,
            after,
        });
    }
    if entries.len() > 1 {
        history.groups.push(Group { label, entries });
    }
}

/// Returns true if the command belongs to a linked refactoring, including a blocked one.
pub fn replay(editor: &EditorData, redo: bool) -> bool {
    let current = editor.doc();
    let common = editor.common.clone();
    let history = common.workspace_undo.borrow();
    if history.blocked.contains(&current.buffer_id) {
        drop(history);
        show_error(editor,"Document history changed outside the editor. Reopen the project before undoing this refactoring.".into());
        return true;
    }
    let Some(native) = history.native.get(&current.buffer_id) else {
        return false;
    };
    let group_for = |matches: &dyn Fn(&Entry) -> bool| {
        history.groups.iter().enumerate().rev().find(|(_, group)| {
            group
                .entries
                .iter()
                .any(|entry| entry.id == current.buffer_id && matches(entry))
        })
    };
    let Some((group_index, group)) =
        group_for(&|entry| native.top(redo) == Some(entry.marker)).or_else(|| {
            redo.then(|| group_for(&|entry| entry.can_restore_redo(native)))
                .flatten()
        })
    else {
        return false;
    };
    let result = (|| {
        let mut docs = Vec::new();
        for entry in &group.entries {
            let Some(doc) = entry.doc.upgrade() else {
                return Err(format!(
                    "{} is no longer open in the workspace.",
                    entry.path.display()
                ));
            };
            let expected = if redo { &entry.before } else { &entry.after };
            let native = history.native.get(&entry.id);
            let restore = redo && native.is_some_and(|n| entry.can_restore_redo(n));
            let valid = doc.loaded()
                && doc.content.with_untracked(|c| {
                    !c.read_only() && c.path() == Some(&entry.path)
                })
                && !history.blocked.contains(&entry.id)
                && native.is_some_and(|native| {
                    (native.top(redo) == Some(entry.marker) || restore)
                        && native.revision == doc.rev()
                })
                && doc.buffer.with_untracked(|buffer| {
                    same_text(buffer.text(), expected)
                        && (!restore || buffer.line_ending() == entry.line_ending)
                });
            if !valid {
                return Err(format!(
                    "Cannot {} '{}': {} has newer edits, a different history, or is read-only. No files were changed.",
                    if redo { "redo" } else { "undo" },
                    group.label,
                    entry.path.display()
                ));
            }
            let edits = restore.then(|| {
                doc.buffer.with_untracked(|buffer| {
                    entry
                        .edits
                        .iter()
                        .map(|edit| {
                            let selection = Selection::region(
                                buffer.offset_of_position(&edit.range.start),
                                buffer.offset_of_position(&edit.range.end),
                            );
                            (selection, edit.new_text.clone())
                        })
                        .collect::<Vec<_>>()
                })
            });
            docs.push((doc, edits));
        }
        Ok(docs)
    })();
    drop(history);
    let docs = match result {
        Ok(docs) => docs,
        Err(error) => {
            show_error(editor, error);
            return true;
        }
    };
    batch(|| {
        for (entry_index, (doc, edits)) in docs.into_iter().enumerate() {
            // UI-thread preflight above verifies the native group for every file.
            let (before, delta, inval, cursor_mode) = doc
                .buffer
                .try_update(|buffer| {
                    if let Some(edits) = &edits {
                        let (before, delta, inval) = buffer.edit(
                            edits
                                .iter()
                                .map(|(selection, text)| (selection, text.as_str())),
                            EditType::Other,
                        );
                        Some((before, delta, inval, None))
                    } else if redo {
                        buffer.do_redo()
                    } else {
                        buffer.do_undo()
                    }
                })
                .flatten()
                .expect("validated native refactoring history");
            let editors = doc.editors.with_editors_untracked(|editors| {
                editors
                    .values()
                    .filter(|editor| {
                        editor
                            .doc_signal()
                            .try_get_untracked()
                            .is_some_and(|current| Rc::ptr_eq(&current, &doc))
                    })
                    .cloned()
                    .collect::<Vec<_>>()
            });
            for target in editors {
                target.cursor().update(|cursor| {
                    if let Some(mode) = cursor_mode.clone() {
                        cursor.mode = if common.config.get_untracked().core.modal {
                            CursorMode::Normal(mode.offset())
                        } else {
                            mode
                        };
                    } else {
                        cursor.apply_delta(&delta);
                    }
                });
            }
            doc.apply_deltas_with_kind(
                &[(before, delta, inval)],
                if edits.is_some() {
                    ChangeKind::Other
                } else if redo {
                    ChangeKind::Redo
                } else {
                    ChangeKind::Undo
                },
            );
            if edits.is_some() {
                let mut history = common.workspace_undo.borrow_mut();
                let marker = history.native[&doc.buffer_id].top(false).unwrap();
                let entry = &mut history.groups[group_index].entries[entry_index];
                entry.marker = marker;
                entry.detached_redo = false;
            }
        }
    });
    true
}
fn show_error(editor: &EditorData, message: String) {
    editor
        .common
        .internal_command
        .send(InternalCommand::ShowAlert {
            title: "Refactoring history".into(),
            msg: message,
            buttons: Vec::new(),
        });
}

fn same_text(a: &Rope, b: &Rope) -> bool {
    a.len() == b.len()
        && a.iter_chunks(..)
            .flat_map(str::bytes)
            .eq(b.iter_chunks(..).flat_map(str::bytes))
}
#[cfg(test)]
mod tests {
    use super::*;
    use sparrow_core::buffer::Buffer;
    #[test]
    fn tracks_native_typing_cycles_undo_redo_and_branching() {
        let mut buffer = Buffer::new("old");
        let mut tracker = NativeHistory::default();
        let mut next = 0;
        for (text, kind) in [
            ("new", EditType::Completion),
            ("changed", EditType::Completion),
            ("new", EditType::Completion),
            ("new!", EditType::InsertChars),
            ("new!!", EditType::InsertChars),
        ] {
            buffer.edit([(&Selection::region(0, buffer.len()), text)], kind);
            tracker.observe(ChangeKind::from_edit_type(kind), 1, &mut next);
        }
        assert_eq!(tracker.undo.len(), 4);
        for expected in ["new", "changed", "new", "old"] {
            assert!(buffer.do_undo().is_some());
            tracker.observe(ChangeKind::Undo, 1, &mut next);
            assert_eq!(buffer.to_string(), expected);
        }
        assert!(buffer.do_undo().is_none());
        assert!(tracker.undo.is_empty());
        buffer.do_redo().unwrap();
        tracker.observe(ChangeKind::Redo, 1, &mut next);
        assert_eq!(buffer.to_string(), "new");
        buffer.edit([(&Selection::caret(3), " branch")], EditType::InsertChars);
        tracker.observe(ChangeKind::Insert, 1, &mut next);
        assert!(tracker.redo.is_empty());
        assert!(buffer.do_redo().is_none());
    }
    #[test]
    fn text_comparison_handles_different_rope_chunks() {
        assert!(same_text(&Rope::from("hello"), &Rope::from("hello")));
        assert!(!same_text(&Rope::from("hello"), &Rope::from("world")));
    }
    #[test]
    fn returning_to_identical_text_does_not_expose_the_refactoring_marker() {
        let mut native = NativeHistory::default();
        let mut next = 0;
        native.observe(ChangeKind::Other, 1, &mut next);
        let refactoring = native.top(false);
        // Replace x with y, replace y with x, type, then undo typing.
        native.observe(ChangeKind::Other, 2, &mut next);
        native.observe(ChangeKind::Insert, 3, &mut next);
        native.observe(ChangeKind::Undo, 1, &mut next);
        assert_ne!(native.top(false), refactoring);
        native.observe(ChangeKind::Undo, 2, &mut next);
        assert_eq!(native.top(false), refactoring);
    }
    #[test]
    fn missing_revision_notifications_block_linked_history() {
        let id = BufferId::next();
        let mut history = History::default();
        history.observe(id, ChangeKind::Other, 1, 1);
        history.groups.push(Group {
            label: "rename".into(),
            entries: vec![Entry {
                doc: Weak::new(),
                id,
                path: PathBuf::from("a.dart"),
                marker: 1,
                previous: None,
                detached_redo: false,
                edits: Vec::new(),
                line_ending: LineEnding::Lf,
                before: Rope::from("a"),
                after: Rope::from("b"),
            }],
        });
        history.observe(id, ChangeKind::Insert, 1, 3);
        assert!(history.blocked.contains(&id));
    }

    #[test]
    fn displaced_linked_redo_requires_undoing_the_new_branch() {
        let id = BufferId::next();
        let mut history = History::default();
        history.observe(id, ChangeKind::Other, 2, 2);
        history.groups.push(Group {
            label: "Quick Fix".into(),
            entries: vec![Entry {
                doc: Weak::new(),
                id,
                path: PathBuf::from("target.dart"),
                marker: 2,
                previous: Some(1),
                detached_redo: false,
                edits: Vec::new(),
                line_ending: LineEnding::Lf,
                before: Rope::from("before"),
                after: Rope::from("after"),
            }],
        });
        history.observe(id, ChangeKind::Undo, 1, 3);
        assert_eq!(history.native[&id].top(true), Some(2));

        history.observe(id, ChangeKind::Insert, 2, 5);
        assert!(history.groups[0].entries[0].detached_redo);
        assert!(
            !history.groups[0].entries[0].can_restore_redo(&history.native[&id])
        );

        history.observe(id, ChangeKind::Undo, 1, 6);
        assert!(history.groups[0].entries[0].can_restore_redo(&history.native[&id]));
        history.observe(id, ChangeKind::Undo, 1, 7);
        assert!(
            !history.groups[0].entries[0].can_restore_redo(&history.native[&id])
        );
    }
}
