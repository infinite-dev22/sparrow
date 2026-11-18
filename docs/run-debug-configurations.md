# Run/Debug Configurations

Open **Edit Configurations…** from the run-configuration selector in the main toolbar.

The dialog follows the supplied Android Studio reference: a 916×732 layout with a configuration sidebar, a Flutter form, before-launch settings, and Run, OK, Cancel and Apply actions. The form scrolls in smaller windows.

- Add fresh Dart or Flutter configurations, remove configurations (including the last one), and duplicate configurations in the sidebar. Names must be unique. Switching selection retains unsaved edits. Cancel, Close and Escape discard edits made since the last Apply.
- **Dart entrypoint** maps to Flutter's `--target` flag. **Build flavor** maps to `--flavor`. The detected/configured Flutter executable remains the program to launch. Dart script configurations expose a Dart file and program arguments while retaining their Dart SDK executable and existing VM options. Non-script Dart commands retain their executable/argument editor. Other executable targets are rejected; auxiliary tools can run in the Terminal or before-launch task.
- **Working directory** accepts an absolute path, a project-relative path, or `${workspace}`. Saving validates the directory and the Dart entrypoint against it.
- Additional arguments support single/double quotes and escaped quotes. Arguments are passed directly to the process, without shell expansion.
- Environment variables use `KEY=value; KEY2=value`. Quote values containing semicolons or whitespace; embedded equals signs are preserved.
- Before launch supports an ordered list of program/argument tasks. Add/remove tasks, enable or disable them, and move them up/down. Every enabled task must finish successfully before the app starts. Stop cancels the chain. Remove unused tasks rather than leaving their Program empty.
- Apply validates and saves without closing. OK saves and closes. Run saves and launches. Use the arrow beside Run for **Debug** or **Attach to Flutter App**, which consumes the saved attach arguments and selected device.
- **Activate tool window** opens the console; **Focus tool window** also moves keyboard focus to it.

**Store as project file** applies to the selected configuration. Shared records are individual `.run/<name>_<hash>.sparrow.run.toml` files; private records and list ordering are stored in Sparrow's user configuration directory under a project-specific key. Loading merges the two sources. Renaming, moving a shared record to private storage, or removing it retires its obsolete owned project file. Existing `.sparrow/run.toml` configurations migrate when saved. Malformed existing files are reported before replacement.

Save errors appear in the dialog. Invalid fields do not replace live configurations. Existing run files without Flutter dialog options remain readable. Edit configuration templates opens Flutter and Dart defaults in the same dialog. Templates may leave entrypoints blank, and supply defaults only to newly added configurations. Apply/OK persist template edits independently of the saved application configurations; Cancel discards unapplied edits. Storage choice, window options and tasks also inherit. The Android Studio-specific “Show this page” option is not included.

Tests cover argument quoting, executable/flag mapping, environment validation, before-launch settings and configuration-file round trips.

The before-launch list displays an empty state until **Add** is selected; **Remove** deletes the selected task. Native rendering and interaction captures are recorded in [the dialog visual check](visual-checks/dialogs.md).

Dart/Flutter debugging uses native SDK debug adapters and does not require a language plugin. See [the scope policy](dart-flutter-scope.md).


A successful Debug before-launch chain transitions to one live adapter-owned session. Failed or cancelled tasks do not launch the application. Every launch has a distinct session identifier so stopped Debug sessions cannot hide later Run launches. The main Stop button targets the active task/session.

The empty-list state disables launch actions. Enter in a form field validates and saves with OK; Tab/Shift+Tab cycle inside the dialog, and Enter can activate focused buttons. Long fields use their full available width instead of the native input's default character-width clipping.

The configuration list is one Tab stop. Up/Down and Home/End switch the selection while retaining unapplied edits; the selected row scrolls into view. Browse and before-launch Add/Remove accept keyboard focus, and Enter opens the Run action dropdown. Disabled task and list controls are skipped by Tab.

Shared storage uses the default `.run` directory; arbitrary shared destination directories and configuration folders are not implemented. Tasks are program/argument commands, rather than the full Android Studio task-type catalog. See [the workflow acceptance](visual-checks/configuration-workflows.md).

Delete removes and Ctrl+D duplicates a configuration when the list has focus. Add and Duplicate focus Name; adding a before-launch task focuses Program. Entrypoint browsing filters Dart files and resolves selections relative to the working directory; the working directory has a folder picker. Debug honors both Activate and Focus tool window options. Dialog popups support arrows, Home/End, letter selection, Enter/Space, disabled-item skipping and popup-only Escape.

Rerun reloads the saved configuration and repeats its enabled before-launch tasks in order. Existing single `prelaunch` records remain readable and become the first task when edited. [Native acceptance and tests](visual-checks/configuration-workflows.md).
