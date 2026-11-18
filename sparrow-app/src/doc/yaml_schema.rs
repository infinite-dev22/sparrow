//! Local schema services. The bundled files use a documented JSON Schema subset.
use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CompletionItem,
    CompletionItemKind, CompletionTextEdit, Diagnostic, DiagnosticSeverity,
    Position, Range, TextEdit, Url, WorkspaceEdit,
};
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::Value as Json;
use serde_yaml::Value as Yaml;
use std::{collections::HashMap, path::Path, sync::LazyLock};

static PUBSPEC: LazyLock<Json> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../resources/yaml-schemas/pubspec.json"))
        .expect("bundled pubspec schema")
});
static ANALYSIS: LazyLock<Json> = LazyLock::new(|| {
    serde_json::from_str(include_str!(
        "../../resources/yaml-schemas/analysis_options.json"
    ))
    .expect("bundled analysis options schema")
});
fn schema(path: &Path) -> Option<&'static Json> {
    match path.file_name()?.to_str()? {
        "pubspec.yaml" => Some(&PUBSPEC),
        "analysis_options.yaml" => Some(&ANALYSIS),
        _ => None,
    }
}
pub(crate) fn supports(path: &Path) -> bool {
    schema(path).is_some()
}
#[derive(Clone, Debug)]
enum Part {
    Key(String),
    Index(usize),
}
struct Issue {
    path: Vec<Part>,
    key: bool,
    message: String,
    replacement: Option<String>,
    warning: bool,
}
fn types(s: &Json) -> Vec<&str> {
    match &s["type"] {
        Json::String(t) => vec![t],
        Json::Array(t) => t.iter().filter_map(Json::as_str).collect(),
        _ => vec![],
    }
}
fn matches_type(value: &Yaml, t: &str) -> bool {
    match t {
        "object" => value.is_mapping(),
        "array" => value.is_sequence(),
        "string" => value.is_string(),
        "boolean" => value.is_bool(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "number" => value.is_number(),
        "null" => value.is_null(),
        _ => true,
    }
}
fn distance(a: &str, b: &str) -> usize {
    let mut row: Vec<_> = (0..=b.chars().count()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.chars().enumerate() {
            let old = row[j + 1];
            row[j + 1] = (diagonal + usize::from(ca != cb))
                .min(row[j] + 1)
                .min(old + 1);
            diagonal = old;
        }
    }
    *row.last().unwrap()
}
fn nearest<'a>(
    key: &str,
    choices: impl Iterator<Item = &'a str>,
) -> Option<&'a str> {
    let mut candidates: Vec<_> = choices.map(|c| (distance(key, c), c)).collect();
    candidates.sort();
    let (d, c) = *candidates.first()?;
    (d > 0
        && d <= if key.len() > 5 { 2 } else { 1 }
        && candidates.get(1).is_none_or(|next| next.0 > d))
    .then_some(c)
}
fn validate(s: &Json, value: &Yaml, path: &mut Vec<Part>, issues: &mut Vec<Issue>) {
    if issues.len() >= 50 {
        return;
    }
    let mut report = |message: String, replacement: Option<String>| {
        issues.push(Issue {
            path: path.clone(),
            key: false,
            message,
            replacement,
            warning: false,
        })
    };
    let ts = types(s);
    if !ts.is_empty() && !ts.iter().any(|t| matches_type(value, t)) {
        let replacement = if ts == ["boolean"] {
            value
                .as_str()
                .and_then(|v| match v.to_ascii_lowercase().as_str() {
                    "true" => Some("true".into()),
                    "false" => Some("false".into()),
                    _ => None,
                })
        } else if ts.contains(&"string") && (value.is_number() || value.is_bool()) {
            Some(format!(
                "'{}'",
                serde_yaml::to_string(value).unwrap_or_default().trim()
            ))
        } else {
            None
        };
        report(format!("Expected {}.", ts.join(" or ")), replacement);
        return;
    }
    if let Some(choices) = s["enum"].as_array() {
        if !choices
            .iter()
            .any(|c| serde_yaml::to_value(c).ok().as_ref() == Some(value))
        {
            let fix = value
                .as_str()
                .and_then(|v| nearest(v, choices.iter().filter_map(Json::as_str)))
                .map(str::to_owned);
            report(
                format!(
                    "Expected one of: {}.",
                    choices
                        .iter()
                        .map(Json::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                fix,
            );
        }
    }
    if s["format"] == "semver"
        && value
            .as_str()
            .is_some_and(|v| semver::Version::parse(v).is_err())
    {
        report(
            "Expected a semantic version such as 1.2.3 or 1.2.3+1.".into(),
            None,
        );
    }
    if let Some(pattern) = s["pattern"].as_str() {
        if value.as_str().is_some_and(|v| {
            regex::Regex::new(pattern).is_ok_and(|r| !r.is_match(v))
        }) {
            report("Use a lower_case_with_underscores package name that does not begin with a digit.".into(),None);
        }
    }
    if let Some(n) = value.as_f64() {
        if s["minimum"].as_f64().is_some_and(|min| n < min)
            || s["maximum"].as_f64().is_some_and(|max| n > max)
        {
            report(
                format!(
                    "Value is outside the allowed range (minimum {}, maximum {}).",
                    s["minimum"], s["maximum"]
                ),
                None,
            );
        }
    }
    if let Some(map) = value.as_mapping() {
        if let Some(required) = s["required"].as_array() {
            for key in required.iter().filter_map(Json::as_str) {
                if !map.contains_key(Yaml::String(key.into())) {
                    issues.push(Issue {
                        path: path.clone(),
                        key: false,
                        message: format!("Missing required field '{key}'."),
                        replacement: None,
                        warning: false,
                    });
                }
            }
        }
        for (key, v) in map {
            let Some(key) = key.as_str() else { continue };
            path.push(Part::Key(key.into()));
            if let Some(child) = s["properties"].get(key) {
                validate(child, v, path, issues);
            } else if s["additionalProperties"].is_object() {
                validate(&s["additionalProperties"], v, path, issues);
            } else {
                let suggestion = s["properties"]
                    .as_object()
                    .and_then(|p| nearest(key, p.keys().map(String::as_str)))
                    .filter(|k| !map.contains_key(Yaml::String((*k).into())));
                if s["additionalProperties"] == false || suggestion.is_some() {
                    issues.push(Issue {
                        path: path.clone(),
                        key: true,
                        message: match suggestion {
                            Some(k) => {
                                format!("Unknown field '{key}'. Did you mean '{k}'?")
                            }
                            None => format!("Unknown field '{key}'."),
                        },
                        replacement: suggestion.map(str::to_owned),
                        warning: true,
                    });
                }
            }
            path.pop();
        }
    }
    if let (Some(items), Some(values)) = (s.get("items"), value.as_sequence()) {
        for (i, v) in values.iter().enumerate() {
            path.push(Part::Index(i));
            validate(items, v, path, issues);
            path.pop();
        }
    }
}

// Ask serde_yaml to mark a deliberate visitor error at a structural path. This
// reuses the YAML parser's real locations for quoted keys, flow maps and lists,
// instead of searching for repeated key names or guessing by indentation.
const MARK: &str = "sparrow-schema-location";
struct Locate<'a> {
    path: &'a [Part],
    key: bool,
}
impl<'de> DeserializeSeed<'de> for Locate<'_> {
    type Value = ();
    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        d.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for Locate<'_> {
    type Value = ();
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(MARK)
    }
    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<(), M::Error> {
        let Some(Part::Key(target)) = self.path.first() else {
            return Err(de::Error::custom(MARK));
        };
        while let Some(k) = map.next_key_seed(KeyProbe {
            target,
            mark: self.key && self.path.len() == 1,
        })? {
            if k.as_str() == Some(target) {
                map.next_value_seed(Locate {
                    path: &self.path[1..],
                    key: self.key,
                })?;
            } else {
                map.next_value::<de::IgnoredAny>()?;
            }
        }
        Ok(())
    }
    fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<(), S::Error> {
        let Some(Part::Index(target)) = self.path.first() else {
            return Err(de::Error::custom(MARK));
        };
        let mut i = 0;
        loop {
            let next = if i == *target {
                seq.next_element_seed(Locate {
                    path: &self.path[1..],
                    key: self.key,
                })?
            } else {
                seq.next_element::<de::IgnoredAny>()?.map(|_| ())
            };
            if next.is_none() {
                break;
            }
            i += 1;
        }
        Ok(())
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> Result<(), E> {
        Err(E::custom(MARK))
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> Result<(), E> {
        Err(E::custom(MARK))
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> Result<(), E> {
        Err(E::custom(MARK))
    }
    fn visit_f64<E: de::Error>(self, _: f64) -> Result<(), E> {
        Err(E::custom(MARK))
    }
    fn visit_str<E: de::Error>(self, _: &str) -> Result<(), E> {
        Err(E::custom(MARK))
    }
    fn visit_unit<E: de::Error>(self) -> Result<(), E> {
        Err(E::custom(MARK))
    }
    fn visit_none<E: de::Error>(self) -> Result<(), E> {
        Err(E::custom(MARK))
    }
}
struct KeyProbe<'a> {
    target: &'a str,
    mark: bool,
}
impl<'de> DeserializeSeed<'de> for KeyProbe<'_> {
    type Value = Yaml;
    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<Yaml, D::Error> {
        struct KeyVisitor<'a>(KeyProbe<'a>);
        impl<'de> Visitor<'de> for KeyVisitor<'_> {
            type Value = Yaml;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a mapping key")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Yaml, E> {
                if self.0.mark && v == self.0.target {
                    Err(E::custom(MARK))
                } else {
                    Ok(Yaml::String(v.into()))
                }
            }
        }
        d.deserialize_any(KeyVisitor(self))
    }
}
fn byte_position(text: &str, offset: usize) -> Position {
    let prefix = &text[..offset];
    let line = prefix.bytes().filter(|c| *c == b'\n').count();
    Position::new(
        line as u32,
        prefix
            .rsplit('\n')
            .next()
            .unwrap_or("")
            .encode_utf16()
            .count() as u32,
    )
}
fn locate(text: &str, issue: &Issue) -> Option<usize> {
    let error = Locate {
        path: &issue.path,
        key: issue.key,
    }
    .deserialize(serde_yaml::Deserializer::from_str(text))
    .err()?;
    if !error.to_string().contains(MARK) {
        return None;
    }
    // Location::index is a byte index (unlike column's Unicode character count).
    let index = error.location()?.index();
    (index <= text.len() && text.is_char_boundary(index)).then_some(index)
}
// Only offer edits for standalone, single-line scalars. Never rewrite anchors,
// aliases, block strings or containers; those may affect other values.
fn scalar_end(text: &str, start: usize) -> usize {
    let tail = &text[start..];
    let mut quote = None;
    let mut escaped = false;
    for (i, c) in tail.char_indices() {
        if c == '\n' || c == '\r' {
            return start + i;
        }
        if i == 0 && matches!(c, '\'' | '"') {
            quote = Some(c);
            continue;
        }
        if let Some(q) = quote {
            if q == '"' && c == '\\' && !escaped {
                escaped = true;
                continue;
            }
            if c == q && !escaped {
                // Doubled single quotes are handled by the parser check below.
                return start + i + c.len_utf8();
            }
            escaped = false;
        } else if matches!(c, ':' | ',' | '}' | ']' | '#') {
            return start + i;
        }
    }
    text.len()
}
fn issues(path: &Path, text: &str) -> Vec<Issue> {
    let Some(s) = schema(path) else { return vec![] };
    let Ok(value) = serde_yaml::from_str::<Yaml>(text) else {
        return vec![];
    };
    let mut out = vec![];
    validate(s, &value, &mut vec![], &mut out);
    out.truncate(50);
    out
}
pub(crate) fn diagnostics(path: &Path, text: &str) -> Vec<Diagnostic> {
    issues(path, text)
        .into_iter()
        .filter_map(|issue| {
            let start = locate(text, &issue)?;
            let end = scalar_end(text, start).max(start).min(text.len());
            Some(Diagnostic {
                range: Range::new(
                    byte_position(text, start),
                    byte_position(text, end),
                ),
                severity: Some(if issue.warning {
                    DiagnosticSeverity::WARNING
                } else {
                    DiagnosticSeverity::ERROR
                }),
                source: Some(super::yaml::SOURCE.into()),
                message: issue.message,
                ..Default::default()
            })
        })
        .collect()
}
pub(crate) fn code_actions(
    path: &Path,
    text: &str,
    line: u32,
) -> Vec<CodeActionOrCommand> {
    let Ok(uri) = Url::from_file_path(path) else {
        return vec![];
    };
    issues(path, text)
        .into_iter()
        .filter_map(|issue| {
            let replacement = issue.replacement.as_ref()?;
            let start = locate(text, &issue)?;
            let end = scalar_end(text, start);
            let raw = text[start..end].trim_end();
            if raw.is_empty()
                || raw.starts_with(['&', '*', '!', '|', '>', '{', '['])
                || byte_position(text, start).line != line
            {
                return None;
            }
            let value = serde_yaml::from_str::<Yaml>(raw).ok()?;
            if issue.key
                && value.as_str()
                    != issue.path.last().and_then(|p| match p {
                        Part::Key(k) => Some(k.as_str()),
                        _ => None,
                    })
            {
                return None;
            }
            let edit = TextEdit {
                range: Range::new(
                    byte_position(text, start),
                    byte_position(text, start + raw.len()),
                ),
                new_text: replacement.clone(),
            };
            Some(CodeActionOrCommand::CodeAction(CodeAction {
                title: format!("Replace {raw} with {replacement}"),
                kind: Some(CodeActionKind::QUICKFIX),
                is_preferred: Some(true),
                edit: Some(WorkspaceEdit {
                    changes: Some(HashMap::from([(uri.clone(), vec![edit])])),
                    ..Default::default()
                }),
                ..Default::default()
            }))
        })
        .collect()
}

