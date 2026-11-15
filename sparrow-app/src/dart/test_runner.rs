//! Dart test discovery and Flutter's JSON reporter model.
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartTestLocation {
    pub name: String,
    pub byte_range: (usize, usize),
    pub line: usize,
    pub is_group: bool,
}

pub fn is_test_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.ends_with("_test.dart"))
        && path.components().any(|c| c.as_os_str() == "test")
}

// Tokenize before looking for calls so comments, strings, and parentheses inside
// them cannot create false test declarations or corrupt enclosing group ranges.
#[derive(Debug)]
struct Token {
    start: usize,
    end: usize,
    text: String,
    literal: bool,
}
fn tokens(source: &str) -> Vec<Token> {
    let b = source.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let start = i;
        if b[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if b[i..].starts_with(b"//") {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if b[i..].starts_with(b"/*") {
            i += 2;
            let mut depth = 1;
            while i < b.len() && depth > 0 {
                if b[i..].starts_with(b"/*") {
                    depth += 1;
                    i += 2;
                } else if b[i..].starts_with(b"*/") {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        let raw = b[i] == b'r' && matches!(b.get(i + 1), Some(b'\'' | b'"'));
        if raw || matches!(b[i], b'\'' | b'"') {
            if raw {
                i += 1;
            }
            let q = b[i];
            let n = if b.get(i..i + 3).is_some_and(|s| s == [q, q, q]) {
                3
            } else {
                1
            };
            i += n;
            let mut value = String::new();
            let mut valid = true;
            let mut closed = false;
            while i < b.len() {
                if b.get(i..i + n).is_some_and(|s| s.iter().all(|c| *c == q)) {
                    i += n;
                    closed = true;
                    break;
                }
                if !raw && b[i] == b'$' {
                    valid = false;
                }
                if !raw && b[i] == b'\\' {
                    i += 1;
                    if i >= b.len() {
                        break;
                    }
                    match b[i] {
                        b'n' => value.push('\n'),
                        b'r' => value.push('\r'),
                        b't' => value.push('\t'),
                        b'\\' => value.push('\\'),
                        b'\'' => value.push('\''),
                        b'"' => value.push('"'),
                        b'$' => value.push('$'),
                        _ => valid = false,
                    }
                    i += 1;
                } else {
                    let ch = source[i..].chars().next().unwrap();
                    value.push(ch);
                    i += ch.len_utf8();
                }
            }
            out.push(Token {
                start,
                end: i,
                text: value,
                literal: valid && closed,
            });
        } else if b[i].is_ascii_alphabetic() || b[i] == b'_' {
            i += 1;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            out.push(Token {
                start,
                end: i,
                text: source[start..i].into(),
                literal: false,
            });
        } else {
            let ch = source[i..].chars().next().unwrap();
            i += ch.len_utf8();
            out.push(Token {
                start,
                end: i,
                text: ch.to_string(),
                literal: false,
            });
        }
    }
    out
}

pub fn discover_tests(source: &str) -> Vec<DartTestLocation> {
    let ts = tokens(source);
    let mut calls = Vec::new();
    let mut opens = Vec::new();
    let mut ends = BTreeMap::new();
    for (i, t) in ts.iter().enumerate() {
        if t.text == "(" && !t.literal {
            opens.push(i);
        }
        if t.text == ")" && !t.literal {
            if let Some(open) = opens.pop() {
                ends.insert(open, t.end);
            }
        }
    }
    for (i, t) in ts.iter().enumerate() {
        if t.literal || !matches!(t.text.as_str(), "test" | "testWidgets" | "group")
        {
            continue;
        }
        // Ignore member methods; these aren't package:test declarations.
        if i > 0 && ts[i - 1].text == "." {
            continue;
        }
        let (Some(open), Some(name), Some(comma)) =
            (ts.get(i + 1), ts.get(i + 2), ts.get(i + 3))
        else {
            continue;
        };
        if open.text != "(" || !name.literal || comma.text != "," {
            continue;
        }
        let Some(end) = ends.get(&(i + 1)).copied() else {
            continue;
        };
        calls.push(DartTestLocation {
            name: name.text.clone(),
            byte_range: (t.start, end),
            line: source[..t.start].bytes().filter(|b| *b == b'\n').count(),
            is_group: t.text == "group",
        });
    }
    let groups = calls
        .iter()
        .filter(|t| t.is_group)
        .cloned()
        .collect::<Vec<_>>();
    for call in &mut calls {
        let mut names = groups
            .iter()
            .filter(|g| {
                g.byte_range.0 < call.byte_range.0
                    && g.byte_range.1 >= call.byte_range.1
            })
            .map(|g| g.name.as_str())
            .collect::<Vec<_>>();
        names.push(&call.name);
        call.name = names.join(" ");
    }
    calls
}

pub fn find_test_at_offset(source: &str, offset: usize) -> Option<DartTestLocation> {
    discover_tests(source)
        .into_iter()
        .filter(|t| offset >= t.byte_range.0 && offset < t.byte_range.1)
        .max_by_key(|t| t.byte_range.0)
}

pub fn project_root(file: &Path) -> Option<PathBuf> {
    file.parent()?
        .ancestors()
        .find(|p| p.join("pubspec.yaml").is_file())
        .map(Path::to_path_buf)
}

pub fn build_test_command(file: &Path, test_name: &str) -> (String, Vec<String>) {
    build_test_command_with_filter(file, test_name, false)
}

/// Failed-test reruns use an anchored expression: --plain-name is a substring
/// filter and would also execute passing tests with a longer matching name.
pub fn build_test_command_with_filter(
    file: &Path,
    test_name: &str,
    exact: bool,
) -> (String, Vec<String>) {
    let root = project_root(file);
    let program = crate::dart::sdk::find_flutter_binary(root.as_deref())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "flutter".into());
    (program, test_arguments(file, test_name, exact))
}

fn test_arguments(file: &Path, test_name: &str, exact: bool) -> Vec<String> {
    let mut args = vec![
        "test".into(),
        "--reporter".into(),
        "json".into(),
        file.display().to_string(),
    ];
    if !test_name.is_empty() {
        args.extend(if exact {
            ["--name".into(), format!("^{}$", regex::escape(test_name))]
        } else {
            ["--plain-name".into(), test_name.into()]
        });
    }
    args
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestStatus {
    Running,
    Passed,
    Failed,
    Skipped,
    Cancelled,
}
impl TestStatus {
    pub fn symbol(self) -> &'static str {
        match self {
            Self::Running => "◌",
            Self::Passed => "✓",
            Self::Failed => "✗",
            Self::Skipped => "–",
            Self::Cancelled => "■",
        }
    }
}
#[derive(Debug, Clone)]
pub struct TestCase {
    pub id: i64,
    pub suite: i64,
    pub groups: Vec<i64>,
    pub name: String,
    pub status: TestStatus,
    pub started: u64,
    pub duration: Option<u64>,
    pub output: Vec<String>,
    pub hidden: bool,
}
#[derive(Debug, Clone)]
pub struct TestGroup {
    pub suite: i64,
    pub parent: Option<i64>,
    pub name: String,
}
#[derive(Debug, Clone, Default)]
pub struct TestRun {
    pub suites: BTreeMap<i64, String>,
    pub groups: BTreeMap<i64, TestGroup>,
    pub tests: BTreeMap<i64, TestCase>,
    pub console: Vec<String>,
    pub running: bool,
    pub success: Option<bool>,
}
impl TestRun {
    pub fn line(&mut self, line: &str) {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            self.console.push(line.into());
            return;
        };
        let time = v["time"].as_u64().unwrap_or_default();
        match v["type"].as_str().unwrap_or_default() {
            "suite" => {
                if let Some(id) = v["suite"]["id"].as_i64() {
                    self.suites.insert(
                        id,
                        v["suite"]["path"].as_str().unwrap_or("Test suite").into(),
                    );
                }
            }
            "group" => {
                let g = &v["group"];
                if let Some(id) = g["id"].as_i64() {
                    self.groups.insert(
                        id,
                        TestGroup {
                            suite: g["suiteID"].as_i64().unwrap_or_default(),
                            parent: g["parentID"].as_i64(),
                            name: g["name"].as_str().unwrap_or_default().into(),
                        },
                    );
                }
            }
            "testStart" => {
                let t = &v["test"];
                if let Some(id) = t["id"].as_i64() {
                    self.tests.insert(
                        id,
                        TestCase {
                            id,
                            suite: t["suiteID"].as_i64().unwrap_or_default(),
                            groups: t["groupIDs"]
                                .as_array()
                                .map(|ids| {
                                    ids.iter().filter_map(Value::as_i64).collect()
                                })
                                .unwrap_or_default(),
                            name: t["name"]
                                .as_str()
                                .unwrap_or("Unnamed test")
                                .into(),
                            status: TestStatus::Running,
                            started: time,
                            duration: None,
                            output: Vec::new(),
                            hidden: false,
                        },
                    );
                }
            }
            "testDone" => {
                if let Some(t) =
                    v["testID"].as_i64().and_then(|id| self.tests.get_mut(&id))
                {
                    t.duration = Some(time.saturating_sub(t.started));
                    t.status = if v["result"] != "success" {
                        TestStatus::Failed
                    } else if v["skipped"] == true {
                        TestStatus::Skipped
                    } else {
                        TestStatus::Passed
                    };
                    t.hidden = v["hidden"] == true && t.status != TestStatus::Failed;
                }
            }
            "error" | "print" => {
                let text = if v["type"] == "error" {
                    format!(
                        "{}\n{}",
                        v["error"].as_str().unwrap_or_default(),
                        v["stackTrace"].as_str().unwrap_or_default()
                    )
                } else {
                    v["message"].as_str().unwrap_or_default().into()
                };
                self.console.extend(text.lines().map(str::to_owned));
                if let Some(t) =
                    v["testID"].as_i64().and_then(|id| self.tests.get_mut(&id))
                {
                    t.output.extend(text.lines().map(str::to_owned));
                    if v["type"] == "error" {
                        t.status = TestStatus::Failed;
                        t.hidden = false;
                    }
                }
            }
            "done" => {
                self.success = v["success"].as_bool();
            }
            "start" | "allSuites" | "debug" => {}
            _ => self.console.push(line.into()),
        }
    }
    pub fn finish(&mut self, success: bool, cancelled: bool) {
        self.running = false;
        for t in self
            .tests
            .values_mut()
            .filter(|t| t.status == TestStatus::Running)
        {
            t.status = if cancelled {
                TestStatus::Cancelled
            } else {
                TestStatus::Failed
            };
        }
        self.success = Some(
            success
                && self.success != Some(false)
                && !cancelled
                && self.tests.values().all(|t| t.status != TestStatus::Failed),
        );
    }
    pub fn failed_targets(&self) -> Vec<(PathBuf, String)> {
        self.tests
            .values()
            .filter(|t| t.status == TestStatus::Failed)
            .filter_map(|t| {
                let path = PathBuf::from(self.suites.get(&t.suite)?);
                // Failed suite loading must be retried by file, without its synthetic name.
                let name = if t.name.starts_with("loading ")
                    || t.name.contains("(setUpAll)")
                    || t.name.contains("(tearDownAll)")
                {
                    String::new()
                } else {
                    t.name.clone()
                };
                Some((path, name))
            })
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn command_filters_preserve_names_and_anchor_failed_reruns() {
        let path = Path::new("/project with spaces/test/a_test.dart");
        let args = test_arguments(path, "group test (1)+", false);
        assert_eq!(
            &args[..4],
            [
                "test",
                "--reporter",
                "json",
                "/project with spaces/test/a_test.dart"
            ]
        );
        assert_eq!(&args[4..], ["--plain-name", "group test (1)+"]);
        let exact = test_arguments(path, "group test (1)+", true);
        assert_eq!(&exact[4..], ["--name", r"^group test \(1\)\+$"]);
        assert_eq!(test_arguments(path, "", false).len(), 4);
    }

    #[test]
    fn discovery_handles_groups_comments_and_strings() {
        let src = r#"void main() {
          // test('fake', () {});
          final s = "test('fake', () {})";
          group('outer', () { group("inner", () {
            testWidgets('it\'s good', (tester) async { /* ) /* nested */ */ expect(')', ')'); });
          }); });
        }"#;
        let tests = discover_tests(src);
        assert_eq!(tests.len(), 3);
        assert_eq!(tests[2].name, "outer inner it's good");
        assert_eq!(
            find_test_at_offset(src, src.find("expect").unwrap()).unwrap(),
            tests[2]
        );
        assert!(find_test_at_offset(src, 0).is_none());
    }
    #[test]
    fn rejects_dynamic_and_incomplete_names() {
        assert!(discover_tests("test('hello $name', () {}); test('a' + name, () {}); test('oops', () {").is_empty());
        assert_eq!(
            discover_tests("test(r'$literal', () {});")[0].name,
            "$literal"
        );
    }
    #[test]
    fn recognizes_only_test_directory_files() {
        assert!(is_test_file(Path::new("/app/test/widgets/a_test.dart")));
        assert!(!is_test_file(Path::new("/app/lib/a_test.dart")));
        assert!(!is_test_file(Path::new("/app/test/main.dart")));
    }
    #[test]
    fn reporter_keeps_late_failures_and_durations() {
        let mut run = TestRun::default();
        for line in [
            r#"{"type":"suite","suite":{"id":1,"path":"test/a_test.dart"}}"#,
            r#"{"type":"testStart","test":{"id":2,"suiteID":1,"name":"group test","groupIDs":[]},"time":10}"#,
            r#"{"type":"testDone","testID":2,"result":"success","hidden":true,"time":35}"#,
            r#"{"type":"error","testID":2,"error":"Expected: 1\nActual: 2","stackTrace":"test/a_test.dart 4:5"}"#,
        ] {
            run.line(line);
        }
        assert_eq!(run.tests[&2].duration, Some(25));
        assert_eq!(run.tests[&2].status, TestStatus::Failed);
        assert!(!run.tests[&2].hidden);
        assert_eq!(
            run.failed_targets(),
            vec![(PathBuf::from("test/a_test.dart"), "group test".into())]
        );
    }
    #[test]
    fn incomplete_stream_cannot_report_success() {
        let mut run = TestRun::default();
        run.line(r#"{"type":"testStart","test":{"id":1,"name":"test"}}"#);
        run.finish(true, false);
        assert_eq!(run.success, Some(false));
        assert_eq!(run.tests[&1].status, TestStatus::Failed);
    }

    #[test]
    fn interrupted_runs_do_not_report_success() {
        let mut run = TestRun::default();
        run.line(r#"{"type":"testStart","test":{"id":1,"name":"test"}}"#);
        run.finish(false, true);
        assert_eq!(run.success, Some(false));
        assert_eq!(run.tests[&1].status, TestStatus::Cancelled);
    }
}
