//! Test console output and expected versus actual assertions.

use crate::{
    config::color::SparrowColor, dart::test_runner::TestRun,
    window_tab::WindowTabData,
};
use floem::{
    View,
    reactive::{SignalGet, SignalWith},
    views::{Decorators, dyn_stack, label, scroll, stack},
};

pub(super) fn test_output(data: &WindowTabData) -> impl View + use<> {
    let run = data.flutter_test.run;
    let selected = data.flutter_test.selected;
    let config = data.common.config;
    let internal = data.common.internal_command;
    let workspace = data.workspace.path.clone();
    let targets = data.flutter_test.last_targets;
    scroll(
        stack((
            label(move || {
                selected
                    .get()
                    .and_then(|id| {
                        run.with(|r| r.tests.get(&id).map(|t| t.name.clone()))
                    })
                    .unwrap_or_else(|| {
                        "Console — select a test for assertion details".into()
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
                        config.get().color(SparrowColor::SOURCE_CONTROL_ADDED),
                    )
                }),
                label(move || {
                    failure_comparison(&run.get(), selected.get())
                        .map(|(_, actual)| format!("Actual\n{actual}"))
                        .unwrap_or_default()
                })
                .style(move |s| {
                    s.width_pct(50.0)
                        .padding(8.0)
                        .selectable(true)
                        .color(config.get().color(SparrowColor::SPARROW_ERROR))
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
                    crate::panel::flutter_run_view::log_line_row(
                        line, config, internal, root,
                    )
                },
            )
            .style(|s| s.flex_col().min_width_full()),
        ))
        .style(|s| s.flex_col().min_width_full()),
    )
    .style(|s| s.flex_grow(1.0_f32).min_width(0.0).height_full())
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
