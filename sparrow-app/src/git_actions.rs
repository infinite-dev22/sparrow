//! Git command review and execution without shell interpolation.
use crate::{
    config::color::SparrowColor,
    popup_menu::{Menu, MenuItem, PopupMenuExt},
    window_tab::WindowTabData,
};
use floem::{
    View,
    event::{Event, EventListener, EventPropagation},
    keyboard::{Key, NamedKey},
    reactive::{RwSignal, Scope, SignalGet, SignalUpdate},
    views::{Decorators, container, label, scroll, stack},
};
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
        std::thread::spawn(move || {
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
pub fn view(window: Rc<WindowTabData>) -> impl View {
    let state = window.git_actions.clone();
    let active = state.active;
    crate::modal_focus::restore_workbench_on_close(active, window.common.clone());
    let title = state.title;
    let args = state.args;
    let output = state.output;
    let busy = state.busy;
    let cancel = state.cancel.clone();
    let cleanup_cancel = state.cancel.clone();
    let config = window.common.config;
    let run = state.clone();
    let w = window.clone();
    let focus = crate::modal_focus::ModalFocus::default();
    let submit = state.clone();
    let w2 = window.clone();
    focus.set_submit(move || submit.run(w2.clone()));
    let input = focus.input(args, config);
    let id = input.id();
    floem::reactive::create_effect(move |_| {
        if active.get() {
            floem::action::exec_after(
                std::time::Duration::from_millis(30),
                move |_| id.request_focus(),
            );
        }
    });
    let commands = window.common.internal_command;
    let body=stack((label(move ||format!("Git · {}",title.get())).style(|s|s.font_bold()),label(||"git").ide_popout_menu(move ||menu(commands,None)),input,label(move ||{let a=args.get();if a.contains("--hard"){"Hard reset discards tracked working-tree and staged changes."}else if a.starts_with("rebase"){"Rebase rewrites commit history. Resolve conflicts before continuing."}else{"Review the Git arguments above, then run. Output and errors appear below."}}),scroll(label(move ||output.get())).style(|s|s.width_full().height(300.0)),stack((focus.control(label(move ||if busy.get(){"Running…"}else{"Run"}).on_click_stop(move |_|run.run(w.clone())).disabled(move ||busy.get())),focus.control(label(||"Copy output").on_click_stop(move |_|crate::menus::copy_text(&output.get_untracked()))),focus.control(label(move ||if busy.get(){"Cancel command"}else{"Close"}).on_click_stop(move |_|{if busy.get_untracked(){cancel.store(true,Ordering::Relaxed);}else{active.set(false);}})))).style(|s|s.gap(20.0)))).style(move |s|s.flex_col().width(720.0).max_width_pct(95.0).gap(12.0).padding(18.0).border_radius(6.0).border(1.0).border_color(config.get().color(SparrowColor::SPARROW_BORDER)).box_shadow_blur(16.0).box_shadow_color(config.get().color(SparrowColor::SPARROW_DROPDOWN_SHADOW)).background(config.get().color(SparrowColor::PANEL_BACKGROUND)).color(config.get().color(SparrowColor::EDITOR_FOREGROUND)));
    let dialog=container(body).style(move |s|s.absolute().size_full().items_center().justify_center().z_index(100).apply_if(!active.get(),|s|s.hide())).on_event_stop(EventListener::PointerDown, |_|{}).on_event(EventListener::KeyDown,move |e|{if matches!(e,Event::KeyDown(k)if k.key.logical_key==Key::Named(NamedKey::Escape))&&!busy.get_untracked(){active.set(false);EventPropagation::Stop}else{EventPropagation::Continue}});
    focus
        .bind_root(dialog, active, config)
        .on_cleanup(move || cleanup_cancel.store(true, Ordering::Relaxed))
}
