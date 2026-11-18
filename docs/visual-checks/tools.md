# Problems, Logcat and Git history — 2026-11-15

Compared native 1920×1048 X11 captures against the supplied Android Studio screenshots `06-49-17` (Problems), `06-49-11` (Logcat), and `06-49-36` (Git history). Preview settings/data/cache were isolated under `/tmp/sparrow-visual`; existing user settings and repository commits were preserved.

These surfaces did not previously exist: the old Problem panel remains Dart Analysis. Three distinct tool-stripe entries now expose **Problems**, **Logcat**, and **Git History**, including restored installations. Each uses the shared 32 px title bar, Move/Hide menu and double-click maximize behavior.

## Problems

[File diagnostics](tools/problems-file.png), [Project Errors](tools/problems-project-errors.png), [source navigation](tools/problems-navigation.png), [Designer state](tools/problems-designer.png).

File follows the active editor and lists actual language-server diagnostics; Project Errors lists workspace diagnostics with error severity only. Rows navigate to the reported source position. Empty states identify the active file or absence of project errors. Designer explicitly reports that Sparrow has no designer diagnostic service.

Verified using the disposable Dart project: an undefined function produces a real error on line 2, and an unused local produces a warning on line 3. File displays both, Project Errors displays only the error, and clicking the error navigates to line 2.

## Logcat

[Native empty state](tools/logcat.png).

Uses adb from Android SDK environment variables, the conventional SDK installation, or PATH. Only devices in adb's `device` state are selectable; offline and unauthorized devices are excluded. Selecting a device polls `adb -s SERIAL logcat -d -t 500 -v threadtime` every two seconds while visible and unpaused. Text filtering is case-insensitive. Pause holds the displayed snapshot; Refresh resumes polling. Clear removes displayed text and pauses, without clearing the device's log buffer. Commands have a five-second timeout, and stale device/pause/clear results are ignored.

No Android device was connected during verification. The actual no-device state was rendered; live Android log retrieval and selected-device filtering remain unverified. No sample logs were inserted.

## Git history

[Commit details](tools/git-details.png), [text filtering](tools/git-filter.png).

Local, read-only `git log`, `for-each-ref`, and `show --stat` queries provide commits, refs, authors, dates, and selected-commit summaries. The list is limited to 200 commits. Click a branch or HEAD to scope the list, or enter a revision in the branch field; empty means all refs. Text filtering searches subjects, authors, hashes and refs. Refresh reloads repository state. Revision text is separated from Git options. Late results cannot overwrite newer branch requests or commit selections.

Verified against Sparrow's actual repository: selected a master-branch commit and loaded its real metadata/file summary, switched to HEAD, and filtered to the existing DevTools commit. No Git history was fabricated or changed.

## Remaining visual and feature gaps

This is not full Android Studio parity. Exact JetBrains glyphs, font metrics, Problems action rail, Git branch grouping/graph edges, Git Console and advanced history filters, Logcat query syntax, and multi-session Logcat tabs remain absent. Git details currently show file-change statistics, rather than an interactive diff browser. Logcat uses bounded snapshots rather than an unbounded stream.

Validation: final `cargo build -p sparrow --bin sparrow --offline` passed with existing warnings; `cargo test -p sparrow-app --lib --offline` passed (128 passed, 0 failed, 1 ignored); `git diff --check` passed. Added tests for Git record parsing and adb connected/offline/unauthorized device parsing.

Capture timing: Problems/Logcat captures and Git interaction captures were taken before the final small change that clears selection when its commit is filtered out. The final build and library tests include that guard; a fresh native verification of that guard was inconclusive because preview window focus became unreliable. The earlier Git filtering and selection captures remain valid for those independently verified behaviors.
