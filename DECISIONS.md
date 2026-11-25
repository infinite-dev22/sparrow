# Decisions

## 2026-11-25

- Work on the new branch `overnight/2026-11-25`, created from the current checkout as requested. Do not change the original `sparrow` branch.
- Preserve the pre-existing untracked `scripts/wayland/` directory; do not add it to commits or otherwise modify it.
- For controlled dropdown options, use the option type's equality identity to locate the active item in the option list. Do not infer identity from display text or rely on a separately stored selection index.
- Do not use real credentials, sign-ins, remote pushes, desktop input, or user-facing screenshots. Headless fixture tests may capture frames in memory only; do not save screenshots.
- Use Tier 1 tests for component logic and the verified Tier 2 headless Chrome/CDP fixture for renderer integration when relevant. Do not install GUI automation tools or request permissions as part of this unattended run.
- Keep the run-configuration popup custom because it combines grouped rows with per-row Run/Debug/Edit actions; replace positional popup identity with `DapId` and keep its item list reactive.
- Keep the project-type picker as an always-visible radio group and the file-picker/Git action popups as action menus; neither represents a persistent selected value.
- Migrate value-selection menus to `dropdown_with_label` with stable source values; use display-label closures only for presentation.
- Mark Workstream A partially verified until the documented multi-screen UI script can be run; passing component tests do not establish visual or keyboard acceptance.
- Keep Flutter's official Widget Previewer as the widget catalog and do not rewrite/inject user source; apply Sparrow's device profile to the browser context while Flutter `@Preview` metadata owns per-widget settings.
- For VCS credentials, use keyring 4.2.0's explicit `v1` API (Keychain, Windows Credential Manager, Linux Secret Service); do not silently use plaintext files or select Linux keyutils as an undocumented fallback. If the secure store is unavailable, keep a token in process memory only and show that limitation.
- For the Git CLI, route credential prompts through Sparrow's own askpass entry point with credentials in the child environment; never put tokens in Git arguments, URLs, or repository config. SSH remotes remain SSH-agent-only.
- Keep account host/username/status metadata in normal Sparrow config storage, separate from the keyring secret, and sanitize all HTTP/Git errors before presenting them.
- Use a host-provider boundary with GitHub.com and GitHub Enterprise API support; keep generic HTTPS Git credentials separate and do not claim their tokens were API-validated.
- Keep merge/close controls and GitLab/Bitbucket PR APIs out of this iteration; the prior panel had no merge/close actions. PR list, details, diff/files/comments, and explicit creation are implemented, but require live-account verification.
- Use the `GET /user` scope header only when GitHub reports it; a missing `X-OAuth-Scopes` header does not reliably identify a fine-grained token. Use GitHub's accepted-permissions and SSO headers for actionable API errors without including request bodies or tokens.
- For AI commit messages, this pass is local Ollama only: no cloud endpoint or provider key is accepted. Send only checked commit paths after redaction and an explicit generation click; the exact prompt is shown first. Do not claim cloud providers until their consent/auth/key storage flows and tests exist.
- For Resource Manager manifest changes, prefer targeted edits to a block-style `flutter.assets` sequence over deserialize/serialize; preserve unrelated comments/formatting and refuse unsupported flow-style forms with a clear error. Do not run `flutter pub get` automatically.
- For the initial Resource Manager import checkpoint, retain a reviewed `AssetImportPlan`, revalidate the destination and manifest before applying it, and reject collisions rather than overwrite. Destination folders append the selected filename; batch conflict choices and undo remain deferred, with non-destructive rejection as the interim default.
