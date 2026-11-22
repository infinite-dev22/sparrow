# Settings dialog

Open **File → Settings**, use **Ctrl+,** or **Ctrl+Alt+S** on Linux and Windows, or use **Cmd+,** on macOS.

The modal adapts to smaller windows with scrollable contents. Drag the divider to resize the category sidebar. The dialog saves the sidebar proportion and shows the current page path next to the history controls. [Platform measurements and unresolved values](settings-tokens.toml).

The first visit opens Appearance. Subsequent visits reopen the last selected page. Appearance & Behavior retains links to its supported settings. Appearance includes UI and theme options; System Settings covers the remaining core options. Editor and Terminal settings are generated from Sparrow's existing configuration metadata. The Appearance & Behavior tree expands when you select a subpage.

Search filters the category tree by page titles, setting names, keys and descriptions. Ancestor rows remain visible, and matching terms are highlighted in setting labels. Native theme/enum choices use dropdown menus, booleans use toggles, and numeric/string settings use editable fields. Invalid numbers are reported before saving. Changes survive category navigation, but are not written until Apply or OK. Cancel, Close and Escape with an empty search discard unapplied edits. Unselected pages with pending changes are marked in the tree, including changes in the other scope. Apply saves without closing.

Saving validates the complete pending draft, preserves unrelated TOML values and comments, and atomically replaces the settings file. Malformed settings files and section conflicts are reported rather than overwritten. Platform-specific terminal profile settings retain their existing storage format.

Keymap, local appearance themes, and Run/Debug Configuration links open Sparrow's existing editors. Pending settings must first be applied or cancelled before leaving for one of those editors. Android Studio-only settings and services are not represented by non-functional controls.

The older Settings editor tab remains available for restoring existing sessions; the standard Open Settings action now opens this modal. The scope picker above the settings page selects the values and destination being edited. Project overrides take precedence over IDE defaults.

Native rendering and interaction captures are recorded in [the dialog visual check](visual-checks/dialogs.md).


The Languages & Frameworks section includes Flutter and Dart SDK directories with folder browsing and validation. An empty path keeps automatic SDK detection. Workspace `.sparrow/settings.toml` overrides user settings. Flutter projects use the selected Flutter SDK's bundled Dart; standalone Dart projects may select a separate Dart SDK. Reopen the project after changing SDK paths to restart its existing analysis process with the new SDK.

Search accepts setting keys such as `font-size` as well as words, and result labels retain their category. Dialog fields accept Enter to validate and save with OK. Tab and Shift+Tab cycle among registered visible controls rather than reaching the editor behind the dialog. Escape clears a nonempty search before it can dismiss Settings.

The category tree is one Tab stop. Up/Down from the search field moves selection and focus to the tree. Up/Down in the tree selects visible categories, Home/End selects the first/last category, Right expands a branch or enters it, and Left returns to its parent or collapses it. Collapsing a branch with the mouse selects its parent when necessary. Overview links, history buttons and SDK browse buttons also accept keyboard focus.

Current limitations and the native acceptance record are in [the dialog audit](visual-checks/dialogs.md).

Dropdowns support arrow keys, Home/End, letter selection and Enter/Space. Escape dismisses the popup while retaining Settings and the previous value. Native directory choosers block the parent dialog until completion and restore field focus afterward.

IDE and project scopes retain separate drafts when switching. Apply/OK validate and save pending changes from both scopes, then reload effective settings. Cancel discards both drafts. Use IDE value marks a project override for removal on Apply; it does not copy the IDE value into the project file. Project scope requires a local workspace. [Native scope acceptance](visual-checks/configuration-workflows.md).
