use std::{
    borrow::Cow,
    ffi::OsString,
    fs,
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::SystemTime,
};

use anyhow::{Result, anyhow};
use floem_editor_core::buffer::rope_text::CharIndicesJoin;
use lapce_xi_rope::{RopeDelta, interval::IntervalBounds, rope::Rope};
use lsp_types::*;
use sparrow_core::encoding::offset_utf8_to_utf16;
use sparrow_rpc::buffer::BufferId;

#[derive(Clone)]
pub struct Buffer {
    pub language_id: &'static str,
    pub read_only: bool,
    pub id: BufferId,
    pub rope: Rope,
    pub path: PathBuf,
    pub rev: u64,
    pub mod_time: Option<SystemTime>,
}

impl Buffer {
    pub fn new(id: BufferId, path: PathBuf) -> Buffer {
        let (s, read_only) = match load_file(&path) {
            Ok(s) => (s, false),
            Err(err) => {
                use std::io::ErrorKind;
                match err.downcast_ref::<std::io::Error>() {
                    Some(err) => match err.kind() {
                        ErrorKind::PermissionDenied => {
                            ("Permission Denied".to_string(), true)
                        }
                        ErrorKind::NotFound => ("".to_string(), false),
                        ErrorKind::OutOfMemory => {
                            ("File too big (out of memory)".to_string(), false)
                        }
                        _ => (format!("Not supported: {err}"), true),
                    },
                    None => (format!("Not supported: {err}"), true),
                }
            }
        };
        let rope = Rope::from(s);
        let rev = u64::from(!rope.is_empty());
        let language_id = language_id_from_path(&path).unwrap_or("");
        let mod_time = get_mod_time(&path);
        Buffer {
            id,
            rope,
            read_only,
            path,
            language_id,
            rev,
            mod_time,
        }
    }

    pub fn save(&mut self, rev: u64, create_parents: bool) -> Result<()> {
        if self.read_only {
            return Err(anyhow!("can't save to read only file"));
        }

        if self.rev != rev {
            return Err(anyhow!("not the right rev"));
        }
        let bak_extension = self.path.extension().map_or_else(
            || OsString::from("bak"),
            |ext| {
                let mut ext = ext.to_os_string();
                ext.push(".bak");
                ext
            },
        );
        let path = if self.path.is_symlink() {
            self.path.canonicalize()?
        } else {
            self.path.clone()
        };
        let new_file = !path.exists();

        let bak_file_path = &path.with_extension(bak_extension);
        if !new_file {
            fs::copy(&path, bak_file_path)?;
        }

        if create_parents {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
        }

        let mut f = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)?;
        for chunk in self.rope.iter_chunks(..self.rope.len()) {
            f.write_all(chunk.as_bytes())?;
        }

        self.mod_time = get_mod_time(&path);
        if !new_file {
            fs::remove_file(bak_file_path)?;
        }

        Ok(())
    }

    pub fn update(
        &mut self,
        delta: &RopeDelta,
        rev: u64,
    ) -> Option<TextDocumentContentChangeEvent> {
        if self.rev + 1 != rev || delta.base_len != self.rope.len() {
            tracing::warn!(path = %self.path.display(), current = self.rev,
                incoming = rev, expected_len = self.rope.len(), delta_len = delta.base_len,
                "Buffer update requires synchronization");
            return None;
        }
        self.rev = rev;
        let content_change = get_document_content_changes(delta, self);
        self.rope = delta.apply(&self.rope);
        Some(
            content_change.unwrap_or_else(|| TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: self.get_document(),
            }),
        )
    }

    /// Replace an out-of-sync mirror with the editor's current snapshot.
    /// Older snapshots must never overwrite newer edits already received.
    pub fn synchronize(&mut self, rev: u64, content: String) -> Option<RopeDelta> {
        if rev < self.rev {
            return None;
        }
        let delta = lapce_xi_rope::Delta::simple_edit(
            0..self.rope.len(),
            Rope::from(content),
            self.rope.len(),
        );
        self.rope = delta.apply(&self.rope);
        self.rev = rev;
        Some(delta)
    }

    pub fn get_document(&self) -> String {
        self.rope.to_string()
    }

    pub fn offset_of_line(&self, line: usize) -> usize {
        self.rope.offset_of_line(line)
    }

    pub fn line_of_offset(&self, offset: usize) -> usize {
        self.rope.line_of_offset(offset)
    }

    pub fn offset_to_line_col(&self, offset: usize) -> (usize, usize) {
        let line = self.line_of_offset(offset);
        (line, offset - self.offset_of_line(line))
    }

    /// Converts a UTF8 offset to a UTF16 LSP position  
    pub fn offset_to_position(&self, offset: usize) -> Position {
        let (line, col) = self.offset_to_line_col(offset);
        // Get the offset of line to make the conversion cheaper, rather than working
        // from the very start of the document to `offset`
        let line_offset = self.offset_of_line(line);
        let utf16_col =
            offset_utf8_to_utf16(self.char_indices_iter(line_offset..), col);

        Position {
            line: line as u32,
            character: utf16_col as u32,
        }
    }

    pub fn slice_to_cow<T: IntervalBounds>(&self, range: T) -> Cow<'_, str> {
        self.rope.slice_to_cow(range)
    }

    pub fn line_to_cow(&self, line: usize) -> Cow<'_, str> {
        self.rope
            .slice_to_cow(self.offset_of_line(line)..self.offset_of_line(line + 1))
    }

    /// Iterate over (utf8_offset, char) values in the given range  
    /// This uses `iter_chunks` and so does not allocate, compared to `slice_to_cow` which can
    pub fn char_indices_iter<T: IntervalBounds>(
        &self,
        range: T,
    ) -> impl Iterator<Item = (usize, char)> + '_ {
        CharIndicesJoin::new(self.rope.iter_chunks(range).map(str::char_indices))
    }

    pub fn len(&self) -> usize {
        self.rope.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

pub fn load_file(path: &Path) -> Result<String> {
    read_path_to_string(path)
}

pub fn read_path_to_string<P: AsRef<Path>>(path: P) -> Result<String> {
    let path = path.as_ref();

    let mut file = File::open(path)?;
    // Read the file in as bytes
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;

    // Parse the file contents as utf8
    let contents = String::from_utf8(buffer)?;

    Ok(contents.to_string())
}

/// Semantic language services are supplied only by the Dart SDK.
/// Flutter's assets, platform scaffolding and config files stay editable,
/// but never activate a second programming-language server.
pub fn language_id_from_path(path: &Path) -> Option<&'static str> {
    path.extension()?
        .to_str()?
        .eq_ignore_ascii_case("dart")
        .then_some("dart")
}

