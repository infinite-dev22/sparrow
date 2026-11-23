# Native Wayland Project header verification

The desktop portal granted screen capture and keyboard/pointer input. Testing
used the normal GNOME Wayland session (`wayland-0`), eDP-1 at 1920×1080,
60.052 Hz, scale 1, with no renderer override. The test workspace and copied
appearance settings were isolated from the user's source files and preferences.

| Check | Observed result |
| --- | --- |
| Project dropdown opens | Passed |
| Show Open Editors toggles on/off | Passed; section appeared and disappeared |
| Collapse All | Passed; nested directories closed and root remained visible |
| Expand Loaded Folders | Passed; previously loaded descendants reopened |
| Select Opened File | Passed; reopened lib/nested and selected main.dart |
| New → File | Passed; portal_created.dart was created on disk |
| New → Directory | Passed; portal_directory created and confirmed on disk |
| Dropdown Refresh Project | Passed; menu action returned to populated tree |
| Options Refresh | Passed; menu action returned to populated tree |
| Move to Left/Right/Bottom | Passed; moved right → bottom → left with tree and editor preserved |
| Hide and reopen Project | Passed; hidden with header minus and restored from its stripe |
| Active Project stripe blue | Passed after fix: filled blue while hovered after right/bottom/left docking and hide/reopen |
| Project glow | Passed after fix: warm peak and fade present in all three docking captures |

Test captures are under `target/wayland-test/`. Capture approval is scoped to a
live portal session and is closed after testing. The first helper terminated
with SIGTERM before all interactions completed; a replacement approval request
was approved and all remaining header interactions were completed. No all-pass
visual result is claimed.

## Captured evidence

- `target/wayland-test/toggle-after.png`: Open Editors visible.
- `target/wayland-test/collapsed.png`: active main.dart with its tree path collapsed.
- `target/wayland-test/expanded.png`: loaded folders expanded.
- `target/wayland-test/reveal.png`: active file revealed and selected.
- `target/wayland-test/new-fixture.png`: New menu with scaffold catalog.
- `target/wayland-test/created.png`: created Dart file in the tree.
- `target/wayland-test/options.png`: new directory and Project options.
- `target/wayland-test/right.png`, `bottom.png`: docking results.
- `target/wayland-test/hidden.png`, `reopened-project.png`: hide/reopen results.

## Visual fixes and completed retest (2026-11-19)

The native gradient-brush canvas was replaced by a Rust-generated opaque
180×80 texture displayed over 700×300 logical pixels. A padded image and clip
viewport prevent sampling the icon atlas's transparent gutter at the edges.
The texture is regenerated on appearance changes, rather than on every paint.

The generic clickable icon's nested hover background covered the active blue
fill. Tool-window stripe icons now own their background and pointer states;
active hover/press retain blue, with 16 px white artwork inside a 28 px button.

The final build was tested through approved ScreenCast/RemoteDesktop portals on
`wayland-0`, with the normal scale-1 output and no renderer override. Project
was hidden and reopened in each dock, with the pointer resting on its icon.
The same glow and blue fill were verified in all three captures:

| Capture under target/wayland-test/ | Active fill RGB | Glow peak RGB | Frame RGB |
| --- | --- | --- | --- |
| verified-right-hover.png | 53,116,240 | 93,66,49 | 38,40,44 |
| verified-bottom-hover.png | 53,116,240 | 93,66,49 | 38,40,44 |
| verified-left-hover.png | 53,116,240 | 93,66,49 | 38,40,44 |

These are native desktop-portal captures, not the earlier X11/software preview.
The original failed observations above describe the initial verification; both
visual defects now pass. The test workspace used copied application settings
and its own `.sparrow/settings.toml` project color. Testing did not alter the
user's normal workspace or display settings. Screen sharing and remote input
were closed after verification.

## Narrow header interaction retest (2026-11-19)

Tested the rebuilt `target/debug/sparrow` through a newly approved desktop
portal on native GNOME Wayland, 1920×1080 at scale 1, without a renderer
or display override. Preferences were copied into `target/header-wayland/`;
all created files belong to its isolated workspace.

| Dock | Narrow width | Header | Actions verified |
| --- | --- | --- | --- |
| Left | 150 px panel minimum | Title ellipsis; all three buttons inside panel | New File, More → Refresh, Hide, stripe reopen, Move to Right |
| Right | 150 px panel minimum | Title ellipsis; all three buttons inside panel | New File, More → Refresh, Hide, stripe reopen, Move to Bottom |
| Bottom | approximately 152 px (window narrowed to 220 px) | Title ellipsis; all three buttons inside panel | New Directory, More → Refresh, Hide, stripe reopen, Move to Left |

No header overflow or failed header action was observed at these widths.
The extremely narrow window also clips unrelated title-bar controls; this
check only covers the Project panel header. The current header has New,
More and Hide buttons; its Project label has no dropdown action.

Evidence under `target/wayland-test/`: `header-left-min.png`,
`header-right-min.png`, `header-bottom-narrow.png`, the corresponding
`header-*-new.png`, `header-*-options.png`, `header-*-hidden.png`, and
`header-left-reopened.png`. Created files were confirmed on disk:
`left_created.txt`, `right_created.txt`, and directory `bottom_created`.
The test window and capture/input session were closed after verification.
