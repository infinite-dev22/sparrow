//! Dart formatting dialog and scope chooser.

use crate::{
    widgets::{ButtonKind, action_group, compact_button, dropdown_with_label},
    window_tab::WindowTabData,
};
use floem::{
    View,
    event::{Event, EventListener, EventPropagation},
    keyboard::{Key, NamedKey},
    reactive::{SignalGet, SignalUpdate, SignalWith},
    views::{Decorators, container, dyn_container, label, stack},
};
use std::rc::Rc;

pub fn view(window: Rc<WindowTabData>) -> impl View {
    let state = window.formatting.clone();
    let active = state.active;
    crate::widgets::modal_focus::restore_workbench_on_close(
        active,
        window.common.clone(),
    );
    let busy = state.busy;
    let target = state.target;
    let message = state.message;
    let config = window.common.config;
    let root = window.workspace.path.clone();
    let active_editor = window.main_split.active_editor;
    let focus = crate::widgets::modal_focus::ModalFocus::default();
    let initial_focus = focus.clone();
    floem::reactive::create_effect(move |_| {
        if active.get() {
            let focus = initial_focus.clone();
            floem::action::exec_after(
                std::time::Duration::from_millis(30),
                move |_| focus.focus_first(),
            );
        }
    });
    let run = state.clone();
    let w = window.clone();
    focus.set_submit(move || run.run(w.clone()));
    let run = state.clone();
    let w = window.clone();
    let scope = dyn_container(
        move || {
            active_editor.get().and_then(|editor| {
                editor.doc().content.with(|content| content.path().cloned())
            })
        },
        move |active_file| {
            let parent = active_file
                .as_ref()
                .and_then(|path| path.parent().map(std::path::Path::to_path_buf));
            let mut choices = Vec::new();
            if let Some(path) = active_file.clone() {
                choices.push(path);
            }
            if let Some(path) = parent.clone()
                && !choices.contains(&path)
            {
                choices.push(path);
            }
            if let Some(path) = root.clone()
                && !choices.contains(&path)
            {
                choices.push(path);
            }
            let display_root = root.clone();
            dropdown_with_label(
                target,
                choices,
                config,
                move |path| {
                    if active_file.as_ref() == Some(path) {
                        "Whole file".into()
                    } else if display_root.as_ref() == Some(path) {
                        "Project Files".into()
                    } else if parent.as_ref() == Some(path) {
                        "Directory".into()
                    } else {
                        path.display().to_string()
                    }
                },
                |_| {},
            )
        },
    );
    let body = stack((
        label(|| "Reformat Code").style(|s| s.font_bold()),
        label(|| "Scope:"),
        focus.control(scope.style(|s| s.width_full())),
        label(|| "Formats Dart files using this project’s SDK and formatter settings. Generated build directories are excluded.")
            .style(|s| s.width_full()),
        label(move || if busy.get() { "Formatting…".into() } else { message.get() })
            .style(|s| s.width_full()),
        action_group((
            focus.control(
                compact_button(|| "Run", ButtonKind::Primary, config)
                    .on_click_stop(move |_| run.run(w.clone()))
                    .disabled(move || busy.get()),
            ),
            focus.control(
                compact_button(|| "Close", ButtonKind::Outlined, config)
                    .on_click_stop(move |_| active.set(false))
                    .disabled(move || busy.get()),
            ),
        )),
    ))
    .style(move |s| {
        crate::widgets::dialog_surface(
            s.flex_col()
                .width(460.0)
                .max_width_pct(95.0)
                .gap(14.0)
                .padding(18.0),
            &config.get(),
        )
    });
    let dialog=container(body).style(move |s|s.absolute().size_full().items_center().justify_center().z_index(100).apply_if(!active.get(),|s|s.hide())).on_event_stop(EventListener::PointerDown, |_|{}).on_event(EventListener::KeyDown,move |e|{if matches!(e,Event::KeyDown(k)if k.key.logical_key==Key::Named(NamedKey::Escape))&&!busy.get_untracked(){active.set(false);EventPropagation::Stop}else{EventPropagation::Continue}});
    focus.bind_root(dialog, active, config)
}
