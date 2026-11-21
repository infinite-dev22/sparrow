//! Git command review and execution without shell interpolation.
mod view;

pub use view::view;

use crate::{
    widgets::popup_menu::{Menu, MenuItem},
    window_tab::WindowTabData,
};
use floem::reactive::{RwSignal, Scope, SignalGet, SignalUpdate};
use std::{
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
#[derive(Clone)]
pub struct State {
    pub active: RwSignal<bool>,
    title: RwSignal<String>,
    args: RwSignal<String>,
    output: RwSignal<String>,
    busy: RwSignal<bool>,
    cancel: Arc<AtomicBool>,
}
impl State {
    pub fn cancel_command(&self){self.cancel.store(true,Ordering::Relaxed);}

    pub fn is_busy(&self)->bool{self.busy.get_untracked()}

    pub fn new(cx: Scope) -> Self {
        Self {
            active: cx.create_rw_signal(false),
            title: cx.create_rw_signal(String::new()),
            args: cx.create_rw_signal(String::new()),
            output: cx.create_rw_signal(String::new()),
            busy: cx.create_rw_signal(false),
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }
    pub fn open(&self, title: String, args: Vec<String>) {
        if self.busy.get_untracked() {
            return;
        }
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
        let Some(root) = window.workspace.path.clone() else {
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
        let task = window.common.background_tasks.begin(format!("Git · {}", self.title.get_untracked()), Some(cancel.clone()));
        std::thread::spawn(move || {
            let _task = task;
            let result = (|| -> Result<String, String> {
                use std::io::{Read, Seek};
                let mut log = tempfile::tempfile().map_err(|e| e.to_string())?;
                let mut child = std::process::Command::new("git")
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
                Ok(format!(
                    "{}\n{}{}",
                    if cancel.load(Ordering::Relaxed) {
                        "Cancelled".into()
                    } else if status.success() {
                        "Completed".into()
                    } else {
                        format!("Git exited with {status}")
                    },
                    String::from_utf8_lossy(&bytes),
                    if bytes.len() == 8_000_000 {
                        "\nOutput truncated at 8 MB."
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
