//! Real Flutter assets and Android wrapper resources, with module and variant grouping.
use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
};
mod view;

pub use view::panel;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Asset,
    Drawable,
    Color,
    Layout,
    MipMap,
    String,
    Font,
}
impl Kind {
    fn title(self) -> &'static str {
        match self {
            Self::Asset => "Assets",
            Self::Drawable => "Drawable",
            Self::Color => "Color",
            Self::Layout => "Layout",
            Self::MipMap => "Mip Map",
            Self::String => "String",
            Self::Font => "Fonts",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Variant {
    pub path: PathBuf,
    pub qualifier: String,
    pub value: String,
    pub line: u32,
    pub stamp: Option<(std::time::SystemTime, u64)>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Resource {
    pub module: String,
    pub kind: Kind,
    pub name: String,
    pub variants: Vec<Variant>,
}
#[derive(Clone, Default, PartialEq)]
struct Inventory {
    resources: Vec<Resource>,
    error: String,
}
fn walk(path: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 20 || out.len() > 30_000 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            if !matches!(
                entry.file_name().to_str(),
                Some(
                    "build"
                        | ".gradle"
                        | ".dart_tool"
                        | ".git"
                        | "Pods"
                        | "node_modules"
                        | "target"
                )
            ) {
                walk(&path, out, depth + 1);
            }
        } else if kind.is_file() {
            out.push(path);
        }
    }
}
fn declared(root: &Path, path: &str) -> Vec<PathBuf> {
    let path = root.join(path);
    let mut files = Vec::new();
    if path.is_file() {
        files.push(path);
    } else if let Ok(entries) = std::fs::read_dir(&path) {
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_file() {
                files.push(entry.path());
            } else if kind.is_dir()
                && entry.file_name().to_str().is_some_and(|name| {
                    name.ends_with('x')
                        && name.trim_end_matches('x').parse::<f64>().is_ok()
                })
            {
                if let Ok(variants) = std::fs::read_dir(entry.path()) {
                    for variant in variants.flatten() {
                        if variant.file_type().is_ok_and(|kind| kind.is_file()) {
                            files.push(variant.path());
                        }
                    }
                }
            }
        }
    }
    files
}
pub fn scan(root: &Path) -> Result<Vec<Resource>, String> {
    let mut grouped = BTreeMap::<(String, Kind, String), Vec<Variant>>::new();
    let pubspec = std::fs::read_to_string(root.join("pubspec.yaml"))
        .map_err(|e| e.to_string())?;
    let yaml: serde_yaml::Value =
        serde_yaml::from_str(&pubspec).map_err(|e| format!("pubspec.yaml: {e}"))?;
    let module = yaml["name"].as_str().unwrap_or("Flutter").to_string();
    for asset in yaml["flutter"]["assets"]
        .as_sequence()
        .into_iter()
        .flatten()
    {
        let value = asset.as_str().or_else(|| asset["path"].as_str());
        let Some(value) = value else { continue };
        for path in declared(root, value) {
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            let name = relative
                .split('/')
                .filter(|part| !part.ends_with("x") || part.parse::<f64>().is_err())
                .collect::<Vec<_>>()
                .join("/");
            let name = if let Some(parent) = path
                .parent()
                .and_then(Path::file_name)
                .and_then(|s| s.to_str())
                .filter(|s| {
                    s.ends_with('x')
                        && s.trim_end_matches('x').parse::<f64>().is_ok()
                }) {
                relative.replace(&format!("/{parent}/"), "/")
            } else {
                name
            };
            grouped
                .entry((module.clone(), Kind::Asset, name))
                .or_default()
                .push(Variant {
                    qualifier: "Flutter asset".into(),
                    path,
                    value: String::new(),
                    line: 0,
                    stamp: None,
                });
        }
    }
    for font in yaml["flutter"]["fonts"].as_sequence().into_iter().flatten() {
        for face in font["fonts"].as_sequence().into_iter().flatten() {
            if let Some(value) = face["asset"].as_str() {
                for path in declared(root, value) {
                    grouped
                        .entry((
                            module.clone(),
                            Kind::Font,
                            font["family"].as_str().unwrap_or(value).into(),
                        ))
                        .or_default()
                        .push(Variant {
                            path,
                            qualifier: format!(
                                "weight {}",
                                face["weight"].as_u64().unwrap_or(400)
                            ),
                            value: String::new(),
                            line: 0,
                            stamp: None,
                        });
                }
            }
        }
    }
    let mut android = Vec::new();
    walk(&root.join("android"), &mut android, 0);
    for path in android {
        let relative = path.strip_prefix(root).unwrap_or(&path);
        let components = relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect::<Vec<_>>();
        let Some(res) = components.iter().position(|c| c == "res") else {
            continue;
        };
        if components.len() != res + 3 {
            continue;
        }
        let qualifier = &components[res + 1];
        let folder = qualifier.split('-').next().unwrap_or(qualifier);
        let module = components[..res.saturating_sub(2)].join("/");
        if folder == "values" {
            let Ok(source) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(xml) = roxmltree::Document::parse(&source) else {
                continue;
            };
            for node in xml
                .root_element()
                .children()
                .filter(|node| node.is_element())
            {
                let kind = match node.tag_name().name() {
                    "color" => Kind::Color,
                    "string" | "plurals" | "string-array" => Kind::String,
                    _ => continue,
                };
                let Some(name) = node.attribute("name") else {
                    continue;
                };
                let value = node
                    .descendants()
                    .filter(|node| node.is_text())
                    .filter_map(|n| n.text())
                    .collect::<Vec<_>>()
                    .join("");
                let line = source[..node.range().start]
                    .bytes()
                    .filter(|b| *b == b'\n')
                    .count() as u32;
                grouped
                    .entry((module.clone(), kind, name.into()))
                    .or_default()
                    .push(Variant {
                        path: path.clone(),
                        qualifier: qualifier.clone(),
                        value,
                        line,
                        stamp: None,
                    });
            }
        } else {
            let kind = match folder {
                "drawable" => Kind::Drawable,
                "mipmap" => Kind::MipMap,
                "layout" => Kind::Layout,
                "font" => Kind::Font,
                "color" => Kind::Color,
                _ => continue,
            };
            let name = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            grouped
                .entry((module, kind, name))
                .or_default()
                .push(Variant {
                    path,
                    qualifier: qualifier.clone(),
                    value: String::new(),
                    line: 0,
                    stamp: None,
                });
        }
    }
    Ok(grouped
        .into_iter()
        .map(|((module, kind, name), mut variants)| {
            variants.sort_by(|a, b| {
                a.qualifier.cmp(&b.qualifier).then(a.path.cmp(&b.path))
            });
            variants.dedup_by(|a, b| a.path == b.path && a.line == b.line);
            for variant in &mut variants {
                variant.stamp = std::fs::metadata(&variant.path)
                    .ok()
                    .and_then(|meta| Some((meta.modified().ok()?, meta.len())));
            }
            Resource {
                module,
                kind,
                name,
                variants,
            }
        })
        .collect())
}
fn color(value: &str) -> Option<floem::peniko::Color> {
    let value = value.trim().strip_prefix('#')?;
    let n = u32::from_str_radix(value, 16).ok()?;
    let [r, g, b, a] = match value.len() {
        4 => [
            ((n >> 8) & 15) as u8 * 17,
            ((n >> 4) & 15) as u8 * 17,
            (n & 15) as u8 * 17,
            ((n >> 12) & 15) as u8 * 17,
        ],
        3 => [
            ((n >> 8) & 15) as u8 * 17,
            ((n >> 4) & 15) as u8 * 17,
            (n & 15) as u8 * 17,
            255,
        ],
        6 => [(n >> 16) as u8, (n >> 8) as u8, n as u8, 255],
        8 => [(n >> 16) as u8, (n >> 8) as u8, n as u8, (n >> 24) as u8],
        _ => return None,
    };
    Some(floem::peniko::Color::from_rgba8(r, g, b, a))
}
fn vector(path: &Path) -> Option<String> {
    drawable_svg(path, 0)
}
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
}
fn svg_color(value: &str) -> Option<String> {
    let value = match value {
        "@android:color/white" => "#FFFFFF",
        "@android:color/black" => "#000000",
        "@android:color/transparent" => "#00000000",
        other => other,
    };
    let color = color(value)?;
    let rgba = color.to_rgba8();
    let (r, g, b, a) = (rgba.r, rgba.g, rgba.b, rgba.a);
    Some(format!("#{r:02X}{g:02X}{b:02X}{a:02X}"))
}
fn drawable_svg(path: &Path, depth: usize) -> Option<String> {
    if depth > 8 {
        return None;
    }
    let source = std::fs::read_to_string(path).ok()?;
    let doc = roxmltree::Document::parse(&source).ok()?;
    let root = doc.root_element();
    const A: &str = "http://schemas.android.com/apk/res/android";
    fn number(node: roxmltree::Node<'_, '_>, key: &str, default: f64) -> f64 {
        node.attribute((A, key))
            .and_then(|s| {
                s.trim_end_matches("dp")
                    .trim_end_matches("px")
                    .parse::<f64>()
                    .ok()
            })
            .filter(|v| v.is_finite())
            .unwrap_or(default)
    }
    fn paint(
        node: roxmltree::Node<'_, '_>,
        key: &str,
        path: &Path,
    ) -> Option<String> {
        let value = node.attribute((A, key))?;
        if let Some(name) = value.strip_prefix("@color/") {
            if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return None;
            }
            let res = path.parent()?.parent()?;
            for entry in std::fs::read_dir(res.join("values")).ok()?.flatten() {
                if let Ok(text) = std::fs::read_to_string(entry.path()) {
                    if let Ok(doc) = roxmltree::Document::parse(&text) {
                        if let Some(value) = doc
                            .descendants()
                            .find(|n| {
                                n.has_tag_name("color")
                                    && n.attribute("name") == Some(name)
                            })
                            .and_then(|n| n.text())
                            .and_then(svg_color)
                        {
                            return Some(value);
                        }
                    }
                }
            }
            None
        } else {
            svg_color(value)
        }
    }
    fn referenced(reference: &str, path: &Path, depth: usize) -> Option<String> {
        use base64::Engine;
        let (kind, name) = reference.strip_prefix('@')?.split_once('/')?;
        if !matches!(kind, "drawable" | "mipmap")
            || name.is_empty()
            || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return None;
        }
        let res = path.parent()?.parent()?;
        let mut folders = vec![res.join(kind)];
        if path.parent()?.file_name()?.to_str()?.starts_with(kind) {
            folders.insert(0, path.parent()?.to_path_buf());
        }
        for dir in std::fs::read_dir(res).ok()?.flatten() {
            if dir
                .file_name()
                .to_string_lossy()
                .starts_with(&format!("{kind}-"))
            {
                folders.push(dir.path());
            }
        }
        for folder in folders {
            for ext in ["xml", "png", "webp", "jpg", "jpeg"] {
                let file = folder.join(format!("{name}.{ext}"));
                if !file.is_file() {
                    continue;
                }
                if ext == "xml" {
                    return drawable_svg(&file, depth + 1);
                }
                if std::fs::metadata(&file).ok()?.len() > 8 * 1024 * 1024 {
                    return None;
                }
                let bytes = std::fs::read(file).ok()?;
                let mime = if ext == "jpg" { "jpeg" } else { ext };
                return Some(format!(
                    "<image width=\"24\" height=\"24\" href=\"data:image/{mime};base64,{}\" preserveAspectRatio=\"xMidYMid meet\"/>",
                    base64::engine::general_purpose::STANDARD.encode(bytes)
                ));
            }
        }
        None
    }
    fn render(
        node: roxmltree::Node<'_, '_>,
        path: &Path,
        depth: usize,
    ) -> Option<String> {
        let children = || {
            let mut body = String::new();
            let mut clips = 0;
            for child in node.children().filter(|n| n.is_element()) {
                if child.has_tag_name("clip-path") {
                    let id = format!("clip{}", child.range().start);
                    if let Some(data) = child.attribute((A, "pathData")) {
                        body.push_str(&format!("<defs><clipPath id=\"{id}\"><path d=\"{}\"/></clipPath></defs><g clip-path=\"url(#{id})\">",xml_escape(data)));
                        clips += 1;
                    }
                } else if let Some(fragment) = render(child, path, depth) {
                    body.push_str(&fragment);
                }
            }
            body.push_str(&"</g>".repeat(clips));
            body
        };
        let content = || {
            if let Some(reference) = node.attribute((A, "drawable")) {
                if let Some(value) = paint(node, "drawable", path) {
                    return Some(format!(
                        "<rect width=\"24\" height=\"24\" fill=\"{value}\"/>"
                    ));
                }
                return referenced(reference, path, depth);
            }
            Some(children())
        };
        match node.tag_name().name() {
            "vector" => Some(format!(
                "<g opacity=\"{}\">{}</g>",
                number(node, "alpha", 1.0).clamp(0.0, 1.0),
                children()
            )),
            "group" => Some(format!(
                "<g transform=\"translate({} {}) translate({} {}) rotate({}) scale({} {}) translate({} {})\">{}</g>",
                number(node, "translateX", 0.0),
                number(node, "translateY", 0.0),
                number(node, "pivotX", 0.0),
                number(node, "pivotY", 0.0),
                number(node, "rotation", 0.0),
                number(node, "scaleX", 1.0),
                number(node, "scaleY", 1.0),
                -number(node, "pivotX", 0.0),
                -number(node, "pivotY", 0.0),
                children()
            )),
            "path" => Some(format!(
                "<path d=\"{}\" fill=\"{}\" fill-opacity=\"{}\" stroke=\"{}\" stroke-width=\"{}\" stroke-opacity=\"{}\" fill-rule=\"{}\"/>",
                xml_escape(node.attribute((A, "pathData"))?),
                paint(node, "fillColor", path).unwrap_or_else(|| "none".into()),
                number(node, "fillAlpha", 1.0).clamp(0.0, 1.0),
                paint(node, "strokeColor", path).unwrap_or_else(|| "none".into()),
                number(node, "strokeWidth", 0.0),
                number(node, "strokeAlpha", 1.0).clamp(0.0, 1.0),
                if node.attribute((A, "fillType")) == Some("evenOdd") {
                    "evenodd"
                } else {
                    "nonzero"
                }
            )),
            "shape" => {
                let mut solid = node
                    .children()
                    .find(|n| n.has_tag_name("solid"))
                    .and_then(|n| paint(n, "color", path))
                    .unwrap_or_else(|| "none".into());
                let mut defs = String::new();
                if let Some(gradient) =
                    node.children().find(|n| n.has_tag_name("gradient"))
                {
                    let id = format!("gradient{}", gradient.range().start);
                    let start = paint(gradient, "startColor", path)?;
                    let end = paint(gradient, "endColor", path)?;
                    let center = paint(gradient, "centerColor", path)
                        .map(|c| {
                            format!("<stop offset=\"0.5\" stop-color=\"{c}\"/>")
                        })
                        .unwrap_or_default();
                    let stops = format!(
                        "<stop offset=\"0\" stop-color=\"{start}\"/>{center}<stop offset=\"1\" stop-color=\"{end}\"/>"
                    );
                    let definition = if gradient.attribute((A, "type"))
                        == Some("sweep")
                    {
                        let middle = paint(gradient, "centerColor", path);
                        let rgba = |s: &str| {
                            u32::from_str_radix(s.trim_start_matches('#'), 16).ok()
                        };
                        let first = rgba(&start)?;
                        let last = rgba(&end)?;
                        let mid = middle.as_deref().and_then(rgba);
                        let mut wedges = String::new();
                        let cx = number(gradient, "centerX", 0.5) * 24.0;
                        let cy = number(gradient, "centerY", 0.5) * 24.0;
                        for i in 0..256 {
                            let t = i as f64 / 255.0;
                            let (a, b, t) = if let Some(mid) = mid {
                                if t < 0.5 {
                                    (first, mid, t * 2.0)
                                } else {
                                    (mid, last, (t - 0.5) * 2.0)
                                }
                            } else {
                                (first, last, t)
                            };
                            let mut result = 0u32;
                            for shift in [24, 16, 8, 0] {
                                let a = ((a >> shift) & 255) as f64;
                                let b = ((b >> shift) & 255) as f64;
                                result |=
                                    ((a + (b - a) * t).round() as u32) << shift;
                            }
                            let a = i as f64 * std::f64::consts::TAU / 256.0;
                            let b =
                                (i as f64 + 1.05) * std::f64::consts::TAU / 256.0;
                            wedges.push_str(&format!("<path d=\"M{cx},{cy} L{},{} L{},{} Z\" fill=\"#{result:08X}\"/>",cx+48.0*a.cos(),cy+48.0*a.sin(),cx+48.0*b.cos(),cy+48.0*b.sin()));
                        }
                        format!(
                            "<pattern id=\"{id}\" width=\"24\" height=\"24\" patternUnits=\"userSpaceOnUse\">{wedges}</pattern>"
                        )
                    } else if gradient.attribute((A, "type")) == Some("radial") {
                        format!(
                            "<radialGradient id=\"{id}\" cx=\"{}\" cy=\"{}\" r=\"{}\">{stops}</radialGradient>",
                            number(gradient, "centerX", 0.5),
                            number(gradient, "centerY", 0.5),
                            number(gradient, "gradientRadius", 12.0) / 24.0
                        )
                    } else {
                        let angle = number(gradient, "angle", 0.0).to_radians();
                        let dx = angle.cos() * 0.5;
                        let dy = -angle.sin() * 0.5;
                        format!(
                            "<linearGradient id=\"{id}\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\">{stops}</linearGradient>",
                            0.5 - dx,
                            0.5 - dy,
                            0.5 + dx,
                            0.5 + dy
                        )
                    };
                    defs = format!("<defs>{definition}</defs>");
                    solid = format!("url(#{id})");
                }
                let stroke = node.children().find(|n| n.has_tag_name("stroke"));
                let edge = stroke
                    .and_then(|n| paint(n, "color", path))
                    .unwrap_or_else(|| "none".into());
                let width = stroke.map(|n| number(n, "width", 0.0)).unwrap_or(0.0);
                if node.attribute((A, "shape")) == Some("oval") {
                    Some(format!(
                        "{defs}<ellipse cx=\"12\" cy=\"12\" rx=\"11\" ry=\"11\" fill=\"{solid}\" stroke=\"{edge}\" stroke-width=\"{width}\"/>"
                    ))
                } else {
                    let radius = node
                        .children()
                        .find(|n| n.has_tag_name("corners"))
                        .map(|n| number(n, "radius", 0.0))
                        .unwrap_or(0.0);
                    let corners =
                        node.children().find(|n| n.has_tag_name("corners"));
                    let r = |key| {
                        corners
                            .map(|n| number(n, key, radius))
                            .unwrap_or(radius)
                            .clamp(0.0, 12.0)
                    };
                    let (tl, tr, br, bl) = (
                        r("topLeftRadius"),
                        r("topRightRadius"),
                        r("bottomRightRadius"),
                        r("bottomLeftRadius"),
                    );
                    Some(format!(
                        "{defs}<path d=\"M{tl},0 H{} Q24,0 24,{tr} V{} Q24,24 {},24 H{bl} Q0,24 0,{} V{tl} Q0,0 {tl},0 Z\" fill=\"{solid}\" stroke=\"{edge}\" stroke-width=\"{width}\"/>",
                        24.0 - tr,
                        24.0 - br,
                        24.0 - br,
                        24.0 - bl
                    ))
                }
            }
            "layer-list" => Some(children()),
            "selector" => node
                .children()
                .find(|n| {
                    n.has_tag_name("item")
                        && n.attributes().all(|a| !a.name().starts_with("state_"))
                })
                .or_else(|| node.children().find(|n| n.has_tag_name("item")))
                .and_then(|n| render(n, path, depth)),
            "item" | "inset" => {
                let inset = node.has_tag_name("inset");
                let all = number(node, "inset", 0.0);
                let l = number(node, if inset { "insetLeft" } else { "left" }, all);
                let t = number(node, if inset { "insetTop" } else { "top" }, all);
                let r =
                    number(node, if inset { "insetRight" } else { "right" }, all);
                let b =
                    number(node, if inset { "insetBottom" } else { "bottom" }, all);
                Some(format!(
                    "<svg x=\"{l}\" y=\"{t}\" width=\"{}\" height=\"{}\" viewBox=\"0 0 24 24\" preserveAspectRatio=\"none\">{}</svg>",
                    (24.0 - l - r).max(0.0),
                    (24.0 - t - b).max(0.0),
                    content()?
                ))
            }
            "bitmap" => {
                let gravity = node.attribute((A, "gravity")).unwrap_or("center");
                let align = if gravity.contains("fill") {
                    "none".to_string()
                } else {
                    format!(
                        "{}{} meet",
                        if gravity.contains("left") {
                            "xMin"
                        } else if gravity.contains("right") {
                            "xMax"
                        } else {
                            "xMid"
                        },
                        if gravity.contains("top") {
                            "YMin"
                        } else if gravity.contains("bottom") {
                            "YMax"
                        } else {
                            "YMid"
                        }
                    )
                };
                referenced(node.attribute((A, "src"))?, path, depth)
                    .map(|s| s.replace("xMidYMid meet", &align))
            }
            _ => None,
        }
    }
    let body = render(root, path, depth)?;
    let width = number(root, "viewportWidth", 24.0).max(1.0);
    let height = number(root, "viewportHeight", 24.0).max(1.0);
    Some(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {width} {height}\">{body}</svg>"
    ))
}
#[derive(Clone, Copy)]
struct ManifestLine {
    start: usize,
    content_end: usize,
    end: usize,
}

