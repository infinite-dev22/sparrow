# Run/Debug Configurations

Open **Edit Configurations…** from the run-configuration selector in the main toolbar.

The dialog follows the supplied Android Studio reference: a 916×684 layout with a configuration sidebar, a Flutter form, before-launch settings, and Run, OK, Cancel and Apply actions. The form scrolls in smaller windows.

- Add, remove and duplicate configurations in the sidebar. Switching selection retains unsaved edits. Cancel, Close and Escape discard edits made since the last Apply.
- **Dart entrypoint** maps to Flutter's `--target` flag. **Build flavor** maps to `--flavor`. The detected/configured Flutter executable remains the program to launch. Dart script configurations retain their Dart SDK executable and argument list. Other executable targets are rejected; auxiliary tools can run in the Terminal or before-launch task.
- Additional arguments support single/double quotes and escaped quotes. Arguments are passed directly to the process, without shell expansion.
- Environment variables use `KEY=value; KEY2=value`. Quote values containing semicolons or whitespace; embedded equals signs are preserved.
- Before launch supports the existing single program/argument task. Leave Program empty to omit the task.
- Apply validates and saves without closing. OK saves and closes. Run saves and launches. Use the arrow beside Run for **Attach to Flutter App**, which consumes the saved attach arguments and selected device.
- **Activate tool window** opens the console; **Focus tool window** also moves keyboard focus to it.

**Store as project file** writes `.sparrow/run.toml`. When unchecked, configurations are saved in Sparrow's user configuration directory under a project-specific key. Local configurations take precedence when reopening a project. The storage choice applies to the configuration list in this dialog. Switching back to project storage removes the obsolete local override after the project file has been saved successfully.

Save errors appear in the dialog. Invalid fields do not replace live configurations. Existing run files without Flutter dialog options remain readable. The dialog does not include placeholder controls for configuration templates or the Android Studio-specific “Show this page” setting.

Tests cover argument quoting, executable/flag mapping, environment validation, before-launch settings and configuration-file round trips.

The before-launch list displays an empty state until **Add** is selected; **Remove** clears the supported task. Native rendering and interaction captures are recorded in [the dialog visual check](visual-checks/dialogs.md).

Dart/Flutter debugging uses native SDK debug adapters and does not require a language plugin. See [the scope policy](dart-flutter-scope.md).