const CURSOR: &str = "sparrow_yaml_completion_cursor_9c274";
fn cursor_path(value: &Yaml, path: &mut Vec<Part>) -> Option<(Vec<Part>, bool)> {
    match value {
        Yaml::Mapping(map) => {
            for (k, v) in map {
                if k.as_str() == Some(CURSOR) {
                    return Some((path.clone(), true));
                }
                if let Some(k) = k.as_str() {
                    path.push(Part::Key(k.into()));
                    let result = cursor_path(v, path);
                    path.pop();
                    if result.is_some() {
                        return result;
                    }
                }
            }
        }
        Yaml::Sequence(seq) => {
            for (i, v) in seq.iter().enumerate() {
                path.push(Part::Index(i));
                let result = cursor_path(v, path);
                path.pop();
                if result.is_some() {
                    return result;
                }
            }
        }
        Yaml::String(s) if s == CURSOR => return Some((path.clone(), false)),
        _ => {}
    }
    None
}
fn at_path<'a>(mut schema: &'a Json, path: &[Part]) -> Option<&'a Json> {
    for part in path {
        schema = match part {
            Part::Key(k) => schema["properties"].get(k).or_else(|| {
                schema.get("additionalProperties").filter(|s| s.is_object())
            })?,
            Part::Index(_) => schema.get("items")?,
        };
    }
    Some(schema)
}
fn value_at<'a>(mut value: &'a Yaml, path: &[Part]) -> Option<&'a Yaml> {
    for part in path {
        value = match part {
            Part::Key(k) => value.get(k)?,
            Part::Index(i) => value.get(*i)?,
        };
    }
    Some(value)
}
/// Parse a temporary sentinel into the current document to derive the actual
/// mapping/list context, including flow YAML. No buffer mutation is involved.
pub(crate) fn completions(
    path: &Path,
    text: &str,
    offset: usize,
) -> Option<(usize, Vec<CompletionItem>)> {
    let root = schema(path)?;
    if !text.is_char_boundary(offset) || text.contains(CURSOR) {
        return None;
    }
    let word = |c: char| c.is_alphanumeric() || matches!(c, '_' | '-' | '.');
    let start = text[..offset]
        .rfind(|c: char| !word(c))
        .map_or(0, |i| i + text[i..].chars().next().unwrap().len_utf8());
    let end = offset
        + text[offset..]
            .find(|c: char| !word(c))
            .unwrap_or(text.len() - offset);
    let input = &text[start..offset];
    let mut found = None;
    for replacement in [CURSOR.to_owned(), format!("{CURSOR}: null")] {
        let probe = format!("{}{replacement}{}", &text[..start], &text[end..]);
        if let Ok(value) = serde_yaml::from_str::<Yaml>(&probe) {
            if let Some((path, key)) = cursor_path(&value, &mut vec![]) {
                // An unfinished mapping key can parse as its parent's scalar
                // value. Try the key probe before accepting that interpretation.
                if !key
                    && at_path(root, &path).is_some_and(|s| {
                        types(s).contains(&"object")
                            && !types(s).contains(&"string")
                            && !types(s).contains(&"array")
                    })
                {
                    continue;
                }
                found = Some((value, path, key));
                break;
            }
        }
    }
    let (document, context, key) = found?;
    let s = at_path(root, &context)?;
    let mut options: Vec<(String, String, String)> = vec![];
    let colon_exists = text[end..]
        .trim_start_matches(['\'', '"', ' ', '\t'])
        .starts_with(':');
    if key {
        if let Some(properties) = s["properties"].as_object() {
            for (name, property) in properties {
                if value_at(&document, &context)
                    .and_then(|v| v.get(name))
                    .is_some()
                    && name != &text[start..end]
                {
                    continue;
                }
                let insert = if colon_exists {
                    name.clone()
                } else {
                    format!("{name}: ")
                };
                options.push((
                    name.clone(),
                    insert,
                    property["description"]
                        .as_str()
                        .unwrap_or("Configuration field")
                        .into(),
                ));
            }
        }
        if let Some(suggestions) = s["x-key-suggestions"].as_array() {
            for name in suggestions.iter().filter_map(Json::as_str) {
                options.push((
                    name.into(),
                    if colon_exists {
                        name.into()
                    } else {
                        format!("{name}: ")
                    },
                    "Common rule (other rules are also accepted)".into(),
                ));
            }
        }
    } else {
        if let Some(values) = s["enum"].as_array() {
            for value in values {
                let label = value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string());
                options.push((label.clone(), label, "Allowed value".into()));
            }
        }
        if types(s).contains(&"boolean") {
            for v in ["true", "false"] {
                options.push((v.into(), v.into(), "Boolean value".into()));
            }
        }
        if let Some(values) = s["examples"].as_array() {
            for value in values.iter().filter_map(Json::as_str) {
                options.push((value.into(), value.into(), "Example value".into()));
            }
        }
    }
    let quoted = start > 0
        && end < text.len()
        && matches!(text.as_bytes()[start - 1], b'\'' | b'"')
        && text.as_bytes()[start - 1] == text.as_bytes()[end];
    let edit_start = if quoted { start - 1 } else { start };
    let edit_end = if quoted { end + 1 } else { end };
    let range = Range::new(
        byte_position(text, edit_start),
        byte_position(text, edit_end),
    );
    let items = options
        .into_iter()
        .filter(|(label, _, _)| {
            label.to_lowercase().starts_with(&input.to_lowercase())
        })
        .map(|(label, new_text, detail)| CompletionItem {
            label,
            kind: Some(if key {
                CompletionItemKind::PROPERTY
            } else {
                CompletionItemKind::VALUE
            }),
            detail: Some(detail),
            data: Some(serde_json::json!({"sparrowYaml":true})),
            text_edit: Some(CompletionTextEdit::Edit(TextEdit { range, new_text })),
            ..Default::default()
        })
        .collect();
    Some((start, items))
}

