//! Scoped project search with reviewable, stale-checked replacement.
mod view;

pub use view::view;

use crate::window_tab::WindowTabData;
use floem::{
    reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith},
};
use std::{
    path::{Path, PathBuf},
    rc::Rc,
};
#[derive(Clone, Debug, PartialEq)]
struct Hit {
    path: PathBuf,
    offset: usize,
    position: Option<lsp_types::Position>,
    line: usize,
    text: String,
    action: Option<crate::command::SparrowCommand>,
}
#[derive(Clone, Default)]
struct Results {
    hits: Vec<Hit>,
    files: Vec<(PathBuf, String)>,
    error: String,
    truncated: bool,
}
#[derive(Clone)]
pub struct State {
    pub active: RwSignal<bool>,
    query: RwSignal<String>,
    replace: RwSignal<String>,
    replacement: RwSignal<bool>,
    mode: RwSignal<u8>,
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
    pub fn is_busy(&self)->bool{self.busy.get_untracked()}

    pub fn new(cx: Scope) -> Self {
        Self {
            active: cx.create_rw_signal(false),
            query: cx.create_rw_signal(String::new()),
            replace: cx.create_rw_signal(String::new()),
            replacement: cx.create_rw_signal(false),
            mode: cx.create_rw_signal(0),
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
    pub fn open(&self, replace: bool, everywhere: bool, dir: Option<PathBuf>) {
        self.replacement.set(replace);
        self.mode.set(if everywhere { 3 } else { 0 });
        self.directory
            .set(dir.map(|p| p.display().to_string()).unwrap_or_default());
        self.message.set(String::new());
        self.active.set(true);
    }
    fn apply(&self, window: Rc<WindowTabData>) {
        if self.busy.get_untracked() {
            return;
        }
        let results = self.results.get_untracked();
        if !results.error.is_empty() || results.truncated {
            self.message.set(
                "Narrow the search until all results fit before replacing.".into(),
            );
            return;
        }
        if results.files.is_empty() {
            return;
        }
        if window.main_split.docs.with_untracked(|docs| {
            results
                .files
                .iter()
                .any(|(p, _)| docs.get(p).is_some_and(|d| !d.is_pristine()))
        }) {
            self.message
                .set("Save modified matching files before replacing.".into());
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
        let files = results.files.clone();
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
                        state.message.set(format!(
                            "Replaced matches in {} files.",
                            files.len()
                        ));
                        state.epoch.update(|e| *e += 1);
                    }
                }
            },
        );
        let task = window.common.background_tasks.begin("Replacing project matches", None);
        std::thread::spawn(move || {
            let _task = task;
            send(replace_files(files, &pattern, &replacement, expand))
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
fn replace_files(
    files: Vec<(PathBuf, String)>,
    pattern: &regex::Regex,
    replacement: &str,
    expand: bool,
) -> Result<Vec<(PathBuf, String)>, String> {
    for (path, before) in &files {
        if std::fs::read_to_string(path).ok().as_ref() != Some(before) {
            return Err("A matching file changed since the search. Search again before replacing.".into());
        }
    }
    let files = files
        .into_iter()
        .map(|(path, before)| {
            (
                path,
                if expand {
                    pattern.replace_all(&before, replacement).into_owned()
                } else {
                    pattern
                        .replace_all(&before, regex::NoExpand(replacement))
                        .into_owned()
                },
            )
        })
        .collect::<Vec<_>>();
    crate::persistence::write_files(files.clone())?;
    Ok(files)
}
fn walk(path: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
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
    } else {
        files.push(path.into());
    }
    if files.len() > 50000 {
        return Err("Too many project files. Choose a smaller directory.".into());
    }
    Ok(())
}
fn scan(
    root: &Path,
    dir: &str,
    mask: &str,
    query: &str,
    mode: u8,
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
        walk(&dir, &mut files)?;
        files.sort();
        let mut result = Results::default();
        for path in files {
            if matcher.as_ref().is_some_and(|g| {
                !g.is_match(path.file_name().unwrap_or_default())
                    && !g.is_match(path.strip_prefix(&root).unwrap_or(&path))
            }) {
                continue;
            }
            if mode == 1 || mode == 3 {
                let relative = path
                    .strip_prefix(&root)
                    .unwrap_or(&path)
                    .display()
                    .to_string();
                if pattern.is_match(&relative) {
                    result.hits.push(Hit {
                        path: path.clone(),
                        offset: 0,
                        position: None,
                        line: 0,
                        text: relative,
                        action: None,
                    });
                }
            }
            if mode != 1 {
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
                for m in pattern.find_iter(&text) {
                    matched = true;
                    let offset = m.start();
                    let start = text[..offset].rfind('\n').map_or(0, |i| i + 1);
                    let end =
                        text[offset..].find('\n').map_or(text.len(), |i| offset + i);
                    result.hits.push(Hit {
                        path: path.clone(),
                        offset,
                        position: None,
                        line: text[..offset].bytes().filter(|b| *b == b'\n').count()
                            + 1,
                        text: text[start..end].chars().take(220).collect(),
                        action: None,
                    });
                    if result.hits.len() >= 2000 {
                        result.truncated = true;
                        break;
                    }
                }
                if matched {
                    result.files.push((path, text));
                }
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
        let r = scan(d.path(), "", "*.dart", "counter", 0, false, true, false);
        assert_eq!(r.hits.len(), 2);
        std::fs::write(&p, "changed").unwrap();
        assert!(
            replace_files(
                r.files,
                &pattern("counter", false, true, false).unwrap(),
                "value",
                false
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
        let r = scan(d.path(), "", "*.dart", r"item(\d+)", 0, true, false, true);
        assert_eq!(r.hits.len(), 2);
        replace_files(
            r.files,
            &pattern(r"item(\d+)", true, false, true).unwrap(),
            "value$1",
            true,
        )
        .unwrap();
        assert_eq!(std::fs::read_to_string(p).unwrap(), "value12 value34");
        assert!(
            scan(d.path(), "", "*.yaml", "value", 0, true, false, false)
                .hits
                .is_empty()
        );
    }
}
