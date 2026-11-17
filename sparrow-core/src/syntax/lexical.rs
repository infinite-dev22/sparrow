//! Bundled token highlighting for Flutter companion files when no grammar is installed.
use crate::language::SparrowLanguage;
use lapce_xi_rope::{
    Interval,
    spans::{Spans, SpansBuilder},
};
use once_cell::sync::Lazy;
use regex::Regex;
use sparrow_rpc::style::Style;

const TOKEN_PATTERN: &str = r#"(?P<block>/\*(?s:.*?)(?:\*/|\z)|<!--(?s:.*?)(?:-->|\z))|(?P<string>"""(?s:.*?)(?:"""|\z)|'''(?s:.*?)(?:'''|\z)|"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|`(?:\\.|[^`\\])*`)|(?P<slash>//[^\n]*)|(?P<hash>\#[^\n]*)|(?P<number>\b(?:0[xX][0-9a-fA-F]+|\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)\b)|(?P<word>[@$]?[\p{L}_][\p{L}\p{N}_$.-]*)"#;
const KEYWORDS: &str = "abstract as assert async await bool boolean break case catch char class const constexpr continue data default def defer do double dynamic else enum export extends extension external factory false final finally float for from fun function get if implements import in inline int interface internal is late let long namespace native new null nullptr object operator override package part private protected protocol public readonly record reified required return sealed set short signed static string struct super suspend switch synchronized template this throw throws trait true try typedef typename uint union unsigned using val var virtual void volatile when while with yield";

static TOKENS: Lazy<[Regex; 4]> = Lazy::new(|| {
    [0, 1, 2, 3].map(|flags| {
        let mut pattern = TOKEN_PATTERN.to_string();
        if flags & 1 == 0 {
            pattern = pattern.replace(r"(?P<hash>\#[^\n]*)", r"(?P<hash>\b\B)");
        }
        if flags & 2 == 0 {
            pattern = pattern.replace(r"(?P<slash>//[^\n]*)", r"(?P<slash>\b\B)");
        }
        Regex::new(&pattern).unwrap()
    })
});
static KEYWORD_SET: Lazy<std::collections::HashSet<&'static str>> =
    Lazy::new(|| KEYWORDS.split_whitespace().collect());

pub fn highlight(language: SparrowLanguage, text: &str) -> Option<Spans<Style>> {
    use SparrowLanguage::*;
    if matches!(language, PlainText | Log) {
        return None;
    }
    let hash_comments = matches!(
        language,
        Yaml | Ini
            | Make
            | Cmake
            | Bash
            | ShellScript
            | Toml
            | Gitattributes
            | GitConfig
            | GitCommit
            | GitRebase
    );
    let slash_comments = matches!(
        language,
        Dart | C
            | Cpp
            | Java
            | JavaScript
            | TypeScript
            | ObjectiveC
            | Kotlin
            | KotlinBuildScript
            | Swift
            | Groovy
            | Json5
            | Glsl
            | Ini
    );
    let config = matches!(
        language,
        Css | Yaml | Ini | Make | Cmake | Toml | Json | Json5
    );
    let markup = matches!(language, Xml | Html);
    let mut spans = SpansBuilder::new(text.len());
    for token in TOKENS[(hash_comments as usize) | ((slash_comments as usize) << 1)]
        .captures_iter(text)
    {
        let found = token.get(0).unwrap();
        let rest = &text[found.end()..];
        let word = found.as_str();
        let scope = if token.name("block").is_some()
            || (hash_comments && token.name("hash").is_some())
            || (slash_comments && token.name("slash").is_some())
        {
            "comment"
        } else if token.name("string").is_some() {
            if config && rest.trim_start().starts_with(':') {
                "variable.other.member"
            } else {
                "string"
            }
        } else if token.name("number").is_some() {
            "number"
        } else if token.name("word").is_some() {
            if KEYWORD_SET.contains(word) {
                "keyword"
            } else if config && rest.trim_start().starts_with([':', '=']) {
                "variable.other.member"
            } else if markup
                && text[..found.start()].trim_end().ends_with(['<', '/'])
            {
                "type"
            } else if rest.trim_start().starts_with('(') {
                "function"
            } else if word.starts_with(['@', '$']) {
                "attribute"
            } else if word.chars().next().is_some_and(char::is_uppercase) {
                "type"
            } else {
                continue;
            }
        } else {
            continue;
        };
        spans.add_span(
            Interval::new(found.start(), found.end()),
            Style {
                fg_color: Some(scope.into()),
            },
        );
    }
    Some(spans.build())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn companion_files_are_detected_and_have_bundled_highlighting() {
        for (file, source) in [
            ("Makefile", "build:\n\tcc app.c\n# comment\n"),
            ("CMakeLists.txt", "cmake_minimum_required(VERSION 3.10)"),
            ("pubspec.yaml", "name: demo\nversion: '1.0' # comment"),
            ("local.properties", "sdk.dir=/opt/android\n# comment"),
            (".gitignore", "# ignored\nbuild/"),
            ("main.c", "int main() { return 0; }"),
            ("main.cpp", "class Widget {};"),
            ("app.h", "#pragma once\nint value;"),
            ("index.html", "<html title=\"hello\"></html>"),
            ("index.js", "const label = `hello`; // comment"),
            ("app.ts", "interface Widget { title: string; }"),
            ("style.css", "body { color: \"red\"; }"),
            ("data.json", "{\"label\": \"hello\", \"count\": 2}"),
            ("app.xcconfig", "PRODUCT_NAME = demo // app"),
            ("main.swift", "let name = \"héllo\""),
            ("main.m", "@interface App : NSObject"),
            ("main.kt", "fun main() { val title = \"hello\" }"),
            ("Info.plist", "<plist><string>Hello</string></plist>"),
            (
                "App.entitlements",
                "<plist><key>com.apple.security</key></plist>",
            ),
            (".env", "API_URL=\"https://example.com\""),
            ("Main.java", "public class Main { private int count = 2; }"),
        ] {
            let language = SparrowLanguage::from_path(std::path::Path::new(file));
            assert_ne!(language, SparrowLanguage::PlainText, "{file}");
            let spans = highlight(language, source).unwrap();
            assert!(spans.iter().next().is_some(), "{file}");
            for (range, _) in spans.iter() {
                assert!(source.is_char_boundary(range.start()));
                assert!(source.is_char_boundary(range.end()));
            }
        }
    }
    #[test]
    fn comments_and_multiline_strings_do_not_produce_overlapping_tokens() {
        let text = "/* class notKeyword */\nconst label = `héllo\nworld`;";
        let spans = highlight(SparrowLanguage::JavaScript, text).unwrap();
        let tokens = spans
            .iter()
            .map(|(r, s)| {
                (
                    text[r.start()..r.end()].to_string(),
                    s.fg_color.clone().unwrap(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(tokens[0].1, "comment");
        assert!(
            tokens
                .iter()
                .any(|(text, scope)| text.contains("héllo\nworld")
                    && scope == "string")
        );
    }
}
