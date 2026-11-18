# File-based scaffold packs

All 22 bundled scaffolds now use the same manifest and rendering path as user and project packs. Rust owns discovery, validation, rendering, previews, apply and undo. Template content lives in YAML and ordinary `.dart.tmpl` files; the previous `feature.rs` and Rust catalog strings have been removed.

## Add, update, remove, override

| Location | Purpose | Precedence |
| --- | --- | --- |
| `sparrow-app/resources/scaffolds/<id>/` | Bundled defaults | Lowest |
| User config directory: `sparrow/scaffolds/<id>/` | Personal packs | Overrides bundled |
| Target package: `.sparrow/scaffolds/<id>/` | Team packs | Overrides global and bundled |

On Linux the user root is normally `~/.config/sparrow/scaffolds`, respecting `XDG_CONFIG_HOME`. macOS and Windows use their standard configuration directories. In monorepos the selected folder determines the target package.

Add a directory containing `pack.yaml`. Update its manifest or template files and reopen the New menu or the dialog's pack selector; no IDE restart or rebuild is needed for global/project packs. Deleting an override restores the lower-precedence pack. To hide a bundled pack completely, export it and add `disabled: true` to the override's manifest. Disabled packs must still be valid manifests.

Use **Scaffold: New From Pack…** to choose a pack from the command palette. The file-tree New menu groups packs by their declared category. The preview identifies the pack's source.

Use **Scaffold: Export Bundled Template…**, choose a default, then **Preview Export** and **Apply Changes**. Export copies the manifest and all source templates to the target package's `.sparrow/scaffolds/<id>/`. It does not overwrite a different existing override and can be undone as one scaffold operation. The generator dialog also has an export-default action.

Malformed overrides remain visible as errors; Sparrow does not silently fall back to a bundled template. Duplicate IDs within one root are errors. Across roots, later roots override earlier ones by the manifest's ID. Name the override directory after its ID so even an unreadable manifest can be associated with the overridden default.

## Template manager

Open **Scaffold: Manage Templates…** from the command palette, or **Manage templates…** in a scaffold dialog.

- Filter by title, ID or source. The list includes bundled, user and project layers, including disabled, invalid and shadowed entries.
- Choose **Destination: Project / User** for new packs, imports, duplicates, exports and bundled-disable overrides. Project destinations use the nearest package selected by the active editor.
- Enter a new pack ID and choose **Create…** to preview a starter manifest and source file.
- **Import…** opens a folder chooser. Select a folder containing `pack.yaml`; the manager validates the manifest and source references, then previews a copy under the original pack ID. The New pack ID field is not used for import. Existing destinations are never overwritten.
- **Duplicate…** uses the **New pack ID** field and the selected template to preview an independent, enabled copy. Choose a different ID. The copy gets a “(Copy)” title and preserves template bodies, dependencies, compatibility and registration metadata. Bundled, user, project, disabled and legacy templates can be used as starting points.
- **Edit Manifest** and **Edit Sources** open the selected pack's files in Sparrow's editor. Save changes, then reopen the manager or choose **Refresh** to reload and validate. Normal editor undo applies to these edits.
- **Export…** copies the selected pack's manifest and source tree to the destination. Export a bundled pack first to create an editable override. Existing files are never overwritten.
- **Enable / Disable…** changes the manifest flag. Disabling a bundled entry creates a disabled override in the selected destination. If an override already exists, select that entry to change its state.
- **Remove…** previews removal of the selected user/project pack's files. Bundled defaults cannot be removed; removing an override reveals any lower-priority default.
- Review the diff, then choose **Apply Preview**. **Undo Last Change…** previews the inverse transaction, including restores after removal. Apply refuses to overwrite files changed since preview or unsaved editor changes. This undo history is shared with scaffold generation in the current window session.

Legacy single-file templates can be edited, removed or duplicated into a manifest-based pack. Use the resulting pack for enable/disable and export. Packs remain plain files suitable for version control; the manager requires no separate template database.

