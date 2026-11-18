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
pub(crate) fn workspace_color(source: &str) -> Option<[u8; 3]> {
    let json: serde_json::Value = serde_json::from_str(source).ok()?;
    let appearance = json.get("appearance")?;
    appearance.get("color").and_then(|v| v.as_str()).and_then(hex)
        .or_else(|| appearance.get("palette_index").and_then(|v| v.as_u64())
            .and_then(|i| PALETTE.get(i as usize).copied()))
}
pub(crate) fn color(root: &Path) -> [u8; 3] {
    if let Ok(text) = std::fs::read_to_string(root.join(".sparrow/settings.toml")) {
        if let Ok(settings) = toml::from_str::<toml::Value>(&text) {
            if let Some(color) = settings.get("project").and_then(|v| v.get("color"))
                .and_then(|v| v.as_str()).and_then(hex) { return color; }
        }
    }
    let workspace = root.join(".sparrow/workspace.json");
    if std::fs::metadata(&workspace).is_ok_and(|m| m.len() <= 2 * 1024 * 1024) {
        if let Ok(text) = std::fs::read_to_string(workspace) {
            if let Some(color) = workspace_color(&text) {
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
    fn reads_native_workspace_appearance() {
        assert_eq!(workspace_color(r##"{"appearance":{"color":"#b46a38"}}"##), Some([180,106,56]));
        assert_eq!(workspace_color(r#"{"appearance":{"palette_index":0}}"#), Some(PALETTE[0]));
        assert_eq!(workspace_color(r#"{"appearance":{"palette_index":99}}"#), None);
        assert_eq!(workspace_color("broken JSON"), None);
    }
}
