use std::{
    path::{Path, PathBuf},
};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// Returns the first item yielded from `items` if at least one item is yielded, all yielded items
/// are `Some`, and all yielded items compare equal, else returns `None`.
fn try_all_equal_value<T: PartialEq, I: IntoIterator<Item = Option<T>>>(
    items: I,
) -> Option<T> {
    let mut items = items.into_iter();
    let first = items.next().flatten()?;

    items.try_fold(first, |initial_item, item| {
        item.and_then(|item| (item == initial_item).then_some(initial_item))
    })
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "kebab-case", default)]
pub struct IconThemeConfig {
    #[serde(skip)]
    pub path: PathBuf,
    pub name: String,
    pub use_editor_color: Option<bool>,
    pub ui: IndexMap<String, String>,
    pub foldername: IndexMap<String, String>,
    pub filename: IndexMap<String, String>,
    pub extension: IndexMap<String, String>,
}

impl IconThemeConfig {
    /// If all paths in `paths` have the same file type (as determined by the file name or
    /// extension), and there is an icon associated with that file type, returns the path of the
    /// icon.
    pub fn resolve_path_to_icon(&self, paths: &[&Path]) -> Option<PathBuf> {
        let resolve = |path: &&Path| {
            let name = path.file_name()?.to_str()?;
            self.filename
                .get(name)
                .or_else(|| {
                    self.filename
                        .iter()
                        .filter_map(|(pattern, icon)| {
                            let prefix = pattern.strip_suffix('*')?;
                            (pattern.ends_with(".*") && name.starts_with(prefix))
                                .then_some((prefix.len(), icon))
                        })
                        .max_by_key(|(length, _)| *length)
                        .map(|(_, icon)| icon)
                })
                .or_else(|| {
                    let lower = name.to_ascii_lowercase();
                    // Try each suffix from longest to shortest: freezed.dart
                    // wins over dart, and gradle.kts wins over kts.
                    lower.match_indices('.').find_map(|(index, _)| {
                        self.extension.get(&lower[index + 1..])
                    })
                })
                .cloned()
        };
        try_all_equal_value(paths.iter().map(resolve))
            .or_else(|| {
                // Preserve diff-tab behavior: distinct named files with a shared
                // extension can still use the common extension icon.
                try_all_equal_value(paths.iter().map(|path| {
                    let name = path.file_name()?.to_str()?.to_ascii_lowercase();
                    name.match_indices('.')
                        .find_map(|(index, _)| {
                            self.extension.get(&name[index + 1..])
                        })
                        .cloned()
                }))
            })
            .map(|icon| self.path.join(icon))
    }

    pub fn resolve_folder_to_icon(&self, path: &Path) -> Option<PathBuf> {
        let name = path.file_name()?.to_str()?;
        self.foldername
            .get(name)
            .or_else(|| {
                let lower = name.to_ascii_lowercase();
                self.foldername
                    .iter()
                    .filter(|(suffix, _)| {
                        suffix.starts_with('.')
                            && lower.ends_with(&suffix.to_ascii_lowercase())
                    })
                    .max_by_key(|(suffix, _)| suffix.len())
                    .map(|(_, icon)| icon)
            })
            .map(|icon| self.path.join(icon))
    }
}

#[cfg(test)]
mod tests {
    use super::IconThemeConfig;
    use crate::config::icon_theme::try_all_equal_value;

    #[test]
    fn try_all_equal_value_empty_none() {
        assert_eq!(Option::<u32>::None, try_all_equal_value([]));
    }

    #[test]
    fn try_all_equal_value_any_none_none() {
        assert_eq!(Option::<u32>::None, try_all_equal_value([None]));
        assert_eq!(
            Option::<i32>::None,
            try_all_equal_value([None, Some(1), Some(1)])
        );
        assert_eq!(Option::<u64>::None, try_all_equal_value([Some(0), None]));
        assert_eq!(
            Option::<u8>::None,
            try_all_equal_value([Some(3), Some(3), None, Some(3)])
        );
    }

    #[test]
    fn try_all_equal_value_any_different_none() {
        assert_eq!(Option::<u32>::None, try_all_equal_value([Some(1), Some(2)]));
        assert_eq!(
            Option::<u128>::None,
            try_all_equal_value([Some(1), Some(10), Some(1)])
        );
        assert_eq!(
            Option::<i16>::None,
            try_all_equal_value([Some(3), Some(3), Some(3), Some(3), Some(2)])
        );
        assert_eq!(
            Option::<i64>::None,
            try_all_equal_value([Some(5), Some(4), Some(4), Some(4), Some(4)])
        );
        assert_eq!(
            Option::<i128>::None,
            try_all_equal_value([Some(3), Some(0), Some(9), Some(20), Some(1)])
        );
    }

    #[test]
    fn try_all_equal_value_all_same_some() {
        assert_eq!(Option::<u32>::Some(1), try_all_equal_value([Some(1)]));
        assert_eq!(Option::<i16>::Some(-2), try_all_equal_value([Some(-2); 2]));
        assert_eq!(Option::<i128>::Some(0), try_all_equal_value([Some(0); 3]));
        assert_eq!(Option::<u8>::Some(30), try_all_equal_value([Some(30); 57]));
    }