Import and duplicate leave their sources unchanged. Changes to copied source files after preview invalidate Apply; preview again to pick up the edits. Imported packs retain their enabled/disabled state; duplicates start enabled. Import supports Sparrow schema 1/2 pack folders, not archives or other template formats.

Manager validation: 295 application tests passed (9 SDK-dependent tests ignored), including lifecycle, source snapshot, overwrite protection, invalid-pack removal, symlink rejection and filtered-selection regression tests. Native desktop checks cover creation, disable, removal, undo, opening a manifest in the editor, filtered refresh, exporting to the user template root, and duplicate preview/apply/undo using keyboard navigation. Import validation and copying are covered by automated tests; the OS folder chooser uses the existing modal picker.

## Built-in template editor

Choose **Template Editor…** in the manager. The left pane lists `pack.yaml` and all pack files; select a file to edit it in the central multiline editor. Edit the YAML manifest to change metadata, dependencies, output paths and registration settings. **Add File** and **Remove File** update the in-memory pack. The manifest itself cannot be removed.

Validation refreshes after a short typing pause. It checks manifest syntax/schema, source references, compatibility constraint syntax, template variables, duplicate outputs and path safety. **Sample name** controls the generated-file preview on the right, using the same renderer as scaffolding. Preview does not write generated application files or run registration, Dart analysis or code generation. Disabled packs can be previewed without enabling them. The display shows at most 2,000 lines; validation processes the full pack.

**Preview Save** validates the current buffers and shows the proposed template-file diff. **Apply Save** commits it as one undoable transaction. Any subsequent edit invalidates the save preview. Files changed on disk since opening the draft, conflicting newly added files, and unsaved editor documents are protected by the normal transaction checks. Return to the manager and use **Undo Last Change…** to preview an inverse save.

Bundled packs save as an override in the destination selected before opening the editor; an existing override is never overwritten. Open that existing project/user entry to edit it instead. Legacy single-file templates must first be duplicated into a pack. Invalid manifest-based packs can be opened and repaired.

**Back to Manager** keeps unsaved buffers for the window session; **Resume Editor…** returns to that draft even if another pack is selected. **Discard Draft** explicitly drops unsaved buffers without changing saved files. Drafts are not persisted across IDE restarts. The existing **Edit Manifest** / **Edit Sources** actions still open the ordinary workbench editor.

Editor validation: 303 application tests passed (9 SDK-dependent tests ignored), including byte parity with all bundled generators, invalid-pack repair, preview without application writes, added-file conflicts, save snapshots and undo. Native desktop checks verified multiline editing, live errors, file switching with document undo, save preview/apply/undo, and draft resume/discard.

## Schema 2 example

```yaml
schema: 2
id: team.counter
version: 1.0.0
title: Team counter
description: A notifier and its test, beside the selected file.
category: state
target: sibling
suffix: _notifier
compatible_with:
  provider: '>=6.0.0 <7.0.0'
files:
  - path: '{{suffixed_snake}}.dart'
    source: templates/notifier.dart.tmpl
  - path: '{{suffixed_snake}}_test.dart'
    source: templates/notifier_test.dart.tmpl
dependencies:
  - package: flutter_test
    sdk: flutter
    dev: true
```

`templates/notifier.dart.tmpl`:

```dart
import 'package:flutter/foundation.dart';

class {{suffixed_name}} extends ValueNotifier<int> {
  {{suffixed_name}}() : super(0);
  void increment() => value++;
}
```

`templates/notifier_test.dart.tmpl`:

```dart
import 'package:flutter_test/flutter_test.dart';
import '{{suffixed_snake}}.dart';

void main() {
  test('increments', () {
    final notifier = {{suffixed_name}}();
    addTearDown(notifier.dispose);
    notifier.increment();
    expect(notifier.value, 1);
  });
}
```

