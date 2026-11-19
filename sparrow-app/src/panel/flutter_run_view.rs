//! Dedicated Flutter Run & Console panel for logs, progress, and actions.

use std::rc::Rc;

use floem::{
    View, IntoView,
    reactive::{SignalGet, SignalUpdate},
    style::CursorStyle,
    views::{
        Decorators, dyn_container, empty, label, stack,
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
    let devtools_window = window_tab_data.clone();
    let hot_reload = window_tab_data.flutter_hot_reload.clone();
    let config = window_tab_data.common.config;

    let state = hot_reload.state;

    let run_name = window_tab_data.run_configs;
    let run_index = window_tab_data.active_run_config_idx;
    let device = hot_reload.selected_device;
    let run_sessions = window_tab_data.terminal.clone();
    let console_window = window_tab_data.clone();
    let tracked = hot_reload.active_term;
    let logs = hot_reload.run_logs;
    let console = dyn_container(move || tracked.get(), move |id| {
        let window = console_window.clone();
        if let Some(terminal) = id.and_then(|id| window.terminal.get_terminal(&id)) {
            let panel = window.terminal.clone();
            let focus = window.common.focus;
            let id = terminal.term_id;
            crate::terminal::view::terminal_view(id, terminal.raw.read_only(),
                terminal.mode.read_only(), terminal.run_debug.read_only(), panel.clone(),
                terminal.launch_error, window.common.internal_command, window.workspace.clone(), super::kind::PanelKind::FlutterRun)
                .on_event_cont(floem::event::EventListener::PointerDown, move |_| {
                    panel.focus_terminal_in_panel(id, super::kind::PanelKind::FlutterRun);
                    focus.set(crate::window_tab::Focus::Panel(super::kind::PanelKind::FlutterRun));
                }).style(|s| s.size_full()).into_any()
        } else if id.is_some() {
            floem::views::scroll(floem::views::dyn_stack(
                move || logs.get().into_iter().enumerate().collect::<Vec<_>>(),
                |(index, line)| (*index, line.clone()),
                move |(_, line)| label(move || line.clone()).style(move |s| {
                    let c = config.get();
                    s.font_family(c.terminal_font_family().to_owned())
                        .font_size(c.terminal_font_size() as f32).min_width_full()
                }),
            ).style(|s| s.flex_col().width_full()))
                .style(|s| s.size_full()).into_any()
        } else { label(|| "Run a Flutter configuration to start a session.")
            .style(|s| s.padding(12.0)).into_any() }
    }).style(|s| s.flex_grow(1.0_f32).flex_basis(0.0).min_height(0.0).width_full());
    stack((
        stack((
            label(|| "Run").style(|s| s.height_full().items_center().margin_horiz(12.0).font_bold()),
            label(move || {
                let name = tracked.get().and_then(|id| run_sessions.run_debug_process(true).into_iter()
                    .find(|(term, _)| *term == id).map(|(_, process)| process.config.name))
                    .or_else(|| run_name.get().get(run_index.get()).map(|c| c.name.clone()))
                    .unwrap_or_else(|| "Flutter".into());
                device.get().map(|d| format!("{name} ({})", d.name)).unwrap_or(name)
            }).style(move |s| s.padding_horiz(10.0).padding_vert(3.0).border_radius(4.0)
                .border(1.0).border_color(config.get().color(SparrowColor::SPARROW_BORDER))),
        )).style(|s| s.height(32.0).items_center().flex_shrink(0.0_f32)),
        // Header toolbar with quick actions
        stack((
            clickable_icon(
                || SparrowIcons::FLUTTER_HOT_RELOAD,
                {
                    let hr = hot_reload.clone();
                    move || hr.common.workbench_command.send(crate::command::SparrowWorkbenchCommand::FlutterHotReload)
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
                    move || hr.common.workbench_command.send(crate::command::SparrowWorkbenchCommand::FlutterHotRestart)
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
                || SparrowIcons::TOOLBOX,
                move || devtools_window.open_devtools(crate::dart::devtools::DevToolsPage::Home),
                || false, || false, || "Open DevTools", config,
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
            empty().style(|s| s.flex_grow(1.0_f32)),
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

        ))
        .style(move |s| {
            let c = config.get();
            s.height(26.0)
                .items_center()
                .border_bottom(1.0)
                .border_color(c.color(SparrowColor::SPARROW_BORDER))
        }),

        console,

    ))
    .style(|s| s.absolute().size_full().flex_col())
    .debug_name("Flutter Run Panel")
}

pub(super) fn log_line_row(
    line: String,
    config: floem::reactive::ReadSignal<
        std::sync::Arc<crate::config::SparrowConfig>,
    >,
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
    // Accept both VM traces (`file.dart:12:3`) and package:test traces
    // (`file.dart 12:3`). URI prefixes must survive parsing the line/column.
    static TRACE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| {
            regex::Regex::new(r"([^\s()\[\]]+\.dart)(?::|\s+)(\d+)(?::(\d+))?")
                .unwrap()
        });
    for captures in TRACE.captures_iter(line) {
        let path = &captures[1];
        let full_path = if path.starts_with("file:") {
            url::Url::parse(path)
                .ok()
                .and_then(|u| u.to_file_path().ok())
        } else if let Some(package_path) = path.strip_prefix("package:") {
            resolve_package_path(ws, package_path)
        } else if std::path::Path::new(path).is_absolute() {
            Some(std::path::PathBuf::from(path))
        } else {
            Some(ws.join(path))
        };
        let Some(full_path) = full_path.filter(|p| p.is_file()) else {
            continue;
        };
        let line_num = captures[2].parse::<u32>().ok()?;
        let col_num = captures
            .get(3)
            .and_then(|c| c.as_str().parse::<u32>().ok())
            .unwrap_or(1);
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
    None
}

fn resolve_package_path(
    root: &std::path::Path,
    package_path: &str,
) -> Option<std::path::PathBuf> {
    let (package, relative) = package_path.split_once('/')?;
    let config_path = root.join(".dart_tool/package_config.json");
    let config: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&config_path).ok()?).ok()?;
    let entry = config["packages"]
        .as_array()?
        .iter()
        .find(|p| p["name"] == package)?;
    let base = url::Url::from_file_path(config_path).ok()?;
    let root_uri = base.join(entry["rootUri"].as_str()?).ok()?;
    let uri = root_uri
        .join(entry["packageUri"].as_str().unwrap_or("lib/"))
        .ok()?
        .join(relative)
        .ok()?;
    uri.to_file_path().ok()
}

#[cfg(test)]
mod trace_tests {
    use super::*;
    #[test]
    fn parses_both_test_and_vm_traces() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("test")).unwrap();
        let file = root.path().join("test/widget_test.dart");
        std::fs::write(&file, "").unwrap();
        for trace in [
            "test/widget_test.dart 12:7 main.<fn>",
            "#0 test/widget_test.dart:12:7",
            "missing.dart:2:1 test/widget_test.dart:12:7",
        ] {
            let location = parse_stack_trace_link(trace, Some(root.path())).unwrap();
            assert_eq!(location.path, file);
            assert!(
                matches!(location.position, Some(EditorPosition::Position(p)) if p.line == 11 && p.character == 6)
            );
        }
        let uri = url::Url::from_file_path(&file).unwrap();
        assert_eq!(
            parse_stack_trace_link(&format!("({uri}:12:7)"), Some(root.path()))
                .unwrap()
                .path,
            file
        );
    }
    #[test]
    fn resolves_package_config_relative_uris() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join(".dart_tool")).unwrap();
        std::fs::create_dir_all(root.path().join("lib")).unwrap();
        let file = root.path().join("lib/main.dart");
        std::fs::write(&file, "").unwrap();
        std::fs::write(
            root.path().join(".dart_tool/package_config.json"),
            r#"{"packages":[{"name":"demo","rootUri":"../","packageUri":"lib/"}]}"#,
        )
        .unwrap();
        let loc =
            parse_stack_trace_link("package:demo/main.dart 4:2", Some(root.path()))
                .unwrap();
        assert_eq!(
            loc.path.canonicalize().unwrap(),
            file.canonicalize().unwrap()
        );
    }
}