    fn get_icon_theme_config() -> IconThemeConfig {
        IconThemeConfig {
            path: "icons".to_owned().into(),
            filename: [("Makefile", "makefile.svg"), ("special.rs", "special.svg")]
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .into(),
            extension: [("rs", "rust.svg"), ("c", "c.svg"), ("py", "python.svg")]
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .into(),
            ..Default::default()
        }
    }

    #[test]
    fn resolve_path_to_icon_no_paths_none() {
        let icon_theme_config = get_icon_theme_config();

        assert_eq!(None, icon_theme_config.resolve_path_to_icon(&[]));
    }

    #[test]
    fn resolve_path_to_icon_different_none() {
        let icon_theme_config = get_icon_theme_config();

        assert_eq!(
            None,
            icon_theme_config
                .resolve_path_to_icon(&["foo.rs", "bar.c"].map(AsRef::as_ref))
        );
        assert_eq!(
            None,
            icon_theme_config.resolve_path_to_icon(
                &["/some/path/main.py", "other/path.py", "dir1/./dir2/file.rs"]
                    .map(AsRef::as_ref)
            )
        );
        assert_eq!(
            None,
            icon_theme_config.resolve_path_to_icon(
                &["/root/Makefile", "dir/dir/special.rs", "../../main.rs"]
                    .map(AsRef::as_ref)
            )
        );
        assert_eq!(
            None,
            icon_theme_config
                .resolve_path_to_icon(&["main.c", "foo.txt"].map(AsRef::as_ref))
        );
    }

    #[test]
    fn resolve_path_to_icon_no_match_none() {
        let icon_theme_config = get_icon_theme_config();

        assert_eq!(
            None,
            icon_theme_config.resolve_path_to_icon(&["foo"].map(AsRef::as_ref))
        );
        assert_eq!(
            None,
            icon_theme_config.resolve_path_to_icon(
                &["/some/path/file.txt", "other/path.txt"].map(AsRef::as_ref)
            )
        );
        assert_eq!(
            None,
            icon_theme_config.resolve_path_to_icon(
                &["folder/file", "/home/user/file", "../../file"].map(AsRef::as_ref)
            )
        );
        assert_eq!(
            None,
            icon_theme_config.resolve_path_to_icon(&[".."].map(AsRef::as_ref))
        );
        assert_eq!(
            None,
            icon_theme_config.resolve_path_to_icon(&["."].map(AsRef::as_ref))
        );
    }

    #[test]
    fn resolve_path_to_icon_file_name_match_some() {
        let icon_theme_config = get_icon_theme_config();

        assert_eq!(
            Some("icons/makefile.svg".to_owned().into()),
            icon_theme_config.resolve_path_to_icon(&["Makefile"].map(AsRef::as_ref))
        );
        assert_eq!(
            Some("icons/makefile.svg".to_owned().into()),
            icon_theme_config.resolve_path_to_icon(
                &[
                    "baz/Makefile",
                    "/foo/bar/dir/Makefile",
                    ".././/././Makefile"
                ]
                .map(AsRef::as_ref)
            )
        );
        assert_eq!(
            Some("icons/special.svg".to_owned().into()),
            icon_theme_config.resolve_path_to_icon(
                &["dir/special.rs", "/dir1/dir2/..//./special.rs"]
                    .map(AsRef::as_ref)
            )
        );
    }

    #[test]
    fn resolve_path_to_icon_extension_match_some() {
        let icon_theme_config = get_icon_theme_config();

        assert_eq!(
            Some("icons/python.svg".to_owned().into()),
            icon_theme_config
                .resolve_path_to_icon(&["source.py"].map(AsRef::as_ref))
        );
        assert_eq!(
            Some("icons/rust.svg".to_owned().into()),
            icon_theme_config.resolve_path_to_icon(
                &["/home/user/main.rs", "../../special.rs.rs", "special.rs"]
                    .map(AsRef::as_ref)
            )
        );
        assert_eq!(
            Some("icons/c.svg".to_owned().into()),
            icon_theme_config.resolve_path_to_icon(
                &["/dir1/Makefile.c", "../main.c"].map(AsRef::as_ref)
            )
        );
    }
}

#[cfg(test)]
mod compound_tests {
    use super::*;
    #[test]
    fn explicit_theme_names_precede_suffixes_and_folder_patterns() {
        let mut theme = IconThemeConfig::default();
        theme.filename.insert("model.g.dart".into(), "custom.svg".into());
        theme.extension.insert("g.dart".into(), "generated.svg".into());
        theme.extension.insert("dart".into(), "dart.svg".into());
        assert_eq!(theme.resolve_path_to_icon(&[Path::new("model.g.dart")]), Some("custom.svg".into()));
        assert_eq!(theme.resolve_path_to_icon(&[Path::new("other.g.dart")]), Some("generated.svg".into()));
        theme.foldername.insert("Assets.xcassets".into(), "custom-assets.svg".into());
        theme.foldername.insert(".xcassets".into(), "assets.svg".into());
        assert_eq!(theme.resolve_folder_to_icon(Path::new("Assets.xcassets")), Some("custom-assets.svg".into()));
        assert_eq!(theme.resolve_folder_to_icon(Path::new("Other.xcassets")), Some("assets.svg".into()));
        assert_eq!(theme.resolve_folder_to_icon(Path::new("ordinary")), None);
    }
}
