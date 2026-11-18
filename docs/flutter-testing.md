# Flutter testing

Open a local Flutter project with a `pubspec.yaml`. Sparrow uses the existing SDK discovery, including project FVM configuration.

In `test/**/*_test.dart`, the gutter displays Run and Debug controls beside `test`, `testWidgets`, and `group` declarations with static string names. Nested group names are included automatically. Dynamic names, interpolated names, and imported aliases are not currently discovered.

The command palette also provides:

- **Flutter: Run Test at Cursor**
- **Flutter: Debug Test at Cursor**
- **Flutter: Run Test File**
- **Flutter: Re-run Failed Tests**

Edited files are saved before launching. Individual tests and groups use `flutter test --plain-name`. Group selection runs the matching descendants. Names are passed directly as arguments, including spaces and punctuation.

The **Flutter Test** bottom panel displays an expandable suite/group/test tree, live status, skipped tests, and durations. Select a test to see its output and an Expected/Actual comparison when supplied by the assertion. Click a file reference in a stack trace to navigate to the assertion. Both `file.dart:line:column` and `file.dart line:column` are supported, along with file URIs and package URIs resolved through `.dart_tool/package_config.json`. **Show Full Console** returns to the complete run output.

Use **Run Again**, **Re-run Failed Tests**, or **Stop Tests** in the panel toolbar. Failed-test reruns use escaped, anchored `--name` expressions so a passing test with a longer matching name is excluded. Suite loading and setup/teardown failures rerun the affected file. Failed tests from multiple files run sequentially and appear in the same result tree.

Debugging uses the installed SDK's `flutter debug-adapter --test`; no debugger plugin is required. Set editor breakpoints, then click Debug. The Flutter Test toolbar provides Resume, Step Over, Step Into, Step Out, and Stop. The Debug panel shows variables and stack frames while paused. Debug results use the same test tree through Flutter's `dart.testNotification` events. A Flutter SDK that supports the test debug adapter is required.

This implementation supports local workspaces. Remote execution and Phase 8 project/asset wizards are outside this phase.

## Protocol references

- [Dart JSON test reporter](https://github.com/dart-lang/test/blob/master/pkgs/test/doc/json_reporter.md)
- [Flutter test debug adapter](https://github.com/flutter/flutter/blob/master/packages/flutter_tools/lib/src/debug_adapters/flutter_test_adapter.dart)

## Validation

Run `cargo check -p sparrow-app --offline` and `cargo test -p sparrow-app --lib --offline`.

The regression fixture at `sparrow-app/src/dart/fixtures/flutter_test_events.jsonl` was captured from a temporary Flutter project with nested groups, passing/failing unit tests, a widget test, and a skipped test. The parser tests also replay it with multiple process namespaces to verify that test/group/suite IDs do not collide during failed-test reruns.
