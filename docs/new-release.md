# Releasing Sparrow

The [Release workflow](../.github/workflows/release.yml) builds seven target-specific packages on GitHub-hosted runners. It waits for every target, checks that the expected assets exist, and generates `SHA256SUMS` before publishing.

- **Validation** — Run the workflow on a branch with **Publish the artifacts to GitHub Releases** unchecked to build and inspect the artifacts. Pull requests that change release inputs also build packages.
- **Stable** — Update the workspace version in `Cargo.toml`, the changelog, the macOS bundle version and the MSI version. Push a matching version tag such as `v0.4.7` on the release commit. To rerun through manual dispatch, select the same commit and supply its existing tag, then enable publishing.
- **Nightly** — The scheduled run builds the default branch and updates the `nightly` prerelease and tag. Manual dispatch with the `nightly` tag and publishing enabled does the same for the selected commit.

The workflow publishes an Apple Silicon DMG, Windows portable ZIPs for all three architectures and an x86-64 MSI, and GNU/Linux archives for all three architectures. See the [download table](getting-started.md#download) for the asset names. Separate architecture-specific `sparrow-proxy` binaries are available for remote workspaces. The Linux proxy binaries require glibc. Distribution packages are built separately through the recipes in `docker-bake.hcl`.

## CPU optimization

Release packages use the `release-lto` profile with link-time optimization and one code generation unit. The workflow sets an explicit CPU target for each build. It uses Apple M1 for macOS, x86-64-v2 for 64-bit x86, Pentium 4 for 32-bit x86, and the compiler’s generic ARM64 CPU for Windows and Linux ARM64. These release settings target users’ machines independently of the runner’s CPU. A native build from source uses `target-cpu=native` to optimize for the machine compiling Sparrow.

## Signing

The DMG contains an ad-hoc signed app. Developer ID signing, Apple notarization, and Windows Authenticode signing require separately provisioned credentials.