#[cfg(test)]
mod tests {
    use super::*;
    const PUB: &str = "/project/pubspec.yaml";
    const OPT: &str = "/project/analysis_options.yaml";
    fn messages(path: &str, text: &str) -> Vec<String> {
        diagnostics(Path::new(path), text)
            .into_iter()
            .map(|d| d.message)
            .collect()
    }
    fn labels(path: &str, source: &str) -> Vec<String> {
        let offset = source.find('|').unwrap();
        let source = source.replacen('|', "", 1);
        completions(Path::new(path), &source, offset)
            .map(|(_, items)| items.into_iter().map(|i| i.label).collect())
            .unwrap_or_default()
    }
    #[test]
    fn schemas_validate_types_and_keep_extension_points_open() {
        let valid = "name: demo\nenvironment: {sdk: ^3.0.0}\ncustom_tool: {settings: true}\ndependencies:\n  local: {path: ../local}\n  remote: {git: {url: https://example.com/repo.git, ref: main}}\n  hosted: {hosted: {name: hosted, url: https://example.com}, version: ^1.0.0}\nflutter:\n  assets: [assets/, {path: assets/staging/, flavors: [staging]}]\n  fonts: [{family: Demo, fonts: [{asset: font.ttf, weight: 400}]}]\n";
        assert!(
            messages(PUB, valid).is_empty(),
            "{:?}",
            messages(PUB, valid)
        );
        let valid = "include: [package:lints/recommended.yaml]\nanalyzer:\n  language: {strict-casts: true}\n  errors: {some_future_diagnostic: ignore}\nlinter:\n  rules: {plugin_rule: true}\nplugins:\n  custom_plugin: {version: ^1.0.0, arbitrary_setting: value}\nformatter: {page_width: 100, trailing_commas: preserve}\n";
        assert!(
            messages(OPT, valid).is_empty(),
            "{:?}",
            messages(OPT, valid)
        );
        let bad = "name: BadName\nenvironment: {sdk: 3}\nflutter: {uses-material-design: 'true'}\n";
        assert_eq!(messages(PUB, bad).len(), 3);
        assert!(messages(OPT,"analyzer: {errors: {unused_import: fatal}}\nformatter: {page_width: 0}\n").len()==2);
        assert!(
            messages(PUB, "dependencies: []\n")
                .iter()
                .any(|m| m.contains("required field 'name'"))
        );
        assert!(messages(OPT, "linter: {rules: [avoid_print]}\n").is_empty());
        assert!(
            diagnostics(Path::new("/project/unrelated.yaml"), "name: 3").is_empty()
        );
    }
    #[test]
    fn context_completion_handles_nested_flow_list_and_existing_keys() {
        assert!(labels(PUB, "name: demo\nenv|\n").contains(&"environment".into()));
        assert!(labels(PUB, "|").contains(&"name".into()));
        assert!(labels(OPT, "|").contains(&"analyzer".into()));
        assert!(labels(PUB, "flutter:\n  |").contains(&"assets".into()));
        assert!(
            labels(PUB, "flutter:\n  uses-|")
                .contains(&"uses-material-design".into())
        );
        assert_eq!(
            labels(OPT, "analyzer: {errors: {unused_import: war|}}"),
            vec!["warning"]
        );
        assert!(
            labels(OPT, "linter:\n  rules:\n    - avoid_|")
                .contains(&"avoid_print".into())
        );
        assert!(
            labels(OPT, "analyzer:\n  language:\n    strict-casts: f|")
                .contains(&"false".into())
        );
        assert!(!labels(PUB, "name: demo\n|\n").contains(&"name".into()));
        assert!(labels(OPT, "# analyzer: |").is_empty());
        assert!(labels(PUB, "description: |\n  hello |").is_empty());
        assert!(labels(PUB, "custom_tool:\n  any|").is_empty());
        let source = "analyzer: {language: {strict-casts: 'f'}}";
        let offset = source.find("'f").unwrap() + 2;
        let (_, items) = completions(Path::new(OPT), source, offset).unwrap();
        let CompletionTextEdit::Edit(edit) = items[0].text_edit.as_ref().unwrap()
        else {
            panic!()
        };
        let fixed =
            crate::code_action_preview::edited(source, &[edit.clone()]).unwrap();
        assert_eq!(fixed, "analyzer: {language: {strict-casts: false}}");
        assert!(messages(OPT, &fixed).is_empty());
    }
    #[test]
    fn exact_locations_and_small_fixes_preserve_comments_and_flow_yaml() {
        let text = "analyzer:\r\n  errors: {unused_import: warnign} # é🙂\r\nformatter: {trailing_commas: presrve}\r\n";
        let diagnostics = diagnostics(Path::new(OPT), text);
        assert_eq!(diagnostics.len(), 2);
        assert_eq!(diagnostics[0].range.start, Position::new(1, 26));
        let actions = code_actions(Path::new(OPT), text, 1);
        assert_eq!(actions.len(), 1);
        let CodeActionOrCommand::CodeAction(a) = &actions[0] else {
            panic!()
        };
        let edit = a.edit.as_ref().unwrap();
        let files = crate::code_action_preview::edit_files(edit).unwrap();
        let fixed =
            crate::code_action_preview::edited(text, &files[0].edits).unwrap();
        assert!(fixed.contains("unused_import: warning} # é🙂\r\n"));
        assert_eq!(messages(OPT, &fixed).len(), 1);
    }
    #[test]
    fn typo_key_and_boolean_fixes_work_but_aliases_are_not_rewritten() {
        let text = "analyzer:\n  langauge: {strict-casts: true}\n";
        let actions = code_actions(Path::new(OPT), text, 1);
        assert_eq!(actions.len(), 1);
        let text = "analyzer:\n  language:\n    strict-casts: 'true' # keep me\n";
        assert_eq!(code_actions(Path::new(OPT), text, 2).len(), 1);
        let text = "custom: &shared 'true'\nflutter: {uses-material-design: *shared}\nname: demo\nenvironment: {sdk: ^3.0.0}\n";
        assert!(code_actions(Path::new(PUB), text, 1).is_empty());
        let text = "analyzer: {errors: {unused_import: 4}}";
        assert_eq!(messages(OPT, text).len(), 1);
        assert!(code_actions(Path::new(OPT), text, 0).is_empty());
    }
}
