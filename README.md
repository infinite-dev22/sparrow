# Sparrow

Sparrow is a native IDE for Dart and Flutter, built in Rust with Floem. It brings Dart analysis, Flutter run and debug tools, and Android device logs into one workspace.

[Download Sparrow](https://github.com/infinite-dev22/sparrow/releases) · [Install and get started](docs/getting-started.md) · [Build from source](docs/building-from-source.md)

## What you can do

- **Edit Dart with SDK-backed tooling.** Use completion, go to definition, rename, quick fixes, and project inspections. Review dependency conflicts and proposed changes in `pubspec.yaml`.
- **Run, debug, and test Flutter apps.** Create projects, choose a device and run configuration, use hot reload, inspect widgets, and review frame, memory, and network activity. Mobile Preview runs Flutter’s web build in a simulated device inside Sparrow.
- **Work with Android logs.** Stream Logcat from a connected device and filter project logs using the selected Gradle build variant.
- **Navigate and manage a project.** Search files and symbols, replace text across files, preview assets, review Git changes and history, and run shell commands in the integrated terminal.

Sparrow provides language intelligence for Dart. Platform files, assets, and other project files can be edited, with syntax coloring where available.

## Quick start

1. Install [Dart](https://dart.dev/get-dart) for Dart projects or [Flutter](https://docs.flutter.dev/install) for Flutter projects.
2. Open the folder containing `pubspec.yaml`. If Sparrow needs help finding the SDK, set its path under **Settings → Languages & Frameworks**.
3. If dependencies are unresolved, select **Run Pub Get** in **Project → External Libraries**. To run a Flutter app, choose a device and a run configuration, then select **Run** or **Debug**.

## Documentation

| Topic | Guide |
| --- | --- |
| Downloads and first launch | [Getting started](docs/getting-started.md) |
| Projects and dependencies | [Flutter projects and assets](docs/flutter-projects-and-assets.md) · [Pub dependencies](docs/pubspec-dependencies.md) |
| Run, debug, and test | [Run and debug configurations](docs/run-debug-configurations.md) · [Flutter testing](docs/flutter-testing.md) · [DevTools](docs/flutter-devtools.md) |
| Customize the IDE | [Settings](docs/settings-dialog.md) · [Scaffolding](docs/scaffolding.md) |
| Build and contribute | [Build from source](docs/building-from-source.md) · [Contributing](CONTRIBUTING.md) · [Release workflow](docs/new-release.md) |

Sparrow’s editor infrastructure originated in [Lapce](https://github.com/lapce/lapce). Sparrow is distributed under the [Apache-2.0 license](LICENSE). Third-party dependencies retain their own licenses.
