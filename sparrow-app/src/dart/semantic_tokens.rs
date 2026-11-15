//! Dart semantic token decoding and editor style mapping.
use lapce_xi_rope::Rope;
use lsp_types::{SemanticTokens, SemanticTokensLegend};
use sparrow_core::{
    buffer::rope_text::{RopeText, RopeTextRef},
    rope_text_pos::RopeTextPosition,
};
use sparrow_rpc::style::{LineStyle, Style};

/// Decode LSP semantic tokens into a `Vec<LineStyle>` (byte-range +
/// theme key). Overlapping spans are resolved last-wins (LSP guarantees
/// non-overlap in `relative` format, but the spec also permits overlap
/// for some servers — we defend against both).
pub fn decode(
    tokens: &SemanticTokens,
    source: &Rope,
    legend: &SemanticTokensLegend,
) -> Vec<LineStyle> {
    let rope_text = RopeTextRef::new(source);
    let mut out = Vec::new();
    let mut line: u32 = 0;
    let mut char_offset: u32 = 0;

    for tok in &tokens.data {
        let delta_line = tok.delta_line;
        let delta_char = tok.delta_start;
        let length = tok.length;
        let token_type_idx = tok.token_type as usize;

        line = line.saturating_add(delta_line);
        char_offset = if delta_line == 0 {
            char_offset.saturating_add(delta_char)
        } else {
            delta_char
        };

        let Some(token_type) = legend.token_types.get(token_type_idx) else {
            continue;
        };
        let Some(theme_key) = map_token_type(token_type.as_str()) else {
            continue;
        };

        let start_pos = lsp_types::Position {
            line,
            character: char_offset,
        };
        let end_pos = lsp_types::Position {
            line,
            character: char_offset.saturating_add(length),
        };

        if end_pos.line != start_pos.line {
            // Multi-line tokens are rare; ignore for now.
            continue;
        }
        // Guard against positions past the end of the file (server bug
        // or race with an edit).
        if rope_text.offset_of_line(line as usize) > source.len() {
            continue;
        }

        let start = rope_text.offset_of_position(&start_pos);
        let end = rope_text.offset_of_position(&end_pos);

        if start >= end || end > source.len() {
            continue;
        }

        out.push(LineStyle {
            start,
            end,
            style: Style {
                fg_color: Some(theme_key.to_string()),
            },
        });
    }

    // Sort and drop overlaps so `SpansBuilder` receives monotone ranges.
    out.sort_by_key(|s| (s.start, s.end));
    let mut filtered = Vec::with_capacity(out.len());
    let mut prev_end = 0;
    for style in out {
        if style.start < prev_end {
            continue;
        }
        prev_end = style.end;
        filtered.push(style);
    }
    filtered
}

