//! Flutter & Dart color swatches and icon glyph previews.
//!
//! Parses inline `Color(0x...)`, `Colors.*`, and `Icons.*` occurrences in Dart
//! source code, enabling visual preview swatches in the editor gutter.

use once_cell::sync::Lazy;
use regex::Regex;

/// An inline color occurrence in source code.
#[derive(Debug, Clone, PartialEq)]
pub struct ColorSwatch {
    /// 0-indexed line number.
    pub line: usize,
    /// 0-indexed column of the occurrence.
    pub col: usize,
    /// RGBA bytes: [R, G, B, A].
    pub rgba: [u8; 4],
    /// Hex representation (e.g. `"#42A5F5"`).
    pub hex: String,
    /// Exact text in source (e.g. `"Color(0xFF42A5F5)"` or `"Colors.blue"`).
    pub source_text: String,
}

/// An inline icon occurrence in source code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconSwatch {
    /// 0-indexed line number.
    pub line: usize,
    /// 0-indexed column of the occurrence.
    pub col: usize,
    /// Name of the icon (e.g. `"home"`, `"settings"`, `"star"`).
    pub icon_name: String,
    /// Unicode codepoint for the icon glyph, if known.
    pub codepoint: Option<u32>,
    /// Whether this is a Cupertino (iOS) icon.
    pub is_cupertino: bool,
}

// ── Color Regexes ─────────────────────────────────────────────────────

static HEX_COLOR_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?:\bColor\s*\(\s*0x([0-9a-fA-F]{8})\s*\)|\bColor\s*\(\s*0x([0-9a-fA-F]{6})\s*\))")
        .expect("hex color regex")
});

static FROM_ARGB_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\bColor\.fromARGB\s*\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*\)")
        .expect("fromARGB regex")
});

static FROM_RGBO_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\bColor\.fromRGBO\s*\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*,\s*([0-9.]+)\s*\)")
        .expect("fromRGBO regex")
});

static MATERIAL_COLORS_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\bColors\.([a-zA-Z]+)(?:\s*\[\s*(\d+)\s*\]|\.shade(\d+))?")
        .expect("material colors regex")
});

static ICONS_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\bIcons\.([a-zA-Z0-9_]+)\b")
        .expect("icons regex")
});

static CUPERTINO_ICONS_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\bCupertinoIcons\.([a-zA-Z0-9_]+)\b")
        .expect("cupertino icons regex")
});

// ── Material Palette Lookup ───────────────────────────────────────────

/// Resolve a Material color name (e.g. `"blue"`, `"red"`, `"deepOrange"`) to RGBA [r, g, b, a].
pub fn resolve_material_color(name: &str, shade: Option<u32>) -> Option<[u8; 4]> {
    let lower = name.to_ascii_lowercase();
    let base_rgb = match lower.as_str() {
        "transparent" => return Some([0, 0, 0, 0]),
        "black" => return Some([0, 0, 0, 255]),
        "white" => return Some([255, 255, 255, 255]),
        "red" => [244, 67, 54],
        "pink" => [233, 30, 99],
        "purple" => [156, 39, 176],
        "deeppurple" => [103, 58, 183],
        "indigo" => [63, 81, 181],
        "blue" => [33, 150, 243],
        "lightblue" => [3, 169, 244],
        "cyan" => [0, 188, 212],
        "teal" => [0, 150, 136],
        "green" => [76, 175, 80],
        "lightgreen" => [139, 195, 74],
        "lime" => [205, 220, 57],
        "yellow" => [255, 235, 59],
        "amber" => [255, 193, 7],
        "orange" => [255, 152, 0],
        "deeporange" => [255, 87, 34],
        "brown" => [121, 85, 72],
        "grey" | "gray" => [158, 158, 158],
        "bluegrey" | "bluegray" => [96, 125, 139],
        _ => return None,
    };

    // If a shade is requested (e.g. 100, 200, 500, 700, 900), adjust luminance
    let [r, g, b] = if let Some(s) = shade {
        adjust_shade(base_rgb, s)
    } else {
        base_rgb
    };

    Some([r, g, b, 255])
}

fn adjust_shade(rgb: [u8; 3], shade: u32) -> [u8; 3] {
    let factor = match shade {
        50 => 0.15,
        100 => 0.30,
        200 => 0.50,
        300 => 0.70,
        400 => 0.85,
        500 => 1.0,
        600 => 1.15,
        700 => 1.30,
        800 => 1.45,
        900 => 1.60,
        _ => 1.0,
    };
    if factor <= 1.0 {
        // Blend towards white
        let inv = 1.0 - factor;
        [
            ((rgb[0] as f32 * factor) + (255.0 * inv)) as u8,
            ((rgb[1] as f32 * factor) + (255.0 * inv)) as u8,
            ((rgb[2] as f32 * factor) + (255.0 * inv)) as u8,
        ]
    } else {
        // Blend towards black
        let mult = (2.0 - factor).max(0.1);
        [
            (rgb[0] as f32 * mult) as u8,
            (rgb[1] as f32 * mult) as u8,
            (rgb[2] as f32 * mult) as u8,
        ]
    }
}

