//! Project identity is a shared backdrop, not separately tinted chrome widgets.
use std::path::Path;
use floem::Renderer;
use crate::app::Color;

const PALETTE: [[u8; 3]; 9] = [
    [224, 130, 61],
    [226, 114, 55],
    [45, 188, 173],
    [56, 149, 225],
    [123, 117, 232],
    [120, 84, 173],
    [143, 69, 147],
    [200, 64, 185],
    [215, 83, 112],
];
fn hex(text: &str) -> Option<[u8; 3]> {
    let text = text.trim().trim_start_matches('#');
    if text.len() != 6 || !text.is_ascii() {
        return None;
    }
    Some([
        u8::from_str_radix(&text[0..2], 16).ok()?,
        u8::from_str_radix(&text[2..4], 16).ok()?,
        u8::from_str_radix(&text[4..6], 16).ok()?,
    ])
}
pub(crate) fn idea_color(source: &str) -> Option<[u8; 3]> {
    let xml = roxmltree::Document::parse(source).ok()?;
    let component = xml
        .descendants()
        .find(|n| n.attribute("name") == Some("ProjectColorInfo"));
    if let Some(component) = component {
        // Current IDEs serialize this state as JSON (often CDATA). Older
        // versions use XML options; read both without modifying .idea files.
        let text = component
            .children()
            .filter_map(|n| n.text())
            .collect::<String>();
        let json = serde_json::from_str::<serde_json::Value>(text.trim()).ok();
        let option = |key| {
            component
                .children()
                .find(|n| n.attribute("name") == Some(key))
                .and_then(|n| n.attribute("value"))
        };
        if let Some(color) = json
            .as_ref()
            .and_then(|v| v["customColor"].as_str())
            .or_else(|| option("customColor"))
            .and_then(hex)
        {
            return Some(color);
        }
        let index = json
            .as_ref()
            .and_then(|v| v["associatedIndex"].as_u64())
            .or_else(|| option("associatedIndex").and_then(|v| v.parse().ok()));
        if let Some(index) = index.and_then(|i| PALETTE.get(i as usize)) {
            return Some(*index);
        }
    }
    xml.descendants()
        .find(|n| n.attribute("name") == Some("PROJECT_TOOLBAR_COLOR"))
        .and_then(|n| n.attribute("value"))
        .and_then(hex)
}
pub(crate) fn color(root: &Path) -> [u8; 3] {
    let workspace = root.join(".idea/workspace.xml");
    if std::fs::metadata(&workspace).is_ok_and(|m| m.len() <= 2 * 1024 * 1024) {
        if let Ok(text) = std::fs::read_to_string(workspace) {
            if let Some(color) = idea_color(&text) {
                return color;
            }
        }
    }
    let hash = root
        .to_string_lossy()
        .bytes()
        .fold(2166136261u32, |hash, c| {
            (hash ^ u32::from(c)).wrapping_mul(16777619)
        });
    PALETTE[hash as usize % PALETTE.len()]
}
/// Paint with two-stop linear brushes, supported by both Floem renderers.
/// Do not use a large SVG here: GPU SVGs share the finite icon texture atlas.
pub(crate) fn paint_glow(
    cx: &mut floem::context::PaintCx,
    base: floem::peniko::Color,
    accent: Option<[u8; 3]>,
) {
    use floem::peniko::{Color, Gradient, kurbo::Rect};
    let accent = accent.unwrap_or([65, 77, 104]);
    let base_rgb = base.to_rgba8();
    // Narrow columns preserve the horizontal fade. Every brush starts at
    // (0, 0): Vger's two-stop conversion does not preserve nonzero starts.
    for x in (0..700).step_by(2) {
        let amount = glow_strength(x as f64 + 1.0, 0.0);
        let blend = |b: u8, a: u8| {
            (f64::from(b) + (f64::from(a) - f64::from(b)) * amount).round() as u8
        };
        let peak = Color::from_rgb8(
            blend(base_rgb.r, accent[0]),
            blend(base_rgb.g, accent[1]),
            blend(base_rgb.b, accent[2]),
        );
        let x = x as f64;
        cx.fill(&Rect::new(x, 0.0, x + 2.0, 300.0),
            &Gradient::new_linear((0.0, 0.0), (0.0, 300.0))
                .with_stops([peak, base]), 0.0);
    }
}

/// Color of the shared backdrop at a window-relative point. Corner masks
/// must reveal this color rather than cutting flat frame-colored squares
/// into the project glow.
pub(crate) fn backdrop_color(
    base: floem::peniko::Color,
    accent: Option<[u8; 3]>,
    point: floem::kurbo::Point,
) -> floem::peniko::Color {
    let base_rgb = base.to_rgba8();
    let accent = accent.unwrap_or([65, 77, 104]);
    let amount = glow_strength(point.x, point.y);
    let blend = |b: u8, a: u8| {
        (f64::from(b) + (f64::from(a) - f64::from(b)) * amount).round() as u8
    };
    Color::from_rgb8(
        blend(base_rgb.r, accent[0]),
        blend(base_rgb.g, accent[1]),
        blend(base_rgb.b, accent[2]),
    )
}

fn glow_strength(x: f64, y: f64) -> f64 {
    let horizontal = if x <= 90.0 { x / 90.0 } else { (700.0 - x) / 610.0 };
    0.3 * horizontal.clamp(0.0, 1.0) * (1.0 - y / 300.0).clamp(0.0, 1.0)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn directional_glow_has_bounded_falloff() {
        assert_eq!(glow_strength(0.0, 0.0), 0.0);
        assert_eq!(glow_strength(90.0, 0.0), 0.3);
        assert_eq!(glow_strength(90.0, 150.0), 0.15);
        assert_eq!(glow_strength(700.0, 0.0), 0.0);
        assert_eq!(glow_strength(90.0, 300.0), 0.0);
        assert_eq!(glow_strength(900.0, 500.0), 0.0);
    }
    #[test]
    fn imports_idea_custom_colors_and_palette_with_safe_fallbacks() {
        assert_eq!(
            idea_color(
                r##"<project><component name="ProjectColorInfo"><![CDATA[{"customColor":"#b46a38","associatedIndex":3}]]></component></project>"##
            ),
            Some([180, 106, 56])
        );
        assert_eq!(
            idea_color(
                r#"<project><component name="ProjectColorInfo"><option name="associatedIndex" value="1"/></component></project>"#
            ),
            Some(PALETTE[1])
        );
        assert_eq!(
            idea_color(
                r#"<project><component name="ProjectColorInfo">{"customColor":"oops","associatedIndex":99}</component></project>"#
            ),
            None
        );
        assert_eq!(idea_color("broken XML"), None);
        assert!(hex("ééé").is_none());
    }
}
