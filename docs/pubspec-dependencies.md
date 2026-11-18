# Dependency assistance in pubspec.yaml

In `dependencies`, `dev_dependencies`, or `dependency_overrides`, type part of a
package name and press **Ctrl+Space** to search pub.dev package names. Accept a
name, then request completion after its colon or while editing its version.
Version completion inserts a caret constraint. Stable releases appear first;
type a prerelease prefix to request prerelease versions explicitly.

Registry responses are fetched off the UI thread. Name completion uses pub.dev's
[documented name index](https://pub.dev/help/api), cached for at least eight hours.
The index is ranked and may omit uncommon packages; an exact-name metadata lookup
also supports those packages. Package version metadata uses the
[Hosted Pub Repository API](https://github.com/dart-lang/pub/blob/master/doc/repository-spec-v2.md).
It is cached for 30 minutes, with disk-cache fallback when offline. Network
failures do not produce missing-package errors. HTTP 404 responses do.

Inline diagnostics check published versions, retractions, discontinued packages,
selected/declared SDK constraints, and conflicts between a candidate release's
requirements and the project's direct dependencies. Explicit overrides take
precedence. Checks use Pub's caret semantics, Dart's null-safe legacy SDK-bound
compatibility, and Flutter's lower-bound-only SDK constraints.

**Alt+Enter** offers version changes for invalid/unavailable constraints and newer
SDK-compatible releases. Changes use the revision-checked diff preview and normal
undo; files are not saved automatically.

The IDE also runs the selected SDK's Pub solver against an isolated copy of the
manifest graph. This includes local path packages, workspace members and
`pubspec_overrides.yaml`, using the active unsaved pubspec and saved sibling
manifests. It does not write the source project's manifests, lockfile or
`.dart_tool`; Pub may populate its shared download cache.

On a conflict, the inline error retains Pub's causal dependency explanation.
The IDE asks `pub outdated --json` for a coordinated set of resolvable versions,
then runs another `pub upgrade --dry-run` with precisely the proposed edits.
Only a successful second solve yields a **Pub-verified compatible versions**
quick fix. If a version-only repair cannot be verified, the conflict explanation
remains available. Local path package constraints and SDK requirements may need
manual changes. Version compatibility does not guarantee source/API compatibility.

Pub-backed fixes use the same diff preview and linked undo across workspace
manifests. The IDE rejects stale fixes when a dependency manifest, override,
lockfile, SDK-selection setting or relevant open buffer has changed. Save sibling
manifest edits and edit/reload the active pubspec to refresh a check.

Checks debounce while typing, cancel superseded processes, cap output and graph
size, and time out after 90 seconds of Pub work. Results are cached briefly;
missing SDKs, network errors and timeouts are reported as unavailable checks,
rather than successful resolution. **Pub Get** remains the explicit action that
writes package resolution into the project.

Path, Git, SDK and custom-hosted packages are excluded from public-registry
lookups, but are included in the actual Pub solve. A custom `PUB_HOSTED_URL`
disables default pub.dev lookups; an explicit pub.dev hosted source can still be
checked. The selected SDK and existing Pub authentication/cache are respected.