fn manifest_lines(source: &str) -> Vec<ManifestLine> {
    let bytes = source.as_bytes();
    let mut lines = Vec::new();
    let mut start = 0;
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'\n' {
            let content_end = if index > start && bytes[index - 1] == b'\r' {
                index - 1
            } else {
                index
            };
            lines.push(ManifestLine {
                start,
                content_end,
                end: index + 1,
            });
            start = index + 1;
        }
    }
    if start < source.len() {
        lines.push(ManifestLine {
            start,
            content_end: source.len(),
            end: source.len(),
        });
    }
    lines
}

fn manifest_line_text<'a>(source: &'a str, line: ManifestLine) -> &'a str {
    &source[line.start..line.content_end]
}

fn manifest_newline(source: &str) -> &'static str {
    source
        .find('\n')
        .map(|index| {
            if index > 0 && source.as_bytes()[index - 1] == b'\r' {
                "\r\n"
            } else {
                "\n"
            }
        })
        .unwrap_or("\n")
}

fn line_indent(line: &str) -> usize {
    line.bytes().take_while(|byte| *byte == b' ').count()
}

fn is_mapping_key(line: &str, indent: usize, key: &str) -> bool {
    if line_indent(line) != indent {
        return false;
    }
    line[indent..]
        .strip_prefix(key)
        .is_some_and(|rest| rest.starts_with(':'))
}

