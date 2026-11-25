# Overnight run log

## Baseline

- 2026-11-25: Started from branch `sparrow`, which was 10 commits ahead of `origin/sparrow`; created `overnight/2026-11-25` without changing the existing branch.
- Pre-existing untracked path `scripts/wayland/` is intentionally preserved and excluded from this work.
- Tool check: Rust 1.99.1, Cargo 1.99.1, Flutter 3.50.0; Google Chrome is installed. `Xvfb`, `xvfb-run`, `xdotool`, `cargo-nextest`, and Chromium are unavailable.
- Available screen-free test tier: Tier 1 is available through Rust unit tests. Tier 2 was verified in B1 with the existing ignored headless Chrome/CDP test. Tier 3 cannot be used because Xvfb/xdotool are unavailable; Tier 4 remains unverified.

## Workstream A — dropdown selection (implemented; UI verification incomplete)

### Existing behavior

- The app has a shared controlled UI in `sparrow-app/src/widgets/dropdown.rs`. It tracks a read function through Floem's `create_updater`, creates a fresh `floem::views::list` when the overlay opens, and forwards selection through `on_accept`.
- The shared dropdown is used by settings scope, settings rows, debug thread selection, and Gradle variant selection. The Gradle selector and several other controls also keep local state and synchronize it to a backing state in callbacks.
- The pinned Floem source at `~/.cargo/git/checkouts/floem-ab9be4e01bb293da/b0a2f3e/src/views/list.rs` shows that each list owns a separate `RwSignal<Option<usize>>` initialized to `None`. Sparrow does not initialize that list selection from the dropdown's active value. Floem's `List::selection()` allows the caller to set it.
- Baseline reproduction test confirmed the list starts with no highlighted value (`None` rather than `Some(1)` for active option B); the test passed after initializing from the controlled value.
- Baseline `Dropdown::update` closed the overlay before calling `on_accept`; selection handling now commits the controlled source and registered callbacks before dismissal.

### Selection-control inventory

| Control | File | Value storage | Popup construction | Reproduces stale-selection bug? |
|---|---|---|---|---|
| Settings scope | `sparrow-app/src/settings/modal.rs` | `project_scope`; `scope_label` is derived reactively | Shared controlled dropdown | Core source/highlight behavior covered by shared tests; no screen-level test |
| Settings modal dropdown row | `sparrow-app/src/settings/modal/setting_row.rs` | Caller-provided `RwSignal<String>` | Shared controlled dropdown | Core source/highlight behavior covered by shared tests; no screen-level test |
| Settings page dropdown row | `sparrow-app/src/settings/setting_row.rs` | Local `RwSignal<String>` initialized from the settings item; callback persists config | Shared controlled dropdown | Core source/highlight behavior covered by shared tests; config persistence not UI-tested |
| Debug thread picker | `sparrow-app/src/panel/debug_inspector/frames.rs` | Local `RwSignal` initialized from the DAP thread id | Shared controlled dropdown with custom label | Core source/highlight behavior covered by shared tests; no screen-level test |
| Gradle variant picker | `sparrow-app/src/panel/logcat/view/variant.rs` | Persisted `state.variant` is now the sole selected-value signal | Shared `dropdown_with_label` inside a `dyn_container` | Shared source/highlight tests pass; Gradle integration not UI-tested |
| Project-type radio picker | `sparrow-app/src/dart/project_wizard/view.rs` and `sparrow-app/src/widgets/radio_group.rs` | `wizard.template` shared `RwSignal` | Floem `RadioButton::new_labeled_rw`; no popup | Not migrated: intentionally always-visible radio group; no dedicated scenario test |
| File-picker navigation actions | `sparrow-app/src/widgets/file_picker/navigation.rs` | Actions, not a persistent selected value | Low-level `Dropdown::custom` action list | Not migrated: action menu, no selected-value state |
| Flutter device picker | `sparrow-app/src/panel/mobile_preview/view.rs`, `session.rs`, `device.rs` | `DeviceChoice::Preset(id)` / `Flexible`; `Device.id` is a stable key | Shared `dropdown_with_label`; flexible dimensions remain in the companion controls | Unique stable-ID/resolution test passes; no UI/device test |
| Run/debug configuration picker | `sparrow-app/src/title/run_config_menu.rs` | `RunDebugConfig.dap_id`; app's active index is resolved to the stable ID | Custom grouped popup with live rows and inline Run/Debug/Edit actions; cursor stores `DapId` | Reorder identity test passes; retained bespoke UI because shared dropdown cannot represent its grouped action rows; no UI scenario |
| Browser, formatter scope, Git history date, Resource Manager module, PR state/author/label filters | `sparrow-app/src/dart/run_config_view/popups.rs`, `sparrow-app/src/formatting/view.rs`, `sparrow-app/src/panel/git_history/view.rs`, `sparrow-app/src/panel/resource_manager/view.rs`, `sparrow-app/src/panel/workflow_tools/pull_requests.rs` | Browser enum, `PathBuf`, and strings held in owning signals | Shared controlled dropdown; reactive option lists use `dyn_container` | Shared behavior tests pass; no per-screen interaction tests |
| Action/context menus, including Git Actions | 27+ `ide_popout_menu`/`Menu` call sites in `sparrow-app/src` | Actions, not a persistent selected value | `popup_menu.rs` builds action menus with a local keyboard cursor | Not migrated: no persistent selection value |

