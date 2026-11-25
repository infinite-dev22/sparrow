# QA cycles

## Cycle A — dropdowns

- Baseline reproduction: added a focused test expecting active option B to select index 1; baseline list selection was `None` and the test failed before the fix.
- Targeted command: `cargo test -p sparrow-app popup -- --nocapture` — 11 passed, 0 failed.
- Device identity command: `cargo test -p sparrow-app device_choices_resolve_by_unique_stable_ids -- --nocapture` — 1 passed, 0 failed.
- Full app tests: `cargo test -p sparrow-app` — 465 passed, 0 failed, 16 ignored; doc tests had 0 failures and 8 ignored.
- Build check: `cargo check -p sparrow-app` — passed.
- Warning: existing unused `Icon::Theme` in `sparrow-app/src/title/branch_switcher.rs:397`.
- Tiers: Tier 1 unit tests ran. Tier 3 was unavailable because `Xvfb`, `xvfb-run`, and `xdotool` are absent. Tier 2 and Tier 4 were not verified or run.
- Manual screen flow, UI tree assertions, layout bounds, contrast, keyboard traversal, screenshots, and image review were not run; do not treat the workstream as visually accepted.