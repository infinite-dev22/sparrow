//! Project identity is a shared backdrop, not separately tinted chrome widgets.
use floem::peniko::{Color, Gradient};
use std::path::Path;
const PALETTE: [[u8; 3]; 9] = [
    [206, 68, 60],
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
fn mix(base: Color, accent: [u8; 3], amount: f32) -> Color {
    let b = base.to_rgba8();
    let blend = |b: u8, a: u8| {
        ((b as f32) * (1.0 - amount) + a as f32 * amount).round() as u8
    };
    Color::from_rgb8(
        blend(b.r, accent[0]),
        blend(b.g, accent[1]),
        blend(b.b, accent[2]),
    )
}
pub(crate) fn backdrop(base: Color, accent: Option<[u8; 3]>) -> Gradient {
    let accent = accent.unwrap_or([65, 77, 104]);
    Gradient::new_radial((130.0, 0.0), 620.0).with_stops([
        (0.0, mix(base, accent, 0.26)),
        (0.45, mix(base, accent, 0.12)),
        (1.0, base),
    ])
}
#[cfg(test)]
mod tests {
    use super::*;
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
