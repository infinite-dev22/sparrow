use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct Formatting {
    pub timestamp: bool,
    pub date: bool,
    pub milliseconds: bool,
    pub pid: bool,
    pub tid: bool,
    pub tag: bool,
    pub package: bool,
    pub process: bool,
    pub level: bool,
    pub tag_width: u16,
    pub package_width: u16,
    pub font_size: u16,
}
impl Default for Formatting {
    fn default() -> Self {
        Self {
            timestamp: true,
            date: true,
            milliseconds: true,
            pid: true,
            tid: true,
            tag: true,
            package: true,
            process: false,
            level: true,
            tag_width: 192,
            package_width: 288,
            font_size: 13,
        }
    }
}
impl Formatting {
    pub fn compact() -> Self {
        Self {
            timestamp: false,
            pid: false,
            tid: false,
            package: false,
            ..Self::default()
        }
    }
    pub fn normalized(mut self) -> Self {
        self.font_size = self.font_size.clamp(10, 24);
        self.tag_width = self.tag_width.clamp(60, 400);
        self.package_width = self.package_width.clamp(80, 600);
        self
    }
    pub fn height(&self) -> f64 {
        (self.font_size as f64 * 1.6).ceil().max(20.0)
    }
    pub fn metadata_width(&self) -> f64 {
        (if self.timestamp {
            if self.date { 200 } else { 105 }
        } else {
            0
        }) as f64
            + (if self.pid || self.tid { 88 } else { 0 }) as f64
            + (if self.tag { self.tag_width } else { 0 }) as f64
            + (if self.package { self.package_width } else { 0 }) as f64
            + (if self.process { self.package_width } else { 0 }) as f64
            + (if self.level { 24 } else { 0 }) as f64
    }
    pub fn time_text(&self, value: &str) -> String {
        let value = if self.date {
            value
        } else {
            value.split_once(' ').map(|(_, v)| v).unwrap_or(value)
        };
        if self.milliseconds {
            value.into()
        } else {
            value.split('.').next().unwrap_or(value).into()
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn presets_and_legacy_defaults() {
        let f: Formatting = toml::from_str("font_size = 99\ntag_width = 1").unwrap();
        let f = f.normalized();
        assert_eq!(f.font_size, 24);
        assert_eq!(f.tag_width, 60);
        assert!(f.timestamp);
        assert_eq!(Formatting::compact().metadata_width(), 216.0);
        assert_eq!(
            Formatting::default().time_text("2026-11-20 12:00:00.123"),
            "2026-11-20 12:00:00.123"
        );
    }
}
