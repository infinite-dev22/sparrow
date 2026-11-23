# Changelog

## Unreleased

### Dart editing and dependencies

- Detect Dart and Flutter SDKs from project configuration, FVM, environment
  variables, and system paths. Allow SDK paths to be set in Settings.
- Add Flutter closing labels, widget guides, color and icon previews, and a
  gutter with separate lanes for breakpoints, test controls, and bookmarks.
- Add contextual completion and live validation for `pubspec.yaml` and
  `analysis_options.yaml`. Complete package names and versions from pub.dev,
  and explain dependency conflicts using Pub's solver output. Preview proposed
  fixes after validating them with another dry run.
- Save edited dependency manifests before running Pub Get, combine overlapping
  requests, and surface failures with retry actions. Show unresolved Dart
  packages in External Libraries only for Dart and Flutter projects.
- Add project-wide **Inspect Code**, severity filters and counts, diagnostic
  details, and source previews with navigation to the relevant line.
- Preview analyzer quick fixes before applying them. Group edits from multi-file
  fixes and renames into one undo or redo operation and block the operation if
  newer edits conflict. Open SDK source files read-only.

### Flutter and Android tooling

- Add a Flutter project wizard, asset path completion, and a Resource Manager
  with previews and usage-aware Android resource renaming.
- Add Dart and Flutter run and debug templates, shared and private
  configurations, and an ordered list of before-launch tasks. Give Run and
  Debug separate sessions and consoles. Keep Terminal focused on shell sessions.
- Add Flutter test gutter actions and a test tool window with live results,
  durations, clickable failure locations, and reruns for failed tests.
- Add a widget inspector, device selection and emulator launch controls, and a
  Flutter Performance dashboard with frame, memory, network, and diagnostics
  views. Keep runtime diagnostic switches in sync across hot reload, restart,
  and rerun.
- Add an in-IDE Mobile Preview with simulated devices and a managed Flutter web
  server.
- Add live Logcat with filters, history, favorites, tabs and splits, source
  links, and export. Resolve project packages from Gradle build variants,
  including product flavors and `applicationIdSuffix`.

### Projects, Git, and workbench

- Add project creation, recent project, and repository cloning actions, plus a
  shared file picker with previews. Expand project tree controls and support
  horizontal scrolling.
- Add file-based scaffold packs and an editor that can create, import, duplicate,
  validate, and preview templates. Preview and undo generated changes, including
  route and provider registration.
- Add Git history, pull requests, and selected-file commits. Support amending
  and pushing with a lease, including recovery when the lease is stale.
- Add Search Everywhere and separate Find and Replace in Files dialogs.
- Improve Settings with IDE and project scopes, a built-in keymap editor,
  configurable Quick Actions, and a resizable sidebar. Preview single search
  matches while preserving navigation history and unsaved settings.
- Refresh the workbench layout, themes, icons, fonts, and project colors. Share
  controls across panels and dialogs, keep active tabs visible, and improve
  keyboard focus, tooltip dismissal, narrow layouts, and dock resizing.
- Add a live IDE log viewer with entry details and crash-report notifications.
  Improve save and session recovery when files or editors disappear, and keep
  breakpoint highlights aligned while scrolling.

### Releases and documentation

- Set Sparrow's Linux application ID for desktop launcher integration, update
  the Floem revision, and refresh application icons.
- Configure releases for Apple Silicon macOS and for ARM64, x86-64, and 32-bit
  x86 on Windows and Linux. Match updater assets to the target architecture and
  generate release checksums. Preserve native CPU optimization for local builds
  and use explicit CPU targets with link-time optimization in release builds.
- Rewrite the README and add separate installation, source-build, and release
  guides.

### Removed

- Remove executable WASI extensions, the inherited plugin marketplace, the
  general-purpose language-server host, and Rust-analyzer-specific commands.
  Sparrow focuses on Dart and Flutter SDK services, with local appearance themes
  and syntax coloring for supported project files.

Earlier inherited editor history is preserved in
[the historical changelog](docs/history/upstream-changelog.md), not as a claim of
current Sparrow language support or releases.