fn mapping_value<'a>(line: &'a str, indent: usize, key: &str) -> Option<&'a str> {
    line[indent..]
        .strip_prefix(key)?
        .strip_prefix(':')
        .map(str::trim)
}

fn mapping_end(source: &str, lines: &[ManifestLine], start: usize, indent: usize) -> usize {
    (start + 1..lines.len())
        .find(|index| {
            let line = manifest_line_text(source, lines[*index]);
            let trimmed = line.trim_start();
            !trimmed.is_empty()
                && !trimmed.starts_with('#')
                && line_indent(line) <= indent
        })
        .unwrap_or(lines.len())
}

fn yaml_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn yaml_scalar_end(value: &str) -> Option<usize> {
    let first = value.as_bytes().first().copied()?;
    match first {
        b'\'' => {
            let bytes = value.as_bytes();
            let mut index = 1;
            while index < bytes.len() {
                if bytes[index] == b'\'' {
                    if bytes.get(index + 1) == Some(&b'\'') {
                        index += 2;
                    } else {
                        return Some(index + 1);
                    }
                } else {
                    index += 1;
                }
            }
            None
        }
        b'"' => {
            let bytes = value.as_bytes();
            let mut index = 1;
            while index < bytes.len() {
                match bytes[index] {
                    b'\\' => index += 2,
                    b'"' => return Some(index + 1),
                    _ => index += 1,
                }
            }
            None
        }
        _ => {
            let end = value.find(" #").unwrap_or(value.len());
            Some(value[..end].trim_end().len())
        }
    }
}

