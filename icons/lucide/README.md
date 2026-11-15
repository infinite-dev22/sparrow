# Lucide icons

File-type variants (`filetype-*.svg`) began with **lucide-static 1.64.0** and now include project artwork edits, filled shapes, and brand-specific replacements. The SVG files are authoritative; the manifest records the original Lucide bases rather than an exact recipe for current artwork. Docker uses the supplied Tabler `brand-docker` glyph. Dart and Flutter retain their bundled brand assets.

`filetypes.json` records the upstream icon and accent for each variant. Source: https://registry.npmjs.org/lucide-static/-/lucide-static-1.64.0.tgz and https://github.com/lucide-icons/lucide. The package LICENSE is included here, covering ISC and Feather-derived MIT icons.

Preserve the current SVG artwork when updating mappings; reimporting upstream icons would overwrite project edits. Other Lucide UI icons in this directory retain their existing mappings.
