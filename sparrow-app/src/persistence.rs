//! Prepare every file before replacing any of them; restore earlier writes on failure.
use std::path::PathBuf;

pub(crate) fn write_files(files: Vec<(PathBuf, String)>) -> Result<(), String> {
    write_changes(
        files
            .into_iter()
            .map(|(path, text)| (path, Some(text)))
            .collect(),
    )
}
pub(crate) fn write_changes(
    files: Vec<(PathBuf, Option<String>)>,
) -> Result<(), String> {
    let mut prepared = Vec::new();
    for (path, contents) in files {
        let previous = match std::fs::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(format!("{}: {error}", path.display())),
        };
        let parent = path.parent().ok_or("The destination has no directory")?;
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let temporary = if let Some(contents) = contents {
            let temporary = tempfile::NamedTempFile::new_in(parent)
                .map_err(|error| error.to_string())?;
            std::fs::write(temporary.path(), contents)
                .map_err(|error| error.to_string())?;
            Some(temporary)
        } else {
            None
        };
        prepared.push((path, previous, temporary));
    }
    let mut replaced: Vec<(PathBuf, Option<Vec<u8>>)> = Vec::new();
    for (path, previous, temporary) in prepared {
        let result = if let Some(temporary) = temporary {
            temporary
                .persist(&path)
                .map(|_| ())
                .map_err(|error| error.to_string())
        } else if previous.is_some() {
            std::fs::remove_file(&path).map_err(|error| error.to_string())
        } else {
            Ok(())
        };
        if let Err(error) = result {
            let mut failures = Vec::new();
            for (path, previous) in replaced.into_iter().rev() {
                let restored = match previous {
                    Some(bytes) => std::fs::write(&path, bytes),
                    None => std::fs::remove_file(&path),
                };
                if let Err(error) = restored {
                    failures.push(format!("{}: {error}", path.display()));
                }
            }
            return Err(format!(
                "{}: {error}{}",
                path.display(),
                if failures.is_empty() {
                    String::new()
                } else {
                    format!("; restore failed: {}", failures.join(", "))
                }
            ));
        }
        replaced.push((path, previous));
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preparation_error_does_not_replace_another_file() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first.toml");
        std::fs::write(&first, "original").unwrap();
        let invalid = root.path().join("not-a-directory");
        std::fs::write(&invalid, "file").unwrap();
        assert!(
            write_files(vec![
                (first.clone(), "replacement".into()),
                (invalid.join("settings.toml"), "value".into())
            ])
            .is_err()
        );
        assert_eq!(std::fs::read_to_string(first).unwrap(), "original");
    }
}
