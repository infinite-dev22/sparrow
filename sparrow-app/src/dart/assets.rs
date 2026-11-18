//! Workspace asset catalog, refreshed off the UI thread on filesystem changes.
use notify::Watcher;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};
struct Catalog {
    paths: Arc<Mutex<Vec<String>>>,
    _watcher: Option<notify::RecommendedWatcher>,
}
static CATALOGS: OnceLock<Mutex<BTreeMap<PathBuf, Catalog>>> = OnceLock::new();
fn scan(root: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if kind.is_dir() {
                walk(root, &path, out);
            } else if kind.is_file()
                && path.extension().and_then(|x| x.to_str()).is_some_and(|x| {
                    matches!(
                        x.to_ascii_lowercase().as_str(),
                        "png"
                            | "jpg"
                            | "jpeg"
                            | "gif"
                            | "webp"
                            | "bmp"
                            | "svg"
                            | "ico"
                            | "ttf"
                            | "otf"
                            | "woff"
                            | "woff2"
                    )
                })
            {
                if let Ok(relative) = path.strip_prefix(root) {
                    out.push(relative.to_string_lossy().replace('\\', "/"));
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(root, &root.join("assets"), &mut out);
    out.sort();
    out
}
pub fn start(root: PathBuf) {
    let catalogs = CATALOGS.get_or_init(Default::default);
    let mut catalogs = catalogs.lock().unwrap();
    if catalogs.contains_key(&root) {
        return;
    }
    let paths = Arc::new(Mutex::new(Vec::new()));
    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher =
        notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            if let Ok(event) = event {
                if !matches!(
                    event.kind,
                    notify::EventKind::Create(_)
                        | notify::EventKind::Modify(_)
                        | notify::EventKind::Remove(_)
                ) {
                    return;
                }
                if event
                    .paths
                    .iter()
                    .any(|p| p.components().any(|c| c.as_os_str() == "assets"))
                {
                    let _ = tx.send(());
                }
            }
        })
        .ok();
    if let Some(w) = &mut watcher {
        let _ = w.watch(&root, notify::RecursiveMode::Recursive);
    }
    let worker_root = root.clone();
    let worker_paths = paths.clone();
    std::thread::spawn(move || {
        *worker_paths.lock().unwrap() = scan(&worker_root);
        while rx.recv().is_ok() {
            while rx.try_recv().is_ok() {}
            *worker_paths.lock().unwrap() = scan(&worker_root);
        }
    });
    catalogs.insert(
        root,
        Catalog {
            paths,
            _watcher: watcher,
        },
    );
}
fn is_code(source: &str) -> bool {
    let b = source.as_bytes();
    let mut i = 0;
    let mut quote = None;
    let mut comment = 0;
    while i < b.len() {
        if comment > 0 {
            if b[i..].starts_with(b"/*") {
                comment += 1;
                i += 2;
            } else if b[i..].starts_with(b"*/") {
                comment -= 1;
                i += 2;
            } else {
                i += 1;
            }
        } else if let Some(q) = quote {
            if b[i] == b'\\' {
                i += 2;
            } else {
                if b[i] == q {
                    quote = None;
                }
                i += 1;
            }
        } else if b[i..].starts_with(b"//") {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            if i == b.len() {
                return false;
            }
        } else if b[i..].starts_with(b"/*") {
            comment = 1;
            i += 2;
        } else {
            if matches!(b[i], b'\'' | b'"') {
                quote = Some(b[i]);
            }
            i += 1;
        }
    }
    quote.is_none() && comment == 0
}
/// Return the opening string offset and matching asset paths only in supported calls.
pub fn complete(
    root: &Path,
    source: &str,
    offset: usize,
) -> Option<(usize, Vec<String>)> {
    let before = source.get(..offset)?;
    static CONTEXT: OnceLock<regex::Regex> = OnceLock::new();
    let re = CONTEXT.get_or_init(|| {
        regex::Regex::new(
            r#"(?:\bAssetImage|\bImage\s*\.\s*asset)\s*\(\s*['"]([^'"\n]*)$"#,
        )
        .unwrap()
    });
    let cap = re.captures(before)?;
    let prefix = cap.get(1)?;
    // Do not offer paths for interpolated expressions or commented-out calls.
    if prefix.as_str().contains('$') || !is_code(&before[..cap.get(0)?.start()]) {
        return None;
    }
    let paths = CATALOGS
        .get()?
        .lock()
        .ok()?
        .get(root)?
        .paths
        .lock()
        .ok()?
        .iter()
        .filter(|p| p.starts_with(prefix.as_str()))
        .cloned()
        .collect();
    Some((prefix.start(), paths))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn watches_changes_and_completes_supported_arguments() {
        let root = tempfile::tempdir().unwrap();
        start(root.path().into());
        std::fs::create_dir(root.path().join("assets")).unwrap();
        std::fs::write(root.path().join("assets/new.png"), "").unwrap();
        let source = "Image.asset('assets/ne";
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
        loop {
            if complete(root.path(), source, source.len())
                .is_some_and(|(_, paths)| paths == ["assets/new.png"])
            {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "watcher did not index new asset"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        for bad in [
            "print('assets/ne",
            "// Image.asset('assets/ne",
            "/* Image.asset('assets/ne",
            "final text = \"Image.asset('assets/ne",
            "Image.asset('assets/$name",
        ] {
            assert!(complete(root.path(), bad, bad.len()).is_none(), "{bad}");
        }
        std::fs::remove_file(root.path().join("assets/new.png")).unwrap();
        while complete(root.path(), source, source.len())
            .is_some_and(|(_, paths)| !paths.is_empty())
        {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
    #[test]
    fn indexes_assets_without_following_symlinks() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("assets/icons")).unwrap();
        for name in [
            "assets/a.png",
            "assets/icons/a.svg",
            "assets/font.ttf",
            "assets/ignore.txt",
        ] {
            std::fs::write(root.path().join(name), "").unwrap();
        }
        assert_eq!(
            scan(root.path()),
            ["assets/a.png", "assets/font.ttf", "assets/icons/a.svg"]
        );
    }
}
