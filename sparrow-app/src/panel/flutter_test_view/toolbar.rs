//! Test progress summary and run, stop and debug actions.

use crate::{
    command::InternalCommand, config::icon::SparrowIcons,
    dart::test_runner::TestStatus, widgets::clickable_icon,
    window_tab::WindowTabData,
};
use floem::{
    View,
    reactive::{SignalGet, SignalUpdate, SignalWith},
    views::{Decorators, empty, label, stack},
};
use std::rc::Rc;

pub(super) fn test_toolbar(data: Rc<WindowTabData>) -> impl View {
    let tests = data.flutter_test.clone();
    let run = tests.run;
    let selected = tests.selected;
    let config = data.common.config;
    let internal = data.common.internal_command;
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
                internal
                    .send(InternalCommand::FlutterRerunTests { failed_only: false })
            },
            || false,
            move || run.with(|r| r.running) || targets.with(|t| t.is_empty()),
            || "Run Again",
            config,
        ),
        clickable_icon(
            || SparrowIcons::DEBUG_RESTART,
            move || {
                internal
                    .send(InternalCommand::FlutterRerunTests { failed_only: true })
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
    .style(|s| s.height(28.0).items_center())
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
