use std::rc::Rc;

use floem::{
    keyboard::Modifiers,
    peniko::kurbo::Rect,
    reactive::{RwSignal, Scope, SignalGet, SignalUpdate},
};
use lsp_types::CodeActionOrCommand;
use sparrow_core::{command::FocusCommand, mode::Mode, movement::Movement};
use sparrow_rpc::plugin::PluginId;

use crate::{
    command::{CommandExecuted, CommandKind, InternalCommand},
    keypress::{KeyPressFocus, condition::Condition},
    window_tab::{CommonData, Focus},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CodeActionStatus {
    Inactive,
    Active,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScoredCodeActionItem {
    pub item: CodeActionOrCommand,
    pub plugin_id: PluginId,
    pub score: i64,
    pub indices: Vec<usize>,
}

impl ScoredCodeActionItem {
    pub fn title(&self) -> &str {
        match &self.item {
            CodeActionOrCommand::Command(c) => &c.title,
            CodeActionOrCommand::CodeAction(c) => &c.title,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CodeActionData {
    pub status: RwSignal<CodeActionStatus>,
    pub active: RwSignal<usize>,
    pub request_id: usize,
    pub input_id: usize,
    pub offset: usize,
    pub items: im::Vector<ScoredCodeActionItem>,
    pub filtered_items: im::Vector<ScoredCodeActionItem>,
    pub layout_rect: Rect,
    pub mouse_click: bool,
    pub origin: Option<(std::path::PathBuf, u64)>,
    pub preview: RwSignal<crate::code_action_preview::Preview>,
    pub show_preview: RwSignal<bool>,
    pub common: Rc<CommonData>,
}

impl KeyPressFocus for CodeActionData {
    fn get_mode(&self) -> Mode {
        Mode::Insert
    }

    fn check_condition(&self, condition: Condition) -> bool {
        matches!(condition, Condition::ListFocus | Condition::ModalFocus)
    }

    fn run_command(
        &self,
        command: &crate::command::SparrowCommand,
        _count: Option<usize>,
        _mods: Modifiers,
    ) -> crate::command::CommandExecuted {
        match &command.kind {
            CommandKind::Focus(cmd) => self.run_focus_command(cmd),
            _ => CommandExecuted::No,
        }
    }

    fn receive_char(&self, _c: &str) {}
}

fn action_priority(action: &CodeActionOrCommand) -> u8 {
    match action {
        CodeActionOrCommand::CodeAction(action) if action.disabled.is_some() => 4,
        CodeActionOrCommand::CodeAction(action)
            if action.is_preferred == Some(true) =>
        {
            0
        }
        CodeActionOrCommand::CodeAction(action)
            if action
                .kind
                .as_ref()
                .is_some_and(|kind| kind.as_str().starts_with("quickfix")) =>
        {
            1
        }
        CodeActionOrCommand::CodeAction(action)
            if action
                .kind
                .as_ref()
                .is_some_and(|kind| kind.as_str().starts_with("refactor")) =>
        {
            2
        }
        _ => 3,
    }
}

impl CodeActionData {
    pub fn new(cx: Scope, common: Rc<CommonData>) -> Self {
        let status = cx.create_rw_signal(CodeActionStatus::Inactive);
        let active = cx.create_rw_signal(0);

        let code_action = Self {
            status,
            active,
            request_id: 0,
            input_id: 0,
            offset: 0,
            items: im::Vector::new(),
            filtered_items: im::Vector::new(),
            layout_rect: Rect::ZERO,
            mouse_click: false,
            origin: None,
            preview: cx.create_rw_signal(Default::default()),
            show_preview: cx.create_rw_signal(true),
            common,
        };

        {
            let code_action = code_action.clone();
            cx.create_effect(move |_| {
                let focus = code_action.common.focus.get();
                if focus != Focus::CodeAction
                    && code_action.status.get_untracked()
                        != CodeActionStatus::Inactive
                {
                    code_action.cancel();
                }
            })
        }

        code_action
    }

    pub fn next(&self) {
        let active = self.active.get_untracked();
        let new =
            Movement::Down.update_index(active, self.filtered_items.len(), 1, true);
        self.active.set(new);
    }

    pub fn previous(&self) {
        let active = self.active.get_untracked();
        let new =
            Movement::Up.update_index(active, self.filtered_items.len(), 1, true);
        self.active.set(new);
    }

    pub fn next_page(&self) {
        let config = self.common.config.get_untracked();
        let count = ((self.layout_rect.size().height
            / config.editor.line_height() as f64)
            .floor() as usize)
            .saturating_sub(1);
        let active = self.active.get_untracked();
        let new = Movement::Down.update_index(
            active,
            self.filtered_items.len(),
            count,
            false,
        );
        self.active.set(new);
    }

    pub fn previous_page(&self) {
        let config = self.common.config.get_untracked();
        let count = ((self.layout_rect.size().height
            / config.editor.line_height() as f64)
            .floor() as usize)
            .saturating_sub(1);
        let active = self.active.get_untracked();
        let new = Movement::Up.update_index(
            active,
            self.filtered_items.len(),
            count,
            false,
        );
        self.active.set(new);
    }

    pub fn show(
        &mut self,
        plugin_id: PluginId,
        code_actions: im::Vector<CodeActionOrCommand>,
        offset: usize,
        mouse_click: bool,
    ) {
        self.active.set(0);
        self.status.set(CodeActionStatus::Active);
        self.offset = offset;
        self.mouse_click = mouse_click;
        self.request_id += 1;
        let mut code_actions: Vec<_> = code_actions.into_iter().collect();
        // Keep server ordering within groups, with fixes ahead of file-wide tools.
        code_actions.sort_by_key(action_priority);
        self.items = code_actions
            .into_iter()
            .map(|code_action| ScoredCodeActionItem {
                item: code_action,
                plugin_id,
                score: 0,
                indices: Vec::new(),
            })
            .collect();
        self.filtered_items = self.items.clone();
        self.common.focus.set(Focus::CodeAction);
    }

    pub fn cancel(&self) {
        self.status.set(CodeActionStatus::Inactive);
        self.common.focus.set(Focus::Workbench);
        self.common
            .window_common
            .app_view_id
            .get_untracked()
            .request_focus();
    }

    pub fn select(&self) {
        if let crate::code_action_preview::Preview::Ready(prepared) =
            self.preview.get_untracked()
        {
            self.common
                .internal_command
                .send(InternalCommand::ApplyCodeActionPreview { prepared });
            self.cancel();
        }
    }

    fn run_focus_command(&self, cmd: &FocusCommand) -> CommandExecuted {
        match cmd {
            FocusCommand::ModalClose => {
                self.cancel();
            }
            FocusCommand::ListNext => {
                self.next();
            }
            FocusCommand::ListNextPage => {
                self.next_page();
            }
            FocusCommand::ListPrevious => {
                self.previous();
            }
            FocusCommand::ListPreviousPage => {
                self.previous_page();
            }
            FocusCommand::ListSelect => {
                self.select();
            }
            _ => return CommandExecuted::No,
        }
        CommandExecuted::Yes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixes_precede_source_tools_and_disabled_actions() {
        let action = |kind, preferred: bool, disabled: bool| {
            CodeActionOrCommand::CodeAction(lsp_types::CodeAction {
                title: String::new(),
                kind: Some(lsp_types::CodeActionKind::new(kind)),
                is_preferred: Some(preferred),
                disabled: disabled.then(|| lsp_types::CodeActionDisabled {
                    reason: "Unavailable".into(),
                }),
                ..Default::default()
            })
        };
        assert!(
            action_priority(&action("quickfix", false, false))
                < action_priority(&action("source.organizeImports", false, false))
        );
        assert!(
            action_priority(&action("quickfix", true, false))
                < action_priority(&action("quickfix", false, false))
        );
        assert!(
            action_priority(&action("quickfix", true, true))
                > action_priority(&action("source", false, false))
        );
    }
}
