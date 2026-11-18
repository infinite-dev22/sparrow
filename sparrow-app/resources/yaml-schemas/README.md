# Dart and Flutter YAML schemas

These bundled schemas power local completion and validation in `pubspec.yaml`
and `analysis_options.yaml`. Sources are recorded in each file's `x-sources`.
Update the JSON data to add fields, types, enum values, descriptions or examples.
No network requests or language server are needed at runtime.

The Rust reader implements `type` (including unions), `properties`,
`additionalProperties`, `required`, `items`, `enum`, `pattern`, `minimum` and
`maximum` and `format: semver`. `description` and `examples` feed completion; `x-key-suggestions`
provides non-exhaustive suggestions for open maps of diagnostic/lint names.
This is a supported subset of JSON Schema, not a general JSON Schema engine.

Custom pubspec fields and plugin configuration remain open. Lint, experiment and
analyzer diagnostic names are SDK/plugin-dependent and are not restricted to a
bundled allowlist. This service checks local field structure and values; it does
not resolve includes, dependency versions, assets, or SDK-specific availability.
The Dart analyzer continues to provide its own diagnostics independently.

Quick fixes replace a single scalar or misspelled key, preserve comments and
formatting, and use the existing revision-checked diff preview and undo flow.
Edits of aliases, anchors, tags, block scalars and containers are withheld.