### Plan

1. Add a regression test that demonstrates a newly opened controlled list has no active row even when its source value is a non-first option.
2. Extend the shared dropdown so a controlled option's stable `PartialEq` identity initializes the newly-created list selection from the current source value; do not compare display labels or use a persisted numeric index.
3. Change selection handling to invoke the callback (which writes the controlled source) before closing the popup.
4. Run the new focused test and relevant app tests, then `cargo check`; document GUI checks that cannot be run without a virtual display.

### Verification and limitations

- Confirmed root cause: each Floem `list` has its own `RwSignal<Option<usize>>`, initialized without the controlled active option. The initial focused reproducer failed (`None` versus expected `Some(1)`) before the fix.
- The shared dropdown now finds the active option by `PartialEq` identity, tracks the source signal while open and on reopen, commits the controlled signal before later callbacks and popup dismissal, and composes multiple acceptance callbacks in registration order.
- Migrated value selectors to the shared component: browser, formatter scope, Git history date, mobile preview device, Resource Manager module, pull-request state/author/label, and the Gradle variant. The Gradle selector no longer mirrors `state.variant` in a local signal.
- The run-configuration popup remains custom for its grouped entries and inline actions, but stores `DapId` for row identity, updates its list from reactive configuration data, and follows active changes. The radio group and file-picker action list remain distinct for documented semantic reasons.
- Tests: `cargo test -p sparrow-app popup -- --nocapture` (11 passed); `cargo test -p sparrow-app device_choices_resolve_by_unique_stable_ids -- --nocapture` (1 passed); full `cargo test -p sparrow-app` (472 passed, 0 failed, 17 ignored; doc tests 0 failed, 8 ignored); `cargo check -p sparrow-app` passed.
- Known warning: `sparrow-app/src/title/branch_switcher.rs:397` has an unused `Icon::Theme` variant; it is outside this workstream.
- Tier 3 GUI verification is unavailable (`Xvfb`, `xvfb-run`, `xdotool` absent). Tier 2 was not used for A, then verified with B1's headless Chrome/CDP test. Tier 4 remains unverified. No per-screen UI tree/checkmark/label test or manual five-screen script has run; A is therefore partially verified, not claimed complete.
- Review identified extra formatter-driven diffs in touched legacy Rust files; no unrelated behavioral changes were intended. The pre-existing untracked `scripts/wayland/` remains untouched.

### A manual verification script (not run in this environment)

1. In Settings, change a dropdown to its second option, close/reopen it, and verify both the label and highlighted row; change it again through its backing setting and reopen.
2. In Run/Debug Configurations, select a different configuration, reopen the chip, reorder or rename configurations, and confirm the active `DapId` row remains highlighted; test keyboard arrows and Enter.
3. In the Dart web-run dialog, change Browser from Default to Chrome and verify the current browser label and selected row persist after reopening.
4. In Git History, switch Date to Last 7 days and confirm its active row and filter value remain aligned.
5. In Resource Manager, select a module, then change the module list and verify the selected module remains represented; also test All modules.
6. In Mobile Preview, select a non-first device and Flexible device; verify the visible label, stable id, and dimension controls.
7. In Pull Requests, change state, author, and label filters and verify each selected row and query update.

## Workstream B1 — Mobile Preview (not reproduced)

