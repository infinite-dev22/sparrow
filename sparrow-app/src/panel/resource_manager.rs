//! Real Flutter assets and Android wrapper resources, with module and variant grouping.
use super::kind::PanelKind;
use crate::{
    app::clickable_icon,
    command::InternalCommand,
    config::{color::SparrowColor, icon::SparrowIcons},
    editor::location::{EditorLocation, EditorPosition},
    popup_menu::{Menu, MenuItem, PopupMenuExt},
    window_tab::WindowTabData,
};
use floem::{
    IntoView, View,
    reactive::{
        Scope, SignalGet, SignalUpdate, SignalWith, create_effect, create_memo,
        create_rw_signal,
    },
    views::{
        Decorators, dyn_stack, empty, img_from_path, label, scroll, stack, svg,
        text_input,
    },
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    rc::Rc,
    time::Duration,
};
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
            "vector" => Some(children()),
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
                        == Some("radial")
                    {
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
                    Some(format!(
                        "{defs}<rect x=\"0\" y=\"0\" width=\"24\" height=\"24\" rx=\"{radius}\" fill=\"{solid}\" stroke=\"{edge}\" stroke-width=\"{width}\"/>"
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
            "bitmap" => referenced(node.attribute((A, "src"))?, path, depth),
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
fn register_asset(source: &str, path: &str) -> Result<String, String> {
    let doc: serde_yaml::Value =
        serde_yaml::from_str(source).map_err(|e| e.to_string())?;
    if doc["flutter"]["assets"]
        .as_sequence()
        .is_some_and(|values| {
            values.iter().any(|value| value.as_str() == Some(path))
        })
    {
        return Ok(source.into());
    }
    let mut lines = source.lines().map(String::from).collect::<Vec<_>>();
    let flutter = lines.iter().position(|line| line.trim_end() == "flutter:");
    if let Some(flutter) = flutter {
        let end = (flutter + 1..lines.len())
            .find(|&i| {
                !lines[i].trim().is_empty()
                    && !lines[i].starts_with(char::is_whitespace)
                    && !lines[i].starts_with('#')
            })
            .unwrap_or(lines.len());
        if let Some(assets) =
            (flutter + 1..end).find(|&i| lines[i].trim() == "assets:")
        {
            let indent = lines[assets].len() - lines[assets].trim_start().len();
            lines.insert(
                assets + 1,
                format!(
                    "{}- '{}'",
                    " ".repeat(indent + 2),
                    path.replace('\'', "''")
                ),
            );
        } else if (flutter + 1..end)
            .any(|i| lines[i].trim_start().starts_with("assets:"))
        {
            return Err(
                "Convert flutter.assets to a YAML block list before importing."
                    .into(),
            );
        } else {
            lines.insert(
                end,
                format!("  assets:\n    - '{}'", path.replace('\'', "''")),
            );
        }
    } else if doc.get("flutter").is_none() {
        lines.push(format!(
            "flutter:\n  assets:\n    - '{}'",
            path.replace('\'', "''")
        ));
    } else {
        return Err(
            "Convert flutter to a YAML block mapping before importing.".into()
        );
    }
    let text = lines.join("\n") + "\n";
    serde_yaml::from_str::<serde_yaml::Value>(&text).map_err(|e| e.to_string())?;
    Ok(text)
}
pub fn import_asset(
    root: &Path,
    source: &Path,
    destination: &Path,
) -> Result<(), String> {
    use std::path::Component;
    if destination.is_absolute()
        || destination.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err("Choose a destination inside the project.".into());
    }
    let target = root.join(destination);
    if target.exists() {
        return Err("A file already exists at the destination.".into());
    }
    let pubspec = root.join("pubspec.yaml");
    let original = std::fs::read_to_string(&pubspec).map_err(|e| e.to_string())?;
    let android = destination.starts_with("android")
        && destination
            .components()
            .any(|part| part.as_os_str() == "res");
    if android {
        let stem = destination
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        if stem.is_empty()
            || !stem.chars().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.'
            })
        {
            return Err("Android resource filenames must use lowercase letters, digits and underscores.".into());
        }
    }
    let updated = if android {
        None
    } else {
        Some(register_asset(
            &original,
            &destination.to_string_lossy().replace('\\', "/"),
        )?)
    };
    let parent = target.parent().ok_or("Invalid destination")?;
    let canonical_root = root.canonicalize().map_err(|e| e.to_string())?;
    let existing = parent
        .ancestors()
        .find(|path| path.exists())
        .ok_or("Invalid destination")?;
    if !existing
        .canonicalize()
        .map_err(|e| e.to_string())?
        .starts_with(&canonical_root)
    {
        return Err("The destination resolves outside the project.".into());
    }
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    if !parent
        .canonicalize()
        .map_err(|e| e.to_string())?
        .starts_with(root.canonicalize().map_err(|e| e.to_string())?)
    {
        return Err("The destination resolves outside the project.".into());
    }
    let temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    std::fs::copy(source, temporary.path()).map_err(|e| e.to_string())?;
    temporary
        .persist_noclobber(&target)
        .map_err(|e| e.to_string())?;
    let Some(updated) = updated else {
        return Ok(());
    };
    if std::fs::read_to_string(&pubspec).ok().as_deref() != Some(original.as_str()) {
        let _ = std::fs::remove_file(&target);
        return Err(
            "pubspec.yaml changed during import. Retry using the current manifest."
                .into(),
        );
    }
    if let Err(error) = crate::persistence::write_files(vec![(pubspec, updated)]) {
        let _ = std::fs::remove_file(target);
        return Err(error);
    }
    Ok(())
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

pub fn panel(window: Rc<WindowTabData>) -> impl View {
    let config = window.common.config;
    let root = window
        .workspace
        .path
        .clone()
        .filter(|_| window.workspace.kind.is_local());
    let inventory = create_rw_signal(Inventory::default());
    let refresh = create_rw_signal(0u64);
    let generation = create_rw_signal(0u64);
    let busy = create_rw_signal(false);
    let query = create_rw_signal(String::new());
    let module = create_rw_signal(String::new());
    let kind = create_rw_signal(Kind::Asset);
    let grid = create_rw_signal(false);
    let size = create_rw_signal(48.0f32);
    let selected = create_rw_signal(None::<Resource>);
    let import_open = create_rw_signal(false);
    let create_kind = create_rw_signal(None::<Kind>);
    let source = create_rw_signal(String::new());
    let destination = create_rw_signal(String::from("assets/"));
    let error = create_rw_signal(String::new());
    let importing = create_rw_signal(false);
    let alive = Rc::new(std::cell::Cell::new(true));
    let live = alive.clone();
    let scope = Scope::current();
    let load_root = root.clone();
    let visible = window.panel.clone();
    create_effect(move |_| {
        refresh.get();
        visible.styles.get();
        visible.panels.get();
        if !visible.is_panel_visible(&PanelKind::ResourceManager)
            || busy.get_untracked()
        {
            return;
        }
        busy.set(true);
        generation.update(|n| *n += 1);
        let epoch = generation.get_untracked();
        let root = load_root.clone();
        let live = live.clone();
        let send =
            floem::ext_event::create_ext_action(scope, move |result: Inventory| {
                if !live.get() {
                    return;
                }
                busy.set(false);
                if generation.get_untracked() == epoch {
                    inventory.set(result);
                }
                let live = live.clone();
                floem::action::exec_after(Duration::from_secs(2), move |_| {
                    if live.get() {
                        refresh.update(|n| *n += 1);
                    }
                });
            });
        std::thread::spawn(move || {
            send(match root {
                Some(root) => match scan(&root) {
                    Ok(resources) => Inventory {
                        resources,
                        error: String::new(),
                    },
                    Err(error) => Inventory {
                        error,
                        ..Default::default()
                    },
                },
                None => Inventory {
                    error: "Open a local Flutter project to browse resources."
                        .into(),
                    ..Default::default()
                },
            });
        });
    });
    let modules = create_memo(move |_| {
        inventory.with(|data| {
            data.resources
                .iter()
                .map(|resource| resource.module.clone())
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
        })
    });
    let filtered = create_memo(move |_| {
        inventory.with(|data| {
            data.resources
                .iter()
                .filter(|resource| {
                    resource.kind == kind.get()
                        && (module.with(|m| m.is_empty() || *m == resource.module))
                        && resource
                            .name
                            .to_lowercase()
                            .contains(&query.get().to_lowercase())
                })
                .cloned()
                .collect::<Vec<_>>()
        })
    });
    let internal = window.common.internal_command;
    let file_explorer = window.file_explorer.clone();
    let reference = window.clone();
    let import_root = root.clone();
    stack((
        super::chrome::header(
            window.clone(),
            PanelKind::ResourceManager,
            "Resource Manager",
            empty(),
        ),
        stack((
            clickable_icon(
                || SparrowIcons::ADD,
                || {},
                || false,
                || false,
                || "Add resource",
                config,
            ).ide_popout_menu(move ||Menu::new("")
                .entry(MenuItem::new("Import Flutter asset…").action(move ||{create_kind.set(None);source.set(String::new());destination.set("assets/".into());import_open.set(true);}))
                .entry(MenuItem::new("Import Android resource…").action(move ||{create_kind.set(None);source.set(String::new());destination.set("android/app/src/main/res/drawable/".into());import_open.set(true);}))
                .entry(MenuItem::new("New color value…").action(move ||{create_kind.set(Some(Kind::Color));source.set("#FFFFFFFF".into());destination.set("accent".into());import_open.set(true);}))
                .entry(MenuItem::new("New string value…").action(move ||{create_kind.set(Some(Kind::String));source.set(String::new());destination.set("label".into());import_open.set(true);}))
            ),
            clickable_icon(
                || SparrowIcons::IMAGE_LOADING,
                move || refresh.update(|n| *n += 1),
                || false,
                move || busy.get(),
                || "Refresh resources",
                config,
            ),
            label(move || {
                format!(
                    "{} ⌄",
                    module.with(|m| if m.is_empty() {
                        "All modules".into()
                    } else {
                        m.clone()
                    })
                )
            })
            .ide_popout_menu(move || {
                let mut menu = Menu::new("").entry(
                    MenuItem::new("All modules")
                        .action(move || module.set(String::new())),
                );
                for name in modules.get_untracked() {
                    menu = menu.entry(
                        MenuItem::new(&name)
                            .action(move || module.set(name.clone())),
                    );
                }
                menu
            })
            .style(|s| s.max_width(160.0).text_ellipsis()),
            text_input(query)
                .placeholder("Search resources")
                .style(move |s| {
                    s.height(24.0)
                        .flex_grow(1.0_f32)
                        .min_width(30.0)
                        .border(1.0)
                        .border_radius(3.0)
                        .border_color(
                            config.get().color(SparrowColor::SPARROW_BORDER),
                        )
                }),
        ))
        .style(|s| {
            s.width_full()
                .height(34.0)
                .items_center()
                .gap(6.0)
                .padding_horiz(6.0)
        }),
        scroll(
            dyn_stack(
                || {
                    vec![
                        Kind::Asset,
                        Kind::Drawable,
                        Kind::Color,
                        Kind::Layout,
                        Kind::MipMap,
                        Kind::String,
                        Kind::Font,
                    ]
                },
                |k| *k,
                move |tab| {
                    label(move || tab.title())
                        .on_click_stop(move |_| kind.set(tab))
                        .style(move |s| {
                            s.height(24.0)
                                .items_center()
                                .padding_horiz(9.0)
                                .border_radius(3.0)
                                .apply_if(kind.get() == tab, |s| {
                                    s.background(config.get().color(
                                        SparrowColor::SPARROW_TAB_ACTIVE_BACKGROUND,
                                    ))
                                })
                        })
                },
            )
            .style(|s| s.items_center()),
        )
        .style(move |s| {
            s.width_full()
                .height(30.0)
                .border_bottom(1.0)
                .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
        }),
        label(move || format!("{} ({})", module.with(|m|if m.is_empty(){"Project resources".to_string()}else{m.clone()}),filtered.with(|r|r.len())))
            .style(move |s|s.width_full().height(38.0).padding_horiz(10.0).items_center().border_bottom(1.0).border_color(config.get().color(SparrowColor::SPARROW_BORDER))),
        label(move || inventory.with(|data| data.error.clone())).style(move |s| {
            s.padding(10.0)
                .width_full()
                .apply_if(inventory.with(|d| d.error.is_empty()), |s| s.hide())
        }),
        label(move || {
            if busy.get() {
                "Loading resources…"
            } else {
                "No resources match this category or search."
            }
        })
        .style(move |s| {
            s.padding(16.0)
                .width_full()
                .apply_if(!filtered.with(|r| r.is_empty()), |s| s.hide())
        }),
        scroll(
            dyn_stack(
                move || filtered.get(),
                |r| (r.module.clone(), r.kind, r.name.clone(), r.variants.clone()),
                move |resource| {
                    let preview = resource.variants.first().cloned();
                    let chosen = resource.clone();
                    let menu_resource = resource.clone();
                    let explorer = file_explorer.clone();
                    let reference = reference.clone();
                    let info = resource.clone();
                    let check = (resource.module.clone(), resource.kind, resource.name.clone());
                    let image = if let Some(variant) = preview {
                        if let Some(color) = color(&variant.value) {
                            empty()
                                .style(move |s| {
                                    s.size(size.get(), size.get())
                                        .background(color)
                                        .border(1.0)
                                        .border_color(
                                            config
                                                .get()
                                                .color(SparrowColor::SPARROW_BORDER),
                                        )
                                })
                                .into_any()
                        } else if matches!(
                            variant.path.extension().and_then(|s| s.to_str()),
                            Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp")
                        ) {
                            img_from_path(move || variant.path.clone())
                                .style(move |s| s.size(size.get(), size.get()))
                                .into_any()
                        } else if variant
                            .path
                            .extension()
                            .is_some_and(|s| s == "svg")
                        {
                            svg(move || {
                                std::fs::read_to_string(&variant.path)
                                    .unwrap_or_default()
                            })
                            .style(move |s| s.size(size.get(), size.get()))
                            .into_any()
                        } else if let Some(vector) = vector(&variant.path) {
                            svg(move || vector.clone())
                                .style(move |s| s.size(size.get(), size.get()))
                                .into_any()
                        } else {
                            svg(move || config.get().file_svg(&variant.path).0)
                                .style(move |s| s.size(size.get(), size.get()))
                                .into_any()
                        }
                    } else {
                        empty().into_any()
                    };
                    stack((
                        stack((
                            svg(|| "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"128\" height=\"128\"><defs><pattern id=\"checks\" width=\"16\" height=\"16\" patternUnits=\"userSpaceOnUse\"><rect width=\"16\" height=\"16\" fill=\"#303234\"/><path d=\"M0 0h8v8H0zM8 8h8v8H8z\" fill=\"#3b3d3f\"/></pattern></defs><rect width=\"128\" height=\"128\" fill=\"url(#checks)\"/></svg>".to_string()).style(move |s|s.absolute().size(size.get(),size.get())),
                            image,
                        )).style(move |s|s.size(size.get(),size.get()).items_center().justify_center()),
                        stack((
                            label(move || info.name.clone())
                                .style(|s| s.text_ellipsis().min_width(0.0)),
                            label(move || {
                                format!(
                                    "{} · {} · {} version{}",
                                    resource.module,
                                    resource.kind.title(),
                                    resource.variants.len(),
                                    if resource.variants.len() == 1 {
                                        ""
                                    } else {
                                        "s"
                                    }
                                )
                            })
                            .style(move |s| {
                                s.font_size(11.0).color(
                                    config.get().color(SparrowColor::EDITOR_DIM),
                                )
                            }),
                        ))
                        .style(|s| s.flex_col().min_width(0.0).flex_grow(1.0_f32)),
                    ))
                    .on_click_stop(move |_| selected.set(Some(chosen.clone())))
                    .on_double_click_stop({
                        let resource = menu_resource.clone();
                        move |_| {
                            if let Some(variant) = resource.variants.first() {
                                internal.send(InternalCommand::JumpToLocation {
                                    location: EditorLocation {
                                        path: variant.path.clone(),
                                        position: Some(EditorPosition::Position(
                                            lsp_types::Position::new(
                                                variant.line,
                                                0,
                                            ),
                                        )),
                                        scroll_offset: None,
                                        ignore_unconfirmed: false,
                                        same_editor_tab: false,
                                    },
                                });
                            }
                        }
                    })
                    .ide_context_menu(move || {
                        let variant = menu_resource.variants.first().cloned();
                        let open = variant.clone();
                        let reveal = variant.clone();
                        let rename = variant.clone();
                        let explorer = explorer.clone();
                        let renamed = explorer.clone();
                        let reference = reference.clone();let rename_window=reference.clone();let usages=reference.clone();let find_name=menu_resource.name.clone();
                        let value = if menu_resource.kind == Kind::Asset {
                            menu_resource.name.clone()
                        } else {
                            format!(
                                "@{}/{}",
                                menu_resource
                                    .kind
                                    .title()
                                    .to_lowercase()
                                    .replace(' ', "").trim_end_matches('s'),
                                menu_resource.name
                            )
                        };
                        Menu::new("")
                            .entry(MenuItem::new("Open definition").action(
                                move || {
                                    if let Some(variant) = &open {
                                        internal.send(InternalCommand::JumpToLocation {location: EditorLocation {path: variant.path.clone(),position: Some(EditorPosition::Position(lsp_types::Position::new(variant.line,0))),scroll_offset: None,ignore_unconfirmed:false,same_editor_tab:false}});
                                    }
                                },
                            ))
                            .entry(
                                MenuItem::new("Find usages").action(move ||{usages.global_search.set_pattern(find_name.clone());usages.show_panel(PanelKind::Search);}),
                            )
                            .entry(
                                MenuItem::new("Copy resource reference")
                                    .action(move || crate::menus::copy_text(&value)),
                            )
                            .entry(MenuItem::new("Reveal in Project").action(
                                move || {
                                    if let Some(variant) = &reveal {
                                        reference
                                            .show_panel(PanelKind::FileExplorer);
                                        explorer.reveal_in_file_tree(
                                            variant.path.clone(),
                                        );
                                    }
                                },
                            ))
                            .entry(
                                MenuItem::new("Rename file…")
                                    .enabled(!matches!(
                                        menu_resource.kind,
                                        Kind::String | Kind::Color
                                    ))
                                    .action(move || {
                                        if let Some(variant) = &rename {
                                            rename_window.show_panel(PanelKind::FileExplorer);
                                            renamed
                                                .rename_path(variant.path.clone());
                                        }
                                    }),
                            )
                    })
                    .style(move |s| {
                        s.padding(10.0)
                            .gap(10.0)
                            .items_center()
                            .border_bottom(1.0)
                            .border_color(
                                config.get().color(SparrowColor::SPARROW_BORDER),
                            )
                            .apply_if(grid.get(), |s| s.width(170.0).flex_col())
                            .apply_if(!grid.get(), |s| s.width_full())
                            .apply_if(
                                selected.with(|r| {
                                    r.as_ref().is_some_and(|r| (r.module.clone(),r.kind,r.name.clone()) == check)
                                }),
                                |s| {
                                    s.background(config.get().color(
                                        SparrowColor::PANEL_HOVERED_BACKGROUND,
                                    ))
                                },
                            )
                    })
                },
            )
            .style(move |s| {
                s.width_full()
                    .flex_wrap(floem::taffy::FlexWrap::Wrap)
                    .apply_if(!grid.get(), |s| s.flex_col())
            }),
        )
        .style(|s| s.flex_grow(1.0_f32).min_height(0.0).width_full()),
        dyn_stack(
            move || {
                selected
                    .get()
                    .map(|resource| resource.variants)
                    .unwrap_or_default()
            },
            |v| (v.path.clone(), v.line),
            move |variant| {
                let path = variant.path.clone();
                label(move || {
                    format!(
                        "{}  {}",
                        variant.qualifier,
                        variant
                            .path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                    )
                })
                .on_click_stop(move |_| {
                    internal.send(InternalCommand::JumpToLocation {
                        location: EditorLocation {
                            path: path.clone(),
                            position: Some(EditorPosition::Position(
                                lsp_types::Position::new(variant.line, 0),
                            )),
                            scroll_offset: None,
                            ignore_unconfirmed: false,
                            same_editor_tab: false,
                        },
                    })
                })
                .style(move |s| {
                    s.height(24.0)
                        .width_full()
                        .padding_horiz(12.0)
                        .items_center()
                        .color(config.get().color(SparrowColor::EDITOR_LINK))
                })
            },
        )
        .style(|s| s.flex_col().width_full().max_height(120.0)),
        stack((
            label(|| "List").on_click_stop(move |_| grid.set(false)),
            label(|| "Grid").on_click_stop(move |_| grid.set(true)),
            empty().style(|s| s.flex_grow(1.0_f32)),
            label(|| "−")
                .on_click_stop(move |_| size.update(|v| *v = (*v - 8.0).max(24.0))),
            label(|| "+")
                .on_click_stop(move |_| size.update(|v| *v = (*v + 8.0).min(128.0))),
        ))
        .style(move |s| {
            s.width_full()
                .height(30.0)
                .items_center()
                .gap(12.0)
                .padding_horiz(12.0)
                .border_top(1.0)
                .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
        }),
        stack((
            label(move ||match create_kind.get(){Some(Kind::Color)=>"New color resource",Some(Kind::String)=>"New string resource",_=>"Import resource"}).style(|s| s.font_bold()),
            text_input(source).placeholder("Source file path / resource value").style(move |s|s.width_full().padding(6.0).border(1.0).border_color(config.get().color(SparrowColor::SPARROW_BORDER))),
            text_input(destination).placeholder("Destination path / resource name").style(move |s|s.width_full().padding(6.0).border(1.0).border_color(config.get().color(SparrowColor::SPARROW_BORDER))),
            label(move || error.get()),
            stack((
                label(|| "Cancel").on_click_stop(move |_| import_open.set(false)),
                label(move ||if create_kind.get().is_some(){"Create"}else{"Import"}).on_click_stop(move |_| {
                    let Some(root) = import_root.clone() else {
                        return;
                    };
                    let dirty = window.main_split.docs.with_untracked(|docs| {
                        docs.get(&root.join("pubspec.yaml"))
                            .is_some_and(|doc| !doc.is_pristine())
                    });
                    if dirty {
                        error.set(
                            "Save pubspec.yaml before importing an asset.".into(),
                        );
                        return;
                    }
                    if let Some(kind)=create_kind.get_untracked() {
                        let module=module.get_untracked();let module=if module.starts_with("android/"){module}else{"android/app".into()};
                        let target=root.join(module).join("src/main/res/values").join(if kind==Kind::Color{"colors.xml"}else{"strings.xml"});
                        if window.main_split.docs.with_untracked(|docs|docs.get(&target).is_some_and(|doc|!doc.is_pristine())){error.set("Save the resource definition before adding another value.".into());return;}
                    }
                    if importing.get_untracked() {
                        return;
                    }
                    importing.set(true);
                    error.set("Importing…".into());
                    let source = source.get_untracked();
                    let destination = destination.get_untracked();
                    let create=create_kind.get_untracked();let selected_module=module.get_untracked();
                    let send = floem::ext_event::create_ext_action(
                        scope,
                        move |result: Result<(), String>| {
                            importing.set(false);
                            match result {
                                Ok(()) => {
                                    import_open.set(false);
                                    error.set(String::new());
                                    refresh.update(|n| *n += 1);
                                }
                                Err(message) => error.set(message),
                            }
                        },
                    );
                    std::thread::spawn(move || {
                        send(if let Some(kind)=create{create_value(&root,&selected_module,kind,&destination,&source)}else{import_asset(&root,Path::new(&source),Path::new(&destination))})
                    });
                }),
            ))
            .style(|s| s.gap(16.0)),
        ))
        .style(move |s| {
            s.absolute()
                .inset_left(12.0)
                .inset_right(12.0)
                .inset_top(100.0)
                .padding(16.0)
                .gap(12.0)
                .flex_col()
                .border(1.0)
                .border_radius(5.0)
                .background(config.get().color(SparrowColor::PANEL_BACKGROUND))
                .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
                .z_index(50)
                .apply_if(!import_open.get(), |s| s.hide())
        }),
    ))
    .on_cleanup(move || alive.set(false))
    .style(|s| s.absolute().size_full().flex_col().font_size(13.0))
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
        assert!(svg.contains("rx=\"4\""));
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
