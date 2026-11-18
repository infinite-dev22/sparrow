use std::path::PathBuf;

use lsp_types::{Range, SymbolKind};
use sparrow_core::line_ending::LineEnding;
use sparrow_rpc::dap_types::RunDebugConfig;

use crate::{
    command::{SparrowCommand, SparrowWorkbenchCommand},
    debug::RunDebugMode,
    editor::location::EditorLocation,
    workspace::{SparrowWorkspace, SshHost},
};

#[derive(Clone, Debug, PartialEq)]
pub struct PaletteItem {
    pub content: PaletteItemContent,
    pub filter_text: String,
    pub score: u32,
    pub indices: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PaletteItemContent {
    PaletteHelp {
        cmd: SparrowWorkbenchCommand,
    },
    File {
        path: PathBuf,
        full_path: PathBuf,
    },
    FileChooserEntry {
        path: PathBuf,
        full_path: PathBuf,
        is_dir: bool,
    },
    ConfirmFileChooserEntry {
        full_path: PathBuf,
    },
    Line {
        line: usize,
        content: String,
    },
    Command {
        cmd: SparrowCommand,
    },
    Workspace {
        workspace: SparrowWorkspace,
    },
    Reference {
        path: PathBuf,
        location: EditorLocation,
    },
    DocumentSymbol {
        kind: SymbolKind,
        name: String,
        range: Range,
        container_name: Option<String>,
    },
    WorkspaceSymbol {
        kind: SymbolKind,
        name: String,
        container_name: Option<String>,
        location: EditorLocation,
    },
    SshHost {
        host: SshHost,
    },
    #[cfg(windows)]
    WslHost {
        host: crate::workspace::WslHost,
    },
    RunAndDebug {
        mode: RunDebugMode,
        config: RunDebugConfig,
    },
    ColorTheme {
        name: String,
    },
    IconTheme {
        name: String,
    },
    Language {
        name: String,
    },
    LineEnding {
        kind: LineEnding,
    },
    SCMReference {
        name: String,
    },
    TerminalProfile {
        name: String,
        profile: sparrow_rpc::terminal::TerminalProfile,
    },
    PubPackage {
        name: String,
        description: String,
        version: String,
    },
}
