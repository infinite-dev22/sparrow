use strum_macros::EnumIter;

use crate::command::SparrowWorkbenchCommand;

#[derive(Clone, Copy, Debug, PartialEq, Eq, EnumIter)]
pub enum PaletteKind {
    PaletteHelp,
    File,
    FileChooser,
    Line,
    Command,
    Workspace,
    Reference,
    DocumentSymbol,
    WorkspaceSymbol,
    SshHost,
    #[cfg(windows)]
    WslHost,
    RunAndDebug,
    ColorTheme,
    IconTheme,
    Language,
    LineEnding,
    SCMReferences,
    TerminalProfile,
    DiffFiles,
    HelpAndFile,
}

impl PaletteKind {
    /// The symbol/prefix that is used to signify the behavior of the palette.
    pub fn symbol(&self) -> &'static str {
        match &self {
            PaletteKind::PaletteHelp => "?",
            PaletteKind::Line => "/",
            PaletteKind::DocumentSymbol => "@",
            PaletteKind::WorkspaceSymbol => "#",
            // PaletteKind::GlobalSearch => "?",
            PaletteKind::Workspace => ">",
            PaletteKind::Command => ":",
            PaletteKind::TerminalProfile => "<",
            PaletteKind::File
            | PaletteKind::FileChooser
            | PaletteKind::Reference
            | PaletteKind::SshHost
            | PaletteKind::RunAndDebug
            | PaletteKind::ColorTheme
            | PaletteKind::IconTheme
            | PaletteKind::Language
            | PaletteKind::LineEnding
            | PaletteKind::SCMReferences
            | PaletteKind::HelpAndFile
            | PaletteKind::DiffFiles => "",
            #[cfg(windows)]
            PaletteKind::WslHost => "",
        }
    }

    /// Extract the palette kind from the input string. This is most often a prefix.
    pub fn from_input(input: &str) -> PaletteKind {
        match input {
            _ if input.starts_with('?') => PaletteKind::PaletteHelp,
            _ if input.starts_with('/') => PaletteKind::Line,
            _ if input.starts_with('@') => PaletteKind::DocumentSymbol,
            _ if input.starts_with('#') => PaletteKind::WorkspaceSymbol,
            _ if input.starts_with('>') => PaletteKind::Workspace,
            _ if input.starts_with(':') => PaletteKind::Command,
            _ if input.starts_with('<') => PaletteKind::TerminalProfile,
            _ => PaletteKind::File,
        }
    }

    /// Get the [`SparrowWorkbenchCommand`] that opens this palette kind, if one exists.
    pub fn command(self) -> Option<SparrowWorkbenchCommand> {
        match self {
            PaletteKind::PaletteHelp => Some(SparrowWorkbenchCommand::PaletteHelp),
            PaletteKind::Line => Some(SparrowWorkbenchCommand::PaletteLine),
            PaletteKind::DocumentSymbol => {
                Some(SparrowWorkbenchCommand::PaletteSymbol)
            }
            PaletteKind::WorkspaceSymbol => {
                Some(SparrowWorkbenchCommand::PaletteWorkspaceSymbol)
            }
            PaletteKind::Workspace => Some(SparrowWorkbenchCommand::PaletteWorkspace),
            PaletteKind::Command => Some(SparrowWorkbenchCommand::PaletteCommand),
            PaletteKind::File => Some(SparrowWorkbenchCommand::Palette),
            PaletteKind::FileChooser => None,
            PaletteKind::HelpAndFile => {
                Some(SparrowWorkbenchCommand::PaletteHelpAndFile)
            }
            PaletteKind::Reference => None, // InternalCommand::PaletteReferences
            PaletteKind::SshHost => Some(SparrowWorkbenchCommand::ConnectSshHost),
            #[cfg(windows)]
            PaletteKind::WslHost => Some(SparrowWorkbenchCommand::ConnectWslHost),
            PaletteKind::RunAndDebug => {
                Some(SparrowWorkbenchCommand::PaletteRunAndDebug)
            }
            PaletteKind::ColorTheme => Some(SparrowWorkbenchCommand::ChangeColorTheme),
            PaletteKind::IconTheme => Some(SparrowWorkbenchCommand::ChangeIconTheme),
            PaletteKind::Language => Some(SparrowWorkbenchCommand::ChangeFileLanguage),
            PaletteKind::LineEnding => {
                Some(SparrowWorkbenchCommand::ChangeFileLineEnding)
            }
            PaletteKind::SCMReferences => {
                Some(SparrowWorkbenchCommand::PaletteSCMReferences)
            }
            PaletteKind::TerminalProfile => None, // InternalCommand::NewTerminal
            PaletteKind::DiffFiles => Some(SparrowWorkbenchCommand::DiffFiles),
        }
    }

    // pub fn has_preview(&self) -> bool {
    //     matches!(
    //         self,
    //         PaletteType::Line
    //             | PaletteType::DocumentSymbol
    //             | PaletteType::WorkspaceSymbol
    //             | PaletteType::GlobalSearch
    //             | PaletteType::Reference
    //     )
    // }

    pub fn get_input<'a>(&self, input: &'a str) -> &'a str {
        match self {
            #[cfg(windows)]
            PaletteKind::WslHost => input,
            PaletteKind::File
            | PaletteKind::FileChooser
            | PaletteKind::Reference
            | PaletteKind::SshHost
            | PaletteKind::RunAndDebug
            | PaletteKind::ColorTheme
            | PaletteKind::IconTheme
            | PaletteKind::Language
            | PaletteKind::LineEnding
            | PaletteKind::SCMReferences | PaletteKind::HelpAndFile
            | PaletteKind::DiffFiles => input,
            PaletteKind::PaletteHelp
            | PaletteKind::Command
            | PaletteKind::Workspace
            | PaletteKind::DocumentSymbol
            | PaletteKind::WorkspaceSymbol
            | PaletteKind::Line
            | PaletteKind::TerminalProfile
            // | PaletteType::GlobalSearch
             => input.get(1..).unwrap_or(""),
        }
    }

    /// Get the palette kind that it should be considered as based on the current
    /// [`PaletteKind`] and the current input.
    pub fn get_palette_kind(&self, input: &str) -> PaletteKind {
        if self == &PaletteKind::HelpAndFile && input.is_empty() {
            return *self;
        }

        if self != &PaletteKind::File
            && self != &PaletteKind::HelpAndFile
            && self.symbol() == ""
        {
            return *self;
        }

        PaletteKind::from_input(input)
    }
}
