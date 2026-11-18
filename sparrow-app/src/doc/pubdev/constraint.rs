//! Pub constraints are not Rust semver requirements: exact versions really are
//! exact, and ^0.0.x extends to the next minor release in Pub.
use semver::Version;
#[derive(Clone, Debug)]
pub(super) struct Constraint {
    min: Option<(Version, bool)>,
    max: Option<(Version, bool)>,
}
impl Constraint {
    pub(super) fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let mut result = Self {
            min: None,
            max: None,
        };
        if text == "any" || text.is_empty() {
            return Some(result);
        }
        if let Some(v) = text.strip_prefix('^') {
            let v = Version::parse(v.trim()).ok()?;
            let next = if v.major > 0 {
                Version::new(v.major.checked_add(1)?, 0, 0)
            } else {
                Version::new(0, v.minor.checked_add(1)?, 0)
            };
            result.lower(v, true);
            result.upper(next, false);
            return Some(result);
        }
        // Permit whitespace between comparison operators and versions.
        static OPERATORS: std::sync::LazyLock<regex::Regex> =
            std::sync::LazyLock::new(|| {
                regex::Regex::new(r"(>=|<=|>|<)\s+").unwrap()
            });
        let normalized = OPERATORS.replace_all(text, "$1");
        for part in normalized.split_whitespace() {
            let (op, value) = if let Some(v) = part.strip_prefix(">=") {
                (">=", v)
            } else if let Some(v) = part.strip_prefix("<=") {
                ("<=", v)
            } else if let Some(v) = part.strip_prefix('>') {
                (">", v)
            } else if let Some(v) = part.strip_prefix('<') {
                ("<", v)
            } else {
                ("=", part)
            };
            let version = Version::parse(value).ok()?;
            match op {
                ">=" => result.lower(version, true),
                ">" => result.lower(version, false),
                "<=" => result.upper(version, true),
                "<" => result.upper(version, false),
                _ => {
                    result.lower(version.clone(), true);
                    result.upper(version, true);
                }
            }
        }
        Some(result)
    }
    fn lower(&mut self, v: Version, inclusive: bool) {
        if self
            .min
            .as_ref()
            .is_none_or(|(old, inc)| v > *old || (v == *old && !inclusive && *inc))
        {
            self.min = Some((v, inclusive));
        }
    }
    fn upper(&mut self, mut v: Version, inclusive: bool) {
        // Pub's exclusive stable upper bound excludes prereleases of that bound.
        if !inclusive && v.pre.is_empty() {
            v.pre = semver::Prerelease::new("0").ok().unwrap();
        }
        if self
            .max
            .as_ref()
            .is_none_or(|(old, inc)| v < *old || (v == *old && !inclusive && *inc))
        {
            self.max = Some((v, inclusive));
        }
    }
    pub(super) fn dart_sdk(mut self) -> Self {
        // Pub widens null-safe packages' legacy <3.0.0 SDK bounds to <4.0.0.
        if self.null_safe()
            && self.max.as_ref().is_some_and(|(v, included)| {
                !included
                    && v.major == 3
                    && v.minor == 0
                    && v.patch == 0
                    && v.pre.as_str() == "0"
            })
        {
            self.max = None;
            self.upper(Version::new(4, 0, 0), false);
        }
        self
    }
    pub(super) fn flutter_sdk(mut self) -> Self {
        // Flutter SDK constraints only enforce their lower bound in Pub.
        self.max = None;
        self
    }
    pub(super) fn null_safe(&self) -> bool {
        self.min
            .as_ref()
            .is_some_and(|(v, _)| v.major > 2 || (v.major == 2 && v.minor >= 12))
    }
    pub(super) fn contains(&self, v: &Version) -> bool {
        self.min
            .as_ref()
            .is_none_or(|(m, i)| v > m || (*i && v == m))
            && self
                .max
                .as_ref()
                .is_none_or(|(m, i)| v < m || (*i && v == m))
    }
    pub(super) fn overlaps(&self, other: &Self) -> bool {
        let mut both = self.clone();
        if let Some((v, i)) = &other.min {
            both.lower(v.clone(), *i);
        }
        if let Some((v, i)) = &other.max {
            both.upper(v.clone(), *i);
        }
        match (&both.min, &both.max) {
            (Some((a, ai)), Some((b, bi))) => a < b || (a == b && *ai && *bi),
            _ => true,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pub_semantics_and_intersections() {
        let caret = Constraint::parse("^0.0.3").unwrap();
        assert!(caret.contains(&Version::parse("0.0.9").unwrap()));
        assert!(!caret.contains(&Version::parse("0.1.0").unwrap()));
        let exact = Constraint::parse("1.2.3").unwrap();
        assert!(!exact.contains(&Version::parse("1.2.4").unwrap()));
        let range = Constraint::parse(">= 3.0.0 < 4.0.0").unwrap();
        assert!(!range.overlaps(&Constraint::parse("^2.0.0").unwrap()));
        assert!(range.overlaps(&Constraint::parse("^3.3.0").unwrap()));
        assert!(
            !Constraint::parse("^1.0.0")
                .unwrap()
                .contains(&Version::parse("2.0.0-dev.1").unwrap())
        );
        let sdk = Constraint::parse(">=2.12.0 <3.0.0").unwrap().dart_sdk();
        assert!(sdk.contains(&Version::new(3, 5, 0)));
        assert!(!Constraint::parse(">=2.9.0 <3.0.0").unwrap().null_safe());
        assert!(
            Constraint::parse(">=3.0.0 <4.0.0")
                .unwrap()
                .flutter_sdk()
                .contains(&Version::new(4, 1, 0))
        );
        assert!(Constraint::parse("~1.0").is_none());
        assert!(Constraint::parse("not a version").is_none());
    }
}