fn asset_entry(source: &str, line: ManifestLine, indent: usize) -> Option<(String, usize, usize)> {
    let text = manifest_line_text(source, line);
    let rest = text.get(indent..)?.strip_prefix('-')?;
    let leading = rest.len() - rest.trim_start().len();
    let scalar_start = indent + 1 + leading;
    let scalar = text.get(scalar_start..)?;
    let scalar_end = yaml_scalar_end(scalar)?;
    let parsed: serde_yaml::Value = serde_yaml::from_str(&scalar[..scalar_end]).ok()?;
    Some((parsed.as_str()?.to_string(), scalar_start, scalar_start + scalar_end))
}

fn insert_after_line(source: &str, line: ManifestLine, text: &str) -> String {
    let ending = if line.end > line.content_end {
        &source[line.content_end..line.end]
    } else {
        manifest_newline(source)
    };
    let mut updated = String::with_capacity(source.len() + text.len() + ending.len());
    updated.push_str(&source[..line.end]);
    if line.end == line.content_end {
        updated.push_str(ending);
    }
    updated.push_str(text);
    if line.end > line.content_end {
        updated.push_str(ending);
    }
    updated.push_str(&source[line.end..]);
    updated
}

fn insert_before_line(source: &str, line: ManifestLine, text: &str) -> String {
    let mut updated = String::with_capacity(source.len() + text.len());
    updated.push_str(&source[..line.start]);
    updated.push_str(text);
    updated.push_str(&source[line.start..]);
    updated
}

fn append_lines(source: &str, additions: &[String]) -> String {
    let newline = manifest_newline(source);
    let has_trailing_newline = source.ends_with('\n');
    let mut updated = String::with_capacity(source.len() + additions.join(newline).len() + 8);
    updated.push_str(source);
    if !source.is_empty() && !has_trailing_newline {
        updated.push_str(newline);
    }
    updated.push_str(&additions.join(newline));
    if has_trailing_newline {
        updated.push_str(newline);
    }
    updated
}

