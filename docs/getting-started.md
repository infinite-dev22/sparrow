# Getting started

## Download

Choose an asset from [GitHub Releases](https://github.com/infinite-dev22/sparrow/releases) for your operating system and processor.

| Platform | Download |
| --- | --- |
| macOS, Apple Silicon | `Sparrow-macos.dmg` |
| Windows, ARM64 | `Sparrow-windows-arm64-portable.zip` |
| Windows, x86-64 | `Sparrow-windows.msi` or `Sparrow-windows-portable.zip` |
| Windows, 32-bit x86 | `Sparrow-windows-i686-portable.zip` |
| Linux, ARM64 | `Sparrow-linux-arm64.tar.gz` |
| Linux, x86-64 | `Sparrow-linux-amd64.tar.gz` |
| Linux, 32-bit x86 | `Sparrow-linux-i686.tar.gz` |

The matrix takes effect with releases produced by the new workflow. Existing releases may have a different set of assets. Windows and Linux x86-64 builds require an x86-64-v2 CPU. Linux releases use glibc and the desktop libraries from Debian 12 or a compatible distribution. The macOS image is ad-hoc signed. Verify downloads with the `SHA256SUMS` file included in each release.

Open the DMG and drag **Sparrow** into Applications on macOS. On Windows, run the MSI or extract the portable ZIP and launch `sparrow.exe`. On Linux, extract the archive and run `./Sparrow/sparrow` from the extraction directory. If Linux reports a missing system library, see [Build from source](building-from-source.md) for the dependency list.

## Open a project

Install the [Dart SDK](https://dart.dev/get-dart) or [Flutter SDK](https://docs.flutter.dev/install). Open the folder that contains `pubspec.yaml`, then use **Settings → Languages & Frameworks** to configure SDK paths if needed. Open **Project → External Libraries** and select **Run Pub Get** if dependencies are unresolved.

For Flutter, select a run configuration and a device in the toolbar. See [run and debug configurations](run-debug-configurations.md) or [Flutter testing](flutter-testing.md) to set up the rest of your workflow. Mobile Preview uses Flutter’s web build. Use a simulator or a physical device to check native behavior.
