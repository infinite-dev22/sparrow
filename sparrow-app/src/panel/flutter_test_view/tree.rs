//! Test suites and nested test results with collapse and selection.

use crate::{
    config::color::SparrowColor,
    dart::test_runner::{TestRun, TestStatus},
    window_tab::WindowTabData,
};
use floem::{
    View,
    reactive::{SignalGet, SignalUpdate, SignalWith},
    views::{Decorators, dyn_stack, label, scroll, stack},
};
use std::collections::BTreeSet;

use floem::reactive::RwSignal;

pub(super) fn test_tree(
    data: &WindowTabData,
    collapsed: RwSignal<BTreeSet<String>>,
) -> impl View + use<> {
    let run = data.flutter_test.run;
    let selected = data.flutter_test.selected;
    let config = data.common.config;
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
                        a.duration(std::time::Duration::from_secs(u64::from(
                            status == Some(TestStatus::Running),
                        )))
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
                            Some(TestStatus::Failed) => SparrowColor::SPARROW_ERROR,
                            Some(TestStatus::Passed) => {
                                SparrowColor::SOURCE_CONTROL_ADDED
                            }
                            _ => SparrowColor::PANEL_FOREGROUND,
                        }))
                        .apply_if(test.is_some() && selected.get() == test, |s| {
                            s.background(
                                c.color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                            )
                        })
                        .hover(|s| {
                            s.background(
                                c.color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                            )
                        })
                })
            },
        )
        .style(|s| s.flex_col().min_width_full()),
    )
    .style(|s| s.width_pct(45.0).height_full())
}

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
}
