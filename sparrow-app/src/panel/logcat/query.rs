use super::model::{Entry, priority};
use regex::{Regex, RegexBuilder};
use std::collections::BTreeMap;
#[derive(Clone, Debug)]
enum Token {
    Word(String),
    And,
    Or,
    Open,
    Close,
}
#[derive(Clone)]
pub enum Query {
    All,
    Term(Term),
    And(Vec<Query>),
    Or(Vec<Query>),
}
#[derive(Clone)]
pub struct Term {
    key: String,
    value: String,
    negated: bool,
    regex: Option<Regex>,
    case: bool,
}
impl Query {
    pub fn parse(input: &str, case: bool) -> Result<Self, String> {
        let tokens = lex(input)?;
        let mut parser = Parser {
            tokens,
            index: 0,
            case,
        };
        if parser.tokens.is_empty() {
            return Ok(Self::All);
        }
        let q = parser.implicit()?;
        if parser.index != parser.tokens.len() {
            return Err("Unexpected closing parenthesis".into());
        }
        Ok(q)
    }
    pub fn matches(&self, e: &Entry, mine: &[String], now: i64) -> bool {
        match self {
            Self::All => true,
            Self::And(q) => q.iter().all(|q| q.matches(e, mine, now)),
            Self::Or(q) => q.iter().any(|q| q.matches(e, mine, now)),
            Self::Term(t) => {
                let matched = match t.key.as_str() {
                    "name" => true,
                    "level" => {
                        priority(&e.level.to_string()).unwrap_or(0)
                            >= priority(&t.value).unwrap_or(6)
                    }
                    "age" => {
                        now.saturating_sub(e.time) <= age(&t.value).unwrap_or(0)
                            && e.time > 0
                    }
                    "package" if t.value == "mine" => {
                        mine.iter().any(|p| e.package == *p)
                    }
                    "is" => match t.value.as_str() {
                        "crash" => {
                            e.level == 'F'
                                || e.tag == "AndroidRuntime"
                                || e.message.contains("FATAL EXCEPTION")
                        }
                        "stacktrace" => {
                            e.message.trim_start().starts_with("at ")
                                || e.message.contains("Exception:")
                                || e.message.starts_with("Caused by:")
                        }
                        _ => false,
                    },
                    _ => {
                        let text = match t.key.as_str() {
                            "tag" => &e.tag,
                            "package" => &e.package,
                            "process" => &e.process,
                            "message" => &e.message,
                            _ => &e.raw,
                        };
                        if let Some(regex) = &t.regex {
                            regex.is_match(text)
                        } else if t.case {
                            text.contains(&t.value)
                        } else {
                            text.to_lowercase().contains(&t.value.to_lowercase())
                        }
                    }
                };
                matched != t.negated
            }
        }
    }
}
fn age(s: &str) -> Option<i64> {
    let n: i64 = s.get(..s.len().checked_sub(1)?)?.parse().ok()?;
    if n < 0 {
        return None;
    }
    n.checked_mul(match s.chars().last()? {
        's' => 1,
        'm' => 60,
        'h' => 3600,
        'd' => 86400,
        _ => return None,
    })
}
fn lex(s: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut escape = false;
    let flush = |word: &mut String, tokens: &mut Vec<Token>| {
        if !word.is_empty() {
            tokens.push(Token::Word(std::mem::take(word)));
        }
    };
    for ch in s.chars() {
        if let Some(q) = quote {
            if escape {
                if ch != q && ch != '\\' {
                    word.push('\\');
                }
                word.push(ch);
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == q {
                quote = None;
            } else {
                word.push(ch);
            }
            continue;
        }
        match ch {
            '"' | '\'' => quote = Some(ch),
            '&' | '|' | '(' | ')' => {
                flush(&mut word, &mut tokens);
                tokens.push(match ch {
                    '&' => Token::And,
                    '|' => Token::Or,
                    '(' => Token::Open,
                    _ => Token::Close,
                });
            }
            c if c.is_whitespace() => flush(&mut word, &mut tokens),
            _ => word.push(ch),
        }
    }
    if quote.is_some() {
        return Err("Unclosed quoted value".into());
    }
    flush(&mut word, &mut tokens);
    let mut merged: Vec<Token> = Vec::new();
    for token in tokens {
        if let Token::Word(value) = &token {
            if !value.contains(':') {
                if let Some(Token::Word(previous)) = merged.last_mut() {
                    if !previous.contains(':') {
                        previous.push(' ');
                        previous.push_str(value);
                        continue;
                    }
                }
            }
        }
        merged.push(token);
    }
    Ok(merged)
}
struct Parser {
    tokens: Vec<Token>,
    index: usize,
    case: bool,
}
impl Parser {
    fn implicit(&mut self) -> Result<Query, String> {
        let mut clauses = vec![self.or()?];
        while matches!(
            self.tokens.get(self.index),
            Some(Token::Word(_) | Token::Open)
        ) {
            clauses.push(self.or()?);
        }
        // Positive terms with the same key use implicit OR; other terms use AND.
        let mut grouped: BTreeMap<String, Vec<Query>> = BTreeMap::new();
        let mut result = Vec::new();
        for clause in clauses {
            match &clause {
                Query::Term(t) if !t.negated && !t.key.is_empty() => {
                    grouped.entry(t.key.clone()).or_default().push(clause)
                }
                _ => result.push(clause),
            }
        }
        result.extend(grouped.into_values().map(|mut q| {
            if q.len() == 1 {
                q.remove(0)
            } else {
                Query::Or(q)
            }
        }));
        Ok(if result.len() == 1 {
            result.remove(0)
        } else {
            Query::And(result)
        })
    }
    fn or(&mut self) -> Result<Query, String> {
        let mut q = vec![self.and()?];
        while matches!(self.tokens.get(self.index), Some(Token::Or)) {
            self.index += 1;
            q.push(self.and()?);
        }
        Ok(if q.len() == 1 {
            q.remove(0)
        } else {
            Query::Or(q)
        })
    }
    fn and(&mut self) -> Result<Query, String> {
        let mut q = vec![self.atom()?];
        while matches!(self.tokens.get(self.index), Some(Token::And)) {
            self.index += 1;
            q.push(self.atom()?);
        }
        Ok(if q.len() == 1 {
            q.remove(0)
        } else {
            Query::And(q)
        })
    }
    fn atom(&mut self) -> Result<Query, String> {
        let token = self
            .tokens
            .get(self.index)
            .cloned()
            .ok_or("Expected a filter after the operator")?;
        self.index += 1;
        match token {
            Token::Open => {
                let q = self.implicit()?;
                if !matches!(self.tokens.get(self.index), Some(Token::Close)) {
                    return Err("Missing closing parenthesis".into());
                }
                self.index += 1;
                Ok(q)
            }
            Token::Word(mut word) => {
                if word.ends_with(':') {
                    if let Some(Token::Word(value)) = self.tokens.get(self.index) {
                        word.push_str(value);
                        self.index += 1;
                    }
                }
                let (mut key, value) = word
                    .split_once(':')
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .unwrap_or((String::new(), word.clone()));
                let negated = key.starts_with('-');
                if negated {
                    key.remove(0);
                }
                let regex = key.ends_with('~');
                if regex {
                    key.pop();
                }
                if ![
                    "", "tag", "package", "process", "message", "line", "level",
                    "age", "is", "name",
                ]
                .contains(&key.as_str())
                {
                    return Err(format!("Unknown filter key: {key}"));
                }
                if value.is_empty() {
                    return Err(format!("Missing value for {key}"));
                }
                if key == "level" && priority(&value).is_none() {
                    return Err(
                        "Level must be VERBOSE, DEBUG, INFO, WARN, ERROR or ASSERT"
                            .into(),
                    );
                }
                if key == "age" && age(&value).is_none() {
                    return Err(
                        "Age must be a nonnegative number followed by s, m, h or d"
                            .into(),
                    );
                }
                if key == "is" && !matches!(value.as_str(), "crash" | "stacktrace") {
                    return Err("Use is:crash or is:stacktrace".into());
                }
                let regex = if regex {
                    Some(
                        RegexBuilder::new(&value)
                            .case_insensitive(!self.case)
                            .size_limit(1024 * 1024)
                            .build()
                            .map_err(|e| e.to_string())?,
                    )
                } else {
                    None
                };
                Ok(Query::Term(Term {
                    key,
                    value,
                    negated,
                    regex,
                    case: self.case,
                }))
            }
            _ => Err("Expected a filter or parenthesized expression".into()),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn entry() -> Entry {
        Entry {
            tag: "ActivityManager".into(),
            package: "com.example.app".into(),
            process: "com.example.app:worker".into(),
            level: 'W',
            message: "Hello World".into(),
            raw: "Hello World".into(),
            time: 100,
            ..Default::default()
        }
    }
    #[test]
    fn boolean_grouping_and_implicit_same_key_or() {
        let e = entry();
        for s in [
            "tag:no tag:Activity package:mine",
            "(tag:no | level:WARN) & package:mine",
            "message:\"Hello World\" -tag:spam",
            "age:5m level:INFO",
            "tag~:Activity.*",
        ] {
            assert!(
                Query::parse(s, false).unwrap().matches(
                    &e,
                    &[e.package.clone()],
                    200
                ),
                "{s}"
            );
        }
        assert!(
            !Query::parse("tag:Activity -tag:Manager", false)
                .unwrap()
                .matches(&e, &[], 200)
        );
    }
    #[test]
    fn bare_text_is_a_phrase_and_implicit_and_has_lower_precedence() {
        let mut e = entry();
        e.raw = "Hello other World".into();
        assert!(
            !Query::parse("Hello World", false)
                .unwrap()
                .matches(&e, &[], 200)
        );
        e.raw = "Hello World".into();
        assert!(
            Query::parse("Hello World tag:no | tag:Activity", false)
                .unwrap()
                .matches(&e, &[], 200)
        );
    }
    #[test]
    fn reports_invalid_filters() {
        for s in [
            "(tag:x",
            "level:no",
            "age:3x",
            "tag~:[",
            "tag:",
            "tag:x |",
            "foo:x",
            "\"unclosed",
        ] {
            assert!(Query::parse(s, false).is_err(), "{s}");
        }
    }
    #[test]
    fn package_mine_never_matches_unknown_packages() {
        assert!(!Query::parse("package:mine", false).unwrap().matches(
            &entry(),
            &[],
            200
        ));
    }
}