- No Flutter application belonging to the supplied project was present, so did not run `flutter run` in the Sparrow repository. Generated an isolated default app at `/tmp/sparrow-preview-repro-Vkabrr` with `flutter create --no-pub`; resolved its 31 cached dependencies using `flutter pub get --offline`.
- Environment: Flutter 3.50.0 stable, Dart 3.14.0, Linux; Google Chrome is installed. `flutter run -h --verbose` confirms `--machine` outputs structured JSON, `--web-hostname` chooses the server host and `--web-port` accepts an explicit port (or selects one when omitted).
- Command run in the disposable app: `CI=1 FLUTTER_SUPPRESS_ANALYTICS=true timeout --signal=INT --kill-after=5s 90s flutter run --no-pub -d web-server --web-hostname 127.0.0.1 --web-port 0 --machine`.
- Captured output: `app.start` for `web-server`, local server at `http://127.0.0.1:34993`, `[ {"event":"app.webLaunchUrl","params":{"url":"http://127.0.0.1:34993","launched":false}} ]`, followed by `app.started`. The `launch_url` parser accepts this actual array event; no parser change was warranted.
- The command's exit code was 124 because `timeout` sent SIGINT after the server had reached `app.started`; this was intentional cleanup, not a Flutter startup failure. No Sparrow project files were used or changed.
- `cargo test -p sparrow-app accepts_only_the_flutter_local_launch_event -- --nocapture` passed. `cargo test -p sparrow-app captures_and_interacts_with_a_windowless_device_surface -- --ignored --nocapture` passed (1 test), verifying headless Chrome startup, CDP viewport/dark-mode changes, touch interaction, screenshot decoding in memory, and profile cleanup.
- Result: the reported Mobile Preview failure was not reproduced with a clean Flutter web app. The local sample verifies Flutter web-server startup and the isolated renderer separately, but not Sparrow's full desktop UI with a user app; retain the user report as unresolved pending a project-specific reproduction.
- No screenshot files or desktop input were created. Manual UI reproduction remains: open the isolated app in Sparrow, select Mobile Preview → Run app, then record the status text and whether the preview surface receives a frame.

## Workstream B2 — web build and permissions

- Repository audit found no first-party Flutter app, `web/index.html`, Android manifest, iOS `Info.plist`, or app assets. `sparrow-app/resources/scaffold_analyzer/pubspec.yaml` belongs to a Dart analyzer helper, not a runnable Flutter app. The only repository `Info.plist` is the Sparrow macOS bundle.
- The isolated Flutter scaffold contains `pubspec.yaml`, `web/index.html`, `web/manifest.json`, default web icons, `android/app/src/main/AndroidManifest.xml`, and `ios/Runner/Info.plist`; its `flutter.assets` section is empty/commented. `flutter build web --release --no-pub` succeeded and emitted `build/web` (62.3s); icon tree-shaking and the successful Wasm dry-run were informational.
- Initial hypothesis: Mobile Preview's current `flutter run -d web-server` path consumes the Flutter project's web target and declared web assets; it does not build or launch an APK/IPA, so Android/iOS permission manifests should not affect preview startup. Sparrow's asset indexer provides path suggestions but does not edit `pubspec.yaml` (`docs/flutter-projects-and-assets.md`).
- The wizard documents Android, iOS, Web, Linux, macOS, and Windows app targets (`docs/flutter-projects-and-assets.md`). Any native permission requirements are project/plugin-specific and cannot be inferred without the user's Flutter app; no permission or manifest change is recommended based on this repository audit.
- Still to verify against an actual reported project: presence of the Flutter web platform, declared assets/plugin web implementations, and any browser permissions or web-only startup errors. Do not treat the sample scaffold's success as proof for an arbitrary app.

## Workstream B3 — Flutter preview modes and commands

- Confirmed `flutter run -h --verbose` documents `--machine`, `--web-hostname`, `--web-port`, and the `web-server` device. `flutter widget-preview start --help --verbose` documents `--web-server` and `--machine`.
- In the disposable app, `flutter widget-preview start --web-server --machine` emitted `widget_preview.app.webLaunchUrl` and `widget_preview.started`, accepted by `launch_url`; the scaffold contained no widget-preview declaration, so the app reported completion without proving a real annotated widget preview. No widget-preview syntax or third-party integration is inferred.
- Sparrow's web-server mode renders the local URL through its built-in headless Chromium/CDP renderer. External URLs can be opened manually but are not attached to the Flutter process and therefore expose no reload controls.
- Unsupported/custom behavior: no APK/IPA build or native-device launch occurs; the Widget Previewer does not support hot reload controls; third-party web viewers are out of scope.

