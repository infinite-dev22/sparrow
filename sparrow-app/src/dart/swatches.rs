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
    Regex::new(
        r"\bColor\.fromARGB\s*\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*\)",
    )
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

static ICONS_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\bIcons\.([a-zA-Z0-9_]+)\b").expect("icons regex"));

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

/// Resolve Flutter MaterialIcons names using the definitions matching the bundled font.
pub fn resolve_material_icon_codepoint(name: &str) -> Option<u32> {
    static CODES: Lazy<std::collections::HashMap<String, u32>> = Lazy::new(|| {
        serde_json::from_str(include_str!(
            "../../../extra/fonts/MaterialIcons/codepoints.json"
        ))
        .expect("bundled Flutter icon definitions")
    });
    CODES.get(name).copied()
}

pub fn resolve_cupertino_icon_codepoint(name: &str) -> Option<u32> {
    static CODES: Lazy<std::collections::HashMap<String, u32>> = Lazy::new(|| {
        serde_json::from_str(include_str!(
            "../../../extra/fonts/CupertinoIcons/codepoints.json"
        ))
        .expect("bundled Cupertino glyph definitions")
    });
    CODES.get(name).copied()
}

/// Extract all inline color and icon swatches from source text.
pub fn extract_swatches(source: &str) -> (Vec<ColorSwatch>, Vec<IconSwatch>) {
    let skipped = super::regex::skip_ranges(source);
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
        if skipped.iter().any(|(start, end)| *start <= m.start() && m.start() < *end) { continue; }
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
        if skipped.iter().any(|(start, end)| *start <= m.start() && m.start() < *end) { continue; }
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
        if skipped.iter().any(|(start, end)| *start <= m.start() && m.start() < *end) { continue; }
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
        if skipped.iter().any(|(start, end)| *start <= m.start() && m.start() < *end) { continue; }
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
        if skipped.iter().any(|(start, end)| *start <= m.start() && m.start() < *end) { continue; }
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
        if skipped.iter().any(|(start, end)| *start <= m.start() && m.start() < *end) { continue; }
        let icon_name = cap[1].to_string();
        let (line, col) = offset_to_line_col(m.start());

        icons.push(IconSwatch {
            line,
            col,
            codepoint: resolve_cupertino_icon_codepoint(&icon_name),
            icon_name,
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

#[cfg(test)]
mod bundled_glyph_tests {
    use super::*;
    #[test]
    fn flutter_glyphs_use_the_bundled_font_instead_of_material_symbols() {
        assert_eq!(resolve_material_icon_codepoint("add"), Some(0xe047));
        assert!(resolve_material_icon_codepoint("add_circle_outline").is_some());
        assert!(
            resolve_material_icon_codepoint("not_a_real_flutter_icon").is_none()
        );
    }
}

#[cfg(test)]
mod code_only_tests {
    #[test]
    fn comments_and_strings_do_not_create_decorations() {
        let source = "// Colors.amber Icons.add\n/* Colors.red */\nfinal text = 'Colors.blue Icons.home';\nfinal icon = Icon(Icons.add, color: Colors.amber);";
        let (colors, icons) = super::extract_swatches(source);
        assert_eq!(colors.len(), 1);
        assert_eq!(icons.len(), 1);
        assert_eq!(colors[0].line, 3);
        assert_eq!(icons[0].line, 3);
    }
}