fn edit_pubspec_asset(
    source: &str,
    old_path: Option<&str>,
    new_path: Option<&str>,
) -> Result<String, String> {
    let doc: serde_yaml::Value =
        serde_yaml::from_str(source).map_err(|error| error.to_string())?;
    let lines = manifest_lines(source);
    let flutter_value = doc.get("flutter");

    if let Some(path) = new_path {
        if path.trim().is_empty()
            || path.contains(['\n', '\r'])
            || std::path::Path::new(path).is_absolute()
            || std::path::Path::new(path)
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir))
        {
            return Err("Choose a project-relative asset path without parent-directory segments.".into());
        }
    }
    if old_path == new_path {
        return Ok(source.to_string());
    }

    if flutter_value.is_none() {
        if old_path.is_some() {
            return Err("The asset is not declared in pubspec.yaml.".into());
        }
        let path = new_path.ok_or("A new asset path is required")?;
        let additions = vec![
            "flutter:".to_string(),
            "  assets:".to_string(),
            format!("    - {}", yaml_string(path)),
        ];
        let updated = append_lines(source, &additions);
        serde_yaml::from_str::<serde_yaml::Value>(&updated)
            .map_err(|error| error.to_string())?;
        return Ok(updated);
    }

    let flutter_value = flutter_value.unwrap();
    if !flutter_value.is_mapping() {
        return Err("Convert flutter to a YAML block mapping before importing.".into());
    }
    let Some(flutter_line) = lines.iter().position(|line| {
        let text = manifest_line_text(source, *line);
        is_mapping_key(text, 0, "flutter")
    }) else {
        return Err("Could not safely locate the flutter mapping in pubspec.yaml.".into());
    };
    let flutter_indent = line_indent(manifest_line_text(source, lines[flutter_line]));
    if mapping_value(
        manifest_line_text(source, lines[flutter_line]),
        flutter_indent,
        "flutter",
    )
    .is_some_and(|value| !value.is_empty() && !value.starts_with('#'))
    {
        return Err("Convert flutter to a YAML block mapping before importing.".into());
    }
    let flutter_end = mapping_end(source, &lines, flutter_line, flutter_indent);
    let child_indent = (flutter_line + 1..flutter_end)
        .map(|index| manifest_line_text(source, lines[index]))
        .filter(|text| {
            let trimmed = text.trim_start();
            !trimmed.is_empty() && !trimmed.starts_with('#')
        })
        .map(line_indent)
        .filter(|indent| *indent > flutter_indent)
        .min()
        .unwrap_or(flutter_indent + 2);

    let assets_value = flutter_value.get("assets");
    let assets_line = (flutter_line + 1..flutter_end).find(|index| {
        let text = manifest_line_text(source, lines[*index]);
        is_mapping_key(text, child_indent, "assets")
    });
    if assets_value.is_some() && assets_line.is_none() {
        return Err("The flutter.assets entry is not a supported block-style mapping.".into());
    }
    if let Some(value) = assets_value
        && !value.is_null()
        && !value.is_sequence()
    {
        return Err("Convert flutter.assets to a YAML block list before editing it.".into());
    }

    if let Some(assets_line) = assets_line {
        let assets_indent = line_indent(manifest_line_text(source, lines[assets_line]));
        let value_text = mapping_value(
            manifest_line_text(source, lines[assets_line]),
            assets_indent,
            "assets",
        )
        .unwrap_or_default();
        if !value_text.is_empty() && !value_text.starts_with('#') {
            return Err("Convert flutter.assets to a YAML block list before editing it.".into());
        }
        let assets_end = mapping_end(source, &lines, assets_line, assets_indent);
        let item_indent = (assets_line + 1..assets_end)
            .filter_map(|index| {
                let text = manifest_line_text(source, lines[index]);
                let indent = line_indent(text);
                (indent > assets_indent && text[indent..].starts_with('-'))
                    .then_some(indent)
            })
            .min();
        let entries = item_indent
            .map(|indent| {
                (assets_line + 1..assets_end)
                    .filter(|index| {
                        let text = manifest_line_text(source, lines[*index]);
                        line_indent(text) == indent
                            && text[indent..].starts_with('-')
                    })
                    .filter_map(|index| {
                        asset_entry(source, lines[index], indent)
                            .map(|(value, start, end)| (index, value, start, end))
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        if let Some(new_path) = new_path
            && entries.iter().any(|(_, value, _, _)| value == new_path)
        {
            if old_path.is_none() {
                return Ok(source.to_string());
            }
            return Err("The new asset path is already declared in pubspec.yaml.".into());
        }

        if let Some(old_path) = old_path {
            let Some((index, _, scalar_start, scalar_end)) =
                entries.iter().find(|(_, value, _, _)| value == old_path)
            else {
                return Err("The asset is not declared in pubspec.yaml.".into());
            };
            let line = lines[*index];
            if let Some(new_path) = new_path {
                let mut updated = String::with_capacity(source.len() + new_path.len());
                updated.push_str(&source[..line.start + scalar_start]);
                updated.push_str(&yaml_string(new_path));
                updated.push_str(&source[line.start + scalar_end..]);
                serde_yaml::from_str::<serde_yaml::Value>(&updated)
                    .map_err(|error| error.to_string())?;
                return Ok(updated);
            }
            let text = manifest_line_text(source, line);
            let suffix = &text[*scalar_end..];
            if let Some(comment) = suffix.find('#')
                && suffix[..comment].trim().is_empty()
            {
                let replacement = format!(
                    "{}{}",
                    &text[..line_indent(text)],
                    suffix[comment..].trim_start()
                );
                let mut updated = String::with_capacity(source.len());
                updated.push_str(&source[..line.start]);
                updated.push_str(&replacement);
                updated.push_str(&source[line.content_end..]);
                serde_yaml::from_str::<serde_yaml::Value>(&updated)
                    .map_err(|error| error.to_string())?;
                return Ok(updated);
            }
            let mut updated = String::with_capacity(source.len());
            updated.push_str(&source[..line.start]);
            updated.push_str(&source[line.end..]);
            serde_yaml::from_str::<serde_yaml::Value>(&updated)
                .map_err(|error| error.to_string())?;
            return Ok(updated);
        }

        let path = new_path.ok_or("A new asset path is required")?;
        let list_indent = item_indent.unwrap_or(assets_indent + 2);
        let entry = format!("{}- {}", " ".repeat(list_indent), yaml_string(path));
        let insert_after = entries
            .last()
            .map(|(index, _, _, _)| *index)
            .or_else(|| {
                (assets_line + 1..assets_end).find(|index| {
                    let text = manifest_line_text(source, lines[*index]);
                    let trimmed = text.trim_start();
                    !trimmed.is_empty()
                        && !trimmed.starts_with('#')
                        && line_indent(text) > assets_indent
                })
            })
            .unwrap_or(assets_line);
        let updated = insert_after_line(source, lines[insert_after], &entry);
        serde_yaml::from_str::<serde_yaml::Value>(&updated)
            .map_err(|error| error.to_string())?;
        return Ok(updated);
    }

    if old_path.is_some() {
        return Err("The asset is not declared in pubspec.yaml.".into());
    }
    let path = new_path.ok_or("A new asset path is required")?;
    let block = format!(
        "{}assets:{}{}- {}{}",
        " ".repeat(child_indent),
        manifest_newline(source),
        " ".repeat(child_indent + 2),
        yaml_string(path),
        manifest_newline(source)
    );
    let updated = if flutter_end < lines.len() {
        insert_before_line(source, lines[flutter_end], &block)
    } else {
        let additions = vec![
            format!("{}assets:", " ".repeat(child_indent)),
            format!(
                "{}- {}",
                " ".repeat(child_indent + 2),
                yaml_string(path)
            ),
        ];
        append_lines(source, &additions)
    };
    serde_yaml::from_str::<serde_yaml::Value>(&updated)
        .map_err(|error| error.to_string())?;
    Ok(updated)
}

fn register_asset(source: &str, path: &str) -> Result<String, String> {
    edit_pubspec_asset(source, None, Some(path))
}

fn manifest_diff(before: &str, after: &str) -> String {
    let before = before.lines().collect::<Vec<_>>();
    let after = after.lines().collect::<Vec<_>>();
    let prefix = before
        .iter()
        .zip(&after)
        .take_while(|(left, right)| left == right)
        .count();
    let suffix = before[prefix..]
        .iter()
        .rev()
        .zip(after[prefix..].iter().rev())
        .take_while(|(left, right)| left == right)
        .count();
    let mut diff = String::new();
    for line in &before[prefix..before.len() - suffix] {
        diff.push_str("− ");
        diff.push_str(line);
        diff.push('\n');
    }
    for line in &after[prefix..after.len() - suffix] {
        diff.push_str("+ ");
        diff.push_str(line);
        diff.push('\n');
    }
    if diff.is_empty() {
        "No pubspec.yaml changes are needed.".into()
    } else {
        format!("pubspec.yaml changes:\n{diff}")
    }
}

fn resolve_import_destination(source: &Path, destination: &Path) -> Result<PathBuf, String> {
    let destination_text = destination.to_string_lossy();
    if destination_text.ends_with(std::path::MAIN_SEPARATOR)
        || destination.is_dir()
    {
        let filename = source
            .file_name()
            .ok_or("The selected source file has no filename")?;
        Ok(destination.join(filename))
    } else {
        Ok(destination.to_path_buf())
    }
}

#[derive(Clone)]
struct AssetImportEntry {
    source: PathBuf,
    destination: PathBuf,
    skipped: bool,
}

#[derive(Clone)]
pub(super) struct AssetImportPlan {
    root: PathBuf,
    entries: Vec<AssetImportEntry>,
    pubspec_change: Option<(String, String)>,
    details: String,
}

impl AssetImportPlan {
    pub(super) fn details(&self) -> &str {
        &self.details
    }

    fn apply(&self) -> Result<(), String> {
        let pubspec = self.root.join("pubspec.yaml");
        if let Some((original, _)) = &self.pubspec_change
            && std::fs::read_to_string(&pubspec).ok().as_deref()
                != Some(original.as_str())
        {
            return Err(
                "pubspec.yaml changed after preview. Review the current manifest and retry."
                    .into(),
            );
        }
        let canonical_root = self.root.canonicalize().map_err(|error| error.to_string())?;
        let imports = self.entries.iter().filter(|entry| !entry.skipped).collect::<Vec<_>>();
        for entry in &imports {
            if !entry.source.is_file() {
                return Err(format!("The source file no longer exists: {}", entry.source.display()));
            }
            if self.root.join(&entry.destination).exists() {
                return Err(format!(
                    "A file appeared at {} after preview. Review the import and retry.",
                    entry.destination.display()
                ));
            }
        }

        let mut copied = Vec::new();
        for entry in imports {
            let result = (|| {
                let target = self.root.join(&entry.destination);
                let parent = target.parent().ok_or("Invalid destination")?;
                let existing = parent
                    .ancestors()
                    .find(|path| path.exists())
                    .ok_or("Invalid destination")?;
                if !existing
                    .canonicalize()
                    .map_err(|error| error.to_string())?
                    .starts_with(&canonical_root)
                {
                    return Err("The destination resolves outside the project.".to_string());
                }
                std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
                if !parent
                    .canonicalize()
                    .map_err(|error| error.to_string())?
                    .starts_with(&canonical_root)
                {
                    return Err("The destination resolves outside the project.".to_string());
                }
                let temporary = tempfile::NamedTempFile::new_in(parent)
                    .map_err(|error| error.to_string())?;
                std::fs::copy(&entry.source, temporary.path())
                    .map_err(|error| error.to_string())?;
                temporary
                    .persist_noclobber(&target)
                    .map_err(|error| error.to_string())?;
                Ok(target)
            })();
            match result {
                Ok(target) => copied.push(target),
                Err(error) => {
                    for target in copied {
                        let _ = std::fs::remove_file(target);
                    }
                    return Err(error);
                }
            }
        }

        if let Some((original, updated)) = &self.pubspec_change {
            if std::fs::read_to_string(&pubspec).ok().as_deref() != Some(original.as_str()) {
                for target in copied {
                    let _ = std::fs::remove_file(target);
                }
                return Err(
                    "pubspec.yaml changed during import. Review the current manifest and retry."
                        .into(),
                );
            }
            if updated != original
                && let Err(error) = crate::persistence::write_files(vec![(
                    pubspec,
                    updated.clone(),
                )])
            {
                for target in copied {
                    let _ = std::fs::remove_file(target);
                }
                return Err(error);
            }
        }
        Ok(())
    }
}

pub(super) fn preview_asset_import(
    root: &Path,
    source: &Path,
    destination: &Path,
) -> Result<AssetImportPlan, String> {
    let destination = resolve_import_destination(source, destination)?;
    if root.join(&destination).exists() {
        return Err("A file already exists at the destination.".into());
    }
    preview_asset_import_batch(root, &[source.to_path_buf()], &destination)
}

pub(super) fn preview_asset_import_batch(
    root: &Path,
    sources: &[PathBuf],
    destination: &Path,
) -> Result<AssetImportPlan, String> {
    use std::path::Component;
    if sources.is_empty() {
        return Err("Choose one or more existing local files to import.".into());
    }
    if destination.as_os_str().is_empty()
        || destination.is_absolute()
        || destination.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err("Choose a destination inside the project.".into());
    }
    let mut entries = Vec::with_capacity(sources.len());
    let mut planned_destinations = HashSet::new();
    let mut manifest = None::<(String, String)>;
    let mut details = Vec::with_capacity(sources.len());
    let mut copied_count = 0;
    let mut skipped_count = 0;
    for source in sources {
        if !source.is_file() {
            return Err(format!("Choose an existing local file to import: {}", source.display()));
        }
        let entry_destination = resolve_import_destination(source, destination)?;
        let android = entry_destination.starts_with("android")
            && entry_destination
                .components()
                .any(|part| part.as_os_str() == "res");
        if android {
            let stem = entry_destination
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("");
            if stem.is_empty()
                || !stem.chars().all(|character| {
                    character.is_ascii_lowercase()
                        || character.is_ascii_digit()
                        || character == '_'
                        || character == '.'
                })
            {
                return Err("Android resource filenames must use lowercase letters, digits and underscores.".into());
            }
        }
        let target_exists = root.join(&entry_destination).exists();
        let duplicate_destination = !planned_destinations.insert(entry_destination.clone());
        let skipped = target_exists || duplicate_destination;
        let name = source.file_name().unwrap_or_default().to_string_lossy();
        if skipped {
            skipped_count += 1;
            details.push(format!(
                "Skip {name} → {} (destination already exists or is duplicated in this selection).",
                entry_destination.display()
            ));
        } else {
            copied_count += 1;
            details.push(format!("Copy {name} to {}", entry_destination.display()));
            if !android {
                let pubspec = root.join("pubspec.yaml");
                let (original, current) = match &manifest {
                    Some((original, current)) => (original.clone(), current.clone()),
                    None => {
                        let original = std::fs::read_to_string(&pubspec)
                            .map_err(|error| error.to_string())?;
                        (original.clone(), original)
                    }
                };
                let updated = register_asset(
                    &current,
                    &entry_destination.to_string_lossy().replace('\\', "/"),
                )?;
                manifest = Some((original, updated));
            }
        }
        entries.push(AssetImportEntry {
            source: source.clone(),
            destination: entry_destination,
            skipped,
        });
    }
    let pubspec_change = manifest.filter(|(original, updated)| original != updated);
    let changes = match &pubspec_change {
        Some((original, updated)) => manifest_diff(original, updated),
        None if copied_count > 0 && entries.iter().all(|entry| {
            entry.destination.starts_with("android")
                && entry.destination.components().any(|part| part.as_os_str() == "res")
        }) => "Android resources do not require a pubspec.yaml change.".to_string(),
        None => "No pubspec.yaml changes are needed.".into(),
    };
    details.push(format!("{copied_count} file(s) to copy; {skipped_count} skipped."));
    details.push(changes);
    Ok(AssetImportPlan {
        root: root.to_path_buf(),
        entries,
        pubspec_change,
        details: details.join("\n"),
    })
}

pub(super) fn apply_asset_import(plan: AssetImportPlan) -> Result<(), String> {
    plan.apply()
}

pub fn import_asset(
    root: &Path,
    source: &Path,
    destination: &Path,
) -> Result<(), String> {
    let plan = preview_asset_import(root, source, destination)?;
    apply_asset_import(plan)
}
pub fn create_value(
    root: &Path,
    module: &str,
    kind: Kind,
    name: &str,
    value: &str,
) -> Result<(), String> {
    if !matches!(kind, Kind::Color | Kind::String) {
        return Err("Choose a color or string resource.".into());
    }
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        || name.starts_with(char::is_numeric)
    {
        return Err("Use a lowercase Android resource name containing letters, digits and underscores.".into());
    }
    let module = if module.starts_with("android/") && !module.contains("..") {
        module
    } else {
        "android/app"
    };
    let tag = if kind == Kind::Color {
        "color"
    } else {
        "string"
    };
    if kind == Kind::Color && color(value).is_none() && !value.starts_with("@color/")
    {
        return Err(
            "Enter an RGB/ARGB hexadecimal color or @color reference.".into()
        );
    }
    let path = root
        .join(module)
        .join("src/main/res/values")
        .join(format!("{tag}s.xml"));
    let source = match std::fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            "<resources>\n</resources>\n".into()
        }
        Err(error) => return Err(error.to_string()),
    };
    let xml = roxmltree::Document::parse(&source).map_err(|e| e.to_string())?;
    if xml.root_element().tag_name().name() != "resources" {
        return Err("The destination is not an Android resources document.".into());
    }
    if xml
        .root_element()
        .children()
        .any(|node| node.attribute("name") == Some(name))
    {
        return Err("A resource with this name already exists.".into());
    }
    let value = value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let entry = format!("  <{tag} name=\"{name}\">{value}</{tag}>\n");
    let updated = if let Some(end) = source.rfind("</resources>") {
        format!("{}{}{}", &source[..end], entry, &source[end..])
    } else if let Some(end) = source.rfind("/>") {
        format!("{}>\n{entry}</resources>\n", &source[..end])
    } else {
        return Err("Could not find the resources document root.".into());
    };
    crate::persistence::write_files(vec![(path, updated)])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inventory_reads_declared_assets_android_values_and_variants() {
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        std::fs::create_dir_all(r.join("assets/2.0x")).unwrap();
        std::fs::write(r.join("assets/icon.png"), "image").unwrap();
        std::fs::write(r.join("assets/2.0x/icon.png"), "image").unwrap();
        std::fs::write(
            r.join("pubspec.yaml"),
            "name: probe\nflutter:\n  assets:\n    - assets/\n",
        )
        .unwrap();
        for qualifier in ["values", "values-fr"] {
            let dir = r.join("android/app/src/main/res").join(qualifier);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("strings.xml"),"<resources><string name=\"title\">Hello &amp; world</string><color name=\"accent\">#FFAA00</color></resources>").unwrap();
        }
        let data = scan(r).unwrap();
        assert_eq!(
            data.iter()
                .find(|item| item.kind == Kind::Asset)
                .unwrap()
                .variants
                .len(),
            2
        );
        assert_eq!(
            data.iter()
                .find(|item| item.kind == Kind::String)
                .unwrap()
                .variants
                .len(),
            2
        );
        assert!(color("#FFAA00").is_some());
        assert!(color("@color/accent").is_none());
    }
    #[test]
    fn import_registers_asset_preserves_comments_and_refuses_escape_or_overwrite() {
        let root = tempfile::tempdir().unwrap();
        let source = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(source.path(), [1u8, 2, 3]).unwrap();
        std::fs::write(
            root.path().join("pubspec.yaml"),
            "name: probe\n# keep this\nflutter:\n  uses-material-design: true\n",
        )
        .unwrap();
        import_asset(root.path(), source.path(), Path::new("assets/icon.png"))
            .unwrap();
        assert!(
            std::fs::read_to_string(root.path().join("pubspec.yaml"))
                .unwrap()
                .contains("# keep this")
        );
        assert_eq!(scan(root.path()).unwrap().len(), 1);
        assert!(
            import_asset(root.path(), source.path(), Path::new("../outside.png"))
                .is_err()
        );
        assert!(
            import_asset(root.path(), source.path(), Path::new("assets/icon.png"))
                .is_err()
        );
    }

    #[test]
    fn asset_registration_preserves_pubspec_formatting_outside_the_inserted_entry() {
        let source = concat!(
            "name: probe\r\n",
            "# top-level note\r\n",
            "flutter:\r\n",
            "   uses-material-design: true\r\n",
            "   assets:\r\n",
            "      # assets header\r\n",
            "      - 'assets/existing/' # keep inline\r\n",
            "\r\n",
            "      # keep later comment\r\n",
            "   fonts:\r\n",
            "     - family: Demo\r\n",
            "       fonts:\r\n",
            "         - asset: fonts/demo.ttf\r\n",
        );
        let expected = concat!(
            "name: probe\r\n",
            "# top-level note\r\n",
            "flutter:\r\n",
            "   uses-material-design: true\r\n",
            "   assets:\r\n",
            "      # assets header\r\n",
            "      - 'assets/existing/' # keep inline\r\n",
            "      - 'assets/new.json'\r\n",
            "\r\n",
            "      # keep later comment\r\n",
            "   fonts:\r\n",
            "     - family: Demo\r\n",
            "       fonts:\r\n",
            "         - asset: fonts/demo.ttf\r\n",
        );
        assert_eq!(register_asset(source, "assets/new.json").unwrap(), expected);
    }

    #[test]
    fn asset_manifest_entries_can_be_renamed_and_removed_without_losing_comments() {
        let source = concat!(
            "name: probe\r\n",
            "flutter:\r\n",
            "   assets:\r\n",
            "      - 'assets/old.png' # keep this note\r\n",
            "   fonts:\r\n",
            "     - family: Demo\r\n",
            "       fonts:\r\n",
            "         - asset: fonts/demo.ttf\r\n",
        );
        let renamed = edit_pubspec_asset(
            source,
            Some("assets/old.png"),
            Some("assets/new.png"),
        )
        .unwrap();
        assert_eq!(
            renamed,
            source.replace("assets/old.png", "assets/new.png")
        );

        let removed =
            edit_pubspec_asset(&renamed, Some("assets/new.png"), None).unwrap();
        assert_eq!(
            removed,
            concat!(
                "name: probe\r\n",
                "flutter:\r\n",
                "   assets:\r\n",
                "      # keep this note\r\n",
                "   fonts:\r\n",
                "     - family: Demo\r\n",
                "       fonts:\r\n",
                "         - asset: fonts/demo.ttf\r\n",
            )
        );
    }

    #[test]
    fn asset_manifest_editor_is_idempotent_and_rejects_flow_style_assets() {
        let source = "name: probe\nflutter:\n  assets:\n    - assets/old.png\n";
        assert_eq!(
            register_asset(source, "assets/old.png").unwrap(),
            source
        );
        assert!(register_asset(
            "name: probe\nflutter:\n  assets: [assets/old.png]\n",
            "assets/new.png"
        )
        .is_err());
    }

    #[test]
    fn asset_import_preview_is_read_only_and_folder_destination_uses_filename() {
        let root = tempfile::tempdir().unwrap();
        let pubspec = root.path().join("pubspec.yaml");
        let original = "name: probe\nflutter:\n  uses-material-design: true\n";
        std::fs::write(&pubspec, original).unwrap();
        let source = root.path().join("outside/icon.png");
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        std::fs::write(&source, [1_u8, 2, 3]).unwrap();
        let plan = preview_asset_import(
            root.path(),
            &source,
            Path::new("assets/"),
        )
        .unwrap();
        assert!(plan.details().contains("to assets/icon.png"));
        assert!(plan.details().contains("+     - 'assets/icon.png'"));
        assert!(plan.details().contains("pubspec.yaml changes:"));
        assert!(!root.path().join("assets").exists());
        assert_eq!(std::fs::read_to_string(&pubspec).unwrap(), original);

        apply_asset_import(plan)
            .unwrap();
        assert!(root.path().join("assets").is_dir());
        assert!(root.path().join("assets/icon.png").is_file());
        assert!(std::fs::read_to_string(&pubspec)
            .unwrap()
            .contains("assets/icon.png"));
    }

    #[test]
    fn batch_asset_import_skips_existing_targets_and_registers_only_copied_files() {
        let root = tempfile::tempdir().unwrap();
        let sources = tempfile::tempdir().unwrap();
        let pubspec = root.path().join("pubspec.yaml");
        std::fs::write(&pubspec, "name: probe\nflutter:\n  assets:\n").unwrap();
        std::fs::create_dir_all(root.path().join("assets")).unwrap();
        std::fs::write(root.path().join("assets/existing.png"), "keep this").unwrap();
        let existing_source = sources.path().join("existing.png");
        let fresh_source = sources.path().join("fresh.png");
        let duplicate_source_dir = sources.path().join("another-folder");
        std::fs::create_dir_all(&duplicate_source_dir).unwrap();
        let duplicate_source = duplicate_source_dir.join("fresh.png");
        std::fs::write(&existing_source, "replacement").unwrap();
        std::fs::write(&fresh_source, "new asset").unwrap();
        std::fs::write(&duplicate_source, "duplicate name").unwrap();

        let plan = preview_asset_import_batch(
            root.path(),
            &[existing_source, fresh_source, duplicate_source],
            Path::new("assets/"),
        )
        .unwrap();
        assert!(plan.details().contains("Skip existing.png"));
        assert!(plan.details().contains("Copy fresh.png"));
        assert!(plan.details().contains("Skip fresh.png"));
        assert!(plan.details().contains("1 file(s) to copy; 2 skipped."));
        apply_asset_import(plan).unwrap();

        assert_eq!(
            std::fs::read_to_string(root.path().join("assets/existing.png")).unwrap(),
            "keep this"
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join("assets/fresh.png")).unwrap(),
            "new asset"
        );
        let manifest = std::fs::read_to_string(pubspec).unwrap();
        assert!(manifest.contains("assets/fresh.png"));
        assert!(!manifest.contains("assets/existing.png"));
    }

    #[test]
    fn stale_manifest_prevents_every_file_in_a_batch_from_being_copied() {
        let root = tempfile::tempdir().unwrap();
        let sources = tempfile::tempdir().unwrap();
        let pubspec = root.path().join("pubspec.yaml");
        std::fs::write(&pubspec, "name: probe\nflutter:\n  assets:\n").unwrap();
        let first = sources.path().join("first.png");
        let second = sources.path().join("second.png");
        std::fs::write(&first, "first").unwrap();
        std::fs::write(&second, "second").unwrap();
        let plan = preview_asset_import_batch(
            root.path(),
            &[first, second],
            Path::new("assets/"),
        )
        .unwrap();
        std::fs::write(&pubspec, "name: changed\nflutter:\n  assets:\n").unwrap();

        assert!(apply_asset_import(plan).is_err());
        assert!(!root.path().join("assets/first.png").exists());
        assert!(!root.path().join("assets/second.png").exists());
    }

    #[test]
    fn asset_import_plan_rejects_stale_manifest_and_new_destination_collisions() {
        let root = tempfile::tempdir().unwrap();
        let pubspec = root.path().join("pubspec.yaml");
        std::fs::write(&pubspec, "name: probe\nflutter:\n  assets:\n").unwrap();
        let source = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(source.path(), [7_u8, 8, 9]).unwrap();

        let stale_plan = preview_asset_import(
            root.path(),
            source.path(),
            Path::new("assets/icon.png"),
        )
        .unwrap();
        std::fs::write(&pubspec, "name: changed\nflutter:\n  assets:\n").unwrap();
        assert!(apply_asset_import(stale_plan).is_err());
        assert!(!root.path().join("assets/icon.png").exists());

        let collision_plan = preview_asset_import(
            root.path(),
            source.path(),
            Path::new("assets/icon.png"),
        )
        .unwrap();
        std::fs::create_dir_all(root.path().join("assets")).unwrap();
        std::fs::write(root.path().join("assets/icon.png"), "keep existing").unwrap();
        assert!(apply_asset_import(collision_plan).is_err());
        assert_eq!(
            std::fs::read_to_string(root.path().join("assets/icon.png")).unwrap(),
            "keep existing"
        );
    }
}

