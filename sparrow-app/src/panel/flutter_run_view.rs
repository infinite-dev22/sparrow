//! Run sessions, project task output, and Flutter run actions.

use std::rc::Rc;

use floem::{
    IntoView, View,
    event::EventListener,
    reactive::{SignalGet, SignalUpdate, SignalWith},
    style::CursorStyle,
    views::{Decorators, dyn_container, dyn_stack, empty, label, stack},
};

use super::{kind::PanelKind, position::PanelPosition};
use crate::{
    command::InternalCommand,
    config::{color::SparrowColor, icon::SparrowIcons},
    dart::FlutterRunState,
    debug::RunDebugMode,
    editor::location::{EditorLocation, EditorPosition},
    widgets::{clickable_icon, tab_strip},
    window_tab::{Focus, WindowTabData},
};

pub fn flutter_run_panel(
    window: Rc<WindowTabData>,
    _position: PanelPosition,
) -> impl View {
    let config = window.common.config;
    let terminal = window.terminal.clone();
    let active = terminal.active_run;
    let focus = window.common.focus;

    let sessions = terminal.clone();
    let tab_terminal = terminal.clone();
    let tabs = tab_strip(|strip| {
        dyn_stack(
            move || {
                sessions
                    .run_debug_process(true)
                    .into_iter()
                    .filter(|(_, process)| process.mode == RunDebugMode::Run)
                    .collect::<Vec<_>>()
            },
            |(id, process)| (*id, process.config.name.clone(), process.stopped),
            move |(id, process)| {
                let close = tab_terminal.clone();
                strip.button(
                    stack((
                        label(move || process.config.name.clone())
                            .style(|s| s.max_width(220.0).text_ellipsis()),
                        crate::widgets::tab_close_button(
                        move || close.close_run_debug_session(id),
                        "Close run session",
                        config,
                    )
                        .style(|s| s.size(18.0, 18.0).padding(2.0).margin_left(6.0)),
                    ))
                    .style(|s| s.items_center()),
                    move || active.get() == Some(id),
                    move || active.set(Some(id)),
                    config,
                )
            },
        )
        .style(|s| s.items_center().height_full())
    })
    .style(|s| s.flex_grow(1.0_f32).min_width(0.0));

    let header = super::chrome::header(window.clone(), PanelKind::FlutterRun, "Run", tabs);

    let actions = terminal.clone();
    let monitor = terminal.clone();
    let hot_reload = window.flutter_hot_reload.clone();
    let flutter_session = hot_reload.active_term;
    let flutter_state = hot_reload.state;
    let progress = hot_reload.progress_message;
    let logs = hot_reload.run_logs;
    let controls = dyn_container(
        move || {
            let id = active.get()?;
            let process = monitor.console_terminal(id, true)?.run_debug.get()?;
            Some((
                id,
                process.stopped,
                crate::dart::run_config::is_flutter_run(&process.config),
            ))
        },
        move |session| {
            let Some((id, stopped, flutter)) = session else {
                return empty().into_any();
            };
            let rerun = actions.clone();
            let stop = actions.clone();
            let clear = actions.clone();
            let tools = window.clone();
            let reload = window.clone();
            let restart = window.clone();
            let can_reload = move || {
                !stopped
                    && flutter_session.get() == Some(id)
                    && flutter_state.get() == FlutterRunState::Running
            };
            stack((
                clickable_icon(
                    || SparrowIcons::DEBUG_RESTART,
                    move || {
                        rerun.restart_run_debug(id);
                    },
                    || false,
                    || false,
                    || "Rerun",
                    config,
                ),
                clickable_icon(
                    || SparrowIcons::DEBUG_STOP,
                    move || {
                        stop.stop_run_debug(id);
                    },
                    || false,
                    move || stopped,
                    || "Stop",
                    config,
                ),
                clickable_icon(
                    || SparrowIcons::FLUTTER_HOT_RELOAD,
                    move || reload.reload_run_session_after_saving(id, false),
                    || false,
                    move || !can_reload(),
                    || "Hot Reload",
                    config,
                )
                .style(move |s| s.apply_if(!flutter, |s| s.hide())),
                clickable_icon(
                    || SparrowIcons::FLUTTER_HOT_RESTART,
                    move || restart.reload_run_session_after_saving(id, true),
                    || false,
                    move || !can_reload(),
                    || "Hot Restart",
                    config,
                )
                .style(move |s| s.apply_if(!flutter, |s| s.hide())),
                clickable_icon(
                    || SparrowIcons::TOOLBOX,
                    move || {
                        tools
                            .open_devtools(crate::dart::devtools::DevToolsPage::Home)
                    },
                    || false,
                    move || !can_reload(),
                    || "Open DevTools",
                    config,
                )
                .style(move |s| s.apply_if(!flutter, |s| s.hide())),
                clickable_icon(
                    || SparrowIcons::SEARCH_CLEAR,
                    move || {
                        if let Some(terminal) = clear.console_terminal(id, false) {
                            terminal.clear_buffer();
                        }
                    },
                    || false,
                    || false,
                    || "Clear Console",
                    config,
                ),
                label(move || {
                    if stopped {
                        "Stopped".to_owned()
                    } else if can_reload() {
                        progress.get().unwrap_or_else(|| "Running".to_owned())
                    } else {
                        "Running".to_owned()
                    }
                })
                .style(move |s| {
                    s.color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
                        .margin_left(8.0)
                        .min_width(0.0)
                        .text_ellipsis()
                }),
            ))
            .style(move |s| {
                s.height(26.0)
                    .items_center()
                    .width_full()
                    .border_bottom(1.0)
                    .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
            })
            .into_any()
        },
    );

    let console = dyn_container(
        move || active.get(),
        move |id| {
            let Some((id, terminal)) = id.and_then(|id| {
                terminal.console_terminal(id, false).map(|t| (id, t))
            }) else {
                return dyn_container(
                    move || logs.with(|logs| logs.is_empty()),
                    move |empty| {
                        if empty {
                            return label(
                                || "Run a project configuration to start a session.",
                            )
                            .style(|s| s.padding(12.0))
                            .into_any();
                        }
                        crate::widgets::sparrow_scroll(
                            dyn_stack(
                                move || logs.get().into_iter().enumerate(),
                                |(index, line)| (*index, line.clone()),
                                move |(_, line)| label(move || line.clone()),
                            )
                            .style(|s| s.flex_col().width_full()),
                        )
                        .style(|s| s.size_full())
                        .into_any()
                    },
                )
                .style(|s| s.size_full())
                .into_any();
            };
            crate::terminal::view::terminal_view(terminal, move || {
                focus.get() == Focus::Panel(PanelKind::FlutterRun)
                    && active.get() == Some(id)
            })
            .on_event_cont(EventListener::PointerDown, move |_| {
                focus.set(Focus::Panel(PanelKind::FlutterRun));
            })
            .style(|s| s.size_full())
            .into_any()
        },
    )
    .style(|s| {
        s.flex_grow(1.0_f32)
            .flex_basis(0.0)
            .min_height(0.0)
            .width_full()
    });

    stack((header, controls, console))
        .style(|s| s.absolute().size_full().flex_col())
        .debug_name("Run Panel")
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

    stack((label(move || display_text.clone()).style(move |s| {
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
    }),))
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
pub(super) fn parse_stack_trace_link(
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