## Workstreams B4–B5 — hot reload, hot restart, and save-triggered reload

- Flutter `app.start` machine event supplies `appId`. Sparrow sends a JSON-RPC `app.restart` request on stdin with `fullRestart=false` for Hot reload and `true` for Hot restart, then waits for the matching response ID and reports success or the Flutter error.
- Recursive file watching is enabled only for Flutter app mode. Dart source Create/Modify/Remove events are filtered to `.dart`, skip `.git`, `.dart_tool`, and `build`, and are debounced for 350 ms. Repeated changes while a request is pending trigger a later reload; manual reload/restart cancels an already-scheduled save reload.
- Auto reload defaults on and persists under `[mobile-preview]`; the in-panel toggle updates both the preference and an active watcher. The file watcher reports setup/runtime errors in the status area.
- Verification: ignored integration test `dart_save_triggers_hot_reload_and_machine_protocol_supports_restart` modified and restored only `/tmp/sparrow-preview-repro-Vkabrr/lib/main.dart`; it observed automatic reload and a successful full restart (1 passed, 18.13s). Machine request/response, JSON-array app ID, debounce path filters, and error messages have unit coverage.
- No changes to the user project, build outputs, credentials, or remote services were involved.

## Workstream B6 — device catalog, geometry, locale, and approximations

- Replaced the inline device array with `sparrow-app/src/panel/mobile_preview/device-presets.toml`. It includes 6 Android phones, 6 iPhones, one iPad, one Android tablet, and 3 generic baselines; stable IDs; inner/cover displays; panel pixels, logical size, scale/density; platform/category; cutout; frame color/radius; status/navigation geometry; source URL/date; and separately dated Sparrow frame-estimate notes.
- The loader rejects duplicate/empty IDs, missing dated provenance, invalid dimensions, and invalid densities. Device dropdowns are grouped by platform/category and labels remain separate from identity.
- Locale options (`en-US`, `en-GB`, `fr-FR`, `de-DE`, `ja-JP`, `ar-SA`) call CDP `Emulation.setLocaleOverride`; font-scale emulation is explicitly unsupported. The headless integration test confirms `Intl.NumberFormat().resolvedOptions().locale` changes to `fr-FR` along with viewport/dark-mode state.
- The panel exposes a dismissible, persisted approximation notice, profile source/frame notes, and detailed limitations: native plugins/permissions/hardware, system fonts/rendering, safe areas, pinch gestures, font scale, and screenshot-file saving are not emulated.
- Catalog coverage, stable IDs, source dates, fold panels, tablet orientation/scale, locale defaults, invalid custom dimensions, and non-sensitive diagnostic output have unit tests.

## Workstream B7 — diagnostics and handoff

- `Copy diagnostic` reports Sparrow version, OS, Flutter/Dart runtime version (when available), preview mode, profile ID/name, viewport, locale, dark mode, auto-reload state, and up to 50 static recent events.
- It deliberately excludes project paths, local preview URLs, source files, credentials, and raw Flutter/exception output; the privacy test sets a secret-bearing URL/status and confirms neither is copied. The adjacent UI explains this before copy.
- Manual status messages identify preview startup, watcher setup, reload/restart in progress, completion, and operation failures. No log files or screenshots were generated.

## Workstream B8 — Flutter Widget Previewer device context

- The official `flutter widget-preview start --web-server --machine` path was already available, but Sparrow forced its browser to `800×760`, 1×, desktop mode regardless of the selected device. Widget mode now uses the selected profile's logical dimensions, scale, orientation, mobile touch context, iOS/Android platform hint, dark preference, and locale, matching the app-preview renderer.
- Renamed the panel heading to **Flutter Preview**, promoted **Widget Previewer**, and hide the app-only save-reload toggle while widget previews are active. Inline help and `docs/flutter-testing.md` explain Flutter's `@Preview` metadata and the reload/native-runtime limitations.
- The new viewport regression test failed before the fix with `(800, 760), 1×, desktop`; it now passes for landscape iPhone 18 Pro at `(874, 402), 3×, iOS`, with dark mode and `fr-FR` preserved.
- Verification: all three Mobile Preview session tests passed; the headless Chromium/CDP device-surface integration test passed; full `cargo test -p sparrow-app` passed (474 passed, 17 ignored); edited-file `rustfmt --check` and `git diff --check` passed.
- No annotated Flutter sample was available for end-to-end Widget Previewer visual review. The previous disposable scaffold had no `@Preview` declarations; desktop visual acceptance remains unverified.

