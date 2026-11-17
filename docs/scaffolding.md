# Project-aware scaffolding

Use **Scaffold: New Feature…** in the command palette, or right-click a folder and select **New → Feature · MVVM / ChangeNotifier**. Enter a PascalCase name, select **Preview Changes**, and inspect each file in the list before **Apply Changes**.

[Native preview](scaffold-preview.png)

The feature pack creates a screen, view model, repository interface and in-memory implementation, immutable model, route factory, and unit/widget tests. Loading, data, empty and error states, retry, refresh, dependency injection through constructors and safe disposal during async loading are included. With automatic registration enabled, supported existing router and provider files are included in the same preview and undo group. The route factory remains available for manual integration.

## Package and convention detection

The selected folder or active file determines the nearest ancestor `pubspec.yaml`, bounded by the workspace. This supports packages inside a monorepo. Package imports use that package's name.

- Existing `lib/features` or `lib/feature` selects feature-first output and retains that spelling.
- Existing `lib/presentation`, `lib/data` or `lib/domain` selects layer-first output.
- Otherwise the default is `lib/features/<feature>`.
- Pin a choice in the package's `.sparrow/scaffold.yaml`:

```yaml
layout: layer-first # or feature-first
```

Declared state packages are reported in the preview. The first complete feature pack explicitly uses MVVM with ChangeNotifier; detection does not silently choose or migrate a state framework. Existing single-file templates remain available.

## Preview, apply and undo

Every scaffold, including single-file templates, goes through a change plan. The file list distinguishes creation, edits and deletion; selecting a file shows a colored unified diff. Planning writes nothing. A changed name requires a new preview.

Apply refuses conflicting files, unsaved affected editor buffers, symlink destinations, and changes to the previewed files or package settings. Identical generated output is skipped. All output is staged before replacement; failed writes roll back earlier writes and clean up newly created empty directories. Existing files with different content are never overwritten by a pack.

**Undo Last Scaffold…**, available in the dialog and command palette, previews the inverse operation for the complete scaffold. It restores edited files and removes generated files only if every affected file still matches the applied result. Later user edits block the entire undo. Empty directories created by the scaffold are removed when possible. Undo history belongs to the current window session; it is separate from editor typing undo and is not persisted across restarts.

The feature pack adds a missing `flutter_test` SDK dependency through the same preview. Existing dependency constraints and YAML comments are retained. Use **Pub Get** after applying; it runs visibly in Terminal in the selected package. Dependency resolution and its lockfile changes are a separate operation and are not included in scaffold undo. Single-file templates that need a missing package retain an explicit **Add required package…** action, followed by a fresh preview.

## Declarative team packs

Add `.sparrow/scaffolds/<pack-name>/pack.yaml` inside the target package. Reopen the folder's New menu to discover it. No IDE core changes or executable hooks are needed. Invalid manifests report errors when previewed.

```yaml
schema: 1
id: team.json-model
version: 1.0.0
title: JSON model
description: A small team model with serialization annotations.
dependencies:
  - package: json_annotation
    version: ^4.9.0
files:
  - path: '{{model_dir}}/{{snake}}.dart'
    content: |
      import 'package:json_annotation/json_annotation.dart';

      @JsonSerializable(createFactory: false, createToJson: false)
      class {{name}} {
        const {{name}}({required this.id});
        final String id;
      }
```

Each output has a package-relative path and inline content. Available variables:

| Variable | Value |
| --- | --- |
| `name` | PascalCase name entered by the developer |
| `snake` | File-name form of that name |
| `variable` | Name with its initial letter lowercased |
| `package` | Target pubspec package name |
| `feature_root` | Feature folder, including `lib/` |
| `screen_dir`, `state_dir`, `model_dir`, `repository_dir` | Convention-aware directories, including `lib/` |
| `screen_import`, `model_import`, `repository_import` | Corresponding paths without `lib/`, for package imports |

Dependencies declare either `version` or `sdk: flutter`; add `dev: true` for development dependencies. Version constraints support Pub caret ranges, exact versions, `any`, and comparisons such as `'>=1.0.0 <2.0.0'`. Resolved versions in `pubspec.lock` outside a pack's declared range produce a compatibility warning. Existing constraints are preserved; the developer resolves a mismatch explicitly.

Schema 1 permits 1–64 output files with up to 2 MB of template content. Outputs cannot traverse outside the package or target Git/build/IDE internals. Packs do not overwrite existing Dart files, run scripts, invoke generators or perform network operations. Dependencies require ordinary YAML block mappings. Legacy `.sparrow/templates/*.dart.tmpl` templates remain supported.

## Automatic router and provider registration

