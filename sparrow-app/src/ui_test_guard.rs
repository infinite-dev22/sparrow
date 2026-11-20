//! Opt-in native-input guard for Wayland regression tests. Inert without the env var.
use floem::action::exec_after;
use std::{
    cell::RefCell,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

struct Guard {
    path: PathBuf,
    focused: bool,
    width: f64,
    height: f64,
    debug: serde_json::Value,
    logcat: serde_json::Value,
}
thread_local! { static GUARD: RefCell<Option<Guard>> = const { RefCell::new(None) }; }

pub fn start() {
    let Some(path) = std::env::var_os("SPARROW_WAYLAND_GUARD").map(PathBuf::from)
    else {
        return;
    };
    GUARD.with(|state| {
        *state.borrow_mut() = Some(Guard {
            path,
            focused: false,
            width: 0.0,
            height: 0.0,
            debug: serde_json::Value::Null,
            logcat: serde_json::Value::Null,
        })
    });
    pulse();
}
pub fn focus(focused: bool) {
    ensure_started();
    GUARD.with(|state| {
        if let Some(state) = state.borrow_mut().as_mut() {
            state.focused = focused;
        }
    });
    write();
}
pub fn size(width: f64, height: f64) {
    ensure_started();
    GUARD.with(|state| {
        if let Some(state) = state.borrow_mut().as_mut() {
            state.width = width;
            state.height = height;
        }
    });
    write();
}
fn write() {
    GUARD.with(|state| {
        let state = state.borrow();
        let Some(state) = state.as_ref() else { return };
        let temporary = state.path.with_extension("tmp");
        let json = serde_json::json!({"pid":std::process::id(), "focused":state.focused,
            "width":state.width, "height":state.height, "debug":state.debug, "logcat":state.logcat,
            "time":SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64()});
        if std::fs::write(&temporary, json.to_string()).is_ok() { let _ = std::fs::rename(temporary, &state.path); }
    });
}
fn pulse() {
    write();
    exec_after(Duration::from_millis(100), |_| pulse());
}

fn ensure_started() {
    if GUARD.with(|state| state.borrow().is_none()) {
        start();
    }
}

/// Observations only: this endpoint cannot dispatch commands or inject input.
pub fn observe(window: std::rc::Rc<crate::window_tab::WindowTabData>) {
    if std::env::var_os("SPARROW_WAYLAND_GUARD").is_none() {
        return;
    }
    use floem::reactive::{SignalGet, SignalWith};
    floem::reactive::create_effect(move |_| {
        let sessions = window.terminal.run_debug_process(true).into_iter().map(|(_,p)|
            serde_json::json!({"id":p.config.dap_id.0,"name":p.config.name,"stopped":p.stopped})).collect::<Vec<_>>();
        let dap = window.terminal.get_active_dap(true);
        let evaluations = dap.as_ref().map(|dap| dap.evaluation.entries.get().into_iter().map(|entry|
            serde_json::json!({"expression":entry.expression,"watch":entry.watch,"result":entry.result})).collect::<Vec<_>>()).unwrap_or_default();
        let selected_frame = dap.as_ref().and_then(|dap| dap.selected_frame.get().map(|frame|frame.id));
        let (paused, frame) = dap.map(|dap| {
            let paused = dap.stopped.get();
            let frame = dap.thread_id.get().and_then(|id|dap.stack_traces.with(|traces|traces.get(&id)
                .and_then(|trace|trace.frames.with(|frames|frames.front().map(|frame|serde_json::json!({"name":frame.name,"line":frame.line,"id":frame.id}))))));
            (paused, frame)
        }).unwrap_or_default();
        let active_file = window.main_split.active_editor.get().map(|editor| {
            let offset = editor.cursor().with(|cursor|cursor.offset());
            serde_json::json!({"path":editor.doc().content.with(|content|content.path().cloned()),"offset":offset,"confirmed":editor.confirmed.get()})
        });
        let snapshot = serde_json::json!({"sessions":sessions,"paused":paused,"frame":frame,"selected_frame":selected_frame,"evaluations":evaluations,
            "active_file":active_file,"settings":window.common.debugger_settings.get(),
            "inline_values":window.common.debug_inline_values.with(|values|values.as_ref().map(|v|v.text.clone())),
            "muted":window.terminal.debug.muted.get(),
            "breakpoints":window.terminal.debug.breakpoints.with(|paths|paths.iter().map(|(path,lines)|serde_json::json!({"path":path,"lines":lines.values().map(|bp|serde_json::json!({"line":bp.line+1,"active":bp.active,"verified":bp.verified})).collect::<Vec<_>>()})).collect::<Vec<_>>())});
        GUARD.with(|state| {
            if let Some(state) = state.borrow_mut().as_mut() {
                state.debug = snapshot;
            }
        });
        write();
    });
}

/// Read-only observations for portal-driven Logcat checks; disabled in normal launches.
pub fn observe_logcat(snapshot: impl Fn() -> serde_json::Value + 'static) {
    if std::env::var_os("SPARROW_WAYLAND_GUARD").is_none() { return; }
    floem::reactive::create_effect(move |_| {
        let value=snapshot();
        GUARD.with(|state|if let Some(state)=state.borrow_mut().as_mut(){state.logcat=value;});
        write();
    });
}
