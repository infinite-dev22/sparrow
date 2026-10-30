use regex::Regex;

#[derive(Debug, Clone)]
pub struct DartTestLocation {
    pub name: String,
    /// Byte range of the full `testWidgets(...)` / `test(...)` call.
    pub byte_range: (usize, usize),
}

/// Find the innermost `test(...)` / `testWidgets(...)` whose body contains
/// `offset`. Returns `None` if the cursor isn't inside a test.
pub fn find_test_at_offset(source: &str, offset: usize) -> Option<DartTestLocation> {
    // Match test declarations. Group name in capture 2.
    let re = Regex::new(
        r#"(?m)^[ \t]*(?:testWidgets|test)\s*\(\s*(['"])(.*?)\1"#,
    )
        .ok()?;

    let mut best: Option<DartTestLocation> = None;
    for cap in re.captures_iter(source) {
        let m = cap.get(0).unwrap();
        let name = cap.get(2)?.as_str().to_string();

        // Find opening `(` of the call and matching `)`.
        let paren = source[m.start()..].find('(').map(|o| m.start() + o)?;
        let end = matching_paren(source, paren)?;

        if offset >= paren && offset < end {
            // Keep the innermost (largest start, smallest span).
            let byte_range = (paren, end);
            let is_better = match &best {
                None => true,
                Some(b) => byte_range.0 > b.byte_range.0,
            };
            if is_better {
                best = Some(DartTestLocation { name, byte_range });
            }
        }
    }
    best
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
                // Skip string literal
                let q = bytes[i];
                let triple = i + 2 < bytes.len()
                    && bytes[i + 1] == q
                    && bytes[i + 2] == q;
                let mut j = i + if triple { 3 } else { 1 };
                while j < bytes.len() {
                    if triple {
                        if j + 2 < bytes.len()
                            && bytes[j] == q
                            && bytes[j + 1] == q
                            && bytes[j + 2] == q
                        {
                            j += 3;
                            break;
                        }
                    } else if bytes[j] == q {
                        j += 1;
                        break;
                    }
                    if bytes[j] == b'\\' && j + 1 < bytes.len() {
                        j += 2;
                    } else {
                        j += 1;
                    }
                }
                i = j;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    Some(i)
}

/// Construct the `flutter test` command line for the given test.
pub fn build_test_command(
    file: &std::path::Path,
    test_name: &str,
) -> (String, Vec<String>) {
    let program = "flutter".to_string();
    let args = vec![
        "test".to_string(),
        file.display().to_string(),
        "--plain-name".to_string(),
        test_name.to_string(),
        "--reporter".to_string(),
        "expanded".to_string(),
    ];
    (program, args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_test_widgets() {
        let src = r#"
void main() {
  testWidgets('my test', (tester) async {
    expect(1, 1);
  });
}
"#;
        let offset = src.find("expect").unwrap();
        let t = find_test_at_offset(src, offset).unwrap();
        assert_eq!(t.name, "my test");
    }

    #[test]
    fn finds_plain_test() {
        let src = "void main() { test('plain', () { expect(1, 1); }); }";
        let offset = src.find("expect").unwrap();
        assert_eq!(find_test_at_offset(src, offset).unwrap().name, "plain");
    }

    #[test]
    fn no_test_outside() {
        let src = "void main() { print('hi'); }";
        assert!(find_test_at_offset(src, 15).is_none());
    }

    #[test]
    fn string_with_paren_does_not_break_nesting() {
        let src = r#"test('has (parens)', () { expect(1, 1); });"#;
        let offset = src.find("expect").unwrap();
        assert_eq!(
            find_test_at_offset(src, offset).unwrap().name,
            "has (parens)"
        );
    }
}