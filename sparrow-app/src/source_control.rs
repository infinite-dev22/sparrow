use sparrow_core::buffer::rope_text::RopeText;
use std::{path::PathBuf, rc::Rc};

use floem::{
    keyboard::Modifiers,
    reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith},
};
use indexmap::IndexMap;
use sparrow_core::mode::Mode;
use sparrow_rpc::source_control::FileDiff;

use crate::{
    command::{CommandExecuted, CommandKind},
    editor::EditorData,
    keypress::{KeyPressFocus, condition::Condition},
    main_split::Editors,
    window_tab::CommonData,
};

#[derive(Clone, Debug)]
pub struct SourceControlData {
    // VCS modified files & whether they should be included in the next commit
    pub amend: RwSignal<bool>,
    pub busy: RwSignal<bool>,
    pub status: RwSignal<String>,
    pub untracked: RwSignal<std::collections::HashSet<PathBuf>>,
    pub file_diffs: RwSignal<IndexMap<PathBuf, (FileDiff, bool)>>,
    pub branch: RwSignal<String>,
    pub branches: RwSignal<im::Vector<String>>,
    pub tags: RwSignal<im::Vector<String>>,
    pub editor: EditorData,
    pub common: Rc<CommonData>,
}

impl KeyPressFocus for SourceControlData {
    fn get_mode(&self) -> Mode {
        Mode::Insert
    }

    fn check_condition(&self, condition: Condition) -> bool {
        matches!(
            condition,
            Condition::PanelFocus | Condition::SourceControlFocus
        )
    }

    fn run_command(
        &self,
        command: &crate::command::SparrowCommand,
        count: Option<usize>,
        mods: Modifiers,
    ) -> CommandExecuted {
        match &command.kind {
            CommandKind::Edit(_)
            | CommandKind::Move(_)
            | CommandKind::MultiSelection(_) => {
                self.editor.run_command(command, count, mods)
            }
            _ => CommandExecuted::No,
        }
    }

    fn receive_char(&self, c: &str) {
        self.editor.receive_char(c);
    }
}

impl SourceControlData {
    pub fn new(cx: Scope, editors: Editors, common: Rc<CommonData>) -> Self {
        Self {
            amend: cx.create_rw_signal(false),
            busy: cx.create_rw_signal(false),
            status: cx.create_rw_signal(String::new()),
            untracked: cx.create_rw_signal(Default::default()),
            file_diffs: cx.create_rw_signal(IndexMap::new()),
            branch: cx.create_rw_signal("".to_string()),
            branches: cx.create_rw_signal(im::Vector::new()),
            tags: cx.create_rw_signal(im::Vector::new()),
            editor: editors.make_local(cx, common.clone()),
            common,
        }
    }

    pub fn refresh(&self) {
        self.common
            .proxy
            .notification(sparrow_rpc::proxy::ProxyNotification::GitRefresh {});
    }
    pub fn commit(&self) {
        self.commit_with_options(false);
    }
    pub fn set_amend(&self, value: bool) {
        self.amend.set(value);
        if !value || !self.editor.doc().buffer.with_untracked(|b| b.is_empty()) {
            return;
        }
        let Some(root) = self.common.workspace.path.clone() else {
            return;
        };
        let editor = self.editor.clone();
        let amend = self.amend;
        let send = floem::ext_event::create_ext_action(
            self.common.scope,
            move |message: Option<String>| {
                if amend.get_untracked()
                    && editor.doc().buffer.with_untracked(|b| b.is_empty())
                {
                    if let Some(message) = message {
                        editor
                            .doc()
                            .reload(lapce_xi_rope::Rope::from(message), false);
                    }
                }
            },
        );
        std::thread::spawn(move || {
            send(
                std::process::Command::new("git")
                    .args(["log", "-1", "--format=%B"])
                    .current_dir(root)
                    .output()
                    .ok()
                    .filter(|out| out.status.success())
                    .map(|out| {
                        String::from_utf8_lossy(&out.stdout).trim().to_string()
                    }),
            )
        });
    }
    pub fn commit_with_options(&self, push: bool) {
        if self.busy.get_untracked() {
            return;
        }
        let Some(root) = self
            .common
            .workspace
            .path
            .clone()
            .filter(|_| self.common.workspace.kind.is_local())
        else {
            self.status.set("Open a local Git project first.".into());
            return;
        };
        let diffs = self.file_diffs.with_untracked(|items| {
            items
                .values()
                .filter(|(_, checked)| *checked)
                .map(|(diff, _)| diff.clone())
                .collect::<Vec<_>>()
        });
        let message = self.editor.doc().buffer.with_untracked(|b| b.to_string());
        let amend = self.amend.get_untracked();
        if message.trim().is_empty() {
            self.status.set("Enter a commit message.".into());
            return;
        }
        if diffs.is_empty() && !amend {
            self.status.set("Select changes to commit.".into());
            return;
        }
        self.busy.set(true);
        self.status.set("Committing…".into());
        let data = self.clone();
        let send = floem::ext_event::create_ext_action(
            self.common.scope,
            move |result: Result<String, String>| {
                data.busy.set(false);
                data.refresh();
                match result {
                    Ok(message) => {
                        data.editor.reset();
                        data.amend.set(false);
                        data.status.set(message);
                    }
                    Err(error) => data.status.set(error),
                }
            },
        );
        let task = self.common.background_tasks.begin(if push { "Committing and pushing changes" } else { "Committing changes" }, None);
        std::thread::spawn(move || {
            let _task = task;
            send(sparrow_proxy::dispatch::commit_selected(
                &root, &message, diffs, amend, push,
            ))
        });
    }
}
