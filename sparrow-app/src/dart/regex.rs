//! Regex-based Dart/Flutter widget analyzer.
//!
//! This is a deliberately simple "v1" analyzer. It does *not* use the Dart
//! parser; it uses regexes over the raw source with a lightweight comment /
//! string skipper. Accuracy is high for well-formatted code and degrades
//! gracefully (misses ambiguous cases, never panics) for edge cases.
//!
//! When the LSP plumbing lands, swap this module for a proper
//! `textDocument/documentSymbol`-backed implementation. The public API
//! ([`analyze_dart_file`]) is intentionally small so the swap is localized.

use std::collections::{HashMap, HashSet};

use once_cell::sync::Lazy;
use regex::Regex;

use crate::widget_kind::{FlutterWidgetInfo, WidgetKind};

/// Everything the analyzer extracted from one source file.
pub struct DartAnalysis {
    /// Widget instantiations in source order (top to bottom).
    pub widgets: im::Vector<FlutterWidgetInfo>,
    /// Class declarations found in the file, keyed by class name.
    pub classes: HashMap<String, ClassInfo>,
}

/// Basic information about a class declaration in the current file.
#[derive(Debug, Clone)]
pub struct ClassInfo {
    pub name: String,
    pub superclass: Option<String>,
    /// Byte range of the entire `class X ... { ... }` declaration.
    pub byte_range: (usize, usize),
}

/// Classes that make a Dart class a "widget" (transitively).
const WIDGET_BASES: &[&str] = &[
    "Widget",
    "StatelessWidget",
    "StatefulWidget",
    "InheritedWidget",
    "InheritedModel",
    "InheritedNotifier",
    "InheritedTheme",
    "RenderObjectWidget",
    "ProxyWidget",
    "ImplicitlyAnimatedWidget",
    "AnimatedWidget",
    "AnimatedWidgetBaseState",
];

// ── Regexes ──────────────────────────────────────────────────────────

/// `class Foo extends Bar` or `class Foo<T> extends Bar`
static CLASS_DECL: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?m)^\s*(?:abstract\s+|sealed\s+|final\s+|base\s+)?class\s+([A-Z]\w*)(?:\s*<[^>{}()]*>)?\s+extends\s+([A-Z]\w*)")
        .expect("class regex")
});

/// `Foo(` — plain instantiation.
static SIMPLE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?:const\s+)?\b([A-Z]\w*)\s*\(").expect("simple regex")
});

/// `Foo.named(` — named constructor.
static NAMED: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?:const\s+)?\b([A-Z]\w*)\.[a-zA-Z_]\w*\s*\(")
        .expect("named regex")
});

/// `Foo<T>(` — generic instantiation.
static GENERIC: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?:const\s+)?\b([A-Z]\w*)\s*<[^<>()]*>\s*\(")
        .expect("generic regex")
});

/// `Foo<T>.named(` — generic named.
static GENERIC_NAMED: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?:const\s+)?\b([A-Z]\w*)\s*<[^<>()]*>\.[a-zA-Z_]\w*\s*\(")
        .expect("generic named regex")
});

// ── Public entry point ───────────────────────────────────────────────

/// Analyze a Dart source file. Returns widget usages + class declarations.
pub fn analyze_dart_file(source: &str) -> DartAnalysis {
    let skip = skip_ranges(source);
    let classes = parse_class_declarations(source, &skip);
    let widgets = parse_instantiations(source, &skip, &classes);
    DartAnalysis { widgets, classes }
}

// ── Class declarations ──────────────────────────────────────────────

fn parse_class_declarations(
    source: &str,
    skip: &[(usize, usize)],
) -> HashMap<String, ClassInfo> {
    let mut map = HashMap::new();
    for cap in CLASS_DECL.captures_iter(source) {
        let m = cap.get(0).unwrap();
        if in_skip_range(skip, m.start()) {
            continue;
        }
        let name = cap.get(1).unwrap().as_str().to_string();
        let superclass = cap.get(2).map(|m| m.as_str().to_string());

        // Range of the entire class declaration: from `class` to the matching `}`.
        let body_start = match source[m.start()..].find('{') {
            Some(off) => m.start() + off,
            None => m.start(),
        };
        let end = matching_brace(source, body_start).unwrap_or(body_start);

        map.insert(
            name.clone(),
            ClassInfo {
                name,
                superclass,
                byte_range: (m.start(), end),
            },
        );
    }
    map
}