// ── Icon Codepoints ───────────────────────────────────────────────────

/// Resolve a Material Icon name to its standard Material Symbols codepoint.
pub fn resolve_material_icon_codepoint(name: &str) -> Option<u32> {
    match name {
        "home" => Some(0xe88a),
        "settings" => Some(0xe8b8),
        "favorite" | "favorite_border" => Some(0xe87d),
        "star" | "star_border" => Some(0xe838),
        "search" => Some(0xe8b6),
        "add" => Some(0xe145),
        "remove" => Some(0xe15b),
        "delete" => Some(0xe872),
        "edit" => Some(0xe3c9),
        "check" => Some(0xe5ca),
        "close" => Some(0xe5cd),
        "arrow_back" => Some(0xe5c4),
        "arrow_forward" => Some(0xe5c8),
        "refresh" => Some(0xe5d5),
        "menu" => Some(0xe5d2),
        "more_vert" => Some(0xe5d4),
        "share" => Some(0xe80d),
        "person" => Some(0xe7fd),
        "email" | "mail" => Some(0xe0be),
        "phone" => Some(0xe0cd),
        "camera" | "camera_alt" => Some(0xe3b0),
        "image" => Some(0xe3f4),
        "play_arrow" => Some(0xe037),
        "pause" => Some(0xe034),
        "stop" => Some(0xe047),
        "lock" => Some(0xe897),
        "visibility" => Some(0xe8f4),
        "notifications" => Some(0xe7f4),
        "folder" => Some(0xe2c7),
        "code" => Some(0xe86f),
        "build" => Some(0xe869),
        "info" => Some(0xe88e),
        "warning" => Some(0xe002),
        "error" => Some(0xe000),
        _ => Some(0xe88a), // fallback to general icon glyph
    }
}

// ── Extraction Pipeline ───────────────────────────────────────────────

