//! Scoped project search with reviewable, stale-checked replacement.
pub(crate) mod preview;
pub(crate) mod view;

pub use view::view;

use crate::window_tab::WindowTabData;
use floem::reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith};
use std::{
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Hit {
    pub(crate) path: PathBuf,
    pub(crate) offset: usize,
    pub(crate) position: Option<lsp_types::Position>,
    pub(crate) line: usize,
    pub(crate) span: std::ops::Range<usize>,
    pub(crate) text: Arc<str>,
}
#[derive(Clone, Default)]
pub(crate) struct Results {
    pub(crate) hits: Vec<Hit>,
    files: Vec<(PathBuf, String)>,
    pub(crate) error: String,
    truncated: bool,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum FileScope {
    Project,
    Module,
    Directory,
    Scope,
}

impl FileScope {
    const TABS: [(Self, &'static str); 4] = [
        (Self::Project, "In Project"),
        (Self::Module, "Module"),
        (Self::Directory, "Directory"),
        (Self::Scope, "Scope"),
    ];
}

impl Hit {
    pub(crate) fn snippet(&self) -> (String, Vec<usize>) {
        let start = self
            .text
            .floor_char_boundary(self.span.start.saturating_sub(45));
        let end = self
            .text
            .floor_char_boundary((start + 360).min(self.text.len()));
        let context = &self.text[start..end];
        let trimmed = context.trim();
        let offset = start + context.len() - context.trim_start().len();
        let indices = trimmed
            .char_indices()
            .filter_map(|(index, _)| {
                self.span.contains(&(offset + index)).then_some(index)
            })
            .collect();
        (trimmed.to_owned(), indices)
    }

    pub(crate) fn location(&self) -> crate::editor::location::EditorLocation {
        use crate::editor::location::{EditorLocation, EditorPosition};
        EditorLocation {
            path: self.path.clone(),
            position: Some(self.position.map_or(
                EditorPosition::Offset(self.offset),
                EditorPosition::Position,
            )),
            scroll_offset: None,
            ignore_unconfirmed: false,
            same_editor_tab: false,
        }
    }
}

#[derive(Clone)]
pub struct State {
    pub active: RwSignal<bool>,
    query: RwSignal<String>,
    replace: RwSignal<String>,
    replacement: RwSignal<bool>,
    scope: RwSignal<FileScope>,
    module: RwSignal<PathBuf>,
    open_only: RwSignal<bool>,
    mask_enabled: RwSignal<bool>,
    directory: RwSignal<String>,
    mask: RwSignal<String>,
    case: RwSignal<bool>,
    word: RwSignal<bool>,
    regex: RwSignal<bool>,
    results: RwSignal<Results>,
    epoch: RwSignal<u64>,
    busy: RwSignal<bool>,
    message: RwSignal<String>,
}
impl State {
    pub fn is_busy(&self) -> bool {
        self.busy.get_untracked()
    }

    pub fn new(cx: Scope) -> Self {
        Self {
            active: cx.create_rw_signal(false),
            query: cx.create_rw_signal(String::new()),
            replace: cx.create_rw_signal(String::new()),
            replacement: cx.create_rw_signal(false),
            scope: cx.create_rw_signal(FileScope::Project),
            module: cx.create_rw_signal(PathBuf::new()),
            open_only: cx.create_rw_signal(false),
            mask_enabled: cx.create_rw_signal(false),
            directory: cx.create_rw_signal(String::new()),
            mask: cx.create_rw_signal(String::new()),
            case: cx.create_rw_signal(false),
            word: cx.create_rw_signal(false),
            regex: cx.create_rw_signal(false),
            results: cx.create_rw_signal(Results::default()),
            epoch: cx.create_rw_signal(0),
            busy: cx.create_rw_signal(false),
            message: cx.create_rw_signal(String::new()),
        }
    }
    pub fn open(&self, replace: bool, dir: Option<PathBuf>) {
        floem::reactive::batch(|| {
            self.replacement.set(replace);
            self.scope.set(if dir.is_some() {
                FileScope::Directory
            } else {
                FileScope::Project
            });
            if let Some(dir) = dir {
                self.directory.set(dir.display().to_string());
            }
            self.message.set(String::new());
            self.active.set(true);
        });
    }
    fn apply(&self, window: Rc<WindowTabData>, selected: Option<usize>) {
        if self.busy.get_untracked() {
            return;
        }
        let results = self.results.get_untracked();
        if !results.error.is_empty() || (selected.is_none() && results.truncated) {
            self.message.set(
                "Narrow the search until all results fit before replacing.".into(),
            );
            return;
        }
        if results.files.is_empty() {
            return;
        }
        let pattern = match pattern(
            &self.query.get_untracked(),
            self.case.get_untracked(),
            self.word.get_untracked(),
            self.regex.get_untracked(),
        ) {
            Ok(p) => p,
            Err(e) => {
                self.message.set(e);
                return;
            }
        };
        let replacement = self.replace.get_untracked();
        let expand = self.regex.get_untracked();
        let hit = selected.and_then(|index| results.hits.get(index)).cloned();
        if selected.is_some() && hit.is_none() {
            return;
        }
        let files = results.files;
        // Replace Current must touch exactly one match in its original file.
        let files = files
            .into_iter()
            .filter(|(path, _)| hit.as_ref().is_none_or(|hit| hit.path == *path))
            .collect::<Vec<_>>();
        if window.main_split.docs.with_untracked(|docs| {
            files
                .iter()
                .any(|(p, _)| docs.get(p).is_some_and(|d| !d.is_pristine()))
        }) {
            self.message
                .set("Save modified matching files before replacing.".into());
            return;
        }
        self.busy.set(true);
        let state = self.clone();
        let w = window.clone();
        let send = floem::ext_event::create_ext_action(
            window.scope,
            move |result: Result<Vec<(PathBuf, String)>, String>| {
                state.busy.set(false);
                match result {
                    Err(e) => state.message.set(e),
                    Ok(files) => {
                        for (path, text) in &files {
                            w.main_split.docs.with_untracked(|docs| {
                                if let Some(doc) = docs.get(path) {
                                    if doc.is_pristine() {
                                        doc.reload(
                                            lapce_xi_rope::Rope::from(text.clone()),
                                            true,
                                        );
                                    }
                                }
                            });
                        }
                        let count = files.len();
                        let noun = if count == 1 { "file" } else { "files" };
                        state.message.set(format!("Updated {count} {noun}."));
                        state.epoch.update(|e| *e += 1);
                    }
                }
            },
        );
        let task = window
            .common
            .background_tasks
            .begin("Replacing project matches", None);
        std::thread::spawn(move || {
            let _task = task;
            send(replace(
                files,
                &pattern,
                &replacement,
                expand,
                hit.as_ref().map(|hit| hit.offset),
            ))
        });
    }
}
fn pattern(
    query: &str,
    case: bool,
    word: bool,
    is_regex: bool,
) -> Result<regex::Regex, String> {
    let query = if is_regex {
        query.to_string()
    } else {
        regex::escape(query)
    };
    let query = if word {
        format!(r"\b(?:{query})\b")
    } else {
        query
    };
    regex::RegexBuilder::new(&query)
        .case_insensitive(!case)
        .multi_line(true)
        .build()
        .map_err(|e| e.to_string())
}
fn replace(
    files: Vec<(PathBuf, String)>,
    pattern: &regex::Regex,
    replacement: &str,
    expand: bool,
    offset: Option<usize>,
) -> Result<Vec<(PathBuf, String)>, String> {
    for (path, before) in &files {
        if std::fs::read_to_string(path).ok().as_ref() != Some(before) {
            return Err("A matching file changed since the search. Search again before replacing.".into());
        }
    }
    let mut edited = Vec::with_capacity(files.len());
    for (path, before) in files {
        let text = if let Some(offset) = offset {
            let captures = pattern
                .captures_at(&before, offset)
                .filter(|capture| {
                    capture
                        .get(0)
                        .is_some_and(|matched| matched.start() == offset)
                })
                .ok_or_else(|| {
                    "The selected match is no longer available.".to_owned()
                })?;
            let matched = captures.get(0).unwrap();
            let mut text = before[..matched.start()].to_owned();
            if expand {
                captures.expand(replacement, &mut text);
            } else {
                text.push_str(replacement);
            }
            text.push_str(&before[matched.end()..]);
            text
        } else if expand {
            pattern.replace_all(&before, replacement).into_owned()
        } else {
            pattern
                .replace_all(&before, regex::NoExpand(replacement))
                .into_owned()
        };
        edited.push((path, text));
    }
    crate::persistence::write_files(edited.clone())?;
    Ok(edited)
}
pub(crate) fn walk(path: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    if path.is_symlink() {
        return Ok(());
    }
    if path.is_dir() {
        for entry in std::fs::read_dir(path).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            if matches!(
                entry.file_name().to_str(),
                Some(
                    ".git"
                        | ".dart_tool"
                        | ".gradle"
                        | "build"
                        | "Pods"
                        | "node_modules"
                        | ".sparrow"
                )
            ) {
                continue;
            }
            walk(&entry.path(), files)?;
        }
    } else if path.is_file() {
        files.push(path.into());
    }
    if files.len() > 50000 {
        return Err("Too many project files. Choose a smaller directory.".into());
    }
    Ok(())
}
pub(crate) fn scan(
    root: &Path,
    dir: &str,
    mask: &str,
    query: &str,
    open_paths: Option<&std::collections::HashSet<PathBuf>>,
    case: bool,
    word: bool,
    is_regex: bool,
) -> Results {
    let mut result = Results::default();
    if query.is_empty() {
        return result;
    }
    let run = || -> Result<Results, String> {
        let root = root.canonicalize().map_err(|e| e.to_string())?;
        let dir = if dir.is_empty() {
            root.clone()
        } else {
            let path = PathBuf::from(dir);
            if path.is_absolute() {
                path
            } else {
                root.join(path)
            }
        }
        .canonicalize()
        .map_err(|e| e.to_string())?;
        if !dir.starts_with(&root) {
            return Err("Choose a directory inside this project.".into());
        }
        let matcher = if mask.is_empty() {
            None
        } else {
            Some(
                globset::Glob::new(mask)
                    .map_err(|e| e.to_string())?
                    .compile_matcher(),
            )
        };
        let pattern = pattern(query, case, word, is_regex)?;
        let mut files = vec![];
        if let Some(paths) = open_paths {
            files.extend(
                paths
                    .iter()
                    .filter_map(|path| path.canonicalize().ok())
                    .filter(|path| path.starts_with(&dir) && path.is_file()),
            );
        } else {
            walk(&dir, &mut files)?;
        }
        files.sort();
        let mut result = Results::default();
        for path in files {
            if matcher.as_ref().is_some_and(|g| {
                !g.is_match(path.file_name().unwrap_or_default())
                    && !g.is_match(path.strip_prefix(&root).unwrap_or(&path))
            }) {
                continue;
            }
            if std::fs::metadata(&path)
                .map(|m| m.len() > 2_000_000)
                .unwrap_or(true)
            {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            if text.contains('\0') {
                continue;
            }
            let mut matched = false;
            let mut line = 1;
            let mut start = 0;
            let mut end = text.find('\n').unwrap_or(text.len());
            let mut line_text: Arc<str> = text[..end].into();
            for m in pattern.find_iter(&text) {
                matched = true;
                let offset = m.start();
                if offset > end {
                    let prefix = &text[end..offset];
                    line += prefix.bytes().filter(|&byte| byte == b'\n').count();
                    start = end + prefix.rfind('\n').map_or(0, |pos| pos + 1);
                    end = text[offset..]
                        .find('\n')
                        .map_or(text.len(), |pos| offset + pos);
                    line_text = text[start..end].into();
                }
                result.hits.push(Hit {
                    path: path.clone(),
                    offset,
                    position: None,
                    line,
                    span: offset - start..m.end().min(end) - start,
                    text: line_text.clone(),
                });
                if result.hits.len() >= 2000 {
                    result.truncated = true;
                    break;
                }
            }
            if matched {
                result.files.push((path, text));
            }
            if result.hits.len() >= 2000 {
                result.truncated = true;
                break;
            }
        }
        Ok(result)
    };
    match run() {
        Ok(r) => r,
        Err(e) => {
            result.error = e;
            result
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scoped_regex_search_and_stale_replace() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.dart");
        std::fs::write(&p, "Counter counter\n").unwrap();
        let r = scan(d.path(), "", "*.dart", "counter", None, false, true, false);
        assert_eq!(r.hits.len(), 2);
        std::fs::write(&p, "changed").unwrap();
        assert!(
            replace(
                r.files,
                &pattern("counter", false, true, false).unwrap(),
                "value",
                false,
                None,
            )
            .is_err()
        );
        assert_eq!(std::fs::read_to_string(p).unwrap(), "changed");
    }
    #[test]
    fn regex_captures_and_masks_are_applied() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.dart");
        std::fs::write(&p, "item12 item34").unwrap();
        let r = scan(
            d.path(),
            "",
            "*.dart",
            r"item(\d+)",
            None,
            true,
            false,
            true,
        );
        assert_eq!(r.hits.len(), 2);
        replace(
            r.files,
            &pattern(r"item(\d+)", true, false, true).unwrap(),
            "value$1",
            true,
            None,
        )
        .unwrap();
        assert_eq!(std::fs::read_to_string(p).unwrap(), "value12 value34");
        assert!(
            scan(d.path(), "", "*.yaml", "value", None, true, false, false)
                .hits
                .is_empty()
        );
    }

    #[test]
    fn replaces_only_the_selected_match_and_keeps_other_files_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first.dart");
        let second = dir.path().join("second.dart");
        std::fs::write(&first, "item12\n  item34\n").unwrap();
        std::fs::write(&second, "item56\n").unwrap();
        let open = [first.clone()].into_iter().collect();
        let result = scan(
            dir.path(),
            "",
            "*.dart",
            r"item(\d+)",
            Some(&open),
            true,
            false,
            true,
        );
        assert_eq!(result.hits.len(), 2);
        let hit = &result.hits[1];
        assert_eq!((hit.line, hit.span.clone()), (2, 2..8));
        assert_eq!(hit.snippet().0, "item34");
        replace(
            result.files,
            &pattern(r"item(\d+)", true, false, true).unwrap(),
            "new$1",
            true,
            Some(hit.offset),
        )
        .unwrap();
        assert_eq!(std::fs::read_to_string(first).unwrap(), "item12\n  new34\n");
        assert_eq!(std::fs::read_to_string(second).unwrap(), "item56\n");
    }
}
