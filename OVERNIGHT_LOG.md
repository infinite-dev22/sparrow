# Overnight run log

## Baseline

- 2026-11-25: Started from branch `sparrow`, which was 10 commits ahead of `origin/sparrow`; created `overnight/2026-11-25` without changing the existing branch.
- Pre-existing untracked path `scripts/wayland/` is intentionally preserved and excluded from this work.
- Tool check: Rust 1.99.1, Cargo 1.99.1, Flutter 3.50.0; Google Chrome is installed. `Xvfb`, `xvfb-run`, `xdotool`, `cargo-nextest`, and Chromium are unavailable.
- Available screen-free test tier: Tier 1 is available through Rust unit tests. Tier 3 cannot be used because Xvfb/xdotool are unavailable. Tier 2 and Tier 4 are not yet verified.

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
- Tests: `cargo test -p sparrow-app popup -- --nocapture` (11 passed); `cargo test -p sparrow-app device_choices_resolve_by_unique_stable_ids -- --nocapture` (1 passed); full `cargo test -p sparrow-app` (465 passed, 0 failed, 16 ignored; doc tests 0 failed, 8 ignored); `cargo check -p sparrow-app` passed.
- Known warning: `sparrow-app/src/title/branch_switcher.rs:397` has an unused `Icon::Theme` variant; it is outside this workstream.
- Tier 3 GUI verification is unavailable (`Xvfb`, `xvfb-run`, `xdotool` absent). Tier 2 and Tier 4 remain unverified. No per-screen UI tree/checkmark/label test or manual five-screen script has run; A is therefore partially verified, not claimed complete.
- Review identified extra formatter-driven diffs in touched legacy Rust files; no unrelated behavioral changes were intended. The pre-existing untracked `scripts/wayland/` remains untouched.

### A manual verification script (not run in this environment)

1. In Settings, change a dropdown to its second option, close/reopen it, and verify both the label and highlighted row; change it again through its backing setting and reopen.
2. In Run/Debug Configurations, select a different configuration, reopen the chip, reorder or rename configurations, and confirm the active `DapId` row remains highlighted; test keyboard arrows and Enter.
3. In the Dart web-run dialog, change Browser from Default to Chrome and verify the current browser label and selected row persist after reopening.
4. In Git History, switch Date to Last 7 days and confirm its active row and filter value remain aligned.
5. In Resource Manager, select a module, then change the module list and verify the selected module remains represented; also test All modules.
6. In Mobile Preview, select a non-first device and Flexible device; verify the visible label, stable id, and dimension controls.
7. In Pull Requests, change state, author, and label filters and verify each selected row and query update.
