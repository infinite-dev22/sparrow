# Settings dialog

Open **File → Settings**, use **Ctrl+,** or **Ctrl+Alt+S** on Linux and Windows, or use **Cmd+,** on macOS.

The modal adapts to smaller windows with scrollable contents. Drag the divider to resize the category sidebar. The dialog saves the sidebar proportion and shows the current page path next to the history controls. [Platform measurements and unresolved values](settings-tokens.toml).

The first visit opens Appearance. Subsequent visits reopen the last selected page. Expand Appearance & Behavior, Editor, Version Control, Build, Execution, Deployment, Languages & Frameworks, and Tools to reach their subpages. Selecting a subpage opens its ancestors and scrolls its row into view.

Appearance groups themes, UI scale, typography, and workbench controls. System Settings contains Project and Files pages. The Editor branch organizes editing, fonts, tabs, completion, hints, and diagnostics. Tools contains Actions on Save, Android Devices, Logcat, and Terminal. Android device and Logcat pages open their respective tool windows.

Search filters the category tree by page titles, setting names, keys and descriptions. Ancestor rows remain visible, and matching terms are highlighted in setting labels. Native theme/enum choices use dropdown menus, booleans use toggles, and numeric/string settings use editable fields. Invalid numbers are reported before saving. Changes survive category navigation, but are not written until Apply or OK. Cancel, Close and Escape with an empty search discard unapplied edits. Unselected pages with pending changes are marked in the tree, including changes in the other scope. Apply saves without closing.

Saving validates the complete pending draft, preserves unrelated TOML values and comments, and atomically replaces the settings file. Malformed settings files and section conflicts are reported rather than overwritten. Platform-specific terminal profile settings retain their existing storage format.

Keymap embeds Sparrow's shortcut editor. Its shortcut capture confirmation saves directly to the keymap file. Menus and Toolbars edits the ordered Quick Actions list shared by the application menu and toolbar. Build, Execution, Deployment contains Run Configurations and the project Debugger page. Debugger changes are staged until Apply or OK alongside other settings. Run Configurations, Git tools, and the plugin manager open their existing tools. Apply or cancel pending changes before opening one of those tools.

The older Settings editor tab remains available for restoring existing sessions; the standard Open Settings action now opens this modal. The scope picker above the settings page selects the values and destination being edited. Project overrides take precedence over IDE defaults.

Native rendering and interaction captures are recorded in [the dialog visual check](visual-checks/dialogs.md).


The Languages & Frameworks section includes Flutter and Dart SDK directories with folder browsing and validation. An empty path keeps automatic SDK detection. Workspace `.sparrow/settings.toml` overrides user settings. Flutter projects use the selected Flutter SDK's bundled Dart; standalone Dart projects may select a separate Dart SDK. Reopen the project after changing SDK paths to restart its existing analysis process with the new SDK.

Search accepts setting keys such as `font-size` as well as words, and result labels retain their category. Dialog fields accept Enter to validate and save with OK. Tab and Shift+Tab cycle among registered visible controls rather than reaching the editor behind the dialog. Escape clears a nonempty search before it can dismiss Settings.

The category tree is one Tab stop. Up/Down from the search field moves selection and focus to the tree. Up/Down in the tree selects visible categories, Home/End selects the first/last category, Right expands a branch or enters it, and Left returns to its parent or collapses it. Collapsing a branch with the mouse selects its parent when necessary. Overview links, history buttons and SDK browse buttons also accept keyboard focus.

Current limitations and the native acceptance record are in [the dialog audit](visual-checks/dialogs.md).

Dropdowns support arrow keys, Home/End, letter selection and Enter/Space. Escape dismisses the popup while retaining Settings and the previous value. Native directory choosers block the parent dialog until completion and restore field focus afterward.

IDE and project scopes retain separate drafts when switching. Apply/OK validate and save pending changes from both scopes, then reload effective settings. Cancel discards both drafts. Use IDE value marks a project override for removal on Apply; it does not copy the IDE value into the project file. Project scope requires a local workspace. [Native scope acceptance](visual-checks/configuration-workflows.md).

## Source-token regression tests

Run `cargo test -p sparrow-app --lib --locked --offline settings_layout` to check the Settings styles without a display server.

| Checked | Source values used by Sparrow |
| --- | --- |
| Dialog overlay | Preferred size uses the platform root minimum of 900 × 700. Sparrow also caps it to the available workbench. |
| Sidebar | Default split is 20%. Clamping reserves the divider and button space and handles narrow and zero-width layouts. |
| Search | Field height uses the declared 28 px minimum. Inner corner radius, icon gap, wrapper insets, and dark and light focus outline widths and colors are asserted. |
| Navigation | Root and child indent, icon gap, square row corner, banner and history insets, and dark and light modified label colors are asserted. |
| Footer | Compact padding, base button spacing, and button minimum dimensions are asserted. |

Computed geometry and paint remain `UNVERIFIED`, including the effective Settings row heights, font metrics, wrapped footer height, outer window size and chrome, and native focus-ring path. The tests cover the declared style properties and split calculation. Remaining source questions are tracked in [the token audit](settings-tokens.toml).
