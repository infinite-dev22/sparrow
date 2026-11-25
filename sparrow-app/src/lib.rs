pub mod about;
pub mod ai_commit;
pub mod app;
pub mod code_action;
pub mod code_lens;
pub mod command;
pub mod completion;
pub mod config;
pub mod db;
pub mod debug;
mod debug_settings;
mod debug_evaluate;
pub mod doc;
pub mod editor;
pub mod editor_tab;
pub mod file_explorer;
pub mod find;
pub mod global_search;
pub mod history;
pub mod hover;
pub mod id;
pub mod inline_completion;
pub mod keymap;
pub mod keypress;
pub mod listener;
pub mod lsp;
pub mod main_split;
pub mod markdown;
pub mod menus;
pub mod palette;
pub mod panel;
pub mod plugin;
pub mod proxy;
pub mod rename;
pub mod workspace_undo;
pub(crate) mod project_appearance;
pub mod settings;
pub mod snippet;
pub mod source_control;
pub mod status;
pub mod terminal;
pub mod title;
pub mod tracing;
pub mod update;
pub mod window;
pub mod window_tab;
pub mod workspace;
pub mod widget_kind;
pub mod widget_tree;
pub mod dart;
pub mod outline;

#[cfg(windows)]
extern crate windows_sys as windows;

mod persistence;

pub mod code_action_preview;


pub mod project_session;

pub mod markdown_preview;

pub mod file_refactor;

pub mod scaffold;

pub mod formatting;

pub mod project_search;
pub mod search_everywhere;

pub mod git_actions;
pub mod vcs_credentials;

pub mod background_tasks;

mod ui_test_guard;
pub mod widgets;

// Keep the original module paths available to existing callers.
pub use widgets::{
    alert, divider, dropdown, focus_text, modal_focus, popup_menu, text_area,
    text_input, wave, web_link,
};
