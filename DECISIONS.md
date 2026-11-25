# Decisions

## 2026-11-25

- Work on the new branch `overnight/2026-11-25`, created from the current checkout as requested. Do not change the original `sparrow` branch.
- Preserve the pre-existing untracked `scripts/wayland/` directory; do not add it to commits or otherwise modify it.
- For controlled dropdown options, use the option type's equality identity to locate the active item in the option list. Do not infer identity from display text or rely on a separately stored selection index.
- Do not use real credentials, sign-ins, remote pushes, desktop input, or screenshots. Use local fixtures/mocks and screen-free tests only.
- Until an automation capability is verified, use Tier 1 tests. Do not install GUI automation tools or request permissions as part of this unattended run.
- Keep the run-configuration popup custom because it combines grouped rows with per-row Run/Debug/Edit actions; replace positional popup identity with `DapId` and keep its item list reactive.
- Keep the project-type picker as an always-visible radio group and the file-picker/Git action popups as action menus; neither represents a persistent selected value.
- Migrate value-selection menus to `dropdown_with_label` with stable source values; use display-label closures only for presentation.
- Mark Workstream A partially verified until the documented multi-screen UI script can be run; passing component tests do not establish visual or keyboard acceptance.
