//! Test tree and assertion details, backed by the machine-readable test stream.
use super::position::PanelPosition;
use crate::{
    app::clickable_icon,
    command::InternalCommand,
    config::{color::SparrowColor, icon::SparrowIcons},
    dart::test_runner::{TestRun, TestStatus},
    window_tab::WindowTabData,
};
use floem::{
    View,
    reactive::{SignalGet, SignalUpdate, SignalWith, create_rw_signal},
    views::{Decorators, dyn_stack, empty, label, scroll, stack},
};
use std::{collections::BTreeSet, rc::Rc};

#[derive(Clone)]
struct Row {
    key: String,
    text: String,
    depth: usize,
    test: Option<i64>,
    status: Option<TestStatus>,
}
fn rows(run: &TestRun, collapsed: &BTreeSet<String>) -> Vec<Row> {
    let mut result = Vec::new();
    let suites = run
        .suites
        .keys()
        .copied()
        .chain(run.tests.values().map(|t| t.suite))
        .collect::<BTreeSet<_>>();
    for suite in suites {
        let key = format!("s{suite}");
        let closed = collapsed.contains(&key);
        result.push(Row {
            key,
            text: format!(
                "{} {}",
                if closed { "▸" } else { "▾" },
                run.suites
                    .get(&suite)
                    .map(String::as_str)
                    .unwrap_or("Loading tests")
            ),
            depth: 0,
            test: None,
            status: None,
        });
        if closed {
            continue;
        }
        group_rows(run, collapsed, suite, None, 1, &mut result);
    }
    result
}
fn group_rows(
    run: &TestRun,
    collapsed: &BTreeSet<String>,
    suite: i64,
    parent: Option<i64>,
    depth: usize,
    rows: &mut Vec<Row>,
) {
    if depth > 100 {
        return;
    }
    for (id, group) in run
        .groups
        .iter()
        .filter(|(_, g)| g.suite == suite && g.parent == parent)
    {
        let visible = !group.name.is_empty();
        let key = format!("g{id}");
        let closed = collapsed.contains(&key);
        if visible {
            rows.push(Row {
                key,
                text: format!("{} {}", if closed { "▸" } else { "▾" }, group.name),
                depth,
                test: None,
                status: None,
            });
        }
        if !closed {
            group_rows(
                run,
                collapsed,
                suite,
                Some(*id),
                depth + usize::from(visible),
                rows,
            );
        }
    }
    for t in run.tests.values().filter(|t| {
        t.suite == suite && !t.hidden && t.groups.last().copied() == parent
    }) {
        rows.push(Row {
            key: format!("t{}", t.id),
            text: format!(
                "{}{}",
                t.name,
                t.duration
                    .map(|ms| format!("  ({ms} ms)"))
                    .unwrap_or_default()
            ),
            depth,
            test: Some(t.id),
            status: Some(t.status),
        });
    }
}

