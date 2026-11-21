//! Git command review dialog and command output.

use crate::{widgets::popup_menu::PopupMenuExt, window_tab::WindowTabData};
use floem::{
    View,
    event::{Event, EventListener, EventPropagation},
    keyboard::{Key, NamedKey},
    reactive::{SignalGet, SignalUpdate},
    views::{Decorators, container, label, scroll, stack},
};
use std::{rc::Rc, sync::atomic::Ordering};

use super::menu;

pub fn view(window: Rc<WindowTabData>) -> impl View {
    let state = window.git_actions.clone();
    let active = state.active;
    crate::widgets::modal_focus::restore_workbench_on_close(active, window.common.clone());
    let title = state.title;
    let args = state.args;
    let output = state.output;
    let busy = state.busy;
    let cancel = state.cancel.clone();
    let cleanup_cancel = state.cancel.clone();
    let config = window.common.config;
    let run = state.clone();
    let w = window.clone();
    let focus = crate::widgets::modal_focus::ModalFocus::default();
    let submit = state.clone();
    let w2 = window.clone();
    focus.set_submit(move || submit.run(w2.clone()));
    let input = focus.input(args, config);
    let id = input.id();
    floem::reactive::create_effect(move |_| {
        if active.get() {
            floem::action::exec_after(
                std::time::Duration::from_millis(30),
                move |_| id.request_focus(),
            );
        }
    });
    let commands = window.common.internal_command;
    let background = state.active;
    let body=stack((label(move ||format!("Git · {}",title.get())).style(|s|s.font_bold()),label(||"git").ide_popout_menu(move ||menu(commands,None)),input,label(move ||{let a=args.get();if a.contains("--hard"){"Hard reset discards tracked working-tree and staged changes."}else if a.starts_with("rebase"){"Rebase rewrites commit history. Resolve conflicts before continuing."}else{"Review the Git arguments above, then run. Output and errors appear below."}}),scroll(label(move ||output.get())).style(|s|s.width_full().height(300.0)),stack((focus.control(label(||"Run in Background").on_click_stop(move |_|background.set(false)).style(move |s|s.apply_if(!busy.get(),|s|s.hide()))),focus.control(label(move ||if busy.get(){"Running…"}else{"Run"}).on_click_stop(move |_|run.run(w.clone())).disabled(move ||busy.get())),focus.control(label(||"Copy output").on_click_stop(move |_|crate::menus::copy_text(&output.get_untracked()))),focus.control(label(move ||if busy.get(){"Cancel command"}else{"Close"}).on_click_stop(move |_|{if busy.get_untracked(){cancel.store(true,Ordering::Relaxed);}else{active.set(false);}})))).style(|s|s.gap(20.0)))).style(move |s| {
        crate::widgets::dialog_surface(
            s.flex_col()
                .width(720.0)
                .max_width_pct(95.0)
                .gap(12.0)
                .padding(18.0),
            &config.get(),
        )
    });
    let dialog=container(body).style(move |s|s.absolute().size_full().items_center().justify_center().z_index(100).apply_if(!active.get(),|s|s.hide())).on_event_stop(EventListener::PointerDown, |_|{}).on_event(EventListener::KeyDown,move |e|{if matches!(e,Event::KeyDown(k)if k.key.logical_key==Key::Named(NamedKey::Escape))&&!busy.get_untracked(){active.set(false);EventPropagation::Stop}else{EventPropagation::Continue}});
    focus
        .bind_root(dialog, active, config)
        .on_cleanup(move || cleanup_cancel.store(true, Ordering::Relaxed))
}
