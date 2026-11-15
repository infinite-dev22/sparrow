# Settings dialog

Open Settings from the gear menu or with **Ctrl+,** (**Cmd+,** on macOS).

The dialog uses the supplied Android Studio reference: a 900×700 modal, a 256 px category sidebar with search, a main settings page, navigation history, and OK, Cancel and Apply buttons. It adapts to smaller windows with scrollable contents.

Appearance & Behavior opens an overview with links to supported settings. Appearance includes UI and theme options; System Settings covers the remaining core options. Editor, Terminal, and installed-plugin settings are generated from Sparrow's existing configuration metadata. The Appearance & Behavior tree expands when you select a subpage.

Search matches setting names and descriptions across all categories. Native theme/enum choices use dropdown menus, booleans use toggles, and numeric/string settings use editable fields. Invalid numbers are reported before saving. Changes survive category navigation, but are not written until Apply or OK. Cancel, Close and Escape discard un-applied edits. Apply saves without closing.

Saving validates the complete pending draft, preserves unrelated TOML values and comments, and atomically replaces the settings file. Malformed settings files and section conflicts are reported rather than overwritten. Platform-specific terminal profile settings retain their existing storage format.

Keymap, plugin management, and Run/Debug Configuration links open Sparrow's existing editors. Pending settings must first be applied or cancelled before leaving for one of those editors. Android Studio-only settings and services are not represented by non-functional controls.

The older Settings editor tab remains available for restoring existing sessions; the standard Open Settings action now opens this modal. Workspace-specific overrides may take precedence over values saved to the user settings file.

Native rendering and interaction captures are recorded in [the dialog visual check](visual-checks/dialogs.md).
