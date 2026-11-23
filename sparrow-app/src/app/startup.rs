use std::{cell::RefCell, rc::Rc, sync::Arc, time::Duration};

use floem::{
    View,
    action::exec_after,
    reactive::{ReadSignal, Scope, SignalGet, SignalUpdate},
    views::{Decorators, empty, stack, svg},
};

use crate::{
    config::{SparrowConfig, color::SparrowColor},
    widgets::dialog_frame::dialog_frame,
};

fn animate(pulse: floem::reactive::RwSignal<u32>) {
    exec_after(Duration::from_millis(30), move |_| {
        if pulse
            .try_update(|frame| *frame = (*frame + 1) % 120)
            .is_some()
        {
            animate(pulse);
        }
    });
}

pub(super) fn view(
    config: ReadSignal<Arc<SparrowConfig>>,
    inherit_shell: bool,
    open: impl FnOnce() + 'static,
) -> impl View {
    let scope = Scope::new();
    let pulse = scope.create_rw_signal(0);
    animate(pulse);
    let opening = Rc::new(RefCell::new(Some(open)));
    let timeout = opening.clone();
    let deadline = exec_after(Duration::from_secs(3), move |_| {
        if let Some(open) = timeout.borrow_mut().take() {
            tracing::warn!("Login shell environment did not arrive before startup");
            open();
        }
    });
    let deliver = opening.clone();
    let ready = floem::ext_event::create_ext_action(scope, move |environment| {
        if let Some(open) = deliver.borrow_mut().take() {
            super::apply_shell_env(environment);
            exec_after(Duration::from_millis(140), move |_| open());
        }
    });
    std::thread::spawn(move || {
        ready(if inherit_shell {
            super::read_shell_env()
        } else {
            String::new()
        });
    });
    dialog_frame(
        stack((
            empty().style(move |s| {
                let frame = pulse.get() as f64;
                let size =
                    104.0 + (frame * std::f64::consts::TAU / 120.0).sin() * 12.0;
                s.absolute()
                    .size(size, size)
                    .border_radius(size / 2.0)
                    .background(
                        config
                            .get()
                            .color(SparrowColor::EDITOR_LINK)
                            .multiply_alpha(0.15),
                    )
            }),
            svg(|| {
                include_str!("../../../icons/sparrow/sparrow_logo.svg").to_owned()
            })
            .style(|s| s.size(58, 58)),
        ))
        .style(move |s| {
            s.size_full().items_center().justify_center().background(
                config.get().color(SparrowColor::SPARROW_WINDOW_BACKGROUND),
            )
        }),
        config
            .get_untracked()
            .color(SparrowColor::SPARROW_WINDOW_BACKGROUND),
    )
    .on_event_cont(floem::event::EventListener::WindowClosed, move |_| {
        deadline.cancel();
        opening.borrow_mut().take();
        scope.dispose();
    })
}