#[cfg(test)]
mod creation_tests {
    use super::*;
    #[test]
    fn creates_values_escapes_xml_and_refuses_duplicate_names() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("pubspec.yaml"), "name: probe\n").unwrap();
        create_value(
            root.path(),
            "android/app",
            Kind::String,
            "title",
            "Hello & Flutter",
        )
        .unwrap();
        create_value(root.path(), "android/app", Kind::Color, "accent", "#FFC107")
            .unwrap();
        assert!(
            create_value(
                root.path(),
                "android/app",
                Kind::Color,
                "accent",
                "#FFFFFF"
            )
            .is_err()
        );
        assert!(
            create_value(
                root.path(),
                "android/app",
                Kind::String,
                "../outside",
                "bad"
            )
            .is_err()
        );
        let data = scan(root.path()).unwrap();
        assert!(
            data.iter().any(|r| r.kind == Kind::String
                && r.variants[0].value == "Hello & Flutter")
        );
    }
}

#[cfg(test)]
mod drawable_tests {
    use super::*;
    #[test]
    fn layers_shapes_references_and_argb_are_rendered() {
        assert_eq!(
            svg_color("@android:color/white").as_deref(),
            Some("#FFFFFFFF")
        );
        let dir = tempfile::tempdir().unwrap();
        let res = dir.path();
        std::fs::create_dir(res.join("drawable")).unwrap();
        std::fs::create_dir(res.join("values")).unwrap();
        std::fs::write(
            res.join("values/colors.xml"),
            "<resources><color name=\"accent\">#80FF0000</color></resources>",
        )
        .unwrap();
        let path = res.join("drawable/background.xml");
        std::fs::write(&path,r##"<layer-list xmlns:android="http://schemas.android.com/apk/res/android"><item><shape><solid android:color="@color/accent"/><corners android:radius="4dp"/></shape></item></layer-list>"##).unwrap();
        let svg = vector(&path).unwrap();
        assert!(svg.contains("#FF000080"));
        assert!(svg.contains("M4,0"));
        std::fs::write(&path,r##"<selector xmlns:android="http://schemas.android.com/apk/res/android"><item android:state_pressed="true"><shape><solid android:color="#FF0000"/></shape></item><item><shape android:shape="oval"><solid android:color="#00FF00"/></shape></item></selector>"##).unwrap();
        let svg = vector(&path).unwrap();
        assert!(svg.contains("ellipse"));
        assert!(!svg.contains("#FF0000FF"));
    }
    #[test]
    fn cycles_are_bounded_and_vector_strokes_keep_alpha() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("drawable");
        std::fs::create_dir(&path).unwrap();
        let file = path.join("self.xml");
        std::fs::write(&file,r##"<layer-list xmlns:android="http://schemas.android.com/apk/res/android"><item android:drawable="@drawable/self"/></layer-list>"##).unwrap();
        assert!(vector(&file).unwrap().len() < 2000);
        std::fs::write(&file,r##"<vector xmlns:android="http://schemas.android.com/apk/res/android" android:viewportWidth="48" android:viewportHeight="48"><group android:rotation="90"><path android:pathData="M0,0L24,24" android:strokeColor="#800000FF" android:strokeWidth="2"/></group></vector>"##).unwrap();
        let svg = vector(&file).unwrap();
        assert!(svg.contains("#0000FF80"));
        assert!(svg.contains("rotate(90"));
        assert!(svg.contains("stroke-width=\"2\""));
    }
}

#[cfg(test)]
mod complex_drawable_tests {
    use super::*;
    #[test]
    fn gradient_inset_bitmap_and_ordered_clip_are_rendered() {
        let dir = tempfile::tempdir().unwrap();
        let drawable = dir.path().join("drawable");
        std::fs::create_dir(&drawable).unwrap();
        let path = drawable.join("complex.xml");
        std::fs::write(&path,r##"<inset xmlns:android="http://schemas.android.com/apk/res/android" android:insetLeft="3dp" android:insetTop="2dp"><shape><gradient android:startColor="#FF0000" android:centerColor="#00FF00" android:endColor="#0000FF" android:angle="90"/></shape></inset>"##).unwrap();
        let svg = vector(&path).unwrap();
        assert!(svg.contains("width=\"21\""));
        assert!(svg.contains("linearGradient"));
        assert!(svg.contains("offset=\"0.5\""));
        assert!(svg.contains("url(#gradient"));
        std::fs::write(drawable.join("pixel.png"), [137, 80, 78, 71]).unwrap();
        std::fs::write(&path,r##"<bitmap xmlns:android="http://schemas.android.com/apk/res/android" android:src="@drawable/pixel"/>"##).unwrap();
        assert!(
            vector(&path)
                .unwrap()
                .contains("data:image/png;base64,iVBORw==")
        );
        std::fs::write(&path,r##"<vector xmlns:android="http://schemas.android.com/apk/res/android"><path android:pathData="M0,0L24,24"/><clip-path android:pathData="M0,0h12v24H0z"/><path android:pathData="M0,0h24v24H0z" android:fillColor="#FFFFFF"/></vector>"##).unwrap();
        let svg = vector(&path).unwrap();
        assert!(svg.find("M0,0L24,24").unwrap() < svg.find("clipPath").unwrap());
        assert!(svg.contains("clip-path=\"url(#clip"));
        assert!(svg.ends_with("</g></svg>"));
    }
}

#[cfg(test)]
mod remaining_drawable_tests {
    use super::*;
    #[test]
    fn sweep_and_individual_corners_render_with_root_alpha() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sample.xml");
        std::fs::write(&path,r##"<shape xmlns:android="http://schemas.android.com/apk/res/android"><gradient android:type="sweep" android:startColor="#FF0000" android:endColor="#0000FF"/><corners android:topLeftRadius="8dp" android:bottomRightRadius="2dp"/></shape>"##).unwrap();
        let svg = vector(&path).unwrap();
        assert!(svg.contains("<pattern"));
        assert!(svg.contains("M8,0"));
        assert!(svg.contains("Q24,24 22,24"));
        std::fs::write(&path,r##"<vector xmlns:android="http://schemas.android.com/apk/res/android" android:alpha="0.5"><path android:pathData="M0,0L24,24" android:fillColor="#FFFFFF"/></vector>"##).unwrap();
        assert!(vector(&path).unwrap().contains("opacity=\"0.5\""));
    }
}