pub fn flutter_test_panel(
    data: Rc<WindowTabData>,
    _position: PanelPosition,
) -> impl View {
    let tests = data.flutter_test.clone();
    let run = tests.run;
    let selected = tests.selected;
    let collapsed = create_rw_signal(BTreeSet::<String>::new());
    let config = data.common.config;
    let internal = data.common.internal_command;
    let workspace = data.workspace.path.clone();
    let targets = tests.last_targets;
    let controls = stack((
        debug_control(
            data.clone(),
            SparrowIcons::DEBUG_CONTINUE,
            "Resume Tests",
            0,
        ),
        debug_control(data.clone(), SparrowIcons::DEBUG_STEP_OVER, "Step Over", 1),
        debug_control(data.clone(), SparrowIcons::DEBUG_STEP_INTO, "Step Into", 2),
        debug_control(data.clone(), SparrowIcons::DEBUG_STEP_OUT, "Step Out", 3),
    ));
    stack((
        stack((
            label(move || {
                run.with(|r| {
                    let mut passed = 0;
                    let mut failed = 0;
                    let mut skipped = 0;
                    for t in r.tests.values().filter(|t| !t.hidden) {
                        match t.status {
                            TestStatus::Passed => passed += 1,
                            TestStatus::Failed => failed += 1,
                            TestStatus::Skipped => skipped += 1,
                            _ => {}
                        }
                    }
                    format!(
                        "{}  {passed} passed · {failed} failed · {skipped} skipped",
                        if r.running {
                            "◌ Running"
                        } else if r.success == Some(false) {
                            "✗ Failed / stopped"
                        } else if r.success == Some(true) {
                            "✓ Finished"
                        } else {
                            "Flutter Test"
                        }
                    )
                })
            })
            .style(|s| s.margin_left(8.0).font_size(11.0)),
            empty().style(|s| s.flex_grow(1.0_f32)),
            controls,
            clickable_icon(
                || SparrowIcons::DEBUG_CONSOLE,
                move || selected.set(None),
                || false,
                || false,
                || "Show Full Console",
                config,
            ),
            clickable_icon(
                || SparrowIcons::START,
                move || {
                    internal.send(InternalCommand::FlutterRerunTests {
                        failed_only: false,
                    })
                },
                || false,
                move || run.with(|r| r.running) || targets.with(|t| t.is_empty()),
                || "Run Again",
                config,
            ),
            clickable_icon(
                || SparrowIcons::DEBUG_RESTART,
                move || {
                    internal.send(InternalCommand::FlutterRerunTests {
                        failed_only: true,
                    })
                },
                || false,
                move || run.with(|r| r.running || r.failed_targets().is_empty()),
                || "Re-run Failed Tests",
                config,
            ),
            clickable_icon(
                || SparrowIcons::FLUTTER_STOP,
                move || tests.stop(),
                || false,
                move || !run.with(|r| r.running),
                || "Stop Tests",
                config,
            ),
        ))
        .style(|s| s.height(28.0).items_center()),
        stack((
            scroll(
                dyn_stack(
                    move || run.with(|r| rows(r, &collapsed.get())),
                    |r| (r.key.clone(), r.text.clone()),
                    move |row| {
                        let key = row.key;
                        let test = row.test;
                        let status = row.status;
                        stack((
                            label(move || {
                                status.map(TestStatus::symbol).unwrap_or_default()
                            })
                            .animation(move |a| {
                                a.duration(std::time::Duration::from_secs(
                                    u64::from(status == Some(TestStatus::Running)),
                                ))
                                .keyframe(0, |f| f.style(|s| s.rotate(0.0)))
                                .keyframe(100, |f| f.style(|s| s.rotate(360.0)))
                                .repeat(status == Some(TestStatus::Running))
                            }),
                            label(move || row.text.clone()),
                        ))
                        .on_click_stop(move |_| {
                            if let Some(id) = test {
                                selected.set(Some(id));
                            } else {
                                collapsed.update(|set| {
                                    if !set.remove(&key) {
                                        set.insert(key.clone());
                                    }
                                });
                            }
                        })
                        .style(move |s| {
                            let c = config.get();
                            s.items_center()
                                .gap(4.0)
                                .padding_left((row.depth * 14 + 6) as f32)
                                .padding_vert(3.0)
                                .min_width_full()
                                .color(c.color(match status {
                                    Some(TestStatus::Failed) => {
                                        SparrowColor::SPARROW_ERROR
                                    }
                                    Some(TestStatus::Passed) => {
                                        SparrowColor::SOURCE_CONTROL_ADDED
                                    }
                                    _ => SparrowColor::PANEL_FOREGROUND,
                                }))
                                .apply_if(
                                    test.is_some() && selected.get() == test,
                                    |s| {
                                        s.background(c.color(
                                            SparrowColor::PANEL_HOVERED_BACKGROUND,
                                        ))
                                    },
                                )
                                .hover(|s| {
                                    s.background(c.color(
                                        SparrowColor::PANEL_HOVERED_BACKGROUND,
                                    ))
                                })
                        })
                    },
                )
                .style(|s| s.flex_col().min_width_full()),
            )
            .style(|s| s.width_pct(45.0).height_full()),
            scroll(
                stack((
                    label(move || {
                        selected
                            .get()
                            .and_then(|id| {
                                run.with(|r| {
                                    r.tests.get(&id).map(|t| t.name.clone())
                                })
                            })
                            .unwrap_or_else(|| {
                                "Console — select a test for assertion details"
                                    .into()
                            })
                    })
                    .style(|s| s.padding(6.0).font_bold()),
                    stack((
                        label(move || {
                            failure_comparison(&run.get(), selected.get())
                                .map(|(expected, _)| format!("Expected\n{expected}"))
                                .unwrap_or_default()
                        })
                        .style(move |s| {
                            s.width_pct(50.0).padding(8.0).selectable(true).color(
                                config
                                    .get()
                                    .color(SparrowColor::SOURCE_CONTROL_ADDED),
                            )
                        }),
                        label(move || {
                            failure_comparison(&run.get(), selected.get())
                                .map(|(_, actual)| format!("Actual\n{actual}"))
                                .unwrap_or_default()
                        })
                        .style(move |s| {
                            s.width_pct(50.0).padding(8.0).selectable(true).color(
                                config.get().color(SparrowColor::SPARROW_ERROR),
                            )
                        }),
                    ))
                    .style(move |s| {
                        s.width_full().items_start().apply_if(
                            failure_comparison(&run.get(), selected.get()).is_none(),
                            |s| s.hide(),
                        )
                    }),
                    dyn_stack(
                        move || {
                            let id = selected.get();
                            run.with(|r| {
                                id.and_then(|id| r.tests.get(&id))
                                    .map(|t| t.output.clone())
                                    .unwrap_or_else(|| r.console.clone())
                            })
                            .into_iter()
                            .enumerate()
                            .collect::<Vec<_>>()
                        },
                        |(i, line)| (*i, line.clone()),
                        move |(_, line)| {
                            let root = targets
                                .with_untracked(|t| {
                                    t.first().and_then(|(path, _)| {
                                        crate::dart::test_runner::project_root(path)
                                    })
                                })
                                .or_else(|| workspace.clone());
                            super::flutter_run_view::log_line_row(
                                line, config, internal, root,
                            )
                        },
                    )
                    .style(|s| s.flex_col().min_width_full()),
                ))
                .style(|s| s.flex_col().min_width_full()),
            )
            .style(|s| s.flex_grow(1.0_f32).min_width(0.0).height_full()),
        ))
        .style(|s| s.flex_grow(1.0_f32).min_height(0.0).width_full()),
    ))
    .style(|s| s.size_full().flex_col())
    .debug_name("Flutter Test Panel")
}