## Final validation

- `cargo test -p sparrow-app`: 474 passed, 0 failed, 17 ignored; doc tests 0 failed, 8 ignored.
- `SPARROW_PREVIEW_TEST_PROJECT=/tmp/sparrow-preview-repro-Vkabrr SPARROW_FLUTTER_BIN=/home/infinite/develop/flutter/bin/flutter cargo test -p sparrow-app dart_save_triggers_hot_reload_and_machine_protocol_supports_restart -- --ignored --nocapture`: passed (18.13s).
- `cargo test -p sparrow-app captures_and_interacts_with_a_windowless_device_surface -- --ignored --nocapture`: passed (0.77s).
- `cargo check -p sparrow-app`: passed. Only warning is the pre-existing unused `Icon::Theme` in `sparrow-app/src/title/branch_switcher.rs:397`.
- Tier 2 headless integration is verified; Tier 3 is unavailable without Xvfb/xdotool; Tier 4 and desktop visual review remain unverified. No screenshot was saved.

## Workstream C — VCS credentials audit and plan

### Audit (before implementation)

- `sparrow-app/src/panel/workflow_tools/pull_requests.rs` derives the GitHub repository from `origin`, fetches only the first 100 PRs, and opens a selected URL in the browser. It accepts `GITHUB_TOKEN`/`GH_TOKEN` from the process environment and gives private repositories a generic error; it does not show PR details or create requests.
- `sparrow-app/src/git_actions.rs` runs reviewed Git CLI commands, including clone/fetch/pull/push, with terminal prompting disabled and no account-aware credential callback. `sparrow-proxy/src/dispatch.rs` uses libgit2 for repository inspection, not network fetch/push.
- Settings have Version Control and Git pages, but no account list, token field, secure-store status, or account actions.
- `reqwest` is already available. `git2` 0.21 is already in the workspace; no HTTP Git provider or secure credential integration is present.
- Verified `keyring` 4.2.0 via its docs.rs metadata, `cargo info`, and its downloaded `src/v1.rs`: `Entry::new`, `set_password`, `get_password`, and `delete_credential` are available. The `v1` feature selects macOS Keychain, Windows Credential Manager, and Linux Secret Service; Linux kernel-keyutils is separate and is not the selected v1 backend.

### Plan

1. Add account metadata persistence without any token field, secure-store operations on worker threads, explicit session-only in-memory fallback with a visible status, and unit tests that inspect persisted metadata and redaction. — Done.
2. Add GitHub `/user` token validation and authenticated PR listing with actionable 401/403/SSO/permission messages; keep HTTP failures sanitized and never include the bearer token in an error. — Done.
3. Feed selected HTTPS-host credentials to Git commands through a Sparrow askpass mode and child environment, never command-line arguments or `.git/config`; explain SSH remotes do not use HTTP tokens. — Done.
4. Add Version Control account controls and an inline PR-panel action to open Settings when authorization is missing. — Done.
5. Add PR pagination, details/files/comments, and user-triggered creation; run targeted/full tests and document live-only verification. — Done, with stated caps and live checks outstanding.

### Implementation and verification