/// Extract all inline color and icon swatches from source text.
pub fn extract_swatches(source: &str) -> (Vec<ColorSwatch>, Vec<IconSwatch>) {
    let mut colors = Vec::new();
    let mut icons = Vec::new();

    let line_offsets: Vec<usize> = std::iter::once(0)
        .chain(source.match_indices('\n').map(|(i, _)| i + 1))
        .collect();

    let offset_to_line_col = |offset: usize| -> (usize, usize) {
        let line = match line_offsets.binary_search(&offset) {
            Ok(idx) => idx,
            Err(idx) => idx.saturating_sub(1),
        };
        let line_start = line_offsets.get(line).copied().unwrap_or(0);
        let col = offset.saturating_sub(line_start);
        (line, col)
    };

    // 1. Color(0xAARRGGBB) or Color(0xRRGGBB)
    for cap in HEX_COLOR_RE.captures_iter(source) {
        let m = cap.get(0).unwrap();
        let (line, col) = offset_to_line_col(m.start());

        let rgba = if let Some(hex8) = cap.get(1) {
            if let Ok(val) = u32::from_str_radix(hex8.as_str(), 16) {
                let a = ((val >> 24) & 0xFF) as u8;
                let r = ((val >> 16) & 0xFF) as u8;
                let g = ((val >> 8) & 0xFF) as u8;
                let b = (val & 0xFF) as u8;
                [r, g, b, a]
            } else {
                continue;
            }
        } else if let Some(hex6) = cap.get(2) {
            if let Ok(val) = u32::from_str_radix(hex6.as_str(), 16) {
                let r = ((val >> 16) & 0xFF) as u8;
                let g = ((val >> 8) & 0xFF) as u8;
                let b = (val & 0xFF) as u8;
                [r, g, b, 255]
            } else {
                continue;
            }
        } else {
            continue;
        };

        let hex = format!("#{:02X}{:02X}{:02X}", rgba[0], rgba[1], rgba[2]);
        colors.push(ColorSwatch {
            line,
            col,
            rgba,
            hex,
            source_text: m.as_str().to_string(),
        });
    }

    // 2. Color.fromARGB(a, r, g, b)
    for cap in FROM_ARGB_RE.captures_iter(source) {
        let m = cap.get(0).unwrap();
        let (line, col) = offset_to_line_col(m.start());
        let a = cap[1].parse::<u8>().unwrap_or(255);
        let r = cap[2].parse::<u8>().unwrap_or(0);
        let g = cap[3].parse::<u8>().unwrap_or(0);
        let b = cap[4].parse::<u8>().unwrap_or(0);
        let rgba = [r, g, b, a];
        let hex = format!("#{:02X}{:02X}{:02X}", r, g, b);

        colors.push(ColorSwatch {
            line,
            col,
            rgba,
            hex,
            source_text: m.as_str().to_string(),
        });
    }

    // 3. Color.fromRGBO(r, g, b, opacity)
    for cap in FROM_RGBO_RE.captures_iter(source) {
        let m = cap.get(0).unwrap();
        let (line, col) = offset_to_line_col(m.start());
        let r = cap[1].parse::<u8>().unwrap_or(0);
        let g = cap[2].parse::<u8>().unwrap_or(0);
        let b = cap[3].parse::<u8>().unwrap_or(0);
        let o = cap[4].parse::<f32>().unwrap_or(1.0).clamp(0.0, 1.0);
        let a = (o * 255.0) as u8;
        let rgba = [r, g, b, a];
        let hex = format!("#{:02X}{:02X}{:02X}", r, g, b);

        colors.push(ColorSwatch {
            line,
            col,
            rgba,
            hex,
            source_text: m.as_str().to_string(),
        });
    }

    // 4. Colors.blue / Colors.red.shade500
    for cap in MATERIAL_COLORS_RE.captures_iter(source) {
        let m = cap.get(0).unwrap();
        let name = &cap[1];
        let shade = cap
            .get(2)
            .or_else(|| cap.get(3))
            .and_then(|s| s.as_str().parse::<u32>().ok());

        if let Some(rgba) = resolve_material_color(name, shade) {
            let (line, col) = offset_to_line_col(m.start());
            let hex = format!("#{:02X}{:02X}{:02X}", rgba[0], rgba[1], rgba[2]);
            colors.push(ColorSwatch {
                line,
                col,
                rgba,
                hex,
                source_text: m.as_str().to_string(),
            });
        }
    }

    // 5. Icons.home
    for cap in ICONS_RE.captures_iter(source) {
        let m = cap.get(0).unwrap();
        let icon_name = cap[1].to_string();
        let (line, col) = offset_to_line_col(m.start());
        let codepoint = resolve_material_icon_codepoint(&icon_name);

        icons.push(IconSwatch {
            line,
            col,
            icon_name,
            codepoint,
            is_cupertino: false,
        });
    }

    // 6. CupertinoIcons.heart
    for cap in CUPERTINO_ICONS_RE.captures_iter(source) {
        let m = cap.get(0).unwrap();
        let icon_name = cap[1].to_string();
        let (line, col) = offset_to_line_col(m.start());

        icons.push(IconSwatch {
            line,
            col,
            icon_name,
            codepoint: None,
            is_cupertino: true,
        });
    }

    colors.sort_by_key(|c| (c.line, c.col));
    icons.sort_by_key(|i| (i.line, i.col));
    (colors, icons)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_color() {
        let src = "final c = Color(0xFF42A5F5);";
        let (colors, _) = extract_swatches(src);
        assert_eq!(colors.len(), 1);
        assert_eq!(colors[0].rgba, [0x42, 0xA5, 0xF5, 0xFF]);
        assert_eq!(colors[0].hex, "#42A5F5");
    }

    #[test]
    fn parses_from_argb_and_rgbo() {
        let src = "final c1 = Color.fromARGB(255, 100, 150, 200);\nfinal c2 = Color.fromRGBO(50, 100, 150, 0.5);";
        let (colors, _) = extract_swatches(src);
        assert_eq!(colors.len(), 2);
        assert_eq!(colors[0].rgba, [100, 150, 200, 255]);
        assert_eq!(colors[1].rgba[0], 50);
        assert_eq!(colors[1].rgba[1], 100);
        assert_eq!(colors[1].rgba[2], 150);
    }

    #[test]
    fn parses_material_colors() {
        let src = "final a = Colors.blue;\nfinal b = Colors.red[700];";
        let (colors, _) = extract_swatches(src);
        assert_eq!(colors.len(), 2);
        assert_eq!(colors[0].line, 0);
        assert_eq!(colors[1].line, 1);
    }

    #[test]
    fn parses_icons() {
        let src = "final i1 = Icons.home;\nfinal i2 = CupertinoIcons.heart;";
        let (_, icons) = extract_swatches(src);
        assert_eq!(icons.len(), 2);
        assert_eq!(icons[0].icon_name, "home");
        assert!(!icons[0].is_cupertino);
        assert_eq!(icons[1].icon_name, "heart");
        assert!(icons[1].is_cupertino);
    }
}
