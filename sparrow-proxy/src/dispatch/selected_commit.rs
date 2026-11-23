use std::{
    collections::HashSet,
    ffi::OsStr,
    path::Path,
    process::{Command, Output, Stdio},
};

#[derive(Debug)]
pub struct SelectedCommitResult {
    pub status: String,
    pub lease: Option<AmendPushLease>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AmendPushLease {
    target: PushTarget,
    expected_tip: String,
    amended_tip: String,
}

impl AmendPushLease {
    pub fn destination(&self) -> String {
        format!(
            "{}/{}",
            self.target.remote,
            self.target.destination.trim_start_matches("refs/heads/")
        )
    }

    pub fn expected_tip(&self) -> &str {
        &self.expected_tip
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PushTarget {
    branch: String,
    remote: String,
    destination: String,
    upstream: String,
    push_url: String,
}

use sparrow_rpc::source_control::FileDiff;

fn git_output(
    workspace: &Path,
    args: &[impl AsRef<OsStr>],
) -> Result<Output, String> {
    Command::new("git")
        .current_dir(workspace)
        .args(args)
        .stdin(Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0")
        // Push porcelain rejection reasons are used to classify the result.
        .env("LC_ALL", "C")
        .output()
        .map_err(|error| error.to_string())
}

fn output_error(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let rejected = stdout
        .lines()
        .filter(|line| line.starts_with("!\t"))
        .collect::<Vec<_>>();
    if !rejected.is_empty() {
        return [rejected.join("\n").as_str(), stderr.trim()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
    }
    let detail = if stderr.trim().is_empty() {
        stdout.trim()
    } else {
        stderr.trim()
    };
    if detail.is_empty() {
        format!("Git exited with {}", output.status)
    } else {
        detail.to_owned()
    }
}

fn git(workspace: &Path, args: &[impl AsRef<OsStr>]) -> Result<String, String> {
    let output = git_output(workspace, args)?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        Err(output_error(&output))
    }
}

fn head(workspace: &Path) -> Result<String, String> {
    git(workspace, &["rev-parse", "--verify", "HEAD"])
}

fn push_target(workspace: &Path) -> Option<PushTarget> {
    let branch = git(workspace, &["symbolic-ref", "--quiet", "HEAD"]).ok()?;
    let spec = git(
        workspace,
        &[
            "for-each-ref",
            "--format=%(upstream:remotename)%00%(upstream:remoteref)%00%(upstream)%00%(push:remotename)",
            &branch,
        ],
    )
    .ok()?;
    let [remote, destination, upstream, push_remote] =
        spec.split('\0').collect::<Vec<_>>().try_into().ok()?;
    // The retry is for the same single upstream branch as the usual push.
    if remote.is_empty()
        || remote == "."
        || remote != push_remote
        || !destination.starts_with("refs/heads/")
        || upstream.is_empty()
    {
        return None;
    }
    let push_url = git(
        workspace,
        &["remote", "get-url", "--push", "--all", "--", remote],
    )
    .ok()?;
    if push_url.lines().count() != 1 {
        return None;
    }
    Some(PushTarget {
        branch,
        remote: remote.to_owned(),
        destination: destination.to_owned(),
        upstream: upstream.to_owned(),
        push_url,
    })
}

fn published_tip(workspace: &Path) -> Option<(PushTarget, String)> {
    let target = push_target(workspace)?;
    let expected_tip = head(workspace).ok()?;
    let upstream =
        git(workspace, &["rev-parse", "--verify", &target.upstream]).ok()?;
    (upstream == expected_tip).then_some((target, expected_tip))
}

fn rejected_as_non_fast_forward(output: &Output, target: &PushTarget) -> bool {
    let refspec = format!("{}:{}", target.branch, target.destination);
    String::from_utf8_lossy(&output.stdout).lines().any(|line| {
        let mut fields = line.split('\t');
        fields.next() == Some("!")
            && fields.next() == Some(refspec.as_str())
            && matches!(
                fields.next(),
                Some("[rejected] (non-fast-forward)" | "[rejected] (fetch first)")
            )
    })
}

#[derive(Debug)]
pub enum AmendLeaseError {
    Stale(String),
    Other(String),
}

/// Push exactly the saved amended commit, guarded by the original published tip.
/// Fetching or refreshing the repository does not change the expected object ID.
pub fn push_amend_with_lease(
    workspace: &Path,
    lease: &AmendPushLease,
) -> Result<String, AmendLeaseError> {
    validate_amend(workspace, lease).map_err(AmendLeaseError::Other)?;
    let option = format!(
        "--force-with-lease={}:{}",
        lease.target.destination, lease.expected_tip
    );
    let refspec = format!("{}:{}", lease.amended_tip, lease.target.destination);
    let output = git_output(
        workspace,
        &[
            "push",
            "--porcelain",
            "--no-follow-tags",
            "--recurse-submodules=no",
            &option,
            "--",
            &lease.target.remote,
            &refspec,
        ],
    )
    .map_err(AmendLeaseError::Other)?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned());
    }
    let stale = String::from_utf8_lossy(&output.stdout).lines().any(|line| {
        let mut fields = line.split('\t');
        fields.next() == Some("!")
            && fields.next().is_some_and(|refs| {
                refs.ends_with(&format!(":{}", lease.target.destination))
            })
            && fields.next() == Some("[rejected] (stale info)")
    });
    let error = output_error(&output);
    Err(if stale {
        AmendLeaseError::Stale(error)
    } else {
        AmendLeaseError::Other(error)
    })
}

fn validate_amend(workspace: &Path, lease: &AmendPushLease) -> Result<(), String> {
    if push_target(workspace).as_ref() != Some(&lease.target)
        || head(workspace)? != lease.amended_tip
    {
        return Err(
            "The current branch, commit, or push destination changed.".into()
        );
    }
    Ok(())
}

/// Merge the latest published history with the local amendment and push normally.
/// If the local branch advances but the push fails, the lease records the new
/// local tip so recovery can be retried against whatever the remote now contains.
pub fn recover_amend_push(
    workspace: &Path,
    lease: &mut AmendPushLease,
) -> Result<String, String> {
    validate_amend(workspace, lease)?;
    let scratch = tempfile::Builder::new()
        .prefix("sparrow-amend-")
        .tempdir()
        .map_err(|error| error.to_string())?;
    let ref_name = format!(
        "refs/sparrow/amend-recovery/{}",
        scratch
            .path()
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
    );
    let result = (|| {
        git(
            workspace,
            &[
                "fetch",
                "--no-tags",
                "--no-recurse-submodules",
                "--no-write-fetch-head",
                "--",
                &lease.target.push_url,
                &format!("{}:{ref_name}", lease.target.destination),
            ],
        )?;
        let newer = git(workspace, &["rev-parse", "--verify", &ref_name])?;
        git(workspace, &["merge-base", "--is-ancestor", &lease.expected_tip, &newer])
            .map_err(|_| "The remote history no longer contains the original published commit. Resolve the branch history before pushing.".to_owned())?;

        let merged =
            merge_amend_in_worktree(workspace, lease, &newer, scratch.path())?;
        validate_amend(workspace, lease)?;
        ensure_unchecked_paths_preserved(workspace, &lease.amended_tip, &merged)?;
        git(workspace, &[
            "-c", "submodule.recurse=false", "merge", "--ff-only", "--no-autostash",
            "--no-overwrite-ignore", &merged,
        ]).map_err(|error| format!("Could not update the local branch. Resolve any overlapping local changes and retry.\n{error}"))?;
        lease.amended_tip = merged;
        let refspec = format!("{}:{}", lease.amended_tip, lease.target.destination);
        git(
            workspace,
            &[
                "push",
                "--porcelain",
                "--no-follow-tags",
                "--recurse-submodules=no",
                "--",
                &lease.target.remote,
                &refspec,
            ],
        )
    })();
    let _ = git(workspace, &["update-ref", "-d", &ref_name]);
    result
}

fn ensure_unchecked_paths_preserved(
    workspace: &Path,
    amended: &str,
    merged: &str,
) -> Result<(), String> {
    let changed_paths = |revisions: &[&str]| {
        let mut args = vec!["diff", "--name-only", "--no-renames", "-z"];
        args.extend_from_slice(revisions);
        args.push("--");
        let output = git_output(workspace, &args)?;
        if output.status.success() {
            Ok(output.stdout)
        } else {
            Err(output_error(&output))
        }
    };
    let incoming = changed_paths(&[amended, merged])?;
    if incoming.is_empty() {
        return Ok(());
    }
    fn names(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
        bytes
            .split(|byte| *byte == 0)
            .filter(|name| !name.is_empty())
    }
    let incoming: HashSet<_> = names(&incoming).collect();
    let staged = changed_paths(&["--cached", amended])?;
    let unstaged = changed_paths(&[])?;
    if let Some(path) = names(&staged)
        .chain(names(&unstaged))
        .find(|path| incoming.contains(path))
    {
        return Err(format!(
            "Could not update the local branch. Incoming changes overlap local changes in {}. Preserve those changes before retrying.",
            String::from_utf8_lossy(path)
        ));
    }
    Ok(())
}

fn merge_amend_in_worktree(
    workspace: &Path,
    lease: &AmendPushLease,
    newer: &str,
    scratch: &Path,
) -> Result<String, String> {
    let amended = &lease.amended_tip;
    if git(workspace, &["merge-base", "--is-ancestor", newer, amended]).is_ok() {
        return Ok(amended.clone());
    }
    if git(workspace, &["merge-base", "--is-ancestor", amended, newer]).is_ok() {
        return Ok(newer.to_owned());
    }
    // Model the amendment as a change from the published tree, including when
    // the amendment replaced the root commit. The final merge uses the real tips.
    let merge_input = if git(
        workspace,
        &["merge-base", "--is-ancestor", &lease.expected_tip, amended],
    )
    .is_ok()
    {
        amended.clone()
    } else {
        git(
            workspace,
            &[
                "commit-tree",
                &format!("{amended}^{{tree}}"),
                "-p",
                &lease.expected_tip,
                "-m",
                "Amendment changes for merge",
            ],
        )?
    };
    let clean = scratch.join("worktree");
    git(
        workspace,
        &[
            OsStr::new("worktree"),
            OsStr::new("add"),
            OsStr::new("--detach"),
            clean.as_os_str(),
            OsStr::new(&merge_input),
        ],
    )?;
    let merged = git(&clean, &[
        "-c", "submodule.recurse=false", "-c", "rerere.enabled=false",
        "merge", "--no-ff", "--no-commit", "--no-autostash",
        "--no-overwrite-ignore", "-m", "Merge remote changes after amendment", newer,
    ])
    .map_err(|error| format!("Could not merge the remote changes in the temporary worktree. Resolve the merge before pushing.\n{error}"))
    .and_then(|_| {
        let tree = git(&clean, &["write-tree"])?;
        git(&clean, &[
            "commit-tree", &tree, "-p", amended, "-p", newer,
            "-m", "Merge remote changes after amendment",
        ])
    });
    let removed = git(
        workspace,
        &[
            OsStr::new("worktree"),
            OsStr::new("remove"),
            OsStr::new("--force"),
            clean.as_os_str(),
        ],
    );
    let merged = merged?;
    removed?;
    Ok(merged)
}

/// Commit only the selected working-tree paths and preserve other staged files.
pub fn commit_selected(
    workspace: &Path,
    message: &str,
    diffs: Vec<FileDiff>,
    amend: bool,
    push: bool,
) -> Result<SelectedCommitResult, String> {
    if message.trim().is_empty() {
        return Err("Enter a commit message.".into());
    }
    let recorded_tip = if amend && push {
        published_tip(workspace)
    } else {
        None
    };
    let relative = |path: &Path| {
        path.strip_prefix(workspace)
            .map(Path::to_path_buf)
            .map_err(|_| "Selected file is outside the project".to_owned())
    };
    let mut paths = Vec::new();
    let mut added = Vec::new();
    for diff in diffs {
        let path = match diff {
            FileDiff::Added(path) => {
                added.push(relative(&path)?);
                path
            }
            FileDiff::Renamed(new, old) => {
                added.push(relative(&new)?);
                paths.push(relative(&old)?);
                new
            }
            FileDiff::Modified(path) | FileDiff::Deleted(path) => path,
        };
        paths.push(relative(&path)?);
    }
    paths.sort();
    paths.dedup();
    if paths.is_empty() && !amend {
        return Err("Select changes to commit.".into());
    }
    if !added.is_empty() {
        let mut args: Vec<_> =
            ["--literal-pathspecs", "add", "--intent-to-add", "--"]
                .into_iter()
                .map(OsStr::new)
                .collect();
        args.extend(added.iter().map(|path| path.as_os_str()));
        git(workspace, &args)?;
    }
    let mut args: Vec<_> = [
        "--literal-pathspecs",
        "commit",
        "--only",
        "--message",
        message,
    ]
    .into_iter()
    .map(OsStr::new)
    .collect();
    if amend {
        args.push(OsStr::new("--amend"));
    }
    args.push(OsStr::new("--"));
    args.extend(paths.iter().map(|path| path.as_os_str()));
    let mut result = SelectedCommitResult {
        status: git(workspace, &args)?,
        lease: None,
    };

    if push {
        let amendment = recorded_tip.and_then(|(target, expected_tip)| {
            let amended_tip = head(workspace).ok()?;
            (amended_tip != expected_tip
                && push_target(workspace).as_ref() == Some(&target))
            .then_some(AmendPushLease {
                target,
                expected_tip,
                amended_tip,
            })
        });
        match git_output(workspace, &["push", "--porcelain"]) {
            Ok(output) if output.status.success() => {
                result.status.push('\n');
                result
                    .status
                    .push_str(String::from_utf8_lossy(&output.stdout).trim());
            }
            Ok(output) => {
                result.lease = amendment.filter(|lease| {
                    rejected_as_non_fast_forward(&output, &lease.target)
                });
                result.status = format!(
                    "Commit succeeded. Push failed:\n{}",
                    output_error(&output)
                );
            }
            Err(error) => {
                result.status = format!("Commit succeeded. Push failed:\n{error}")
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod selected_commit_tests {
    use super::*;
    fn git(root: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).to_string()
    }
    #[test]
    fn selected_commit_preserves_unselected_staging_and_amend_is_real() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q"]);
        git(root, &["config", "user.name", "Probe"]);
        git(root, &["config", "user.email", "probe@example.invalid"]);
        std::fs::write(root.join("main.dart"), "first").unwrap();
        std::fs::write(root.join("other.dart"), "other").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-qm", "initial"]);
        std::fs::write(root.join("main.dart"), "changed").unwrap();
        std::fs::write(root.join("other.dart"), "staged").unwrap();
        git(root, &["add", "other.dart"]);
        commit_selected(
            root,
            "selected",
            vec![FileDiff::Modified(root.join("main.dart"))],
            false,
            false,
        )
        .unwrap();
        assert_eq!(git(root, &["show", "HEAD:other.dart"]).trim(), "other");
        assert_eq!(
            git(root, &["diff", "--cached", "--name-only"]).trim(),
            "other.dart"
        );
        std::fs::write(root.join("new.dart"), "new").unwrap();
        commit_selected(
            root,
            "amended",
            vec![FileDiff::Added(root.join("new.dart"))],
            true,
            false,
        )
        .unwrap();
        assert_eq!(git(root, &["rev-list", "--count", "HEAD"]).trim(), "2");
        assert_eq!(git(root, &["show", "HEAD:new.dart"]).trim(), "new");
        assert_eq!(git(root, &["log", "-1", "--format=%s"]).trim(), "amended");
        let error = commit_selected(root, "", vec![], false, false)
            .err()
            .unwrap();
        assert!(error.contains("message"));
    }

    fn published_repo() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf)
    {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("working");
        let remote = directory.path().join("remote.git");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&remote).unwrap();
        git(&remote, &["init", "--bare", "--initial-branch=main"]);
        git(&root, &["init", "--initial-branch=main"]);
        git(&root, &["config", "user.name", "Probe"]);
        git(&root, &["config", "user.email", "probe@example.invalid"]);
        git(
            &root,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        for name in ["selected[1].txt", "selected1.txt", "staged.txt"] {
            std::fs::write(root.join(name), "original").unwrap();
        }
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "published"]);
        git(&root, &["push", "-u", "origin", "main"]);
        (directory, root, remote)
    }