- Added `keyring` 4.2.0 with the explicit `v1` backend and `zeroize`; serialized keyring operations; persisted only host, username, provider, status, storage kind, and active selection via atomic metadata writes. Replacing/removing secrets rolls metadata/secret state back on write failures where possible.
- The account form validates GitHub-compatible tokens with `GET /user`, records classic OAuth scopes only when the response exposes them, links to the host token page, supports multiple accounts per host, and provides add/replace/test/remove/select controls. Generic HTTPS accounts are deliberately not API-validated.
- If the OS store is unavailable, the default opt-in fallback is process-memory-only and clearly labeled; it can be disabled. No plaintext or passphrase-encrypted file fallback is implemented. Keychain, Credential Manager, and Secret Service are selected; Linux kernel-keyutils is not.
- Git HTTPS operations use a Sparrow `GIT_ASKPASS` entry point; keyring secrets are read in the askpass child, while explicitly accepted session-only secrets are passed only in child environment variables. Stored tokens are not placed in Git arguments, URLs, or `.git/config`; SSH remotes use the SSH agent, and Git output is sanitized before display.
- The PR panel now paginates through up to 2,000 requests, displays detail/file patches and conversation/review comments (up to 500 items for each endpoint), and creates a request only after an explicit user action. Private-repository auth failures expose an action that opens Version Control account settings.
- Focused PR tests passed (4 passed, 1 ignored); credential tests passed (11) and Git action tests passed (3). Full `cargo test -p sparrow-app` passed (492 passed, 0 failed, 17 ignored; doc tests 0 failed, 8 ignored). Local HTTP mocks verified bearer headers, scope metadata, POST JSON, and that tokens are not put in request URLs. Git unit tests cover HTTP credential rejection/output redaction and remote selection.
- No live token/account, private repository, clone, fetch, push, real remote mutation, or functioning OS credential-store interaction was used. Keyring integration, GitHub Enterprise API compatibility, and the account/PR UI still need human verification. PR merge/close controls and GitLab/Bitbucket PR APIs are not implemented; no such action existed in the prior panel.

## Workstream F — AI commit messages audit and plan

### Audit (before implementation)

- `SourceControlData` contains the commit-message `EditorData`, checked `file_diffs`, amend state, and commit status. The panel already has a commit text editor plus Commit/Commit and Push buttons.
- `commit_with_options` snapshots only checked `FileDiff` values and the current message, then runs `commit_selected` on a worker; the proxy commits only those paths. `FileDiff` contains paths/kinds, not reusable diff text.
- There is no AI provider/client, model picker, consent flow, token storage for AI, or AI settings page. `reqwest` is already available.
- Existing credential storage can safely inspire but must not reuse the VCS service/account identity for unrelated AI keys. No shared provider key will be added.
- Verified Ollama's current API docs: `GET /api/tags` lists local models; the local OpenAI-compatible endpoint is `POST /v1/chat/completions`, and local server requests do not require a key. No model will be downloaded or installed by Sparrow.

### Plan and scope

1. Build prompt context from the exact selected `FileDiff` set, exclude sensitive/generated/binary files, cap the diff, and redact secret patterns before a request.
2. Provide local Ollama, runtime model listing, click-only generation, and exact prompt preview; do not add a cloud endpoint or embed a key.
3. Keep all endpoint/settings data non-secret; add mock-server tests for local model listing/completion, invalid endpoints, output bounds, and redaction.
4. Add the commit-panel control and AI settings; leave cloud providers visibly unsupported rather than claiming the full F acceptance.

### F implementation and known gaps

- `SparrowConfig.ai_commit` provides the local endpoint, installed model, style, language, diff-size cap, exclusions, and enable flag. Settings can refresh/save model selection; the Source Control AI control lists local Ollama models, builds the preview from only checked paths, displays the exact redacted system/user prompt, and supports generated-subject edit/replace/append without committing.
- The service accepts only `localhost`, `127.0.0.1`, or `::1` over HTTP with proxies disabled. It limits the generated token count and response bytes, bounds Git diff output, disables external diff/textconv, excludes configured sensitive/generated patterns, redacts common credential/private-key forms, and re-redacts generated output. No cloud endpoint or key field is present.
- Six focused tests pass: selected paths/exclusions/redaction/capping, bounded path-scoped tracked diff and traversal rejection, private-key/authorization/key redaction, loopback-only validation, subject cleanup, and a local Ollama mock for model discovery/completion. UI visual acceptance and real Ollama execution were not available.
- Final verification: `cargo test -p sparrow-app --locked -- --quiet` passed (498 passed, 0 failed, 17 ignored; doc tests 0 failed, 8 ignored); `cargo check -p sparrow-app --locked`, targeted `rustfmt --check`, and `git diff --check` passed. Three existing unused-import/dead-code warnings remain in the pre-existing welcome/input/branch-switcher code.
- Still partial: OpenRouter PKCE, GitHub Models, BYO/OpenAI-compatible cloud endpoints with consent/keyring, streaming and cancellation, per-repository disable, recent-commit style matching, provider-specific terms, and the manual live-provider script. The panel's Close hides its preview but cannot interrupt a blocking inference request; the local request has a 90-second timeout.
