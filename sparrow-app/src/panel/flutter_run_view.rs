//! Dedicated Flutter Run & Console panel for logs, progress, and actions.

use std::rc::Rc;

use floem::{
    View,
    reactive::SignalGet,
    style::CursorStyle,
    views::{
        Decorators, empty, label, scroll, stack, virtual_stack,
        VirtualVector,
    },
};

use super::position::PanelPosition;
use crate::{
    app::clickable_icon,
    command::InternalCommand,
    config::{color::SparrowColor, icon::SparrowIcons},
    dart::FlutterRunState,
    editor::location::{EditorLocation, EditorPosition},
    window_tab::WindowTabData,
};

pub fn flutter_run_panel(
    window_tab_data: Rc<WindowTabData>,
    _position: PanelPosition,
) -> impl View {
    let hot_reload = window_tab_data.flutter_hot_reload.clone();
    let config = window_tab_data.common.config;
    let ui_line_height = window_tab_data.common.ui_line_height;
    let internal_command = window_tab_data.common.internal_command;
    let workspace = window_tab_data.workspace.clone();

    let logs = hot_reload.run_logs;
    let state = hot_reload.state;

    stack((
        // Header toolbar with quick actions
        stack((
            label(move || match state.get() {
                FlutterRunState::Idle => "Flutter: Idle".to_string(),
                FlutterRunState::Running => {
                    if let Some(msg) = hot_reload.progress_message.get() {
                        format!("Flutter: {msg}")
                    } else {
                        "Flutter: Running".to_string()
                    }
                }
                FlutterRunState::Error => "Flutter: Stopped / Error".to_string(),
            })
            .style(move |s| {
                let c = config.get();
                s.color(c.color(SparrowColor::PANEL_FOREGROUND_DIM))
                    .font_size(11.0)
                    .margin_left(10.0)
                    .selectable(false)
            }),
            empty().style(|s| s.flex_grow(1.0f32)),
            clickable_icon(
                || SparrowIcons::FLUTTER_HOT_RELOAD,
                {
                    let hr = hot_reload.clone();
                    move || hr.hot_reload()
                },
                || false,
                move || state.get() != FlutterRunState::Running,
                || "Hot Reload",
                config,
            ),
            clickable_icon(
                || SparrowIcons::FLUTTER_HOT_RESTART,
                {
                    let hr = hot_reload.clone();
                    move || hr.hot_restart()
                },
                || false,
                move || state.get() != FlutterRunState::Running,
                || "Hot Restart",
                config,
            ),
            clickable_icon(
                || SparrowIcons::FLUTTER_STOP,
                {
                    let hr = hot_reload.clone();
                    move || hr.stop()
                },
                || false,
                move || state.get() != FlutterRunState::Running,
                || "Stop Application",
                config,
            ),
            clickable_icon(
                || SparrowIcons::CLOSE,
                {
                    let hr = hot_reload.clone();
                    move || hr.clear_logs()
                },
                || false,
                || false,
                || "Clear Console",
                config,
            )
            .style(|s| s.margin_right(4.0)),
        ))
        .style(move |s| {
            let c = config.get();
            s.height(26.0)
                .items_center()
                .border_bottom(1.0)
                .border_color(c.color(SparrowColor::SPARROW_BORDER))
        }),

        // Log viewer with clickable file paths
        scroll(
            virtual_stack(
                move || LogList::new(logs.get()),
                move |(i, line)| (*i, line.clone()),
                move |(_, line)| {
                    log_line_row(line, config, internal_command, workspace.path.clone())
                },
            )
            .item_size_fixed(move || ui_line_height.get())
            .style(|s| s.absolute().flex_col().min_width_full()),
        )
        .style(|s| s.absolute().size_full()),
    ))
    .style(|s| s.absolute().size_full().flex_col())
    .debug_name("Flutter Run Panel")
}

fn log_line_row(
    line: String,
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::SparrowConfig>>,
    internal_command: crate::listener::Listener<InternalCommand>,
    workspace_path: Option<std::path::PathBuf>,
) -> impl View {
    // Check if line contains a stack trace like `lib/main.dart:42:10` or `package:...`
    let link_target = parse_stack_trace_link(&line, workspace_path.as_deref());
    let has_link = link_target.is_some();

    let display_text = line.clone();
    let is_err = line.starts_with('⚠') || line.starts_with('❌');

    stack((
        label(move || display_text.clone()).style(move |s| {
            let c = config.get();
            let base_color = if is_err {
                c.color(SparrowColor::SPARROW_ERROR)
            } else {
                c.color(SparrowColor::EDITOR_FOREGROUND)
            };
            s.selectable(true)
                .color(base_color)
                .min_width(0.0)
                .font_family(c.terminal_font_family().to_string())
                .apply_if(has_link, |s| {
                    s.hover(|s| {
                        s.cursor(CursorStyle::Pointer)
                            .color(c.color(SparrowColor::EDITOR_LINK))
                    })
                })
        }),
    ))
    .on_click_stop(move |_| {
        if let Some(loc) = &link_target {
            internal_command.send(InternalCommand::JumpToLocation {
                location: loc.clone(),
            });
        }
    })
    .style(|s| s.padding_left(8.0).padding_right(8.0).items_center())
}

/// Helper to parse paths from flutter error logs: e.g. "package:flutter/... or lib/main.dart:12:5"
fn parse_stack_trace_link(
    line: &str,
    workspace_root: Option<&std::path::Path>,
) -> Option<EditorLocation> {
    let ws = workspace_root?;
    // Match pattern: `path/to/file.dart:line:col` or `path/to/file.dart:line`
    for word in line.split_whitespace() {
        let trimmed = word.trim_matches(|c| c == '(' || c == ')' || c == '[' || c == ']');
        if trimmed.contains(".dart:") {
            let parts: Vec<&str> = trimmed.split(':').collect();
            if parts.len() >= 2 {
                let file_part = parts[0];
                let line_num: u32 = parts[1].parse().ok()?;
                let col_num: u32 = parts.get(2).and_then(|c| c.parse().ok()).unwrap_or(1);

                let full_path = if std::path::Path::new(file_part).is_absolute() {
                    std::path::PathBuf::from(file_part)
                } else {
                    ws.join(file_part)
                };

                if full_path.exists() {
                    return Some(EditorLocation {
                        path: full_path,
                        position: Some(EditorPosition::Position(lsp_types::Position::new(
                            line_num.saturating_sub(1),
                            col_num.saturating_sub(1),
                        ))),
                        scroll_offset: None,
                        ignore_unconfirmed: false,
                        same_editor_tab: false,
                    });
                }
            }
        }
    }
    None
}

struct LogList {
    items: Vec<(usize, String)>,
}

impl LogList {
    fn new(logs: im::Vector<String>) -> Self {
        Self {
            items: logs.into_iter().enumerate().collect(),
        }
    }
}

impl VirtualVector<(usize, String)> for LogList {
    fn total_len(&self) -> usize {
        self.items.len()
    }

    fn slice(&mut self, range: std::ops::Range<usize>) -> impl Iterator<Item = (usize, String)> {
        let start = range.start.min(self.items.len());
        let end = range.end.min(self.items.len());
        self.items[start..end].iter().cloned()
    }
}