    #[test]
    fn published_amendment_can_be_pushed_with_a_lease_for_only_its_original_tip() {
        let (_directory, root, remote) = published_repo();
        let before = git(&root, &["rev-parse", "HEAD"]);
        std::fs::write(root.join("selected[1].txt"), "selected").unwrap();
        std::fs::write(root.join("selected1.txt"), "unchecked").unwrap();
        std::fs::write(root.join("staged.txt"), "staged").unwrap();
        git(&root, &["add", "staged.txt"]);
        let result = commit_selected(
            &root,
            "amended",
            vec![FileDiff::Modified(root.join("selected[1].txt"))],
            true,
            true,
        )
        .unwrap();
        assert!(result.status.contains("Push failed"), "{}", result.status);
        let lease = result.lease.unwrap();
        assert_eq!(lease.destination(), "origin/main");
        assert_eq!(lease.expected_tip(), before.trim());
        assert_eq!(git(&remote, &["rev-parse", "main"]), before);
        push_amend_with_lease(&root, &lease).unwrap();
        assert_eq!(
            git(&remote, &["rev-parse", "main"]),
            git(&root, &["rev-parse", "HEAD"])
        );
        assert_eq!(git(&remote, &["show", "main:selected[1].txt"]), "selected");
        assert_eq!(git(&remote, &["show", "main:selected1.txt"]), "original");
        assert_eq!(git(&remote, &["show", "main:staged.txt"]), "original");
        assert_eq!(git(&root, &["show", ":staged.txt"]), "staged");
        assert_eq!(
            std::fs::read_to_string(root.join("selected1.txt")).unwrap(),
            "unchecked"
        );
    }

