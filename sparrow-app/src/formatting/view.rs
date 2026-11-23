//! Dart formatting dialog and scope chooser.

use crate::{
    widgets::{
        ButtonKind, action_group, compact_button, menu_button,
        popup_menu::{Menu, MenuItem},
    },
    window_tab::WindowTabData,
};
use floem::{
    View,
    event::{Event, EventListener, EventPropagation},
    keyboard::{Key, NamedKey},
    reactive::{SignalGet, SignalUpdate, SignalWith},
    views::{Decorators, container, label, stack},
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
    let menu_window = window.clone();
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
    let scope = menu_button(
        move || target.get().display().to_string(),
        move || {
            let mut menu = Menu::new("Scope");
            if let Some(path) = menu_window
                .main_split
                .active_editor
                .get_untracked()
                .and_then(|editor| {
                    editor.doc().content.with_untracked(|c| c.path().cloned())
                })
            {
                let parent = path.parent().map(std::path::Path::to_path_buf);
                menu = menu.entry(
                    MenuItem::new("Whole file")
                        .action(move || target.set(path.clone())),
                );
                if let Some(parent) = parent {
                    menu = menu.entry(
                        MenuItem::new("Directory")
                            .action(move || target.set(parent.clone())),
                    );
                }
            }
            if let Some(path) = root.clone() {
                menu = menu.entry(
                    MenuItem::new("Project Files")
                        .action(move || target.set(path.clone())),
                );
            }
            menu
        },
        config,
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
