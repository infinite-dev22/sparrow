# QA cycles

## Cycle A — dropdowns

- Baseline reproduction: added a focused test expecting active option B to select index 1; baseline list selection was `None` and the test failed before the fix.
- Targeted command: `cargo test -p sparrow-app popup -- --nocapture` — 11 passed, 0 failed.
- Device identity command: `cargo test -p sparrow-app device_choices_resolve_by_unique_stable_ids -- --nocapture` — 1 passed, 0 failed.
- Full app tests: `cargo test -p sparrow-app` — 472 passed, 0 failed, 17 ignored; doc tests had 0 failures and 8 ignored.
- Build check: `cargo check -p sparrow-app` — passed.
- Warning: existing unused `Icon::Theme` in `sparrow-app/src/title/branch_switcher.rs:397`.
- Tiers: Tier 1 unit tests ran. Tier 2 was not used for A and was verified later in B1. Tier 3 was unavailable because `Xvfb`, `xvfb-run`, and `xdotool` are absent; Tier 4 remains unverified.
- Manual screen flow, UI tree assertions, layout bounds, contrast, keyboard traversal, screenshots, and image review were not run; do not treat the workstream as visually accepted.

## Cycle B1 — Mobile Preview startup and renderer

- `flutter run --no-pub -d web-server --web-hostname 127.0.0.1 --web-port 0 --machine` in `/tmp/sparrow-preview-repro-Vkabrr` emitted a local `app.webLaunchUrl` and `app.started`; timeout exit 124 followed an intentional SIGINT after ready.
- `cargo test -p sparrow-app accepts_only_the_flutter_local_launch_event -- --nocapture` passed, including the actual `launched:false` JSON-array event shape.
- `cargo test -p sparrow-app captures_and_interacts_with_a_windowless_device_surface -- --ignored --nocapture` passed (1 test, 0.77s); verifies headless Chrome/CDP rendering, locale/dark-mode/viewport updates, touch and in-memory frame capture.
- Result is not reproduced on a clean sample; user-project-specific failure remains open. No UI screenshot or source-project change.

## Cycle B2/B3 — build targets and Flutter modes

- `flutter build web --release --no-pub` in the disposable scaffold passed (62.3s); Android/iOS manifests were not involved in Mobile Preview startup.
- Confirmed `flutter run -h --verbose` documents `web-server`, `--machine`, `--web-hostname`, and `--web-port`; `flutter widget-preview start --help --verbose` documents `--web-server` and `--machine`.
- Disposable widget-preview emitted the URL/started events but had no declared preview widget, so this is protocol validation rather than visual widget-preview acceptance.
- Sparrow repository contains no runnable first-party Flutter project; native manifest/permission requirements remain app-specific.

## Cycle B4/B5 — automatic and manual reload

- Server unit coverage: JSON-RPC `app.restart` fields and request IDs, app ID/result parsing, Dart-path filtering, and settings defaults/overrides.
- `SPARROW_PREVIEW_TEST_PROJECT=/tmp/sparrow-preview-repro-Vkabrr SPARROW_FLUTTER_BIN=/home/infinite/develop/flutter/bin/flutter cargo test -p sparrow-app dart_save_triggers_hot_reload_and_machine_protocol_supports_restart -- --ignored --nocapture` passed (1 test, 18.13s).
- The ignored test changed one Dart comment in the disposable app, observed an automatic non-full reload response, then observed a successful full restart; it restored the original source and stopped the process afterward.

## Cycle B6/B7 — profiles, locale, and diagnostics

- Device catalog tests validate 6 Android phones, 6 iPhones, one iPad, one Android tablet, 3 generic sizes, unique IDs, dated provenance, fold panels, tablet dimensions and invalid definitions.
- Headless CDP test confirms `fr-FR` takes effect in `Intl.NumberFormat`, alongside device scale, dark mode and touch.
- Diagnostic report unit test confirms Flutter/Dart version and useful mode/profile/viewport state while excluding project URL, raw status text, source, paths and secrets; event history is capped at 50 entries.
- No report file, screenshot file, user Flutter workspace, account, or remote service was used.

## Cycle B8 — Flutter Widget Previewer device context

- Regression test reproduced the issue before the fix: landscape iPhone 18 Pro still selected the fixed `(800, 760)`, 1× desktop viewport. Afterward it passed with `(874, 402)`, 3×, mobile/iOS context, dark mode and `fr-FR`.
- All three Mobile Preview session tests passed; the headless Chromium/CDP device-surface integration test passed.
- Full `cargo test -p sparrow-app`: 474 passed, 0 failed, 17 ignored; doc tests had 0 failures and 8 ignored. Edited-file `rustfmt --check` and `git diff --check` passed.
- No annotated Flutter sample was available for end-to-end Widget Previewer visual review; visual acceptance remains unverified.

## Final

- `cargo test -p sparrow-app`: 474 passed, 0 failed, 17 ignored; doctests: 0 failures, 8 ignored.
- `cargo check -p sparrow-app`: passed; one pre-existing unused `Icon::Theme` warning remains.
- Tier 2 headless integration ran successfully; Tier 3 Xvfb/xdotool and Tier 4 visual automation remain unavailable/unverified.

## Cycle C — VCS credentials and pull requests

