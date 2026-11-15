# Android Studio reference layout

The reference is the supplied 1920×1048 Android Studio screenshot. The layout uses a 34 px main toolbar, 32 px editor/tool headers, 35 px side stripes, 400 px Project panel, 24 px status bar, and a Run panel spanning below both Project and editor. Run has a process tab, a left-aligned action toolbar, and a separate scrollable console.

For existing installations, run **Appearance: Apply Android Studio Reference Layout** from the command palette. This deliberately replaces saved theme, font, spacing, and panel placement overrides with the reference preset. It opens Project, hides the right panels, and opens Run only when a Flutter app is running. Idle workspaces use the full editor height. Existing source files and run configurations are preserved.

Inter Regular/Semibold and JetBrains Mono Regular/Italic are bundled under their SIL Open Font Licenses in `extra/fonts`. The standard `vendored-fonts` build feature loads them into Floem's font database. Built-in Dart and Flutter file icons resolve from the embedded icon assets.

Visual checks use a separate X11 preview with temporary XDG directories; they do not change the working IDE's settings. Project content, devices, run configurations, logs and open tabs remain real application state, so their text and availability differ between projects.

This is a reference-matching implementation, not a verified pixel-perfect match. The remaining differences include the exact JetBrains icon set, editor token font styles/semantic colors, and some toolbar and Project header details.

## Idle and running states

At startup, a saved Flutter Run panel is collapsed when no Flutter session is active. Starting a Flutter session opens Run. Merely discovering devices does not open it. Idle toolbar controls use an outlined green Run icon and a dimmed, disabled hot-reload icon; Stop and hot restart appear only while running. Stopping a session preserves its console output instead of discarding or forcibly hiding it. Run remains available through its tool-window stripe.
