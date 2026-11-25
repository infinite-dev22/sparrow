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

## Final

- `cargo test -p sparrow-app`: 472 passed, 0 failed, 17 ignored; doctests: 0 failures, 8 ignored.
- `cargo check -p sparrow-app`: passed; one pre-existing unused `Icon::Theme` warning remains.
- Tier 2 headless integration ran successfully; Tier 3 Xvfb/xdotool and Tier 4 visual automation remain unavailable/unverified.