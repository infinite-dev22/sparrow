//! Verified platform measurements shared by the Settings layouts.
use std::sync::OnceLock;

use floem::{peniko::Color, style::Style};
use toml::Value;

fn value(group: &str, key: &str) -> &'static Value {
    static TOKENS: OnceLock<Value> = OnceLock::new();
    let tokens = TOKENS.get_or_init(|| {
        include_str!("../../../../docs/settings-tokens.toml")
            .parse()
            .expect("Settings reference tokens must be valid TOML")
    });
    let token = &tokens[group][key];
    assert_eq!(token["status"].as_str(), Some("verified"), "{group}.{key}");
    &token["value"]
}

pub(super) fn length(group: &str, key: &str) -> f64 {
    value(group, key).as_integer().expect("integer length") as f64
}

pub(super) fn proportion(group: &str, key: &str) -> f64 {
    value(group, key).as_float().expect("fraction")
}

pub(super) fn text(group: &str, key: &str) -> &'static str {
    value(group, key).as_str().expect("text token")
}

pub(super) fn size(group: &str, key: &str) -> [f64; 2] {
    let values = value(group, key).as_array().expect("size array");
    [0, 1].map(|index| values[index].as_integer().expect("integer size") as f64)
}

pub(super) fn padding(style: Style, group: &str, key: &str) -> Style {
    let values = value(group, key).as_array().expect("insets array");
    let [top, left, bottom, right] = [0, 1, 2, 3]
        .map(|index| values[index].as_integer().expect("integer inset") as f64);
    style
        .padding_top(top)
        .padding_left(left)
        .padding_bottom(bottom)
        .padding_right(right)
}

pub(super) fn color(group: &str, key: &str) -> Color {
    let hex = text(group, key).strip_prefix('#').expect("hex color");
    let rgb = u32::from_str_radix(hex, 16).expect("valid hex color");
    Color::from_rgb8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

pub(super) fn control_minimum_size(key: &str) -> [f64; 2] {
    let (width, height) = text("dark", key).split_once(',').expect("width,height");
    [width, height].map(|dimension| dimension.parse().expect("logical pixels"))
}
