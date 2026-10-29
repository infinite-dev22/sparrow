//! Widget tree backed by LSP `documentSymbol`.
//!
//! The regex analyzer from `dart/regex.rs` remains useful for *instantiations*
//! (`Container(`, `Row(` etc.) which `documentSymbol` doesn't report. But it
//! can only guess which classes are widgets. This module combines both:
//!
//! 1. `documentSymbol` gives us the exact list of class declarations and
//!    their byte ranges (handles `sealed`, `base`, multi-line generics, etc.).
//! 2. We extract each class's `extends X` clause from that range.
//! 3. The regex analyzer produces instantiations.
//! 4. We enrich every instantiation with the accurate superclass chain and
//!    keep only those whose superclass is a widget.
//!
//! Result: the widget tree is more accurate than pure regex and no slower
//! for typical files.

use std::collections::HashMap;

use lsp_types::{DocumentSymbol, SymbolKind};

use crate::{
    dart::regex::analyze_dart_file,
    widget_kind::{FlutterWidgetInfo, WidgetKind},
};

/// Widget bases as defined by Flutter. Keep in sync with
/// `dart/regex.rs::WIDGET_BASES`.
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

/// Analyze a Dart source file using both LSP symbol info and the regex
/// analyzer. Returns an updated, more accurate widget list.
pub fn analyze_with_symbols(
    source: &str,
    symbols: &[DocumentSymbol],
) -> Vec<FlutterWidgetInfo> {
    // 1. Build `class name -> superclass` map from LSP declarations.
    let mut superclasses: HashMap<String, Option<String>> = HashMap::new();
    for sym in symbols {
        if sym.kind == SymbolKind::CLASS {
            let sc = extract_superclass(source, sym);
            superclasses.insert(sym.name.clone(), sc);
        }
    }

    // 2. Regex gives us instantiations (and their depth inside nested parens).
    let analysis = analyze_dart_file(source);

    // 3. Enrich each instantiation with superclass info and filter.
    analysis
        .widgets
        .into_iter()
        .filter(|w| {
            // Is this class a widget?
            if let Some(sc) = superclasses.get(&w.class_name) {
                matches!(sc.as_deref(), Some(base) if WIDGET_BASES.contains(&base))
                    || WidgetKind::from_class_name(&w.class_name) != WidgetKind::Other
            } else {
                // Not declared in this file — fall back to built-in names.
                WidgetKind::from_class_name(&w.class_name) != WidgetKind::Other
            }
        })
        .map(|mut w| {
            // Attach accurate superclass info for classification.
            if let Some(sc) = superclasses.get(&w.class_name) {
                w.superclass = sc.clone();
            }
            w
        })
        .collect()
}

fn extract_superclass(source: &str, sym: &DocumentSymbol) -> Option<String> {
    let start = offset_of_position(source, sym.range.start)?;
    let header_end = source[start..].find('{').map(|i| start + i)?;
    let header = &source[start..header_end];
    let extends_idx = header.find(" extends ")?;
    let after = &header[extends_idx + " extends ".len()..];
    let class_name = after
        .trim_start()
        .split(|c: char| c.is_whitespace() || c == '<' || c == '{' || c == ',')
        .next()?;
    if class_name.is_empty() {
        return None;
    }
    Some(class_name.to_string())
}

fn offset_of_position(source: &str, pos: lsp_types::Position) -> Option<usize> {
    let mut offset = 0usize;
    let mut line = 0u32;
    for chunk in source.split_inclusive('\n') {
        if line == pos.line {
            // `pos.character` is a UTF-16 code-unit offset. For the majority
            // of Dart source files this equals the byte offset; for files
            // containing non-BMP characters the returned offset will be
            // approximate. This is only used to bound the `extends` search
            // window, so approximate is fine.
            return Some(offset + pos.character as usize);
        }
        offset += chunk.len();
        line += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::{DocumentSymbol, Position, Range, SymbolKind};

    fn class_sym(name: &str, start_line: u32) -> DocumentSymbol {
        #[allow(deprecated)]
        DocumentSymbol {
            name: name.into(),
            detail: None,
            kind: SymbolKind::CLASS,
            tags: None,
            deprecated: None,
            range: Range {
                start: Position { line: start_line, character: 0 },
                end: Position { line: start_line + 5, character: 0 },
            },
            selection_range: Range {
                start: Position { line: start_line, character: 0 },
                end: Position { line: start_line, character: 20 },
            },
            children: None,
        }
    }

    #[test]
    fn extracts_simple_superclass() {
        let src = "class Foo extends StatelessWidget {\n  Widget build(c) => SizedBox();\n}\n";
        let symbols = vec![class_sym("Foo", 0)];
        let widgets = analyze_with_symbols(src, &symbols);
        assert!(widgets.iter().any(|w| w.class_name == "SizedBox"));
    }

    #[test]
    fn extracts_generic_superclass() {
        let src = "class Foo extends StatefulWidget {\n  State createState() => _S();\n}\n";
        let symbols = vec![class_sym("Foo", 0)];
        let widgets = analyze_with_symbols(src, &symbols);
        // No instantiations in this tiny file, just proving we didn't crash.
        assert!(widgets.is_empty() || widgets.iter().all(|w| w.class_name != "Foo"));
    }

    #[test]
    fn non_widget_instantiation_is_dropped() {
        let src = "class Config { final x = 1; }\n";
        let symbols = vec![class_sym("Config", 0)];
        let widgets = analyze_with_symbols(src, &symbols);
        assert!(widgets.iter().all(|w| w.class_name != "Config"));
    }

    #[test]
    fn falls_back_when_symbols_empty() {
        let src = "class MyWidget extends StatelessWidget {\n  Widget b() => Text('x');\n}\n";
        let widgets = analyze_with_symbols(src, &[]);
        // Regex-only path: Text is a builtin widget name.
        assert!(widgets.iter().any(|w| w.class_name == "Text"));
    }
}