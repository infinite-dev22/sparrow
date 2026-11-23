//! Settings categories and ownership of persisted Sparrow options.

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum Page {
    Overview,
    Appearance,
    Menus,
    System,
    Project,
    Files,
    Keymap,
    Editor,
    Editing,
    Font,
    Tabs,
    Completion,
    InlayHints,
    Diagnostics,
    Plugins,
    VersionControl,
    Git,
    Run,
    RunConfigurations,
    Debugger,
    Languages,
    Dart,
    Flutter,
    Tools,
    ActionsOnSave,
    Devices,
    Logcat,
    Terminal,
}

use Page::*;

pub(super) const PAGES: &[(Page, &str, &str, Option<Page>)] = &[
    (
        Overview,
        "appearance-and-behavior",
        "Appearance & Behavior",
        None,
    ),
    (Appearance, "appearance", "Appearance", Some(Overview)),
    (Menus, "menus", "Menus and Toolbars", Some(Overview)),
    (System, "system", "System Settings", Some(Overview)),
    (Project, "project", "Project", Some(System)),
    (Files, "files", "Files", Some(System)),
    (Keymap, "keymap", "Keymap", None),
    (Editor, "editor", "Editor", None),
    (Editing, "editing", "Code Editing", Some(Editor)),
    (Font, "font", "Font and Line Height", Some(Editor)),
    (Tabs, "editor-tabs", "Tabs and Breadcrumbs", Some(Editor)),
    (Completion, "completion", "Code Completion", Some(Editor)),
    (InlayHints, "inlay-hints", "Inlay Hints", Some(Editor)),
    (Diagnostics, "diagnostics", "Diagnostics", Some(Editor)),
    (Plugins, "themes", "Plugins and Themes", None),
    (VersionControl, "version-control", "Version Control", None),
    (Git, "git", "Git", Some(VersionControl)),
    (Run, "run", "Build, Execution, Deployment", None),
    (
        RunConfigurations,
        "run-configurations",
        "Run Configurations",
        Some(Run),
    ),
    (Debugger, "debugger", "Debugger", Some(Run)),
    (Languages, "languages", "Languages & Frameworks", None),
    (Dart, "dart", "Dart", Some(Languages)),
    (Flutter, "flutter", "Flutter", Some(Languages)),
    (Tools, "tools", "Tools", None),
    (
        ActionsOnSave,
        "actions-on-save",
        "Actions on Save",
        Some(Tools),
    ),
    (Devices, "devices", "Android Devices", Some(Tools)),
    (Logcat, "logcat", "Logcat", Some(Tools)),
    (Terminal, "terminal", "Terminal", Some(Tools)),
];

impl Page {
    fn spec(self) -> &'static (Page, &'static str, &'static str, Option<Page>) {
        PAGES.iter().find(|(page, ..)| *page == self).unwrap()
    }

    pub(super) fn title(self) -> &'static str {
        self.spec().2
    }
    pub(super) fn id(self) -> &'static str {
        self.spec().1
    }
    pub(super) fn parent(self) -> Option<Page> {
        self.spec().3
    }

    pub(super) fn ancestors(self) -> impl Iterator<Item = Page> {
        std::iter::successors(self.parent(), |page| page.parent())
    }

    pub(super) fn children(self) -> impl Iterator<Item = Page> {
        PAGES.iter().filter_map(move |&(child, _, _, parent)| {
            (parent == Some(self)).then_some(child)
        })
    }

    pub(super) fn search_terms(self) -> &'static str {
        match self {
            Menus => "menus and toolbars quick actions",
            Keymap => "keymap keyboard shortcuts key bindings",
            Git => "git commit history repository",
            RunConfigurations => "run debug launch configurations",
            Devices => "android devices device manager running devices",
            Logcat => "logcat android device logs",
            _ => self.title(),
        }
    }

    pub(super) fn summary(self) -> Option<&'static str> {
        match self {
            Overview => {
                Some("Configure the workbench, its menus, and project behavior.")
            }
            System => {
                Some("Configure project browsing and how Sparrow saves files.")
            }
            Editor => {
                Some("Configure editing, typography, completion, and diagnostics.")
            }
            VersionControl => {
                Some("Configure the diff editor and open repository tools.")
            }
            Run => Some("Set up launch configurations and debugger behavior."),
            Languages => {
                Some("Configure the Dart and Flutter SDKs used by this workspace.")
            }
            Tools => Some(
                "Configure save actions and the terminal, or open Android tools.",
            ),
            _ => None,
        }
    }
}

/// Each setting has one owner, independent of which scope is being edited.
pub(super) fn owner(kind: &str, field: &str) -> Page {
    match (kind, field) {
        ("core", "dart-sdk-path") => Dart,
        ("core", "flutter-sdk-path") => Flutter,
        ("core", "color-theme" | "icon-theme" | "custom-titlebar") => Appearance,
        ("core", "file-explorer-double-click") => Project,
        ("core", "modal") => Editing,
        ("core", _) => System,
        ("ui", "quick-actions") => Menus,
        ("ui", "tab-min-width" | "tab-close-button" | "tab-separator-height") => {
            Tabs
        }
        ("ui", "open-editors-visible") => Project,
        ("ui", _) => Appearance,
        ("editor", "double-click" | "files-exclude") => Project,
        ("editor", "autosave-interval" | "normalize-line-endings") => Files,
        ("editor", "format-on-save" | "format-on-autosave") => ActionsOnSave,
        ("editor", "diff-context-lines") => Git,
        (
            "editor",
            "font-family" | "font-size" | "line-height" | "code-glance-font-size",
        ) => Font,
        ("editor", "show-tab" | "show-bread-crumbs") => Tabs,
        ("editor", field) if field.contains("inlay-hint") => InlayHints,
        ("editor", field) if field.contains("completion") => Completion,
        ("editor", field)
            if field.contains("error-lens")
                || field == "only-render-error-styling" =>
        {
            Diagnostics
        }
        ("editor", _) => Editing,
        ("terminal", _) => Terminal,
        ("debugger", _) => Debugger,
        _ => Plugins,
    }
}

pub(super) fn section(page: Page, field: &str) -> &'static str {
    match page {
        Appearance => match field {
            "color-theme" | "icon-theme" => "Themes",
            "scale" | "font-family" | "font-size" | "icon-size" | "scroll-width" => {
                "Scale and Typography"
            }
            "hover-font-family"
            | "hover-font-size"
            | "trim-search-results-whitespace"
            | "palette-width" => "Search and Popups",
            _ => "Workbench",
        },
        Editing => match field {
            "smart-tab" | "tab-width" | "atomic-soft-tabs" | "show-indent-guide" => {
                "Indentation"
            }
            "modal"
            | "modal-mode-relative-line-numbers"
            | "multicursor-case-sensitive"
            | "multicursor-whole-words" => "Editing Behavior",
            _ => "Display",
        },
        Terminal if field.starts_with("font-") || field == "line-height" => {
            "Typography"
        }
        Terminal => "Profiles",
        _ => "",
    }
}