Each file declares `source` relative to its pack directory, or inline `content`. A nonempty inline body cannot be combined with `source`. Source files are UTF-8 text, bounded and symlink checked. Unknown manifest fields and unknown/unclosed template variables are errors. The renderer performs plain-text substitution without HTML escaping; it is not a full Mustache/Jinja interpreter and does not support arbitrary expressions, loops, includes, remote assets or executable hooks.

`target: sibling` resolves every output beside the selected file/folder and permits multiple files. `target: package` resolves outputs from the target pubspec root and is the default for schema 1 compatibility. Both targets use the same transaction and path guards. Pack IDs must be safe directory names. Categories are `widget`, `state`, `feature` and `other`.

The existing variables in [Scaffolding](scaffolding.md) remain supported. `suffixed_name` converts `_view_model` into `ViewModel` and appends it only when absent. `suffixed_snake` does the corresponding filename operation. Both `Counter` and `CounterViewModel` therefore produce `CounterViewModel` / `counter_view_model`. `name` and `snake` retain their old meanings. Bundled templates deliberately retain their previous class declarations and generated bytes; teams can opt into suffixed class names in overrides.

`compatible_with` checks resolved package versions in `pubspec.lock` without adding dependencies. Dependency declarations can also produce compatibility warnings. Existing constraints are retained. Bundled third-party templates retain `any` dependency declarations rather than claiming untested version ranges; teams should pin the ranges they support. Pub Get remains an explicit subsequent action.

Schema 1 manifests and `.sparrow/templates/*.dart.tmpl` remain supported. Legacy templates become synthetic sibling packs. All rendered output and template inputs are snapshotted; editing a manifest or source after preview invalidates Apply. Regenerate the preview to use the updated template.

## Analyzer registration

Registration is opt-in metadata, not a privilege implied by a pack's ID:

```yaml
target: package
registration: mvvm_change_notifier
```

This capability uses the MVVM contract: the convention-aware screen, view-model and repository filenames/classes used by the bundled MVVM pack. A sibling pack cannot request registration. Packs that change that structure should omit the capability; arbitrary router/provider transformations are not provided by templates. Proposed registration edits still undergo analyzer validation and join the same preview and undo transaction.

## Research and implementation decisions

[Mason](https://docs.brickhub.dev/brick-syntax/) separates metadata from a tree of template files and offers broader Mustache behavior. That separation makes source files easier to edit than embedded strings; Sparrow adopts it without claiming Mason compatibility. [Copier](https://copier.readthedocs.io/en/latest/configuring/) also distinguishes template content, tasks and update behavior. Sparrow keeps executable tasks and merge-based template updates outside this schema. [Tera](https://keats.github.io/tera/) offers expressions and control structures, but that additional language is unnecessary for this migration; the small strict renderer preserves existing behavior and error handling.

Bundled resources use the existing `include_dir` dependency. A build script tracks the whole pack directory so developer additions, removals and edits trigger rebuilding; no generated include-path list or new runtime dependency is needed. Personal/team overrides are read from disk when discovered and reloaded before preview.

The migration is covered by byte-for-byte snapshots of all 21 previous single-file outputs and all seven feature outputs, plus tests for discovery precedence, disabling/removal, invalid and duplicate overrides, multi-file sibling targets, suffixes, compatibility metadata, source snapshots, legacy templates, symlinks, protected destinations and export/undo.

Migration verification: 279 application tests passed (9 SDK-dependent tests ignored in the ordinary run). The explicit real-SDK matrix passed for Flutter 3.50.0 / Dart 3.14.0 with Provider 6.1.5+1 and GoRouter 18.0.1 / 18.1.4, including 10 generated Flutter tests and registration apply/idempotency/undo checks. Native desktop checks verified the pack picker, rendered preview, bundled export and grouped export undo. These results cover the tested versions and workflows; they are not a guarantee for arbitrary third-party templates or package versions.
