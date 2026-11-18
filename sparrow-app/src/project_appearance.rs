//! Project identity is a shared backdrop, not separately tinted chrome widgets.
use std::path::Path;
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
/// Rasterized by Floem's SVG renderer on both GPU and software backends.
/// Direct gradient background brushes are unsupported by Vger, so the frame must
/// keep a solid background and composite this transparent underlay over it.
pub(crate) fn glow_svg(accent: Option<[u8; 3]>) -> String {
    let [r, g, b] = accent.unwrap_or([65, 77, 104]);
    format!(r##"<svg xmlns="http://www.w3.org/2000/svg" width="700" height="300" viewBox="0 0 700 300">
<defs>
<linearGradient id="glow" gradientUnits="userSpaceOnUse" x1="0" x2="700">
<stop offset="0" stop-color="rgb({r},{g},{b})" stop-opacity="0"/>
<stop offset="0.12857143" stop-color="rgb({r},{g},{b})" stop-opacity="0.3"/>
<stop offset="1" stop-color="rgb({r},{g},{b})" stop-opacity="0"/>
</linearGradient>
<linearGradient id="fade" gradientUnits="userSpaceOnUse" x1="0" y1="0" x2="0" y2="300">
<stop offset="0" stop-color="white"/><stop offset="1" stop-color="black"/>
</linearGradient>
<mask id="vertical" maskUnits="userSpaceOnUse" x="0" y="0" width="700" height="300"><rect width="700" height="300" fill="url(#fade)"/></mask>
</defs><rect width="700" height="300" fill="url(#glow)" mask="url(#vertical)"/></svg>"##)
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
