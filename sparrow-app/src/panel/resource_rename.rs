//! Android resource renames with a reviewable, stale-checked change plan.
use super::resource_manager::{Kind, Resource};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
#[derive(Clone)]
pub struct Plan {
    pub moves: Vec<(PathBuf, PathBuf)>,
    pub edits: Vec<(PathBuf, String, String)>,
}
impl Plan {
    pub fn summary(&self) -> String {
        format!(
            "{} file rename(s), {} file(s) with reference or definition changes",
            self.moves.len(),
            self.edits.len()
        )
    }
    pub fn details(&self) -> String {
        let mut lines = vec![self.summary()];
        for (old, new) in &self.moves {
            lines.push(format!(
                "{} → {}",
                old.display(),
                new.file_name().unwrap_or_default().to_string_lossy()
            ));
        }
        for (path, before, after) in &self.edits {
            lines.push(format!("Update {}", path.display()));
            for (a, b) in before
                .lines()
                .zip(after.lines())
                .filter(|(a, b)| a != b)
                .take(4)
            {
                lines.push(format!("− {a}\n+ {b}"));
            }
        }
        lines.join("\n")
    }
    pub fn apply(&self) -> Result<(), String> {
        for (path, old, _) in &self.edits {
            if std::fs::read_to_string(path).ok().as_ref() != Some(old) {
                return Err(format!("{} changed; preview again.", path.display()));
            }
        }
        for (old, new) in &self.moves {
            if !old.is_file() || new.exists() {
                return Err("A resource path changed; preview again.".into());
            }
        }
        let mut moved = Vec::new();
        for (old, new) in &self.moves {
            if let Err(e) = std::fs::rename(old, new) {
                for (old, new) in moved.iter().rev() {
                    let _ = std::fs::rename(new, old);
                }
                return Err(e.to_string());
            }
            moved.push((old.clone(), new.clone()));
        }
        let writes = self
            .edits
            .iter()
            .map(|(path, _, text)| {
                let path = self
                    .moves
                    .iter()
                    .find(|(old, _)| old == path)
                    .map(|(_, new)| new)
                    .unwrap_or(path);
                (path.clone(), text.clone())
            })
            .collect();
        if let Err(e) = crate::persistence::write_files(writes) {
            for (old, new) in moved.iter().rev() {
                let _ = std::fs::rename(new, old);
            }
            return Err(e);
        }
        Ok(())
    }
}
pub fn plan(root: &Path, resource: &Resource, new: &str) -> Result<Plan, String> {
    if new.is_empty()
        || !new.as_bytes()[0].is_ascii_lowercase()
        || !new
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return Err(
            "Use a lowercase Android resource name beginning with a letter.".into(),
        );
    }
    if new == resource.name {
        return Err("Choose a different resource name.".into());
    }
    let kind = match resource.kind {
        Kind::Drawable => "drawable",
        Kind::MipMap => "mipmap",
        Kind::Layout => "layout",
        Kind::Font => "font",
        Kind::Color => "color",
        Kind::String => "string",
        _ => return Err("Use file rename for Flutter assets.".into()),
    };
    let module = root.join(&resource.module);
    let canonical = root.canonicalize().map_err(|e| e.to_string())?;
    if !resource.module.starts_with("android/")
        || !module
            .canonicalize()
            .map_err(|e| e.to_string())?
            .starts_with(&canonical)
    {
        return Err("Open a local Android wrapper module.".into());
    }
    let mut files = Vec::new();
    fn walk(path: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
        for entry in std::fs::read_dir(path).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let ty = entry.file_type().map_err(|e| e.to_string())?;
            if ty.is_symlink() {
                continue;
            }
            if ty.is_dir() {
                if !matches!(
                    entry.file_name().to_str(),
                    Some("build" | ".gradle" | ".git" | ".dart_tool")
                ) {
                    walk(&entry.path(), files)?;
                }
            } else if ty.is_file() {
                files.push(entry.path());
            }
            if files.len() > 30000 {
                return Err("Module is too large to preview safely.".into());
            }
        }
        Ok(())
    }
    walk(&module, &mut files)?;
    let value = matches!(resource.kind, Kind::Color | Kind::String);
    let mut moves = Vec::new();
    for variant in &resource.variants {
        if variant.path.is_symlink()
            || !variant
                .path
                .canonicalize()
                .map_err(|e| e.to_string())?
                .starts_with(&module.canonicalize().map_err(|e| e.to_string())?)
        {
            return Err("Resource variants must remain inside their module.".into());
        }
        if !value {
            let file = variant
                .path
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or("Invalid filename")?;
            let suffix = file
                .strip_prefix(&resource.name)
                .ok_or("Resource filename no longer matches")?;
            let dest = variant.path.with_file_name(format!("{new}{suffix}"));
            if dest.exists() {
                return Err(format!("{} already exists.", dest.display()));
            }
            moves.push((variant.path.clone(), dest));
        }
    }
    let old = regex::escape(&resource.name);
    let refs = regex::Regex::new(&format!(
        r"(@\+?{kind}/|\bR\s*\.\s*{kind}\s*\.\s*){old}\b"
    ))
    .unwrap();
    let mut edits = BTreeMap::new();
    for path in files {
        if !matches!(
            path.extension().and_then(|s| s.to_str()),
            Some("xml" | "java" | "kt" | "kts")
        ) {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let mut updated = refs
            .replace_all(&text, |c: &regex::Captures| format!("{}{new}", &c[1]))
            .into_owned();
        if value && path.extension().is_some_and(|s| s == "xml") {
            let xml =
                roxmltree::Document::parse(&text).map_err(|e| e.to_string())?;
            let mut ranges = Vec::new();
            for node in xml.descendants().filter(|n| n.has_tag_name(kind)) {
                if node.attribute("name") == Some(new) {
                    return Err(format!("{kind}/{new} already exists."));
                }
                if node.attribute("name") == Some(resource.name.as_str()) {
                    if let Some(attr) = node.attribute_node("name") {
                        ranges.push(attr.range_value());
                    }
                }
            }
            // Apply definition edits before references, whose lengths may differ.
            let mut defined = text.clone();
            for range in ranges.into_iter().rev() {
                defined.replace_range(range, new);
            }
            updated = refs
                .replace_all(&defined, |c: &regex::Captures| {
                    format!("{}{new}", &c[1])
                })
                .into_owned();
        }
        if updated != text {
            edits.insert(path, (text, updated));
        }
    }
    Ok(Plan {
        moves,
        edits: edits.into_iter().map(|(p, (a, b))| (p, a, b)).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(kind: Kind) -> (tempfile::TempDir, Resource) {
        let dir = tempfile::tempdir().unwrap();
        let module = dir.path().join("android/app");
        let mut variants = Vec::new();
        for folder in ["drawable", "drawable-night"] {
            let path = module
                .join("src/main/res")
                .join(folder)
                .join("old_icon.xml");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "<shape/>").unwrap();
            variants.push(super::super::resource_manager::Variant {
                path,
                qualifier: folder.into(),
                value: String::new(),
                line: 0,
                stamp: None,
            });
        }
        let res = Resource {
            module: "android/app".into(),
            kind,
            name: "old_icon".into(),
            variants,
        };
        (dir, res)
    }
    #[test]
    fn renames_all_variants_and_updates_exact_xml_java_kotlin_usages() {
        let (dir, res) = fixture(Kind::Drawable);
        let src = dir.path().join("android/app/src/main");
        let xml = src.join("view.xml");
        std::fs::write(&xml,"<view src=\"@drawable/old_icon\" other=\"@drawable/old_icon_extra\" system=\"@android:drawable/old_icon\"/>").unwrap();
        let code = src.join("Runner.kt");
        std::fs::write(
            &code,
            "val icon = R.drawable.old_icon; val other = R.drawable.old_icon_extra",
        )
        .unwrap();
        let plan = plan(dir.path(), &res, "new_icon").unwrap();
        assert_eq!(plan.moves.len(), 2);
        assert_eq!(plan.edits.len(), 2);
        plan.apply().unwrap();
        assert!(!res.variants[0].path.exists());
        assert!(res.variants[0].path.with_file_name("new_icon.xml").exists());
        let text = std::fs::read_to_string(xml).unwrap();
        assert!(text.contains("@drawable/new_icon"));
        assert!(text.contains("@android:drawable/old_icon"));
        assert!(text.contains("old_icon_extra"));
        assert!(
            std::fs::read_to_string(code)
                .unwrap()
                .contains("R.drawable.new_icon")
        );
    }
    #[test]
    fn conflicts_and_stale_plans_do_not_move_or_replace_any_file() {
        let (dir, res) = fixture(Kind::Drawable);
        let xml = dir.path().join("android/app/usage.xml");
        std::fs::write(&xml, "<view src=\"@drawable/old_icon\"/>").unwrap();
        let p = plan(dir.path(), &res, "new_icon").unwrap();
        std::fs::write(&xml, "<changed/>").unwrap();
        assert!(p.apply().is_err());
        assert!(res.variants[0].path.exists());
        std::fs::write(
            res.variants[0].path.with_file_name("new_icon.xml"),
            "existing",
        )
        .unwrap();
        assert!(plan(dir.path(), &res, "new_icon").is_err());
        assert!(plan(dir.path(), &res, "../bad").is_err());
    }
    #[test]
    fn value_symbol_definitions_and_references_change_without_renaming_container() {
        let (dir, mut res) = fixture(Kind::String);
        let path = dir
            .path()
            .join("android/app/src/main/res/values/strings.xml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path,"<resources><string name=\"old_icon\">Title</string><string name=\"alias\">@string/old_icon</string></resources>").unwrap();
        res.variants[0].path = path.clone();
        res.variants.truncate(1);
        let p = plan(dir.path(), &res, "new_title").unwrap();
        assert!(p.moves.is_empty());
        p.apply().unwrap();
        let text = std::fs::read_to_string(path).unwrap();
        assert!(text.contains("name=\"new_title\""));
        assert!(text.contains("@string/new_title"));
    }
}
