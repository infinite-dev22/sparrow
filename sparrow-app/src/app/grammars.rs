//! Upstream syntax assets, limited to Dart and Flutter project-file grammars.
use std::{
    env,
    fs::{self},
    path::PathBuf,
};

use anyhow::{Context, Result, anyhow};
use sparrow_core::directory::Directory;

use crate::{tracing::*, update::ReleaseInfo};

fn get_github_api(url: &str) -> Result<String> {
    let user_agent = format!("sparrow/{}", sparrow_core::meta::VERSION);
    let resp = sparrow_proxy::get_url(url, Some(user_agent.as_str()))?;
    if !resp.status().is_success() {
        return Err(anyhow!("get release info failed {}", resp.text()?));
    }

    Ok(resp.text()?)
}

pub fn find_grammar_release() -> Result<ReleaseInfo> {
    let releases: Vec<ReleaseInfo> = serde_json::from_str(&get_github_api(
        "https://api.github.com/repos/lapce/tree-sitter-grammars/releases?per_page=100",
    ).context("Failed to retrieve releases for tree-sitter-grammars")?)?;

    use sparrow_core::meta::{RELEASE, ReleaseType, VERSION};

    let releases = releases
        .into_iter()
        .filter_map(|f| {
            if matches!(RELEASE, ReleaseType::Debug | ReleaseType::Nightly) {
                return Some(f);
            }

            let tag_name = if f.tag_name.starts_with('v') {
                f.tag_name.trim_start_matches('v')
            } else {
                f.tag_name.as_str()
            };

            use std::cmp::Ordering;

            use semver::Version;

            let sv = Version::parse(tag_name).ok()?;
            let version = Version::parse(VERSION).ok()?;

            if matches!(sv.cmp_precedence(&version), Ordering::Equal) {
                Some(f)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    let Some(release) = releases.first() else {
        return Err(anyhow!("Couldn't find any release"));
    };

    Ok(release.to_owned())
}

pub fn fetch_grammars(release: &ReleaseInfo) -> Result<bool> {
    let dir = Directory::grammars_directory()
        .ok_or_else(|| anyhow!("can't get grammars directory"))?;

    let file_name = format!("grammars-{}-{}", env::consts::OS, env::consts::ARCH);

    let updated = download_release(dir, release, &file_name)?;

    trace!(TraceLevel::INFO, "Successfully downloaded grammars");

    Ok(updated)
}

pub fn fetch_queries(release: &ReleaseInfo) -> Result<bool> {
    let dir = Directory::queries_directory()
        .ok_or_else(|| anyhow!("can't get queries directory"))?;

    let file_name = "queries";

    let updated = download_release(dir, release, file_name)?;

    trace!(TraceLevel::INFO, "Successfully downloaded queries");

    Ok(updated)
}

fn download_release(
    dir: PathBuf,
    release: &ReleaseInfo,
    file_name: &str,
) -> Result<bool> {
    if !dir.exists() {
        fs::create_dir(&dir)?;
    }

    let current_version =
        fs::read_to_string(dir.join("version")).unwrap_or_default();
    let release_version = if release.tag_name == "nightly" {
        format!("nightly-{}", &release.target_commitish[..7])
    } else {
        release.tag_name.clone()
    };

    if release_version == current_version {
        return Ok(false);
    }

    for asset in &release.assets {
        if asset.name.starts_with(file_name) {
            let mut resp =
                sparrow_proxy::get_url(&asset.browser_download_url, None)?;
            if !resp.status().is_success() {
                return Err(anyhow!("download file error {}", resp.text()?));
            }

            let file = tempfile::tempfile()?;

            {
                use std::io::{Seek, Write};
                let file = &mut &file;
                resp.copy_to(file)?;
                file.flush()?;
                file.rewind()?;
            }

            if asset.name.ends_with(".zip") {
                let mut archive = zip::ZipArchive::new(file)?;
                for i in 0..archive.len() {
                    let mut entry = archive.by_index(i)?;
                    let Some(path) = entry.enclosed_name().map(PathBuf::from) else {
                        continue;
                    };
                    if !supported_asset(&path) {
                        continue;
                    }
                    let target = dir.join(&path);
                    if entry.is_dir() {
                        fs::create_dir_all(target)?;
                    } else {
                        if let Some(parent) = target.parent() {
                            fs::create_dir_all(parent)?;
                        }
                        std::io::copy(&mut entry, &mut fs::File::create(target)?)?;
                    }
                }
            } else if asset.name.ends_with(".tar.zst") {
                let mut archive =
                    tar::Archive::new(zstd::stream::read::Decoder::new(file)?);
                for entry in archive.entries()? {
                    let mut entry = entry?;
                    let kind = entry.header().entry_type();
                    if (kind.is_file() || kind.is_dir())
                        && supported_asset(&entry.path()?)
                    {
                        entry.unpack_in(&dir)?;
                    }
                }
            }

            fs::write(dir.join("version"), &release_version)?;
        }
    }
    Ok(true)
}

// Preserve upstream notices while excluding unrelated grammar/query assets.
fn supported_asset(path: &std::path::Path) -> bool {
    let names = sparrow_core::language::SparrowLanguage::syntax_asset_names();
    if path.file_name().and_then(|s| s.to_str()).is_some_and(|s| {
        s.starts_with("LICENSE")
            || s.starts_with("COPYING")
            || s.starts_with("NOTICE")
    }) {
        return true;
    }
    path.components().any(|c| {
        let Some(name) = c.as_os_str().to_str() else {
            return false;
        };
        names.contains(name)
            || names.iter().any(|n| {
                name.starts_with(&format!("libtree-sitter-{n}."))
                    || name.starts_with(&format!("tree-sitter-{n}."))
            })
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_project_syntax_and_notices_are_installed() {
        assert!(supported_asset(std::path::Path::new(
            "queries/dart/highlights.scm"
        )));
        assert!(supported_asset(std::path::Path::new(
            "libtree-sitter-yaml.so"
        )));
        assert!(supported_asset(std::path::Path::new("LICENSE")));
        assert!(!supported_asset(std::path::Path::new(
            "queries/python/highlights.scm"
        )));
        assert!(!supported_asset(std::path::Path::new(
            "libtree-sitter-rust.so"
        )));
    }
}
