//! Classify library source at document open, including navigation and symlinked SDKs.
use std::{
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

static SDK_ROOTS: OnceLock<Mutex<Vec<PathBuf>>> = OnceLock::new();
fn resolved(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

pub fn read_only(path: &Path, workspace: Option<&Path>) -> bool {
    let path = resolved(path);
    // An explicitly opened project remains editable, including a local package.
    if workspace.is_some_and(|root| path.starts_with(resolved(root))) {
        return false;
    }
    if path
        .components()
        .any(|part| part.as_os_str() == ".pub-cache")
        || std::env::var_os("PUB_CACHE")
            .is_some_and(|root| path.starts_with(resolved(Path::new(&root))))
    {
        return true;
    }
    let roots = SDK_ROOTS.get_or_init(Default::default);
    if roots
        .lock()
        .unwrap()
        .iter()
        .any(|root| path.starts_with(root))
    {
        return true;
    }
    // Identify SDKs by their installed layout, without SDK version subprocesses
    // on the UI thread. Canonical paths also cover Go to Definition through FVM.
    for root in path.ancestors().skip(1) {
        let dart = root.join("lib/libraries.json").is_file()
            && (root.join("bin/dart").is_file()
                || root.join("bin/dart.exe").is_file());
        let flutter = root.join("bin/cache/dart-sdk/lib/libraries.json").is_file()
            && (root.join("bin/flutter").is_file()
                || root.join("bin/flutter.bat").is_file());
        if dart || flutter {
            roots.lock().unwrap().push(root.to_path_buf());
            return true;
        }
    }
    false
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sdk_navigation_is_protected_without_opening_the_library_tree() {
        let dir = tempfile::tempdir().unwrap();
        let sdk = dir.path().join("sdk");
        std::fs::create_dir_all(sdk.join("lib/core")).unwrap();
        std::fs::create_dir_all(sdk.join("bin")).unwrap();
        std::fs::write(sdk.join("lib/libraries.json"), "{}").unwrap();
        std::fs::write(sdk.join("bin/dart"), "").unwrap();
        std::fs::write(sdk.join("lib/core/string.dart"), "class String {}").unwrap();
        assert!(read_only(
            &sdk.join("lib/core/string.dart"),
            Some(&dir.path().join("app"))
        ));
        assert!(!read_only(&sdk.join("lib/core/string.dart"), Some(&sdk)));
        assert!(!read_only(
            &dir.path().join("local/lib/local.dart"),
            Some(&dir.path().join("app"))
        ));
        #[cfg(unix)]
        {
            let alias = dir.path().join("sdk-alias");
            std::os::unix::fs::symlink(&sdk, &alias).unwrap();
            assert!(read_only(&alias.join("lib/core/string.dart"), None));
        }
    }
}
