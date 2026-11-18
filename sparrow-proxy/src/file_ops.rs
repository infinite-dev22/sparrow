//! Filesystem operations shared by Project duplicate and paste actions.
use std::{
    fs, io,
    path::{Path, PathBuf},
};

fn resolved_destination(path: &Path) -> io::Result<PathBuf> {
    if !path.is_absolute() {
        return resolved_destination(&std::env::current_dir()?.join(path));
    }
    if path.exists() {
        return fs::canonicalize(path);
    }
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "Destination has no parent")
    })?;
    let name = path.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "Destination has no filename")
    })?;
    Ok(resolved_destination(parent)?.join(name))
}

/// Copy files and directories without overwriting existing paths or following links.
pub fn duplicate_path(source: &Path, destination: &Path) -> io::Result<()> {
    if fs::symlink_metadata(destination).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", destination.display()),
        ));
    }
    let metadata = fs::symlink_metadata(source)?;
    if metadata.is_dir()
        && resolved_destination(destination)?.starts_with(fs::canonicalize(source)?)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Cannot copy a directory into itself",
        ));
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    if metadata.file_type().is_symlink() {
        let target = fs::read_link(source)?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, destination)?;
        #[cfg(windows)]
        if source.is_dir() {
            std::os::windows::fs::symlink_dir(target, destination)?;
        } else {
            std::os::windows::fs::symlink_file(target, destination)?;
        }
    } else if metadata.is_dir() {
        fs::create_dir(destination)?;
        for child in fs::read_dir(source)? {
            let child = child?;
            duplicate_path(&child.path(), &destination.join(child.file_name()))?;
        }
        fs::set_permissions(destination, metadata.permissions())?;
    } else if metadata.is_file() {
        let mut input = fs::File::open(source)?;
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)?;
        io::copy(&mut input, &mut output)?;
        fs::set_permissions(destination, metadata.permissions())?;
    } else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "This filesystem item cannot be copied",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn directories_copy_recursively_and_never_overwrite() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        fs::create_dir_all(source.join("nested")).unwrap();
        fs::write(source.join("nested/model.dart"), "class Model {}\n").unwrap();
        let destination = root.path().join("copy");
        duplicate_path(&source, &destination).unwrap();
        assert_eq!(
            fs::read(destination.join("nested/model.dart")).unwrap(),
            b"class Model {}\n"
        );
        assert_eq!(
            duplicate_path(&source, &destination).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(
            duplicate_path(&source, &source.join("child"))
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        assert!(!source.join("child").exists());
    }
    #[cfg(unix)]
    #[test]
    fn links_are_preserved_and_dangling_destinations_are_not_overwritten() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("original.dart"), "original").unwrap();
        std::os::unix::fs::symlink("original.dart", root.path().join("link.dart"))
            .unwrap();
        duplicate_path(
            &root.path().join("link.dart"),
            &root.path().join("copy.dart"),
        )
        .unwrap();
        assert_eq!(
            fs::read_link(root.path().join("copy.dart")).unwrap(),
            Path::new("original.dart")
        );
        std::os::unix::fs::symlink("missing", root.path().join("occupied")).unwrap();
        assert_eq!(
            duplicate_path(
                &root.path().join("original.dart"),
                &root.path().join("occupied")
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::AlreadyExists
        );
    }
}
