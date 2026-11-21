//! Scoped Dart formatting with a prepared, stale-checked transaction.
mod view;

pub use view::view;

use crate::window_tab::WindowTabData;
use floem::reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith};
use std::{
    path::{Path, PathBuf},
    rc::Rc,
};
#[derive(Clone)]
pub struct State {
    pub active: RwSignal<bool>,
    target: RwSignal<PathBuf>,
    busy: RwSignal<bool>,
    message: RwSignal<String>,
}
impl State {
    pub fn is_busy(&self)->bool{self.busy.get_untracked()}

    pub fn new(cx: Scope) -> Self {
        Self {
            active: cx.create_rw_signal(false),
            target: cx.create_rw_signal(PathBuf::new()),
            busy: cx.create_rw_signal(false),
            message: cx.create_rw_signal(String::new()),
        }
    }
    pub fn open(&self, target: PathBuf) {
        self.target.set(target);
        self.message.set(String::new());
        self.active.set(true);
    }
    fn run(&self, window: Rc<WindowTabData>) {
        if self.busy.get_untracked() {
            return;
        }
        let target = self.target.get_untracked();
        if window.main_split.docs.with_untracked(|docs| {
            docs.iter().any(|(p, d)| {
                (p == &target || p.starts_with(&target)) && !d.is_pristine()
            })
        }) {
            self.message.set(
                "Save modified files in the selected scope before formatting."
                    .into(),
            );
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
                            "Formatted {} changed file(s).",
                            files.len()
                        ));
                    }
                    Err(e) => state.message.set(e),
                }
            },
        );
        let root = window.workspace.path.clone();
        let task = window.common.background_tasks.begin("Formatting Dart files", None);
        std::thread::spawn(move || { let _task = task; send(format_scope(root.as_deref(), &target)); });
    }
}
fn collect(path: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
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
                        | "build"
                        | ".sparrow"
                        | "Pods"
                        | ".gradle"
                )
            ) {
                continue;
            }
            collect(&entry.path(), files)?;
        }
    } else if crate::dart::is_dart_path(path) {
        files.push(path.into());
    }
    if files.len() > 20000 {
        return Err("Too many files in the formatting scope.".into());
    }
    Ok(())
}
fn format_scope(
    root: Option<&Path>,
    target: &Path,
) -> Result<Vec<(PathBuf, String)>, String> {
    let root = root
        .ok_or("Open a project before formatting a scope.")?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !target
        .canonicalize()
        .map_err(|e| e.to_string())?
        .starts_with(&root)
    {
        return Err("The formatting scope must be inside the project.".into());
    }
    let dart = crate::dart::sdk::find_dart_binary(Some(&root))
        .ok_or("Configure a Dart or Flutter SDK to format code.")?;
    let mut paths = vec![];
    collect(target, &mut paths)?;
    paths.sort();
    let mut prepared = vec![];
    for path in paths {
        let before = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let output = std::process::Command::new(&dart)
            .args(["format", "--output=show", "--summary=none"])
            .arg(&path)
            .current_dir(&root)
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(format!(
                "{}: {}",
                path.display(),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        let after = String::from_utf8(output.stdout).map_err(|e| e.to_string())?;
        if before != after {
            prepared.push((path, before, after));
        }
    }
    for (path, before, _) in &prepared {
        if std::fs::read_to_string(path).ok().as_ref() != Some(before) {
            return Err(
                "A file changed while formatting. Run formatting again.".into()
            );
        }
    }
    let files = prepared
        .into_iter()
        .map(|(p, _, text)| (p, text))
        .collect::<Vec<_>>();
    crate::persistence::write_files(files.clone())?;
    Ok(files)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scope_excludes_generated_directories() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("build")).unwrap();
        std::fs::write(dir.path().join("a.dart"), "").unwrap();
        std::fs::write(dir.path().join("build/b.dart"), "").unwrap();
        let mut paths = vec![];
        collect(dir.path(), &mut paths).unwrap();
        assert_eq!(paths, vec![dir.path().join("a.dart")]);
    }
}
