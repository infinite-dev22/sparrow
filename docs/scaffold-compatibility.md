# Scaffold compatibility checks

The registration matrix is an opt-in Rust integration test. It uses real SDKs and real packages; it does not install SDKs or substitute mocked packages for compatibility checks.

```sh
SPARROW_MATRIX_SDKS='["/absolute/path/to/flutter-sdk"]' \
SPARROW_REGISTRATION_EXPORT_ROOT=/tmp/sparrow-matrix-fresh-run \
cargo test -p sparrow-app --lib \
  scaffold::registration::sdk_tests::compatibility_matrix \
  --no-default-features -- --ignored --nocapture
```

The report directory must not already exist. Supply several SDK roots in the JSON array to test several SDKs. The package cases are pinned in [compatibility.json](../sparrow-app/resources/scaffold_analyzer/compatibility.json). Resolution is offline by default; set `SPARROW_MATRIX_ONLINE=1` explicitly if the required packages are not cached and downloads are intended.

Each SDK/package row checks:

- SDK identity using `flutter --version --machine` and exact dependency resolution.
- Analyzer-backed registration without source writes during preview.
- Combined application, idempotency and undo of the generated feature and existing registrations.
- Real Flutter unit/widget tests, including navigation and identity of injected provider state.

The runner writes `report.json` after each row, including SDK metadata, package pins, status, failure details and the fixture path. A failed row does not suppress later rows. Each fixture retains its `pubspec.lock` and `flutter-tests.log`. Preserve the entire report directory as a CI artifact. Package-resolution commands must finish before navigation tests can run; an incompatible or unavailable dependency is a failed row, not a pass or skip.

## Scope

The matrix currently focuses on the combined GoRouter/Provider path. The separate four-scenario integration test still exercises named routes alone, an existing MultiProvider, automatic MaterialApp wrapping, and GoRouter wrapping:

```sh
SPARROW_TEST_FLUTTER_SDK=/absolute/path/to/flutter-sdk \
SPARROW_REGISTRATION_EXPORT_ROOT=/tmp/sparrow-four-scenarios-fresh \
cargo test -p sparrow-app --lib \
  scaffold::registration::sdk_tests::real_flutter_registration_preview_apply_idempotency_and_undo \
  --no-default-features -- --ignored --nocapture
```

That test now executes each fixture's Flutter tests itself. Both commands require access to the selected SDK's cache. Passing on one installed SDK does not establish cross-SDK, cross-platform or broad production-project compatibility.

## Process safety

Normal application tests launch disposable test-helper processes to verify timeout, crash and excessive-output handling. Timed-out or over-producing children are killed and waited for. Output size is checked while the process runs, as well as at completion. Malformed or incomplete success responses are rejected instead of silently accepting an empty registration plan.

## Recorded local run

[Machine-readable results](validation/scaffold-compatibility.json) record the following completed checks on the installed Linux SDK:

| Flutter | Dart | Provider | GoRouter | Result |
| --- | --- | --- | --- | --- |
| 3.50.0 stable | 3.14.0 | 6.1.5+1 | 18.0.1 | Passed |
| 3.50.0 stable | 3.14.0 | 6.1.5+1 | 18.1.4 | Passed |

Each row passed preview/apply/idempotency/undo plus five generated feature/navigation tests (10 Flutter tests total). The normal application suite passed 265 tests; nine opt-in SDK/subprocess fixtures were skipped. Native build passed. Other SDKs, operating systems, package versions and production architectures remain unverified.