/// LSP token type → theme `syntax` key. Returns `None` for types we
/// choose not to highlight (they fall back to tree-sitter grammar).
fn map_token_type(token_type: &str) -> Option<&'static str> {
    Some(match token_type {
        "namespace" => "type",
        "type" => "type",
        "class" => "type",
        "enum" => "enum",
        "interface" => "interface",
        "struct" => "structure",
        "typeParameter" => "type",
        "parameter" => "variable",
        "variable" => "variable",
        "property" => "property",
        "enumMember" => "enum-member",
        "event" => "function",
        "function" => "function",
        "method" => "method",
        "macro" => "keyword",
        "keyword" => "keyword",
        "modifier" => "keyword",
        "comment" => "comment",
        "string" => "string",
        "number" => "number",
        "regexp" => "string",
        "operator" => "keyword",
        "decorator" => "attribute",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use lsp_types::SemanticTokenType;

    use super::*;

    fn legend(types: &[&str]) -> SemanticTokensLegend {
        SemanticTokensLegend {
            token_types: types
                .iter()
                .map(|s| SemanticTokenType::from(s.to_string()))
                .collect(),
            token_modifiers: vec![],
        }
    }

    /// (delta_line, delta_start, length, token_type)
    fn tok(
        delta_line: u32,
        delta_start: u32,
        length: u32,
        token_type: u32,
    ) -> lsp_types::SemanticToken {
        lsp_types::SemanticToken {
            delta_line,
            delta_start,
            length,
            token_type,
            token_modifiers_bitset: 0,
        }
    }

    #[test]
    fn decode_simple() {
        let legend = legend(&["function", "variable"]);
        let source = Rope::from("void foo() { int x; }");
        // foo at byte 5..8 (function, idx 0)
        // x   at byte 17..18 (variable, idx 1)
        let tokens = SemanticTokens {
            result_id: None,
            data: vec![
                tok(0, 5, 3, 0),  // (line 0, char 5, len 3, type 0)
                tok(0, 12, 1, 1), // (same line, 5 + 12 = char 17, len 1, type 1)
            ],
        };
        let styles = decode(&tokens, &source, &legend);
        assert_eq!(styles.len(), 2);
        assert_eq!((styles[0].start, styles[0].end), (5, 8));
        assert_eq!(styles[0].style.fg_color.as_deref(), Some("function"));
        assert_eq!((styles[1].start, styles[1].end), (17, 18));
        assert_eq!(styles[1].style.fg_color.as_deref(), Some("variable"));
    }

    #[test]
    fn decode_multiline() {
        let legend = legend(&["keyword"]);
        let source = Rope::from("int a;\nint b;\n");
        let tokens = SemanticTokens {
            result_id: None,
            data: vec![
                tok(0, 0, 3, 0), // "int" line 0
                tok(1, 0, 3, 0), // "int" line 1 (delta_line=1)
            ],
        };
        let styles = decode(&tokens, &source, &legend);
        assert_eq!(styles.len(), 2);
        assert_eq!(styles[0].start, 0);
        // "int a;\n" is 7 bytes.
        assert_eq!(styles[1].start, 7);
        assert_eq!(styles[1].end, 10);
    }

    #[test]
    fn decode_utf16_positions() {
        // The character `×` is 1 UTF-16 code unit but 2 UTF-8 bytes.
        let legend = legend(&["variable"]);
        let source = Rope::from("×int a;");
        // `a` is at UTF-16 position 4, byte position 5.
        let tokens = SemanticTokens {
            result_id: None,
            data: vec![tok(0, 4, 1, 0)],
        };
        let styles = decode(&tokens, &source, &legend);
        assert_eq!(styles.len(), 1);
        assert_eq!((styles[0].start, styles[0].end), (5, 6));
    }

    #[test]
    fn decode_drops_unknown_token_types() {
        let legend = legend(&["neverHeardOfThis"]);
        let source = Rope::from("foo");
        let tokens = SemanticTokens {
            result_id: None,
            data: vec![tok(0, 0, 3, 0)],
        };
        assert!(decode(&tokens, &source, &legend).is_empty());
    }

    #[test]
    fn decode_never_exceeds_source() {
        // Position->offset conversion may clamp an over-long token to the end
        // of the line (or drop it); either is fine. What must never happen is
        // an empty/inverted span or one that reaches past the buffer.
        let legend = legend(&["function"]);
        let source = Rope::from("hi");
        let tokens = SemanticTokens {
            result_id: None,
            data: vec![
                tok(0, 0, 3, 0), // len 3 past end of "hi"
                tok(0, 5, 1, 0), // starts past end of line: must be dropped
            ],
        };
        let styles = decode(&tokens, &source, &legend);
        assert!(styles.len() <= 1);
        assert!(
            styles
                .iter()
                .all(|s| s.start < s.end && s.end <= source.len())
        );
    }

    #[test]
    fn map_covers_standard_types() {
        assert_eq!(map_token_type("function"), Some("function"));
        assert_eq!(map_token_type("method"), Some("method"));
        assert_eq!(map_token_type("class"), Some("type"));
        assert_eq!(map_token_type("enumMember"), Some("enum-member"));
        assert_eq!(map_token_type("keyword"), Some("keyword"));
        assert_eq!(map_token_type("comment"), Some("comment"));
        assert_eq!(map_token_type("string"), Some("string"));
        assert_eq!(map_token_type("operator"), Some("keyword"));
        assert_eq!(map_token_type("unknown_xyz"), None);
    }
}
