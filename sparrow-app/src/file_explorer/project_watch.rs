//! Watch project metadata without doing IO or SDK discovery on the UI thread.
use super::{Entry, ProjectInfo, package_entries};
use notify::{RecursiveMode, Watcher};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::mpsc::{self, Sender},
    time::Duration,
};

enum Change {
    File(notify::Event),
    Libraries(Vec<PathBuf>),
    Stop,
}
#[derive(Clone)]
pub struct LibraryWatch {
    tx: Sender<Change>,
}
impl LibraryWatch {
    pub fn set_directories(&self, paths: Vec<PathBuf>) {
        let _ = self.tx.send(Change::Libraries(paths));
    }
}
pub struct ProjectWatch {
    tx: Sender<Change>,
}
impl ProjectWatch {
    pub fn libraries(&self) -> LibraryWatch {
        LibraryWatch {
            tx: self.tx.clone(),
        }
    }
    pub fn stop(&self) {
        let _ = self.tx.send(Change::Stop);
    }
}
impl Drop for ProjectWatch {
    fn drop(&mut self) {
        let _ = self.tx.send(Change::Stop);
    }
}
impl ProjectWatch {
    pub fn start(
        root: Option<PathBuf>,
        settings: Option<PathBuf>,
        mut publish: impl FnMut(ProjectInfo) + Send + 'static,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let events = tx.clone();
        std::thread::spawn(move || {
            let Some(root) = root else { return };
            let mut watcher = notify::recommended_watcher(move |event| {
                if let Ok(event) = event {
                    let _ = events.send(Change::File(event));
                }
            })
            .ok();
            let mut watched = HashSet::new();
            let mut previous = ProjectInfo::default();
            let mut signature = Vec::new();
            let mut first = true;
            let mut library_dirs = Vec::<PathBuf>::new();
            loop {
                // Watch parent directories, so atomic file replacement and creation of
                // .dart_tool are observed even when no package_config exists at startup.
                if let Some(watcher) = watcher.as_mut() {
                    let mut dirs = vec![
                        root.clone(),
                        root.join(".dart_tool"),
                        root.join("android"),
                        root.join(".sparrow"),
                    ];
                    if let Some(parent) = settings.as_ref().and_then(|p| p.parent())
                    {
                        dirs.push(parent.to_path_buf());
                    }
                    dirs.extend(library_dirs.clone());
                    for dir in dirs {
                        if dir.is_dir()
                            && !watched.contains(&dir)
                            && watcher
                                .watch(&dir, RecursiveMode::NonRecursive)
                                .is_ok()
                        {
                            watched.insert(dir);
                        }
                    }
                }
                let mut next_signature = fingerprint(&root, settings.as_deref());
                next_signature.extend(library_dirs.iter().map(|path| {
                    let stamp = std::fs::metadata(path)
                        .ok()
                        .and_then(|m| Some((m.modified().ok()?, m.len())));
                    (path.clone(), stamp)
                }));
                if first || next_signature != signature {
                    let next = read_project(&root, previous.clone());
                    previous = next.clone();
                    publish(next);
                    signature = next_signature;
                    first = false;
                }
                match rx.recv_timeout(Duration::from_secs(1)) {
                    Ok(Change::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                        break;
                    }
                    Ok(Change::Libraries(mut dirs)) => {
                        dirs.sort();
                        dirs.dedup();
                        if let Some(watcher) = watcher.as_mut() {
                            for old in &library_dirs {
                                let metadata = old == &root
                                    || old == &root.join(".dart_tool")
                                    || old == &root.join("android")
                                    || old == &root.join(".sparrow")
                                    || settings.as_ref().and_then(|p| p.parent())
                                        == Some(old.as_path());
                                if !dirs.contains(old) && !metadata {
                                    let _ = watcher.unwatch(old);
                                    watched.remove(old);
                                }
                            }
                        }
                        library_dirs = dirs;
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Ok(Change::File(event)) => {
                        let library_change = (event.kind.is_create()
                            || event.kind.is_remove()
                            || matches!(
                                event.kind,
                                notify::EventKind::Modify(
                                    notify::event::ModifyKind::Name(_)
                                )
                            ))
                            && event.paths.iter().any(|path| {
                                library_dirs
                                    .iter()
                                    .any(|dir| path.parent() == Some(dir.as_path()))
                            });
                        if !library_change
                            && !relevant(&root, settings.as_deref(), &event)
                        {
                            continue;
                        }
                        for path in &event.paths {
                            if event.kind.is_remove() {
                                watched.remove(path);
                            }
                        }
                        // Coalesce a pub-get burst, but cap the quiet wait so unrelated
                        // activity can never indefinitely delay a metadata update.
                        let deadline =
                            std::time::Instant::now() + Duration::from_millis(500);
                        while std::time::Instant::now() < deadline {
                            match rx.recv_timeout(Duration::from_millis(150)) {
                                Ok(Change::Stop)
                                | Err(mpsc::RecvTimeoutError::Disconnected) => {
                                    return;
                                }
                                Err(mpsc::RecvTimeoutError::Timeout) => break,
                                Ok(Change::Libraries(mut dirs)) => {
                                    dirs.sort();
                                    dirs.dedup();
                                    library_dirs = dirs;
                                }
                                Ok(Change::File(event)) => {
                                    for path in &event.paths {
                                        if event.kind.is_remove() {
                                            watched.remove(path);
                                        }
                                    }
                                }
                            }
                        }
                        // A watcher event also refreshes same-size/same-timestamp rewrites.
                        signature.clear();
                    }
                }
            }
        });
        Self { tx }
    }
}
fn relevant(root: &Path, settings: Option<&Path>, event: &notify::Event) -> bool {
    if !matches!(
        event.kind,
        notify::EventKind::Create(_)
            | notify::EventKind::Modify(_)
            | notify::EventKind::Remove(_)
    ) {
        return false;
    }
    event.paths.iter().any(|path| {
        if settings == Some(path.as_path()) {
            return true;
        }
        let Ok(relative) = path.strip_prefix(root) else {
            return false;
        };
        matches!(
            relative.to_str(),
            Some(
                ".dart_tool"
                    | ".dart_tool/package_config.json"
                    | "pubspec.yaml"
                    | "pubspec.lock"
                    | "android"
                    | ".sparrow"
                    | ".sparrow/settings.toml"
                    | ".fvmrc"
            )
        ) || (relative.parent() == Some(Path::new("android"))
            && relative.extension().is_some_and(|e| e == "iml"))
    })
}
fn fingerprint(
    root: &Path,
    settings: Option<&Path>,
) -> Vec<(PathBuf, Option<(std::time::SystemTime, u64)>)> {
    let mut paths = vec![
        root.join(".dart_tool/package_config.json"),
        root.join("pubspec.yaml"),
        root.join("pubspec.lock"),
        root.join("android"),
        root.join(".sparrow/settings.toml"),
        root.join(".fvmrc"),
    ];
    paths.extend(settings.map(Path::to_path_buf));
    paths
        .into_iter()
        .map(|path| {
            let stamp = std::fs::metadata(&path)
                .ok()
                .and_then(|m| Some((m.modified().ok()?, m.len())));
            (path, stamp)
        })
        .collect()
}
fn read_project(root: &Path, previous: ProjectInfo) -> ProjectInfo {
    let mut value = ProjectInfo {
        flutter: crate::dart::is_flutter_project(root),
        revision: previous.revision + 1,
        ..Default::default()
    };
    let android = root.join("android");
    if let Ok(files) = std::fs::read_dir(&android) {
        if let Some(module) = files
            .flatten()
            .map(|f| f.path())
            .filter(|p| p.extension().is_some_and(|e| e == "iml"))
            .min()
        {
            if let Some(name) = module.file_stem() {
                value
                    .modules
                    .insert(android, name.to_string_lossy().into_owned());
            }
        }
    }
    let path = root.join(".dart_tool/package_config.json");
    // An interrupted write must not replace the last good tree with an empty one.
    let mut resolution =
        Err("Dependencies are not resolved. Run Pub Get.".to_string());
    for attempt in 0..4 {
        resolution = read_packages(&path, root);
        if resolution.is_ok() {
            break;
        }
        if attempt < 3 {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    match resolution {
        Ok(libraries) => value.libraries = libraries,
        Err(error) => {
            value.error = Some(error);
            if path.exists() {
                value.libraries = previous
                    .libraries
                    .iter()
                    .filter(|e| e.level == 2)
                    .cloned()
                    .collect();
            }
        }
    }
    if let Some(sdk) = crate::dart::find_dart_sdk(Some(root)).and_then(|s| s.root) {
        if sdk.ends_with("bin/cache/dart-sdk") {
            if let Some(flutter) =
                sdk.ancestors().nth(3).and_then(|p| p.canonicalize().ok())
            {
                for library in &mut value.libraries {
                    if library
                        .path
                        .canonicalize()
                        .ok()
                        .is_some_and(|p| p.starts_with(&flutter))
                    {
                        library.read_only = true;
                    }
                }
            }
        }
        value.libraries.push(Entry {
            path: sdk.join("lib"),
            name: "Dart SDK".into(),
            dir: true,
            level: 1,
            read_only: true,
        });
    }
    value
}
fn read_packages(path: &Path, root: &Path) -> Result<Vec<Entry>, String> {
    let source = std::fs::read_to_string(path)
        .map_err(|_| "Dependencies are not resolved. Run Pub Get.".to_string())?;
    let json:serde_json::Value=serde_json::from_str(&source).map_err(|_|"Waiting for a valid package_config.json; keeping the last resolved packages.".to_string())?;
    if !json.get("packages").is_some_and(|p| p.is_array()) {
        return Err("Invalid package_config.json: packages must be an array.".into());
    }
    Ok(package_entries(path, &source, root))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn watches_resolution_and_settings_but_ignores_build_output() {
        let root = Path::new("/p");
        let event = |path: &str| {
            notify::Event::new(notify::EventKind::Modify(
                notify::event::ModifyKind::Data(notify::event::DataChange::Any),
            ))
            .add_path(PathBuf::from(path))
        };
        assert!(relevant(
            root,
            None,
            &event("/p/.dart_tool/package_config.json")
        ));
        assert!(relevant(root, None, &event("/p/.sparrow/settings.toml")));
        assert!(!relevant(root, None, &event("/p/build/app/output")));
    }
    #[test]
    fn expanded_library_directory_refreshes_without_resolution_changes() {
        let base = std::env::temp_dir()
            .join(format!("sparrow-library-watch-{}", std::process::id()));
        let root = base.join("app");
        let library = base.join("dependency/lib");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&library).unwrap();
        let (tx, rx) = mpsc::channel();
        let watch = ProjectWatch::start(Some(root.clone()), None, move |info| {
            let _ = tx.send(info);
        });
        let first = rx.recv_timeout(Duration::from_secs(12)).unwrap();
        watch.libraries().set_directories(vec![library.clone()]);
        let watched = rx.recv_timeout(Duration::from_secs(12)).unwrap();
        assert!(watched.revision > first.revision);
        std::fs::write(library.join("added.dart"), "const added = true;").unwrap();
        assert!(
            rx.recv_timeout(Duration::from_secs(12)).unwrap().revision
                > watched.revision
        );
        drop(watch);
        std::fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn live_watch_handles_creation_atomic_replacement_invalid_write_and_removal() {
        let root = std::env::temp_dir()
            .join(format!("sparrow-resolution-watch-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let (tx, rx) = mpsc::channel();
        let watch = ProjectWatch::start(Some(root.clone()), None, move |info| {
            let _ = tx.send(info);
        });
        let wait = |count, error: bool| {
            let deadline = std::time::Instant::now() + Duration::from_secs(12);
            loop {
                let info = rx
                    .recv_timeout(
                        deadline
                            .saturating_duration_since(std::time::Instant::now()),
                    )
                    .unwrap();
                if info.libraries.iter().filter(|e| e.level == 2).count() == count
                    && info.error.is_some() == error
                {
                    return info;
                }
            }
        };
        wait(0, true);
        let dir = root.join(".dart_tool");
        std::fs::create_dir_all(&dir).unwrap();
        let config = dir.join("package_config.json");
        std::fs::write(
            &config,
            r#"{"packages":[{"name":"one","rootUri":"file:///cache/one/"}]}"#,
        )
        .unwrap();
        let first = wait(1, false);
        let temp = dir.join("config.tmp");
        std::fs::write(&temp,r#"{"packages":[{"name":"two","rootUri":"file:///cache/two/"},{"name":"three","rootUri":"file:///cache/three/"}]}"#).unwrap();
        std::fs::rename(temp, &config).unwrap();
        assert!(wait(2, false).revision > first.revision);
        std::fs::write(&config, "{").unwrap();
        wait(2, true);
        std::fs::write(&config, r#"{"packages":[]}"#).unwrap();
        wait(0, false);
        std::fs::remove_dir_all(&dir).unwrap();
        wait(0, true);
        drop(watch);
        std::fs::remove_dir_all(root).unwrap();
    }
}
