//! Formatted Logcat row with source navigation and marker styling.

use crate::panel::logcat::model::Entry;
use crate::widgets::popup_menu::{Menu, MenuItem, PopupMenuExt};
use floem::{
    View,
    peniko::Color,
    reactive::SignalGet,
    views::{Decorators, label, stack},
};
use std::sync::Arc;

#[derive(Clone, PartialEq)]
pub(super) struct Row {
    pub(super) entry: Arc<Entry>,
    pub(super) index: usize,
    pub(super) message: String,
    pub(super) format: crate::panel::logcat::format::Formatting,
    pub(super) wrap: bool,
}
pub(super) fn wrap_message(message: &str, columns: usize) -> Vec<String> {
    let mut out = Vec::new();
    for line in message.lines() {
        let mut part = String::new();
        let mut width = 0;
        for ch in line.chars() {
            let size = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if width + size > columns && !part.is_empty() {
                out.push(std::mem::take(&mut part));
                width = 0;
            }
            part.push(ch);
            width += size;
        }
        out.push(part);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}
pub(super) fn log_row(
    row: Row,
    window: std::rc::Rc<crate::window_tab::WindowTabData>,
) -> impl View {
    let config = window.common.config;
    let e = row.entry;
    let system = e.pid == 0 || e.marker;
    let format = row.format;
    let height = format.height() as f32;
    let size = format.font_size as f32;
    let level = e.level;
    let color = match level {
        'V' => Color::from_rgb8(160, 160, 160),
        'D' => Color::from_rgb8(0, 170, 190),
        'I' => Color::from_rgb8(205, 205, 0),
        'W' => Color::from_rgb8(225, 210, 0),
        'E' | 'F' | 'A' => Color::from_rgb8(255, 90, 95),
        _ => Color::from_rgb8(180, 180, 180),
    };
    let text = |value: String, width: f32| {
        label(move || value.clone()).style(move |s| {
            s.width(width)
                .min_width(0.0)
                .height(height)
                .items_center()
                .text_ellipsis()
                .flex_shrink(0.0_f32)
        })
    };
    let first = row.index == 0;
    let timestamp = if first {
        format.time_text(&e.timestamp)
    } else {
        String::new()
    };
    let ids = if first && e.pid > 0 {
        match (format.pid, format.tid) {
            (true, true) => format!("{}-{}", e.pid, e.tid),
            (true, false) => e.pid.to_string(),
            (false, true) => e.tid.to_string(),
            _ => String::new(),
        }
    } else {
        String::new()
    };
    let tag = if first { e.tag.clone() } else { String::new() };
    let package = if first && e.pid > 0 {
        if e.package.is_empty() {
            format!("pid-{}", e.pid)
        } else {
            e.package.clone()
        }
    } else {
        String::new()
    };
    let process = if first && e.pid > 0 {
        e.process.clone()
    } else {
        String::new()
    };
    let tag_color = tag_color(&e.tag);
    let message = row.message;
    let source =
        crate::panel::logcat::source::java_location(&message, window.workspace.path.as_deref());
    let jump_source = source.clone();
    let command = window.common.internal_command;
    let raw = e.raw.clone();

    stack((
        text(timestamp, if format.date { 200.0 } else { 105.0 })
            .style(move |s| s.apply_if(!format.timestamp || system, |s| s.hide())),
        text(ids, 88.0).style(move |s| {
            s.apply_if((!format.pid && !format.tid) || system, |s| s.hide())
        }),
        text(tag, format.tag_width as f32).style(move |s| {
            s.color(tag_color)
                .apply_if(system || !format.tag, |s| s.hide())
        }),
        text(package, format.package_width as f32)
            .style(move |s| s.apply_if(!format.package || system, |s| s.hide())),
        text(process, format.package_width as f32)
            .style(move |s| s.apply_if(!format.process || system, |s| s.hide())),
        text(
            if first && level != '\0' {
                level.to_string()
            } else {
                String::new()
            },
            24.0,
        )
        .style(move |s| {
            s.apply_if(system || !format.level, |s| s.hide())
                .justify_center()
                .background(if first {
                    color.multiply_alpha(0.55)
                } else {
                    Color::TRANSPARENT
                })
        }),
        crate::panel::debug_console::row(message, window.clone()).style(move |s| {
            s.height(height)
                .items_center()
                .color(color)
                .selectable(true)
                .apply_if(row.wrap, |s| s.flex_grow(1.0_f32).min_width(0.0))
        }),
    ))
    .on_double_click_stop(move |_| {
        if let Some(location) = jump_source.clone() {
            command
                .send(crate::command::InternalCommand::JumpToLocation { location });
        }
    })
    .ide_context_menu(move || {
        let raw = raw.clone();
        let mut menu = Menu::new("Log entry").entry(
            MenuItem::new("Copy entry")
                .action(move || crate::menus::copy_text(&raw)),
        );
        if let Some(location) = source.clone() {
            menu = menu.entry(MenuItem::new("Go to source").action(move || {
                command.send(crate::command::InternalCommand::JumpToLocation {
                    location: location.clone(),
                })
            }));
        }
        menu
    })
    .style(move |s| {
        s.height(height)
            .min_width_full()
            .items_center()
            .padding_left(10.0)
            .font_family(config.get().terminal.font_family.clone())
            .font_size(size)
    })
}
fn tag_color(tag: &str) -> Color {
    const COLORS: [(u8, u8, u8); 7] = [
        (0, 190, 190),
        (120, 180, 255),
        (210, 170, 230),
        (210, 195, 80),
        (155, 205, 170),
        (200, 170, 150),
        (140, 180, 215),
    ];
    let hash = tag
        .bytes()
        .fold(0usize, |h, b| h.wrapping_mul(31).wrapping_add(b as usize));
    let (r, g, b) = COLORS[hash % COLORS.len()];
    Color::from_rgb8(r, g, b)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_wrapping_preserves_text() {
        assert_eq!(wrap_message("abc世界", 4), vec!["abc", "世界"]);
        assert_eq!(wrap_message("", 10), vec![""]);
    }
}
