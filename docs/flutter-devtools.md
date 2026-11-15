# Flutter DevTools

Use the command palette actions **Flutter: Open DevTools**, **Flutter: Profile Memory**, and **Flutter: Profile Network**. The Flutter Performance panel also provides these actions, and the Flutter Run toolbar has an Open DevTools shortcut.

Sparrow starts the detected SDK's `dart devtools` server on an available loopback port and opens your default browser. If a Flutter app has reported a VM Service URI, the browser URL connects to that app automatically. Memory and Network actions require an app running in debug or profile mode. Open DevTools can also launch the disconnected home page.

Run in profile mode for representative performance measurements. Available data depends on the app's target platform and runtime mode. This integration uses DevTools' memory and network screens; profiling data is displayed in the browser.

The server is reused within the workspace tab. Reopening an action connects to the current app after an app restart. Closing the workspace tab stops and reaps its server. An exited server is restarted on the next request. Startup failures, unsupported SDKs, missing VM connections, and browser launch errors appear in the Flutter Performance panel.

Local workspaces are supported. Remote port forwarding and embedded DevTools browser views are not included.

Validation: `cargo test -p sparrow-app --lib --offline`. The optional integration test `SPARROW_DEVTOOLS_TEST_DART=/path/to/flutter/bin/cache/dart-sdk/bin/dart cargo test -p sparrow-app --lib dart::devtools::tests::installed_sdk_serves_devtools --offline -- --ignored` starts the installed SDK server, checks its HTTP response, and drops the server owner to clean up. It requires access to Dart's normal SDK/analytics cache directories.