The built-in MVVM feature pack has **Automatically register router and providers** enabled by default. Preview scans saved Dart files in the selected package's `lib/`, resolves supported constructors, and proposes imports and registrations alongside the new feature files.

Supported targets are `MaterialApp.routes`, `GoRouter.routes`, and Provider's `MultiProvider.providers`. Collections can be inline literals or direct same-file variables. A missing MaterialApp routes argument is added. If Provider is a declared runtime dependency and there is no MultiProvider, the unique MaterialApp is wrapped in a new MultiProvider. Repository registration precedes ChangeNotifier registration; generated routes consume the registered view model. The screen disposes only a view model it owns and schedules its initial load after the first frame to avoid notifying a shared provider during build.

The route path derives from the feature name (`OrderHistory` → `/order-history`). Identical existing registrations are retained. A conflicting route destination, ambiguous target, dynamic/spread collection, unresolved import, existing analyzer errors in a selected target, or newly introduced analyzer errors stops the complete preview. Generated router files and indirect collections are not rewritten. Provider scopes must enclose the app/router that consumes them.

Pin ambiguous targets in the target package's `.sparrow/scaffold.yaml`:

```yaml
layout: feature-first
registration:
  router: lib/router.dart#appRouter
  providers: lib/app.dart
```

Use a package-relative file path, optionally followed by `#variable` for a constructor assigned to that variable. A configured target that does not match fails explicitly. Packages without a supported target retain constructor-based injection and receive an explanatory preview note. Disable automatic registration to generate files without touching existing configuration.

Planning, UI, conflict checks, writes and undo remain Rust. A bundled, version-pinned Dart analyzer helper provides resolved ASTs and semantic validation of in-memory overlays; it never writes project source. Using the analyzer directly avoids maintaining a second Dart semantic implementation or altering the live editor's analysis overlays. The helper runs on a worker with a timeout. Its pinned packages are prepared in the IDE cache, offline first; initial setup may require network access. Dart 3.9+ and resolved project dependencies are required. No analyzer dependency is added to the user's pubspec.

Both existing-file edits and generated files are one transaction. Stale source/package configuration or dirty affected buffers block Apply. Undo restores router/provider files and removes generated files together, and refuses the whole undo if any affected file has changed.

The helper's 19 resolved-AST tests cover aliases, Unicode/comments, compact collections, direct variable references, multiple targets, conflicts, idempotency, cross-file registration, app wrapping, constant route keys, dynamic collection refusal and semantic validation failures. See [analyzer API](https://pub.dev/documentation/analyzer/10.2.0/dart_ast_ast/), [GoRouter configuration](https://pub.dev/documentation/go_router/latest/topics/Configuration-topic.html) and [Provider](https://pub.dev/packages/provider).

## Remaining work

Support for other router/provider APIs, code-generation tasks with tracked rollback, other complete state packs, Mason import, headless CLI execution, persistent history, pack registries and template-update merges remain future work. The engine intentionally exposes the plan and transaction separately so these can share the same preview and conflict handling.

## Validation

The engine tests cover multi-file apply/undo, idempotency, stale settings and files, user edits before undo, nested packages, layout overrides, dependency preservation, incompatible locked versions, invalid manifests, path traversal, symlinks and preparation failures. The application suite passed 261 tests (seven environment-dependent tests skipped). The generated feature was analyzed and its four Flutter tests passed in each of the feature-first and layer-first packages against the installed SDK. Native desktop checks exercised the command palette, per-file preview, seven-file Apply and grouped Undo while preserving existing code.

Registration verification: four real Flutter fixtures (named routes alone, existing MultiProvider, automatic MaterialApp wrapping, and GoRouter with automatic wrapping) passed combined preview/apply/idempotency/undo checks. Their 20 feature/navigation widget and unit tests passed, including identity checks proving the route consumes the registered view model.

## Hardening pass

Registration follows nested same-file route-list variables when checking path collisions. It refuses nested spreads, opaque route/provider factory entries, shared/cyclic route collections, and conditional imports/exports in selected targets. These refusals are deliberate: the engine cannot prove the full registration set in those structures. Import alias allocation reserves declaration and parameter tokens as well as identifier nodes.

Failure-injection tests interrupt application after earlier edits and creations have succeeded. They verify that original bytes are restored and generated files removed. A separate test simulates a concurrent edit during rollback: the edit is preserved and the transaction reports that recovery needs attention instead of overwriting it.

This remains an early implementation. Current automated coverage is 261 application tests (seven skipped) and 19 resolved-AST tests; the previous real-Flutter four-project/20-test validation remains recorded above. This pass does not establish a multi-version SDK/package compatibility matrix or broad production-project coverage. Those remain required before describing the feature as production-hardened.
