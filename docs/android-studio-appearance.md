# Android Studio reference layout

The reference is the supplied 1920×1048 Android Studio screenshot. The layout uses a 34 px main toolbar, 32 px editor/tool headers, 35 px side stripes, 420 px Project panel, 24 px status bar, and a Run panel spanning below both Project and editor. Run has a process tab, a left-aligned action toolbar, and a separate scrollable console.

The reference layout is the default. Existing workspaces receive a one-time upgrade of theme, font, spacing and panel placement; later personal layout changes are preserved. **Appearance: Apply Android Studio Reference Layout** remains available to reset those changes. It opens Project, hides the right panels, and opens Run only when a Flutter app is running. Idle workspaces use the full editor height. Existing source files and run configurations are preserved.

Inter Regular/Semibold and JetBrains Mono Regular/Italic are bundled under their SIL Open Font Licenses in `extra/fonts`. The standard `vendored-fonts` build feature loads them into Floem's font database. Built-in Dart and Flutter file icons resolve from the embedded icon assets.

Visual checks use a separate X11 preview with temporary XDG directories; they do not change the working IDE's settings. Project content, devices, run configurations, logs and open tabs remain real application state, so their text and availability differ between projects.

This is a reference-matching implementation, not a verified pixel-perfect match. The remaining differences include the exact JetBrains icon set, editor token font styles/semantic colors, and some toolbar and Project header details.

## Idle and running states

At startup, a saved Flutter Run panel is collapsed when no Flutter session is active. Starting a Flutter session opens Run. Merely discovering devices does not open it. Idle toolbar controls use an outlined green Run icon and a dimmed, disabled hot-reload icon; Stop and hot restart appear only while running. Stopping a session preserves its console output instead of discarding or forcibly hiding it. Run remains available through its tool-window stripe.

## Tool-window reference pass

Dart Analysis at the bottom uses a virtualized Description/Location table with severity filters, live counts, source navigation, and an empty state. Side-docked diagnostics keep the existing grouped view. Device Manager uses connected-device and installed-emulator metadata in a Name/SDK/Type table, supports device selection and emulator launch, and displays unavailable metadata as a dash. Its scroll area sits below the headers rather than covering them.

Dart Analysis and Device Manager share title-bar Hide, Move, and maximize controls. Terminal has compact session tabs, a New Terminal button and a Hide action. The reference preset sizes the right column to approximately 466 px at 1920 px width. Active tool stripes use blue, except Project's neutral selection.

The additional screenshots also document Git history, Logcat, Notifications, pull requests, and Running Devices. This pass does not implement those additional services or substitute mock data for them.

Native before/after captures and interaction results for Dart Analysis, Device Manager, and Terminal are in [the panel visual check](visual-checks/panels.md).

Problems, Logcat, and Git History now have separate bottom tool-stripe entries. Their real-data behavior, native captures, validation, and remaining limitations are documented in [the additional tool-window check](visual-checks/tools.md).

Notifications, Pull Requests and Running Devices now use real Dart/Flutter IDE events, GitHub repository data, and Flutter device discovery. Shared file icons distinguish Flutter project file types in Project and editor tabs. See [workflow captures, checks and limitations](visual-checks/workflows.md).


Project source, test and excluded scopes now inherit through descendants. The
`build`, `.dart_tool`, `.idea`, `target`, `.gradle` and `out` scopes carry a muted
brown background and orange labels and folder icons (file icons keep their normal artwork); `test` and `integration_test` carry a
green scope background. Git changes retain their foreground status colors.
Scope matching uses path components, so `test_helpers` does not inherit `test`.

Project identity is painted once beneath the main workbench. The title bar and
both tool-window stripes expose this shared directional glow while editor and panel
surfaces remain opaque. The frame has a solid base; native vector painting
provides a horizontal amber peak near the project chip and a vertical fade ending
at 300 px. It uses two-stop linear brushes, avoiding radial brushes and the GPU SVG icon atlas. Color comes from `.idea/workspace.xml`'s
`ProjectColorInfo` (custom hex color or associated palette index), including
JSON/CDATA and legacy XML representations. Changes refresh through the project
metadata watcher. Projects without valid color metadata receive a stable
path-derived palette color; no JetBrains configuration is written.

The earlier software-rendered directional glow was checked against the supplied
2026-11-18 16:44 reference. At (100, 1), reference RGB is (94, 67, 49)
and Sparrow is (94, 67, 50); at (500, 1), they are (56, 49, 45) and
(56, 49, 46). Both return to frame RGB (38, 40, 44) at (700, 1)
and (30, 300). These samples check the backdrop, not a pixel-perfect match
of the entire IDE. Custom project colors use the same directional falloff.

The earlier isolated preview selected a CPU adapter and silently fell back to
Tiny Skia; it did not establish GPU correctness. The glow now uses native canvas
rectangles and two-stop linear brushes rather than an SVG texture. This also
avoids texture-atlas pressure from a large SVG at high display scales.

The vector replacement was checked on the Intel i915 hardware adapter at
100% and 200% X11 display scale. The isolated process's DRM render engine
reported submitted GPU work, confirming this check did not use the earlier CPU
fallback. The 200% capture is `target/appearance-hardware-2x.png`.
