# Flutter projects and assets

Choose **Flutter: New Project** in the command palette to open the wizard. Enter a Dart package name and an existing absolute parent directory, then choose app, package, plugin, module, or FFI package. Apps offer the counter or empty skeleton template. Android, iOS, Web, Linux, macOS, and Windows targets can be selected for apps and plugins. Flutter's package and module templates do not accept platform flags; FFI packages generate their native build support through the package template.

Create runs the detected SDK's `flutter create` in a background worker and opens the successful project in a new workspace tab. Existing destinations are rejected. Errors remain in the dialog; Flutter may leave a partially generated directory after a failed creation. Package dependency resolution uses Flutter's normal behavior and may require network access.

For local Flutter projects, Sparrow indexes image, SVG/icon, and font files recursively under `assets/` and watches for additions, changes, renames, and deletions. Symlink directories are not traversed. In Dart, type an asset path inside `AssetImage('...')` or `Image.asset('...')` to see suggestions, or invoke completion explicitly. Accepting a suggestion replaces the string contents and escapes Dart string characters. Other Dart completions continue through the language server.

Asset indexing does not modify `pubspec.yaml`. Declare the files/directories there for Flutter to bundle them. This phase provides path completion rather than generating a Dart asset constants file. Runtime-generated paths, package asset catalogs, and asset paths outside `assets/` are not indexed.

Resource Manager can preview and import one or more local files into a project-relative destination. Use the file picker or drop files onto the open import dialog; review the proposed copies and `pubspec.yaml` additions before applying. Choose **Skip** (the default) or **Keep both** for existing and duplicate names; Keep both adds `_2`, `_3`, and later suffixes before the extension. Imports never overwrite files. After an import changes `pubspec.yaml`, Sparrow offers to run `flutter pub get`; it runs only after you confirm, and declining does not undo the imported files. Replace and undo are not yet available.

Validation: `cargo check -p sparrow-app --offline` and `cargo test -p sparrow-app --lib --offline`. Tests cover wizard validation, asset discovery, watcher additions/deletions, and completion contexts. Temporary empty-app and FFI-package creation have also been checked against the installed Flutter SDK.