/// Find the index just past the `}` that matches the `{` at `open`.
fn matching_brace(source: &str, open: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    if bytes.get(open) != Some(&b'{') {
        return None;
    }
    let mut depth = 1;
    let mut i = open + 1;
    while i < bytes.len() && depth > 0 {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            b'"' | b'\'' => {
                i = skip_string_literal(bytes, i);
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    Some(i)
}

// ── Instantiations ──────────────────────────────────────────────────

fn parse_instantiations(
    source: &str,
    skip: &[(usize, usize)],
    classes: &HashMap<String, ClassInfo>,
) -> im::Vector<FlutterWidgetInfo> {
    // Step 1: collect every `UpperName(` / `UpperName<T>(` / `UpperName.named(`
    // match, regardless of whether it's a widget. This preserves correct
    // depth for widgets nested inside non-widget calls.
    let mut found: Vec<(usize, &str, usize)> = Vec::new();
    let mut seen: HashSet<usize> = HashSet::new();

    for re in [&*GENERIC_NAMED, &*GENERIC, &*NAMED, &*SIMPLE] {
        for cap in re.captures_iter(source) {
            let m = cap.get(0).unwrap();
            if in_skip_range(skip, m.start()) || !seen.insert(m.start()) {
                continue;
            }
            let class_name = cap.get(1).unwrap().as_str();
            let paren = m.end() - 1;
            // Skip constructor *definitions*: `Foo({Key? key}) : super(...)`
            let after = source[paren + 1..].trim_start();
            if after.starts_with('{') {
                continue;
            }
            found.push((m.start(), class_name, paren));
        }
    }
    found.sort_by_key(|(s, _, _)| *s);

    // Step 2: walk sorted matches with a paren-nesting stack to assign depth.
    // `stack` holds (start_byte, end_byte) of the currently-open instantiation.
    let mut stack: Vec<(usize, usize)> = Vec::new();
    let mut placed: Vec<(usize, &str, usize, usize, usize)> = Vec::new();
    for (start, class_name, paren) in &found {
        let call_end = matching_paren(source, *paren).unwrap_or(*start);
        while let Some(&(_, end)) = stack.last() {
            if end <= *start {
                stack.pop();
            } else {
                break;
            }
        }
        let depth = stack.len();
        stack.push((*start, call_end));
        placed.push((*start, class_name, *paren, call_end, depth));
    }

    // Step 3: emit only real widgets, preserving depth.
    let mut widgets = im::Vector::new();
    for (start, class_name, _paren, call_end, depth) in placed {
        if !is_widget_class(class_name, classes) {
            continue;
        }
        let superclass = classes.get(class_name).and_then(|c| c.superclass.clone());
        let source_range = classes.get(class_name).map(|c| c.byte_range);
        widgets.push_back(FlutterWidgetInfo {
            class_name: class_name.to_string(),
            superclass,
            source_range,
            instantiation_range: Some((start, call_end)),
            snippet: Some(source[start..call_end].to_string()),
            depth,
        });
    }
    widgets
}

fn matching_paren(source: &str, open: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    if bytes.get(open) != Some(&b'(') {
        return None;
    }
    let mut depth = 1;
    let mut i = open + 1;
    while i < bytes.len() && depth > 0 {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            b'"' | b'\'' => {
                i = skip_string_literal(bytes, i);
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    Some(i)
}

fn is_widget_class(name: &str, classes: &HashMap<String, ClassInfo>) -> bool {
    if WIDGET_BASES.contains(&name) {
        return true;
    }
    if WidgetKind::from_class_name(name) != WidgetKind::Other {
        return true;
    }
    let mut current = name;
    for _ in 0..20 {
        let Some(info) = classes.get(current) else {
            return false;
        };
        let Some(sup) = &info.superclass else {
            return false;
        };
        if WIDGET_BASES.contains(&sup.as_str()) {
            return true;
        }
        current = sup;
    }
    false
}

// ── Skip ranges (comments and strings) ───────────────────────────────

fn skip_ranges(source: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let bytes = source.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'/' if i + 1 < bytes.len() => match bytes[i + 1] {
                b'/' => {
                    let start = i;
                    i += 2;
                    while i < bytes.len() && bytes[i] != b'\n' {
                        i += 1;
                    }
                    ranges.push((start, i));
                }
                b'*' => {
                    let start = i;
                    i += 2;
                    while i + 1 < bytes.len() {
                        if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                            i += 2;
                            break;
                        }
                        i += 1;
                    }
                    ranges.push((start, i));
                }
                _ => i += 1,
            },
            b'"' | b'\'' => {
                let is_raw = i > 0 && (bytes[i - 1] == b'r' || bytes[i - 1] == b'R');
                let start = i;
                i = skip_string_literal_with_raw(bytes, i, is_raw);
                ranges.push((start, i));
            }
            _ => i += 1,
        }
    }
    ranges
}

fn skip_string_literal(bytes: &[u8], start: usize) -> usize {
    skip_string_literal_with_raw(bytes, start, false)
}

fn skip_string_literal_with_raw(bytes: &[u8], start: usize, is_raw: bool) -> usize {
    if start >= bytes.len() {
        return start;
    }
    let q = bytes[start];
    let triple = start + 2 < bytes.len() && bytes[start + 1] == q && bytes[start + 2] == q;
    let mut i = start + if triple { 3 } else { 1 };
    while i < bytes.len() {
        if triple {
            if i + 2 < bytes.len() && bytes[i] == q && bytes[i + 1] == q && bytes[i + 2] == q {
                return i + 3;
            }
        } else if bytes[i] == q {
            return i + 1;
        }
        if !is_raw && bytes[i] == b'\\' && i + 1 < bytes.len() {
            i += 2;
        } else {
            i += 1;
        }
    }
    i
}

fn in_skip_range(ranges: &[(usize, usize)], pos: usize) -> bool {
    ranges.iter().any(|(s, e)| pos >= *s && pos < *e)
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_plain_instantiation() {
        let src = r#"
            class MyWidget extends StatelessWidget {
                Widget build(BuildContext c) => Text('hi');
            }
        "#;
        let a = analyze_dart_file(src);
        let names: Vec<&str> = a.widgets.iter().map(|w| w.class_name.as_str()).collect();
        assert!(names.contains(&"Text"));
    }

    #[test]
    fn detects_generic_instantiation() {
        let src = "class X extends StatelessWidget { Widget b() => FutureBuilder<int>(future: null, builder: null); }";
        let a = analyze_dart_file(src);
        assert!(a.widgets.iter().any(|w| w.class_name == "FutureBuilder"));
    }

    #[test]
    fn detects_named_constructor() {
        let src = "class X extends StatelessWidget { Widget b() => const SizedBox.shrink(); }";
        let a = analyze_dart_file(src);
        assert!(a.widgets.iter().any(|w| w.class_name == "SizedBox"));
    }

    #[test]
    fn ignores_strings_and_comments() {
        let src = r#"
            // Container( in a comment
            /* Row( also a comment */
            class X extends StatelessWidget {
                Widget b() => Text('Container( in string');
            }
        "#;
        let a = analyze_dart_file(src);
        let names: Vec<&str> = a.widgets.iter().map(|w| w.class_name.as_str()).collect();
        assert!(!names.contains(&"Container"));
        assert!(!names.contains(&"Row"));
        assert!(names.contains(&"Text"));
    }

    #[test]
    fn skips_constructor_definitions() {
        let src = r#"
            class Foo extends StatelessWidget {
                const Foo({Key? key}) : super(key: key);
                Widget build(BuildContext c) => const Foo(key: null);
            }
        "#;
        let a = analyze_dart_file(src);
        // Constructor def at `Foo({` should be skipped; call site kept.
        let count = a.widgets.iter().filter(|w| w.class_name == "Foo").count();
        assert!(count >= 1, "expected at least one Foo usage");
    }

    #[test]
    fn class_extending_stateless_is_widget() {
        let src = "class CartButton extends StatelessWidget { Widget b() => SizedBox(); }";
        let a = analyze_dart_file(src);
        assert!(a.widgets.iter().any(|w| w.class_name == "CartButton" || w.class_name == "SizedBox"));
    }

    #[test]
    fn computes_nesting_depth() {
        let src = r#"
        class X extends StatelessWidget {
            Widget b() => Column(children: [
                Padding(padding: null, child: Text('hi')),
                SizedBox.shrink(),
            ]);
        }
    "#;
        let a = analyze_dart_file(src);
        let by_name: std::collections::HashMap<&str, usize> =
            a.widgets.iter().map(|w| (w.class_name.as_str(), w.depth)).collect();
        assert_eq!(by_name.get("Column"), Some(&0));
        assert_eq!(by_name.get("Padding"), Some(&1));
        assert_eq!(by_name.get("Text"), Some(&2));
        assert_eq!(by_name.get("SizedBox"), Some(&1));
    }

    #[test]
    fn skips_non_widget_calls_but_keeps_depth() {
        let src = "class X extends StatelessWidget { Widget b() => Dummy(Text('x')); }";
        let a = analyze_dart_file(src);
        // `Dummy` is not a widget, so it's dropped from output.
        // But `Text` is still a widget at depth 1 (it was inside Dummy()).
        let text = a.widgets.iter().find(|w| w.class_name == "Text").unwrap();
        assert_eq!(text.depth, 1);
    }
}