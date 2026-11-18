# Sparrow

Sparrow is a native **Dart and Flutter IDE**, implemented in Rust with Floem.

Language intelligence comes from the Dart SDK: analysis, completion, navigation,
rename and code actions. Flutter tooling covers project creation, asset paths,
run/debug configurations, device discovery, testing and DevTools. The IDE also
provides an integrated terminal, Git history, pull requests, notifications and
local appearance themes.

Dart is the supported application language. Flutter project configuration,
localization, documentation, shaders, web resources and generated platform/build
files remain editable, with syntax coloring where available. Other programming
languages open as plain text; executable language plugins and the inherited
plugin marketplace are not supported.

## Getting started

Install the Dart SDK or Flutter SDK and open a local Dart/Flutter project.
Configure SDK paths in Settings if they cannot be found automatically. Use
`pubspec.yaml` for package/project metadata and the run-configuration selector to
configure Dart/Flutter SDK targets. The integrated terminal remains available
for package commands and auxiliary platform builds.

## Guides

- [Build from source](docs/building-from-source.md)
- [Flutter testing](docs/flutter-testing.md)
- [Project creation and assets](docs/flutter-projects-and-assets.md)
- [DevTools](docs/flutter-devtools.md)
- [Run/Debug Configurations](docs/run-debug-configurations.md)
- [Settings](docs/settings-dialog.md)
- [Android Studio appearance and visual verification](docs/android-studio-appearance.md)
- [Dart/Flutter scope and upstream cleanup](docs/dart-flutter-scope.md)

## Development

```sh
cargo build --workspace --locked
cargo test --workspace --lib --locked
```

Rust is the implementation/build language of the IDE, not an additional language
service supplied by Sparrow. See [CONTRIBUTING.md](CONTRIBUTING.md).

## Provenance and license

The editor infrastructure originated in Lapce. Sparrow continues to use
third-party components including Floem and the Xi rope text engine. Their real
package names, source URLs, copyright notices and licenses are preserved.
[Inherited historical documentation](docs/history/upstream-changelog.md) is
separate from current feature documentation. Sparrow is distributed under the
[Apache-2.0 license](LICENSE); individual dependencies retain their own licenses.
