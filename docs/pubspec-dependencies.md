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
undo; files are not saved automatically. No package installation runs implicitly.

These are conservative SDK/direct-dependency checks, not a transitive dependency
solver or a guarantee of source/API compatibility. Run **Pub Get** after choosing
versions to perform complete resolution. Path, Git, SDK and custom-hosted
packages are excluded from public-registry checks. A custom `PUB_HOSTED_URL`
disables default pub.dev lookups; an explicit pub.dev hosted source can still be
checked.