- Account metadata round-trip confirms the serialized account record contains identity/provider/status/storage metadata but no token. Host parsing rejects embedded credentials and secret query parameters; remote parsing distinguishes HTTPS credentials from SSH-agent remotes.
- Local HTTP mock tests confirm authenticated requests use `Authorization: Bearer`, capture `X-OAuth-Scopes`, keep the token out of the request URL, and send POST JSON for PR creation. API errors use sanitized status/header-based messages.
- Git action tests reject credential-bearing HTTP URLs/arguments, redact userinfo/query secrets and known tokens from Git output, and resolve `fetch --all` plus remotes after option values. No real Git push/fetch was attempted.
- PR panel unit tests verify GitHub/GHES remote parsing, detail/diff/comment rendering, and local validation of required creation fields. Detail and list pagination enforce 500-item and 2,000-PR safety caps.
- `cargo test -p sparrow-app`: 492 passed, 0 failed, 17 ignored; doc tests: 0 failures, 8 ignored. Existing unused `Icon::Theme` warning remains.
- No real token, OS keyring service, private repository, Git operation over HTTPS, or desktop UI was exercised. Human verification remains required before relying on OS keyring or private remote behavior.

## Cycle F — Local AI commit messages

- Local loopback validation rejects remote endpoints and credentials; the mock Ollama server verifies `/api/tags`, `/v1/chat/completions`, model selection, bounded output tokens, and the redacted prompt body.
- Context tests cover checked added/tracked files, excluded `.env`, private-key/token/authorization redaction, path traversal rejection, diff limits, and subject cleanup. The implementation disables external diff/textconv and caps tracked Git output.
- `cargo test -p sparrow-app ai_commit -- --nocapture`: 6 passed, 0 failed. No live Ollama model or desktop interaction was used.
- `cargo test -p sparrow-app --locked -- --quiet`: 498 passed, 0 failed, 17 ignored; doc tests: 0 failures, 8 ignored. `cargo check -p sparrow-app --locked`, targeted Rust formatting, and `git diff --check` passed.
- Cloud providers, OAuth, real consent, cancellation, style matching, and per-repository disable remain partial/unimplemented; see `BUGS.md` and `QUESTIONS.md`.

## Cycle D — Resource Manager import and manifest edits

- Regression fixture uses CRLF, comments, blank lines, odd indentation, an existing scalar asset entry, and fonts; it confirms add, rename, and remove affect only intended lines and preserve the inline comment.
- Preview/apply tests confirm preview does not write, folder destinations append the source basename, application updates the file and manifest, stale manifests are rejected, and a destination occupied after preview is preserved. Flow-style asset blocks are rejected safely.
- `cargo test -p sparrow-app --locked panel::resource_manager::tests:: -- --nocapture`: 7 passed, 0 failed. `rustfmt --edition 2024 --check --config skip_children=true sparrow-app/src/panel/resource_manager/view/import_form.rs` and `git diff --check` passed.
- Initial checkpoint full validation: `cargo test -p sparrow-app --locked -- --quiet` — 503 passed, 0 failed, 17 ignored; doctests had 0 failures, 8 ignored. `cargo check -p sparrow-app --locked` passed.
- Batch extension: `cargo test -p sparrow-app --locked panel::resource_manager::tests:: -- --nocapture` — 9 passed, 0 failed. New cases verify existing targets are skipped without modification, only copied assets enter the manifest, and stale manifests prevent the batch from copying any files.
- Full app verification after batch changes: `cargo test -p sparrow-app --locked -- --quiet` — 505 passed, 0 failed, 17 ignored; doctests had 0 failures, 8 ignored. `cargo check -p sparrow-app --locked` passed.
- Drag-and-drop follow-up: focused import-form tests — 2 passed, 0 failed; full `cargo test -p sparrow-app --locked -- --quiet` — 507 passed, 0 failed, 17 ignored; doctests had 0 failures, 8 ignored. The import-form `rustfmt --check` and `git diff --check` passed.
- Keep both follow-up: `cargo test -p sparrow-app --locked panel::resource_manager::tests:: -- --nocapture` — 11 passed, 0 failed; `cargo test -p sparrow-app --locked resource_manager::view::import_form::tests -- --nocapture` — 2 passed, 0 failed; full `cargo test -p sparrow-app --locked -- --quiet` — 509 passed, 0 failed, 17 ignored; doctests: 8 ignored; `cargo check -p sparrow-app --locked` passed. Tests cover suffixing past occupied/duplicate names, manifest entries, and a collision introduced after preview. The targeted import-form formatter check passed; three existing compiler warnings remain.
- Explicit Pub Get offer follow-up: Resource Manager tests — 12 passed; import-form tests — 2 passed; full app suite — 510 passed, 0 failed, 17 ignored; doctests — 8 ignored; `cargo check -p sparrow-app --locked` passed. Tests verify the offer predicate for changed versus unchanged manifests. The UI offers the command only after a manifest-changing import and requires a click; no Flutter command or visual UI check was run. Three existing warnings remain.
- The full Resource Manager module has existing rustfmt drift; the check reported unrelated reflows across the file, so no broad formatting sweep was applied. Multi-selection UI, visual review, and actual Flutter consumption remain unverified.
- The upstream `android/ui.resourcemanager/` source could not be fetched because DNS resolution timed out; the official Android Resource Manager guide was reviewed and confirms drag-and-drop and bulk import conventions.
- No GUI drag-and-drop or conflict-choice interaction, Flutter build/pub-get, font registration, or real project test was run. Batch import offers non-destructive Skip and Keep both; Replace, undo, full asset management, and visual acceptance remain open; see `OVERNIGHT_LOG.md`, `BUGS.md`, and `QUESTIONS.md`.