    #[test]
    fn fetching_an_advanced_remote_does_not_update_a_pending_lease() {
        let (_directory, root, remote) = published_repo();
        std::fs::write(root.join("selected[1].txt"), "selected").unwrap();
        let result = commit_selected(
            &root,
            "amended",
            vec![FileDiff::Modified(root.join("selected[1].txt"))],
            true,
            true,
        )
        .unwrap();
        let lease = result.lease.unwrap();
        git(&root, &["switch", "-c", "another-writer", "origin/main"]);
        git(&root, &["commit", "--allow-empty", "-qm", "remote update"]);
        git(&root, &["push", "origin", "HEAD:main"]);
        let advanced = git(&remote, &["rev-parse", "main"]);
        git(&root, &["switch", "main"]);
        git(&root, &["fetch", "origin"]);
        assert_eq!(git(&root, &["rev-parse", "origin/main"]), advanced);
        assert!(matches!(
            push_amend_with_lease(&root, &lease)
                .unwrap_err(),
            AmendLeaseError::Stale(error) if error.contains("stale info")
        ));
        assert_eq!(git(&remote, &["rev-parse", "main"]), advanced);
    }

    #[test]
    fn stale_lease_recovery_preserves_both_commits_and_unchecked_changes() {
        let (_directory, root, remote) = published_repo();
        let write =
            |name, contents| std::fs::write(root.join(name), contents).unwrap();
        write("selected[1].txt", "amendment");
        let mut lease = commit_selected(
            &root,
            "amended",
            vec![FileDiff::Modified(root.join("selected[1].txt"))],
            true,
            true,
        )
        .unwrap()
        .lease
        .unwrap();
        let amended = lease.amended_tip.clone();
        git(&root, &["switch", "-c", "another-writer", "origin/main"]);
        write("remote.txt", "remote contents");
        git(&root, &["add", "remote.txt"]);
        git(&root, &["commit", "-qm", "remote update"]);
        git(&root, &["push", "origin", "HEAD:main"]);
        let newer = git(&remote, &["rev-parse", "main"]);
        git(&root, &["switch", "main"]);
        write("selected1.txt", "unchecked");
        write("staged.txt", "staged");
        git(&root, &["add", "staged.txt"]);
        write("untracked.txt", "untracked");
        write("remote.txt", "overlapping unchecked");

        assert!(matches!(
            push_amend_with_lease(&root, &lease),
            Err(AmendLeaseError::Stale(_))
        ));
        let blocked_status = git(&root, &["status", "--porcelain"]);
        let error = recover_amend_push(&root, &mut lease).unwrap_err();
        assert!(
            error.contains("Could not update the local branch"),
            "{error}"
        );
        assert_eq!(git(&root, &["status", "--porcelain"]), blocked_status);
        assert_eq!(git(&root, &["rev-parse", "HEAD"]).trim(), amended);
        assert_eq!(git(&remote, &["rev-parse", "main"]), newer);
        assert_eq!(
            std::fs::read_to_string(root.join("remote.txt")).unwrap(),
            "overlapping unchecked"
        );
        std::fs::rename(root.join("remote.txt"), root.join("saved.txt")).unwrap();
        write("remote.txt", "remote contents");
        git(&root, &["add", "remote.txt"]);
        let matching_staged = git(&root, &["diff", "--cached", "--binary"]);
        let error = recover_amend_push(&root, &mut lease).unwrap_err();
        assert!(
            error.contains("Incoming changes overlap local changes in remote.txt"),
            "{error}"
        );
        assert_eq!(
            git(&root, &["diff", "--cached", "--binary"]),
            matching_staged
        );
        assert_eq!(git(&remote, &["rev-parse", "main"]), newer);
        git(&root, &["restore", "--staged", "--", "remote.txt"]);
        std::fs::remove_file(root.join("remote.txt")).unwrap();

        let snapshot = |args| git(&root, args);
        let status = snapshot(&["status", "--porcelain"]);
        let index = snapshot(&["diff", "--cached", "--binary"]);
        let working = snapshot(&["diff", "--binary"]);
        recover_amend_push(&root, &mut lease).unwrap();
        assert_eq!(snapshot(&["status", "--porcelain"]), status);
        assert_eq!(snapshot(&["diff", "--cached", "--binary"]), index);
        assert_eq!(snapshot(&["diff", "--binary"]), working);
        assert_eq!(
            std::fs::read_to_string(root.join("untracked.txt")).unwrap(),
            "untracked"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("saved.txt")).unwrap(),
            "overlapping unchecked"
        );
        assert_eq!(git(&remote, &["rev-parse", "main^1"]).trim(), amended);
        assert_eq!(git(&remote, &["rev-parse", "main^2"]), newer);
        assert_eq!(
            git(&remote, &["show", "main:remote.txt"]),
            "remote contents"
        );
        assert_eq!(git(&remote, &["show", "main:selected[1].txt"]), "amendment");
        assert_eq!(git(&remote, &["show", "main:staged.txt"]), "original");
    }
}