fn get_document_content_changes(
    delta: &RopeDelta,
    buffer: &Buffer,
) -> Option<TextDocumentContentChangeEvent> {
    let (interval, _) = delta.summary();
    let (start, end) = interval.start_end();

    // TODO: Handle more trivial cases like typing when there's a selection or transpose
    if let Some(node) = delta.as_simple_insert() {
        let (start, end) = interval.start_end();
        let start = buffer.offset_to_position(start);

        let end = buffer.offset_to_position(end);

        Some(TextDocumentContentChangeEvent {
            range: Some(Range { start, end }),
            range_length: None,
            text: String::from(node),
        })
    }
    // Or a simple delete
    else if delta.is_simple_delete() {
        let end_position = buffer.offset_to_position(end);

        let start = buffer.offset_to_position(start);

        Some(TextDocumentContentChangeEvent {
            range: Some(Range {
                start,
                end: end_position,
            }),
            range_length: None,
            text: String::new(),
        })
    } else {
        None
    }
}

/// Returns the modification timestamp for the file at a given path,
/// if present.
pub fn get_mod_time<P: AsRef<Path>>(path: P) -> Option<SystemTime> {
    File::open(path)
        .and_then(|f| f.metadata())
        .and_then(|meta| meta.modified())
        .ok()
}

#[cfg(test)]
mod dart_scope_tests {
    use super::*;
    #[test]
    fn only_dart_files_have_a_semantic_language_id() {
        assert_eq!(
            language_id_from_path(Path::new("lib/main.dart")),
            Some("dart")
        );
        assert_eq!(
            language_id_from_path(Path::new("tool/TEST.DART")),
            Some("dart")
        );
        for p in [
            "main.rs",
            "app.py",
            "index.ts",
            "pubspec.yaml",
            "android/MainActivity.kt",
        ] {
            assert_eq!(language_id_from_path(Path::new(p)), None);
        }
    }
}

#[cfg(test)]
mod synchronization_tests {
    use super::*;
    use lapce_xi_rope::Delta;

    #[test]
    fn incompatible_delta_preserves_mirror_and_snapshot_restores_editing_and_saving()
    {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pubspec_overrides.yaml");
        fs::write(&path, "old\n").unwrap();
        let mut buffer = Buffer::new(BufferId::next(), path.clone());
        let revision = buffer.rev;
        let invalid = Delta::simple_edit(0..0, Rope::from("x"), 99);
        assert!(buffer.update(&invalid, revision + 1).is_none());
        assert_eq!(buffer.rev, revision);
        assert_eq!(buffer.get_document(), "old\n");
        // A skipped revision must also request a snapshot, even for a valid length.
        let skipped = Delta::simple_edit(0..0, Rope::from("x"), 4);
        assert!(buffer.update(&skipped, revision + 2).is_none());
        let snapshot =
            "dependency_overrides:\n  local_probe:\n    path: local_probe\n";
        assert!(buffer.synchronize(revision + 3, snapshot.into()).is_some());
        let valid = Delta::simple_edit(
            snapshot.len()..snapshot.len(),
            Rope::from("\n"),
            snapshot.len(),
        );
        assert!(buffer.update(&valid, revision + 4).is_some());
        buffer.save(revision + 4, false).unwrap();
        assert_eq!(fs::read_to_string(path).unwrap(), format!("{snapshot}\n"));
    }

    #[test]
    fn delayed_snapshot_cannot_replace_newer_edits() {
        let dir = tempfile::tempdir().unwrap();
        let mut buffer = Buffer::new(BufferId::next(), dir.path().join("main.dart"));
        buffer.synchronize(8, "newer".into()).unwrap();
        assert!(buffer.synchronize(7, "older".into()).is_none());
        assert_eq!(buffer.rev, 8);
        assert_eq!(buffer.get_document(), "newer");
    }
}