fn debug_control(
    data: Rc<WindowTabData>,
    icon: &'static str,
    title: &'static str,
    action: u8,
) -> impl View {
    let id = data.flutter_test.debug_dap;
    let daps = data.terminal.debug.daps;
    let proxy = data.common.proxy.clone();
    let thread = move || {
        id.get().and_then(|id| {
            daps.with(|daps| daps.get(&id).cloned())
                .filter(|dap| dap.stopped.get())
                .and_then(|dap| dap.thread_id.get())
                .map(|thread| (id, thread))
        })
    };
    clickable_icon(
        move || icon,
        move || {
            if let Some((id, thread)) = thread() {
                match action {
                    0 => proxy.dap_continue(id, thread),
                    1 => proxy.dap_step_over(id, thread),
                    2 => proxy.dap_step_into(id, thread),
                    _ => proxy.dap_step_out(id, thread),
                }
            }
        },
        || false,
        move || thread().is_none(),
        move || title,
        data.common.config,
    )
    .style(move |s| s.apply_if(id.get().is_none(), |s| s.hide()))
}

fn failure_comparison(
    run: &TestRun,
    selected: Option<i64>,
) -> Option<(String, String)> {
    let test = run.tests.get(&selected?)?;
    let mut expected = String::new();
    let mut actual = String::new();
    let mut section = 0;
    for line in &test.output {
        let trimmed = line.trim_start();
        if let Some(value) = trimmed.strip_prefix("Expected:") {
            expected = value.trim_start().into();
            section = 1;
        } else if let Some(value) = trimmed.strip_prefix("Actual:") {
            actual = value.trim_start().into();
            section = 2;
        } else if trimmed.starts_with("Which:") {
            break;
        } else if trimmed.is_empty() {
            section = 0;
        } else if section > 0 {
            let value = if section == 1 {
                &mut expected
            } else {
                &mut actual
            };
            value.push('\n');
            value.push_str(line);
        }
    }
    if expected.is_empty() && actual.is_empty() {
        None
    } else {
        Some((expected, actual))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tree_preserves_hierarchy_and_collapse() {
        let mut run = TestRun::default();
        for line in [
            r#"{"type":"suite","suite":{"id":1,"path":"test/a_test.dart"}}"#,
            r#"{"type":"group","group":{"id":10,"suiteID":1,"parentID":null,"name":null}}"#,
            r#"{"type":"group","group":{"id":11,"suiteID":1,"parentID":10,"name":"outer"}}"#,
            r#"{"type":"group","group":{"id":12,"suiteID":1,"parentID":11,"name":"outer inner"}}"#,
            r#"{"type":"testStart","test":{"id":2,"suiteID":1,"name":"outer inner test","groupIDs":[10,11,12]}}"#,
        ] {
            run.line(line);
        }
        let expanded = rows(&run, &BTreeSet::new());
        assert_eq!(
            expanded.iter().map(|r| r.depth).collect::<Vec<_>>(),
            [0, 1, 2, 3]
        );
        assert_eq!(expanded.last().unwrap().test, Some(2));
        assert_eq!(rows(&run, &BTreeSet::from(["g11".into()])).len(), 2);
        assert_eq!(rows(&run, &BTreeSet::from(["s1".into()])).len(), 1);
    }
    #[test]
    fn comparison_keeps_multiline_matcher_values() {
        let mut run = TestRun::default();
        run.line(r#"{"type":"testStart","test":{"id":1,"name":"fails"}}"#);
        run.line(r#"{"type":"error","testID":1,"error":"Expected: [1,\n  2]\n  Actual: [1,\n  3]\n   Which: mismatch","stackTrace":""}"#);
        assert_eq!(
            failure_comparison(&run, Some(1)),
            Some(("[1,\n  2]".into(), "[1,\n  3]".into()))
        );
    }
}
