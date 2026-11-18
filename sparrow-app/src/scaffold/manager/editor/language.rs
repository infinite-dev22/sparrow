//! Offline pack-aware language assistance. No project files or analyzer overlays.
use super::{Draft, engine};
use crate::config::{SparrowConfig, color::SparrowColor};
use floem::{
    text::{Attrs, AttrsList, FamilyOwned},
    views::editor::{
        EditorStyle, id::EditorId, layout::TextLayoutLine, text::Styling,
    },
};
use sparrow_core::{language::SparrowLanguage, style::line_styles, syntax::Syntax};
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    ops::Range,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Diagnostic {
    pub path: PathBuf,
    pub range: Range<usize>,
    pub message: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Completion {
    pub label: String,
    pub insert: String,
    pub range: Range<usize>,
}
pub(super) fn placeholders(text: &str) -> Vec<(Range<usize>, &str)> {
    let mut result = vec![];
    let mut offset = 0;
    while let Some(start) = text[offset..].find("{{") {
        let start = offset + start;
        if text[start + 2..].starts_with('{') {
            offset = start + 1;
            continue;
        }
        let end = text[start + 2..]
            .find("}}")
            .map(|n| start + 2 + n + 2)
            .unwrap_or(text.len());
        let key = text[start + 2..end]
            .strip_suffix("}}")
            .unwrap_or(&text[start + 2..end])
            .trim();
        result.push((start..end, key));
        offset = end;
        if offset == text.len() {
            break;
        }
    }
    result
}
fn token_range(text: &str, needle: &str) -> Range<usize> {
    let start = text.find(needle).unwrap_or(0);
    start..(start + needle.len()).min(text.len())
}
pub(super) fn position(text: &str, offset: usize) -> (usize, usize) {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    let prefix = &text[..offset];
    let line = prefix.bytes().filter(|b| *b == b'\n').count();
    let col = prefix.rsplit('\n').next().unwrap_or("").chars().count();
    (line + 1, col + 1)
}
fn yaml_offset(text: &str, line: usize, col: usize) -> usize {
    let start = text
        .split_inclusive('\n')
        .take(line.saturating_sub(1))
        .map(str::len)
        .sum::<usize>();
    let tail = &text[start.min(text.len())..];
    start
        + tail
            .split('\n')
            .next()
            .unwrap_or("")
            .char_indices()
            .nth(col.saturating_sub(1))
            .map(|(i, _)| i)
            .unwrap_or_else(|| tail.split('\n').next().unwrap_or("").len())
}
pub(super) fn diagnostics(draft: &Draft, sample: &str) -> Vec<Diagnostic> {
    let manifest = PathBuf::from("pack.yaml");
    let Some(text) = draft.files.get(&manifest) else {
        return vec![];
    };
    let mut result = vec![];
    let raw = match serde_yaml::from_str::<engine::Pack>(text) {
        Ok(pack) => Some(pack),
        Err(error) => {
            let offset = error
                .location()
                .map(|p| yaml_offset(text, p.line(), p.column()))
                .unwrap_or(0)
                .min(text.len());
            let end = text[offset..]
                .chars()
                .next()
                .map(|c| offset + c.len_utf8())
                .unwrap_or(offset);
            result.push(Diagnostic {
                path: manifest.clone(),
                range: offset..end,
                message: error.to_string(),
            });
            None
        }
    };
    if let Some(pack) = raw {
        for file in &pack.files {
            let (path, body) = match &file.source {
                Some(source) => match draft.files.get(Path::new(source)) {
                    Some(body) => (PathBuf::from(source), body.as_str()),
                    None => {
                        result.push(Diagnostic {
                            path: manifest.clone(),
                            range: token_range(text, source),
                            message: format!("Missing template source: {source}"),
                        });
                        continue;
                    }
                },
                None => (manifest.clone(), file.content.as_str()),
            };
            for (range, key) in placeholders(body) {
                let closed = body[range.clone()].ends_with("}}");
                if !closed || !engine::TEMPLATE_VARIABLES.contains(&key) {
                    let token = &body[range.clone()];
                    let range = if path == manifest {
                        token_range(text, token)
                    } else {
                        range
                    };
                    result.push(Diagnostic {
                        path: path.clone(),
                        range,
                        message: if closed {
                            format!("Unknown template variable: {key}")
                        } else {
                            "Unclosed template variable".into()
                        },
                    });
                }
            }
            for (range, key) in placeholders(&file.path) {
                if !engine::TEMPLATE_VARIABLES.contains(&key)
                    || !file.path[range.clone()].ends_with("}}")
                {
                    result.push(Diagnostic {
                        path: manifest.clone(),
                        range: token_range(text, &file.path[range]),
                        message: format!("Invalid output path variable: {key}"),
                    });
                }
            }
        }
        if result.is_empty() {
            if let Err(message) = draft.validate(sample) {
                let field = if message.contains("Suffix") {
                    "suffix"
                } else if message.contains("version") || message.contains("Version")
                {
                    "version"
                } else if message.contains("Registration") {
                    "registration"
                } else if message.contains("dependency")
                    || message.contains("Dependency")
                {
                    "dependencies"
                } else if message.contains("path")
                    || message.contains("Output")
                    || message.contains("Duplicate output")
                    || message.contains("replace")
                {
                    "path"
                } else if message.contains("PascalCase") {
                    ""
                } else {
                    "schema"
                };
                // Invalid sample names are UI input errors, not manifest diagnostics.
                if !field.is_empty() {
                    result.push(Diagnostic {
                        path: manifest,
                        range: token_range(text, field),
                        message,
                    });
                }
            }
        }
    }
    result.sort_by(|a, b| {
        (&a.path, a.range.start, &a.message).cmp(&(
            &b.path,
            b.range.start,
            &b.message,
        ))
    });
    result.dedup();
    result
}
const ROOT_FIELDS: &[&str] = &[
    "schema",
    "id",
    "version",
    "title",
    "description",
    "category",
    "target",
    "suffix",
    "compatible_with",
    "registration",
    "disabled",
    "files",
    "dependencies",
];
const DART_WORDS: &[&str] = &[
    "abstract",
    "async",
    "await",
    "bool",
    "build",
    "class",
    "const",
    "double",
    "dynamic",
    "else",
    "enum",
    "extends",
    "factory",
    "false",
    "final",
    "Future",
    "if",
    "implements",
    "import",
    "int",
    "List",
    "Map",
    "mixin",
    "null",
    "override",
    "part",
    "required",
    "return",
    "sealed",
    "setState",
    "static",
    "String",
    "super",
    "this",
    "true",
    "typedef",
    "var",
    "void",
    "Widget",
    "with",
    "yield",
];
pub(super) fn completions(
    path: &Path,
    text: &str,
    offset: usize,
    files: &BTreeMap<PathBuf, String>,
    explicit: bool,
) -> Vec<Completion> {
    if offset > text.len() || !text.is_char_boundary(offset) {
        return vec![];
    }
    let prefix = &text[..offset];
    if let Some(start) = prefix.rfind("{{") {
        let mut start = start;
        while text[start + 2..].starts_with('{') {
            start += 1;
        }
        let fragment = &text[start + 2..offset];
        if !fragment.contains(['{', '}', '\n']) {
            let key = fragment.trim_start();
            let end = text[offset..]
                .find("}}")
                .filter(|n| !text[offset..offset + n].contains(['\n', '{']))
                .map(|n| offset + n + 2)
                .unwrap_or(offset);
            return engine::TEMPLATE_VARIABLES
                .iter()
                .filter(|s| s.starts_with(key))
                .take(12)
                .map(|key| Completion {
                    label: format!("{{{{{key}}}}}"),
                    insert: format!("{{{{{key}}}}}"),
                    range: start..end,
                })
                .collect();
        }
    }
    let start = prefix
        .char_indices()
        .rev()
        .find(|(_, c)| !c.is_alphanumeric() && *c != '_')
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    let word = &text[start..offset];
    let end = offset
        + text[offset..]
            .char_indices()
            .take_while(|(_, c)| c.is_alphanumeric() || *c == '_')
            .map(|(_, c)| c.len_utf8())
            .sum::<usize>();
    if !explicit && word.chars().count() < 2 {
        return vec![];
    }
    let line_start = prefix.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line = &prefix[line_start..];
    let mut values = BTreeSet::new();
    let yaml = path.file_name().is_some_and(|p| p == "pack.yaml");
    let mut suffix = "";
    if yaml {
        if let Some((key, _)) =
            line.trim_start().trim_start_matches("- ").split_once(':')
        {
            match key.trim() {
                "target" => {
                    values.extend(["sibling", "package"].map(str::to_string));
                }
                "category" => {
                    values.extend(
                        ["widget", "state", "feature", "other"].map(str::to_string),
                    );
                }
                "disabled" | "dev" => {
                    values.extend(["true", "false"].map(str::to_string));
                }
                "schema" => {
                    values.insert("2".into());
                }
                "registration" => {
                    values.insert("mvvm_change_notifier".into());
                }
                "sdk" => {
                    values.insert("flutter".into());
                }
                "source" => {
                    let fragment = line
                        .split_once(':')
                        .unwrap()
                        .1
                        .trim_start()
                        .trim_start_matches(['\'', '"']);
                    let begin = offset - fragment.len();
                    return files
                        .keys()
                        .filter(|p| *p != Path::new("pack.yaml"))
                        .filter_map(|p| p.to_str())
                        .filter(|p| p.starts_with(fragment))
                        .take(12)
                        .map(|p| Completion {
                            label: p.into(),
                            insert: p.into(),
                            range: begin
                                ..(offset
                                    + text[offset..]
                                        .find(|c: char| {
                                            c.is_whitespace()
                                                || matches!(c, '\'' | '"' | '#')
                                        })
                                        .unwrap_or(text.len() - offset)),
                        })
                        .collect();
                }
                _ => {}
            }
        } else {
            suffix = ": ";
            let section = prefix[..line_start]
                .lines()
                .rev()
                .find(|s| {
                    !s.starts_with(char::is_whitespace)
                        && !s.starts_with('-')
                        && !s.trim().is_empty()
                })
                .and_then(|s| s.split_once(':'))
                .map(|(key, _)| key.trim());
            let fields = if line.starts_with(char::is_whitespace)
                || line.trim_start().starts_with('-')
            {
                match section {
                    Some("files") => &["path", "source", "content"][..],
                    Some("dependencies") => {
                        &["package", "version", "sdk", "dev"][..]
                    }
                    _ => &[][..],
                }
            } else {
                ROOT_FIELDS
            };
            values.extend(fields.iter().map(|s| s.to_string()));
        }
    } else if path.to_string_lossy().ends_with(".dart.tmpl")
        || path.extension().is_some_and(|e| e == "dart")
    {
        // Do not suggest Dart keywords in comments or strings.
        let before = &prefix[line_start..start];
        if before.contains("//")
            || before.matches('\'').count() % 2 == 1
            || before.matches('"').count() % 2 == 1
        {
            return vec![];
        }
        values.extend(DART_WORDS.iter().map(|s| s.to_string()));
        values.extend(
            text.split(|c: char| !c.is_alphanumeric() && c != '_')
                .filter(|s| {
                    s.chars()
                        .next()
                        .is_some_and(|c| c.is_alphabetic() || c == '_')
                })
                .map(String::from),
        );
    }
    let mut end = end;
    if suffix == ": " && text[end..].starts_with(':') {
        end += 1;
        if text[end..].starts_with(' ') {
            end += 1;
        }
    }
    values
        .into_iter()
        .filter(|s| s.starts_with(word) && s != word)
        .take(12)
        .map(|value| Completion {
            label: value.clone(),
            insert: format!("{value}{suffix}"),
            range: start..end,
        })
        .collect()
}

pub(super) struct CodeStyle {
    syntax: Syntax,
    placeholders: Vec<Range<usize>>,
    diagnostics: Vec<Diagnostic>,
    config: Arc<SparrowConfig>,
    version: u64,
    families: Vec<FamilyOwned>,
}
impl CodeStyle {
    pub fn new(
        path: &Path,
        text: &str,
        diagnostics: &[Diagnostic],
        config: Arc<SparrowConfig>,
        version: u64,
    ) -> Self {
        let language = if path.extension().is_some_and(|e| e == "yaml" || e == "yml")
        {
            SparrowLanguage::Yaml
        } else if path.to_string_lossy().ends_with(".dart.tmpl")
            || path.extension().is_some_and(|e| e == "dart")
        {
            SparrowLanguage::Dart
        } else {
            SparrowLanguage::PlainText
        };
        let mut syntax = Syntax::from_language(language);
        syntax.parse(version, lapce_xi_rope::Rope::from(text), None);
        Self {
            syntax,
            placeholders: placeholders(text).into_iter().map(|(r, _)| r).collect(),
            diagnostics: diagnostics
                .iter()
                .filter(|d| d.path == path)
                .cloned()
                .collect(),
            families: vec![
                FamilyOwned::Name(config.editor.font_family.clone()),
                FamilyOwned::Monospace,
            ],
            config,
            version,
        }
    }
}
impl CodeStyle {
    fn line_range(&self, line: usize) -> Option<(usize, usize)> {
        let last = self.syntax.text.line_of_offset(self.syntax.text.len());
        if line > last {
            return None;
        }
        let start = self.syntax.text.offset_of_line(line);
        let end = if line == last {
            self.syntax.text.len()
        } else {
            self.syntax.text.offset_of_line(line + 1)
        };
        Some((start, end))
    }
}
impl Styling for CodeStyle {
    fn id(&self) -> u64 {
        self.version
    }
    fn font_size(&self, _: EditorId, _: usize) -> usize {
        13
    }
    fn font_family(&self, _: EditorId, _: usize) -> Cow<'_, [FamilyOwned]> {
        Cow::Borrowed(&self.families)
    }
    fn apply_attr_styles(
        &self,
        _: EditorId,
        _: &EditorStyle,
        line: usize,
        default: Attrs,
        attrs: &mut AttrsList,
    ) {
        let Some((start, end)) = self.line_range(line) else {
            return;
        };
        if let Some(styles) = &self.syntax.styles {
            for style in line_styles(&self.syntax.text, line, styles) {
                if let Some(color) = style
                    .style
                    .fg_color
                    .as_deref()
                    .and_then(|key| self.config.style_color(key))
                {
                    attrs.add_span(
                        style.start..style.end,
                        default.clone().color(color),
                    );
                }
            }
        }

        let color = self
            .config
            .style_color("attribute")
            .unwrap_or(self.config.color(SparrowColor::EDITOR_FOREGROUND));
        for range in &self.placeholders {
            let a = range.start.max(start);
            let b = range.end.min(end);
            if a < b {
                attrs.add_span(a - start..b - start, default.clone().color(color));
            }
        }
    }
    fn apply_layout_styles(
        &self,
        _: EditorId,
        _: &EditorStyle,
        line: usize,
        layout: &mut TextLayoutLine,
    ) {
        let Some((start, end)) = self.line_range(line) else {
            return;
        };
        let color = self.config.color(SparrowColor::SPARROW_ERROR);
        for diagnostic in &self.diagnostics {
            let a = diagnostic.range.start.max(start);
            let b = diagnostic.range.end.min(end);
            if a < b {
                layout
                    .extra_style
                    .extend(crate::doc::extra_styles_for_range(
                        &layout.text,
                        a - start,
                        b - start,
                        None,
                        None,
                        Some(color),
                    ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scaffold::manager::{catalog, editor::Draft};
    fn draft() -> (tempfile::TempDir, Draft) {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("pubspec.yaml"),
            "name: language_test
",
        )
        .unwrap();
        let entry = catalog::bundled_entries()
            .into_iter()
            .find(|e| e.id == "sparrow.state.cubit")
            .unwrap();
        let draft = Draft::load(
            &entry,
            root.path(),
            &root.path().join(".sparrow/scaffolds"),
        )
        .unwrap();
        (root, draft)
    }
    fn complete(path: &str, text: &str, offset: usize) -> Vec<Completion> {
        let mut files = BTreeMap::new();
        files.insert(
            "templates/counter.dart.tmpl".into(),
            "class {{name}} {}".into(),
        );
        completions(Path::new(path), text, offset, &files, true)
    }
    fn apply(text: &str, c: &Completion) -> String {
        let mut output = text.to_string();
        output.replace_range(c.range.clone(), &c.insert);
        output
    }
    #[test]
    fn placeholders_handle_unicode_whitespace_named_parameter_braces_and_unclosed_tokens()
     {
        let s = "é {{{ name }}} {{snake}} {{missing";
        let tokens = placeholders(s);
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0].1, "name");
        assert_eq!(&s[tokens[0].0.clone()], "{{ name }}");
        assert_eq!(tokens[2].1, "missing");
        assert!(!s[tokens[2].0.clone()].ends_with("}}"));
        assert_eq!(position(s, 2), (1, 2));
        assert_eq!(
            position(
                "é
🙂x", 7
            ),
            (2, 2)
        );
    }
    #[test]
    fn metadata_completion_is_contextual_and_preserves_existing_separator_and_value()
    {
        for (text, offset, field, expected) in [
            ("cate", 4, "category", "category: "),
            ("categ: state", 5, "category", "category: state"),
            ("category: st", 12, "state", "category: state"),
            ("target: si", 10, "sibling", "target: sibling"),
            (
                "dependencies:
- pa",
                18,
                "package",
                "dependencies:
- package: ",
            ),
            (
                "files:
- so",
                11,
                "source",
                "files:
- source: ",
            ),
        ] {
            let items = complete("pack.yaml", text, offset);
            let item = items
                .iter()
                .find(|i| i.label == field)
                .unwrap_or_else(|| panic!("{text}: {items:?}"));
            assert_eq!(apply(text, item), expected);
        }
        assert!(
            !complete("pack.yaml", "files:\n- pa", 11)
                .iter()
                .any(|c| c.label == "package")
        );
        assert!(
            complete(
                "pack.yaml",
                "dependencies:
- co",
                18
            )
            .is_empty()
        );
    }
    #[test]
    fn source_paths_and_variables_replace_existing_suffix_without_duplication() {
        let text = "  source: 'templates/cou.dart.tmpl'";
        let offset = text.find(".dart").unwrap();
        let items = complete("pack.yaml", text, offset);
        assert_eq!(
            apply(text, &items[0]),
            "  source: 'templates/counter.dart.tmpl'"
        );
        for text in ["{{na", "{{na}}", "é {{{na}}"] {
            let offset = text.find("na").unwrap() + 2;
            let items = complete("templates/counter.dart.tmpl", text, offset);
            let name = items.iter().find(|i| i.label == "{{name}}").unwrap();
            assert!(apply(text, name).contains("{{name}}"));
            assert!(!apply(text, name).contains("}}}}"));
        }
    }
    #[test]
    fn dart_completion_offers_keywords_and_local_names_without_string_or_comment_noise()
     {
        let items = complete("templates/a.dart.tmpl", "cla", 3);
        assert_eq!(
            apply("cla", &items.iter().find(|c| c.label == "class").unwrap()),
            "class"
        );
        let text = "final counter = 1;
cou";
        assert!(
            complete("templates/a.dart.tmpl", text, text.len())
                .iter()
                .any(|c| c.label == "counter")
        );
        for text in ["// cla", "'cla", r#""cla"#] {
            assert!(complete("templates/a.dart.tmpl", text, text.len()).is_empty());
        }
        assert!(
            completions(Path::new("a.dart.tmpl"), "c", 1, &BTreeMap::new(), false)
                .is_empty()
        );
        assert!(complete("other.txt", "cla", 3).is_empty());
        assert!(complete("pack.yaml", "é", 1).is_empty()); // invalid UTF-8 boundary
    }
    #[test]
    fn diagnostics_identify_file_byte_ranges_and_clear_after_repair() {
        let (_root, mut draft) = draft();
        let path = PathBuf::from("templates/template.dart.tmpl");
        let text = "// é
class {{name}} { {{bad}} }
{{open";
        draft.files.insert(path.clone(), text.into());
        let errors = diagnostics(&draft, "Example");
        assert_eq!(errors.len(), 2);
        assert!(errors.iter().all(|d| d.path == path));
        assert_eq!(&text[errors[0].range.clone()], "{{bad}}");
        assert_eq!(position(text, errors[0].range.start), (2, 18));
        assert!(errors[1].message.contains("Unclosed"));
        draft.files.insert(path, "class {{name}} {}".into());
        assert!(diagnostics(&draft, "Example").is_empty());
    }
    #[test]
    fn manifest_syntax_schema_and_missing_sources_have_navigable_locations() {
        let (_root, mut draft) = draft();
        let original = draft.files[Path::new("pack.yaml")].clone();
        for text in [
            "schema: [",
            "schema: 2
id: invalid
version: 1.0.0
title: Test
files: []
unexpected: true
",
        ] {
            draft.files.insert("pack.yaml".into(), text.into());
            let errors = diagnostics(&draft, "Example");
            assert!(!errors.is_empty());
            assert_eq!(errors[0].path, Path::new("pack.yaml"));
            assert!(text.is_char_boundary(errors[0].range.start));
            assert!(errors[0].range.end <= text.len());
        }
        draft.files.insert("pack.yaml".into(), original);
        draft
            .files
            .remove(Path::new("templates/template.dart.tmpl"));
        let errors = diagnostics(&draft, "Example");
        assert!(
            errors
                .iter()
                .any(|d| d.message.contains("Missing template source"))
        );
        assert_eq!(
            &draft.files[Path::new("pack.yaml")][errors[0].range.clone()],
            "templates/template.dart.tmpl"
        );
    }
    #[test]
    fn highlighting_covers_yaml_dart_and_distinguishes_placeholder_ranges() {
        for (path, text) in [
            (
                "pack.yaml",
                "schema: 2
title: 'Counter'
files:
  - path: '{{snake}}.dart'
",
            ),
            (
                "a.dart.tmpl",
                "// comment
class {{name}} { final count = 42; }
",
            ),
        ] {
            let config = Arc::new(SparrowConfig::default());
            let style = CodeStyle::new(Path::new(path), text, &[], config, 7);
            assert_eq!(style.version, 7);
            assert!(!style.placeholders.is_empty());
            assert!(style.syntax.styles.is_some());
            assert_eq!(style.syntax.text.to_string(), text);
            assert_eq!(style.line_range(10_000), None);
            assert!(
                style
                    .line_range(style.syntax.text.line_of_offset(text.len()))
                    .is_some()
            );
        }
    }
}
