//! Git command review and execution without shell interpolation.
mod view;

pub use view::view;

use crate::{
    widgets::popup_menu::{Menu, MenuItem},
    window_tab::WindowTabData,
};
use floem::reactive::{RwSignal, Scope, SignalGet, SignalUpdate};
use std::{
    collections::BTreeSet,
    path::Path,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use zeroize::Zeroizing;

fn contains_embedded_http_credentials(args: &[String]) -> bool {
    args.iter().any(|argument| {
        let Some((prefix, remainder)) = argument.split_once("://") else {
            return false;
        };
        let scheme = prefix
            .rsplit(['=', ':'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !matches!(scheme.as_str(), "http" | "https") {
            return false;
        }
        let url = format!(
            "{scheme}://{}",
            remainder
                .split(['\'', '"', ',', ' '])
                .next()
                .unwrap_or_default()
        );
        url::Url::parse(&url).is_ok_and(|url| {
            !url.username().is_empty()
                || url.password().is_some()
                || url.query_pairs().any(|(key, _)| {
                    matches!(
                        key.to_ascii_lowercase().as_str(),
                        "access_token"
                            | "token"
                            | "password"
                            | "secret"
                            | "api_key"
                            | "apikey"
                    )
                })
        })
    })
}

fn sanitize_git_output(output: &str, secrets: &[Zeroizing<String>]) -> String {
    let mut output = output.to_string();
    for secret in secrets {
        output = crate::vcs_credentials::redact(&output, secret.as_str());
    }
    let output = regex::Regex::new(r"(?i)(https?://)[^/@\s]+@")
        .unwrap()
        .replace_all(&output, "$1[REDACTED]@")
        .into_owned();
    regex::Regex::new(
        r"(?i)([?&](?:access_token|token|password|secret|api_key|apikey)=)[^&\s]+",
    )
    .unwrap()
    .replace_all(&output, "$1[REDACTED]")
    .into_owned()
}

fn remote_url(root: &Path, remote: &str) -> Option<String> {
    if remote.contains("://") || remote.contains('@') {
        return Some(remote.to_string());
    }
    let output = std::process::Command::new("git")
        .args(["remote", "get-url", remote])
        .current_dir(root)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn configured_remotes(root: &Path) -> Result<Vec<String>, String> {
    let output = std::process::Command::new("git")
        .args(["remote"])
        .current_dir(root)
        .output()
        .map_err(|_| "Unable to read configured Git remotes.".to_string())?;
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_string)
        .collect())
}

fn default_remote(root: &Path) -> String {
    let branch = std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(root)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|branch| !branch.is_empty());
    if let Some(branch) = branch
        && let Ok(output) = std::process::Command::new("git")
            .args(["config", "--get", &format!("branch.{branch}.remote")])
            .current_dir(root)
            .output()
        && output.status.success()
    {
        let remote = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !remote.is_empty() {
            return remote;
        }
    }
    "origin".into()
}

fn remote_urls(root: &Path, args: &[String]) -> Result<Vec<String>, String> {
    let Some(operation) = args.first().map(String::as_str) else {
        return Ok(Vec::new());
    };
    if operation == "clone" {
        return Ok(args
            .iter()
            .skip(1)
            .find(|arg| {
                arg.starts_with("https://")
                    || arg.starts_with("http://")
                    || arg.starts_with("ssh://")
                    || (arg.contains('@') && !arg.starts_with('-'))
            })
            .cloned()
            .into_iter()
            .collect());
    }
    if !matches!(operation, "fetch" | "pull" | "push") {
        return Ok(Vec::new());
    }
    if operation == "fetch" && args.iter().any(|arg| arg == "--all") {
        return Ok(configured_remotes(root)?
            .iter()
            .filter_map(|remote| remote_url(root, remote))
            .collect());
    }
    let remotes = configured_remotes(root)?;
    if let Some(remote) = args.iter().skip(1).find(|arg| remotes.contains(arg)) {
        return Ok(remote_url(root, remote).into_iter().collect());
    }
    if let Some(remote) = args.iter().skip(1).find(|arg| {
        arg.starts_with("https://")
            || arg.starts_with("http://")
            || arg.starts_with("ssh://")
            || (arg.contains('@') && !arg.starts_with('-'))
    }) {
        return Ok(Some(remote.clone()).into_iter().collect());
    }
    Ok(remote_url(root, &default_remote(root))
        .into_iter()
        .collect())
}

fn configure_git_auth(
    command: &mut std::process::Command,
    root: &Path,
    args: &[String],
) -> Result<(Vec<Zeroizing<String>>, bool), String> {
    let mut hosts = BTreeSet::new();
    let mut ssh_remote = false;
    for remote in remote_urls(root, args)? {
        if remote.starts_with("http://") {
            continue;
        }
        match crate::vcs_credentials::remote_host(&remote) {
            Ok(crate::vcs_credentials::RemoteHost::Https(host)) => {
                hosts.insert(host);
            }
            Ok(crate::vcs_credentials::RemoteHost::Ssh(_)) => ssh_remote = true,
            Err(error) if remote.starts_with("https://") => return Err(error),
            Err(_) => {}
        }
    }

    let mut secrets = Vec::new();
    let mut session_accounts = Vec::new();
    for host in hosts {
        let Some(account) = crate::vcs_credentials::active_account(&host)? else {
            continue;
        };
        let secret = crate::vcs_credentials::token(&account)?;
        if account.storage == crate::vcs_credentials::StorageKind::SessionOnly {
            session_accounts.push((
                host,
                account.username.clone(),
                Zeroizing::new(secret.to_string()),
            ));
        }
        secrets.push(secret);
    }
    if secrets.is_empty() {
        return Ok((secrets, ssh_remote));
    }

    let askpass = std::env::current_exe().map_err(|_| {
        "Unable to locate Sparrow's Git credential helper.".to_string()
    })?;
    command
        .arg("-c")
        .arg("credential.helper=")
        .env("GIT_ASKPASS", askpass)
        .env("GIT_ASKPASS_REQUIRE", "force")
        .env("SPARROW_GIT_ASKPASS_MODE", "1")
        .env(
            "SPARROW_GIT_SESSION_COUNT",
            session_accounts.len().to_string(),
        );
    for (index, (host, username, token)) in session_accounts.into_iter().enumerate()
    {
        let suffix = index.to_string();
        command
            .env(format!("SPARROW_GIT_SESSION_HOST_{suffix}"), host)
            .env(format!("SPARROW_GIT_SESSION_USERNAME_{suffix}"), username)
            .env(
                format!("SPARROW_GIT_SESSION_TOKEN_{suffix}"),
                token.as_str(),
            );
    }
    Ok((secrets, ssh_remote))
}

#[derive(Clone)]
pub struct State {
    pub active: RwSignal<bool>,
    title: RwSignal<String>,
    cloning: RwSignal<bool>,
    args: RwSignal<String>,
    output: RwSignal<String>,
    busy: RwSignal<bool>,
    cancel: Arc<AtomicBool>,
}
impl State {
    pub fn cancel_command(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn is_busy(&self) -> bool {
        self.busy.get_untracked()
    }

    pub fn new(cx: Scope) -> Self {
        Self {
            active: cx.create_rw_signal(false),
            title: cx.create_rw_signal(String::new()),
            cloning: cx.create_rw_signal(false),
            args: cx.create_rw_signal(String::new()),
            output: cx.create_rw_signal(String::new()),
            busy: cx.create_rw_signal(false),
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }
    pub fn open_clone(&self) {
        if !self.busy.get_untracked() {
            self.open("Clone Repository".into(), vec!["clone".into()]);
            self.cloning.set(true);
            self.output.set("Add a repository URL and an optional destination after clone. Use an absolute path to choose the destination.".into());
        }
    }

    pub fn open(&self, title: String, args: Vec<String>) {
        if self.busy.get_untracked() {
            return;
        }
        self.cloning.set(false);
        self.title.set(title);
        self.args.set(crate::dart::run_config::join_args(&args));
        self.output.set(String::new());
        self.active.set(true);
    }
    fn run(&self, window: Rc<WindowTabData>) {
        if self.busy.get_untracked() {
            return;
        }
        let args =
            match crate::dart::run_config::split_args(&self.args.get_untracked()) {
                Ok(args) if !args.is_empty() => args,
                Ok(_) => return,
                Err(e) => {
                    self.output.set(e);
                    return;
                }
            };
        let root = if self.cloning.get_untracked() {
            window
                .workspace
                .path
                .as_deref()
                .filter(|_| window.workspace.kind.is_local())
                .and_then(std::path::Path::parent)
                .map(std::path::Path::to_path_buf)
                .or_else(sparrow_core::directory::Directory::home_dir)
        } else {
            window.workspace.path.clone()
        };
        let Some(root) = root else {
            self.output.set("No working directory is available.".into());
            return;
        };
        self.busy.set(true);
        self.cancel.store(false, Ordering::Relaxed);
        let cancel = self.cancel.clone();
        let state = self.clone();
        let notices = window.notifications;
        let title = self.title.get_untracked();
        let send = floem::ext_event::create_ext_action(
            window.scope,
            move |result: Result<String, String>| {
                state.busy.set(false);
                let output = result.unwrap_or_else(|e| e);
                state.output.set(output.clone());
                crate::panel::workflow_tools::record(
                    notices,
                    &format!("Git · {title}"),
                    &output,
                );
            },
        );
        let task = window.common.background_tasks.begin(
            format!("Git · {}", self.title.get_untracked()),
            Some(cancel.clone()),
        );
        std::thread::spawn(move || {
            let _task = task;
            let result = (|| -> Result<String, String> {
                use std::io::{Read, Seek};
                if contains_embedded_http_credentials(&args) {
                    return Err(
                        "Do not put credentials in Git URLs or arguments. Add the account in Settings > Version Control > Accounts.".into(),
                    );
                }
                let mut log = tempfile::tempfile().map_err(|e| e.to_string())?;
                let mut command = std::process::Command::new("git");
                let (secrets, ssh_remote) =
                    configure_git_auth(&mut command, &root, &args)?;
                let mut child = command
                    .args(&args)
                    .current_dir(root)
                    .env("GIT_TERMINAL_PROMPT", "0")
                    .env("GIT_EDITOR", "true")
                    .stdin(std::process::Stdio::null())
                    .stdout(log.try_clone().map_err(|e| e.to_string())?)
                    .stderr(log.try_clone().map_err(|e| e.to_string())?)
                    .spawn()
                    .map_err(|e| e.to_string())?;
                let status = loop {
                    if cancel.load(Ordering::Relaxed) {
                        let _ = child.kill();
                        break child.wait().map_err(|e| e.to_string())?;
                    }
                    if let Some(status) =
                        child.try_wait().map_err(|e| e.to_string())?
                    {
                        break status;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                };
                log.rewind().map_err(|e| e.to_string())?;
                let mut bytes = Vec::new();
                log.take(8_000_000)
                    .read_to_end(&mut bytes)
                    .map_err(|e| e.to_string())?;
                let output =
                    sanitize_git_output(&String::from_utf8_lossy(&bytes), &secrets);
                Ok(format!(
                    "{}\n{}{}{}",
                    if cancel.load(Ordering::Relaxed) {
                        "Cancelled".into()
                    } else if status.success() {
                        "Completed".into()
                    } else {
                        format!("Git exited with {status}")
                    },
                    output,
                    if bytes.len() == 8_000_000 {
                        "\nOutput truncated at 8 MB."
                    } else {
                        ""
                    },
                    if ssh_remote {
                        "\nSSH remote detected: Sparrow tokens are not used; use your configured SSH agent or switch the remote to HTTPS."
                    } else {
                        ""
                    }
                ))
            })();
            send(result);
        });
    }
}
/// Every operation opens a reviewable command; execution happens only on Run.
pub fn menu(
    commands: crate::listener::Listener<crate::command::InternalCommand>,
    revision: Option<String>,
) -> Menu {
    let hash = revision.unwrap_or_else(|| "HEAD".into());
    let mut menu = Menu::new("Git Actions");
    for (title, args) in [
        ("Status", vec!["status", "--short", "--branch"]),
        ("Fetch all remotes", vec!["fetch", "--all", "--prune"]),
        ("Pull (fast-forward only)", vec!["pull", "--ff-only"]),
        ("Push", vec!["push"]),
        ("Reflog", vec!["reflog", "-50"]),
        ("Cherry-pick commit", vec!["cherry-pick", &hash]),
        ("Revert commit", vec!["revert", "--no-edit", &hash]),
        (
            "Checkout revision (detached HEAD)",
            vec!["switch", "--detach", &hash],
        ),
        ("Compare with working tree", vec!["diff", &hash]),
        (
            "Show commit and patch",
            vec!["show", "--stat", "--patch", &hash],
        ),
        (
            "Create patch",
            vec!["format-patch", "-1", "--stdout", &hash],
        ),
        ("New branch here…", vec!["branch", "new-branch", &hash]),
        ("New tag here…", vec!["tag", "new-tag", &hash]),
        ("Reset: keep changes staged", vec!["reset", "--soft", &hash]),
        (
            "Reset: keep changes unstaged",
            vec!["reset", "--mixed", &hash],
        ),
        (
            "Reset: discard tracked changes…",
            vec!["reset", "--hard", &hash],
        ),
        ("Merge revision", vec!["merge", "--no-edit", &hash]),
        ("Rebase onto revision", vec!["rebase", &hash]),
        ("Continue rebase", vec!["rebase", "--continue"]),
        ("Abort rebase", vec!["rebase", "--abort"]),
        ("Continue cherry-pick", vec!["cherry-pick", "--continue"]),
        ("Abort cherry-pick", vec!["cherry-pick", "--abort"]),
        ("Abort merge", vec!["merge", "--abort"]),
        (
            "Stash changes",
            vec!["stash", "push", "--include-untracked"],
        ),
        ("List stashes", vec!["stash", "list"]),
        ("Apply latest stash", vec!["stash", "apply"]),
        ("Pop latest stash", vec!["stash", "pop"]),
        ("List worktrees", vec!["worktree", "list"]),
        ("List remotes", vec!["remote", "-v"]),
        (
            "Update submodules",
            vec!["submodule", "update", "--init", "--recursive"],
        ),
        ("Preview untracked cleanup", vec!["clean", "-nd"]),
        ("Verify repository", vec!["fsck"]),
        ("Custom Git command…", vec!["status"]),
    ] {
        let title = title.to_string();
        let argv = args.into_iter().map(str::to_string).collect::<Vec<_>>();
        menu = menu.entry(MenuItem::new(title.clone()).action(move || {
            commands.send(crate::command::InternalCommand::GitAction {
                title: title.clone(),
                args: argv.clone(),
            })
        }));
    }
    menu
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_bearing_http_urls_are_rejected_but_ssh_is_allowed() {
        assert!(contains_embedded_http_credentials(&[
            "clone".into(),
            "https://user:token@github.com/owner/repo.git".into(),
        ]));
        assert!(contains_embedded_http_credentials(&[
            "remote.origin.url=https://github.com/repo?access_token=secret".into(),
        ]));
        assert!(!contains_embedded_http_credentials(&[
            "clone".into(),
            "git@github.com:owner/repo.git".into(),
        ]));
    }

    #[test]
    fn git_output_redacts_url_credentials_and_saved_tokens() {
        let secrets = vec![Zeroizing::new("saved-secret".to_string())];
        let sanitized = sanitize_git_output(
            "https://user:password@github.com/repo and saved-secret and https://github.com/repo?token=other-secret",
            &secrets,
        );
        assert_eq!(
            sanitized,
            "https://[REDACTED]@github.com/repo and [REDACTED] and https://github.com/repo?token=[REDACTED]"
        );
        assert!(!sanitized.contains("password"));
        assert!(!sanitized.contains("other-secret"));
    }

    #[test]
    fn git_remote_selection_handles_fetch_all_and_option_values() {
        let root = tempfile::tempdir().unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q"])
                .current_dir(root.path())
                .status()
                .unwrap()
                .success()
        );
        for (name, url) in [
            ("origin", "https://github.com/owner/main.git"),
            ("upstream", "https://github.com/other/project.git"),
        ] {
            assert!(
                std::process::Command::new("git")
                    .args(["remote", "add", name, url])
                    .current_dir(root.path())
                    .status()
                    .unwrap()
                    .success()
            );
        }

        let fetch_all = vec!["fetch".into(), "--all".into(), "--prune".into()];
        let all_urls = remote_urls(root.path(), &fetch_all).unwrap();
        assert_eq!(all_urls.len(), 2);
        assert!(all_urls.contains(&"https://github.com/owner/main.git".into()));
        assert!(all_urls.contains(&"https://github.com/other/project.git".into()));

        let explicit = vec![
            "fetch".into(),
            "--depth".into(),
            "1".into(),
            "upstream".into(),
        ];
        assert_eq!(
            remote_urls(root.path(), &explicit).unwrap(),
            vec!["https://github.com/other/project.git"]
        );
    }
}
