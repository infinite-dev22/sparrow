//! New Dart and Flutter project form.

use super::PLATFORMS;
use crate::widgets::{labeled_checkbox, text_input};
use crate::{config::color::SparrowColor, window_tab::WindowTabData};
use floem::{
    View,
    reactive::{SignalGet, SignalUpdate, SignalWith},
    views::{Decorators, container, dyn_stack, label, stack},
};
use std::rc::Rc;

pub fn view(window: Rc<WindowTabData>) -> impl View {
    let wizard = window.flutter_project.clone();
    let active = wizard.active;
    let busy = wizard.busy;
    let template = wizard.template;
    let platforms = wizard.platforms;
    let empty = wizard.empty;
    let message = wizard.message;
    let config = window.common.config;
    let name = text_input(wizard.name, config);
    let name_id = name.id();
    floem::reactive::create_effect(move |_| {
        if active.get() {
            floem::action::exec_after(std::time::Duration::from_millis(30), move |_| {
                if active.get_untracked() { name_id.request_focus(); }
            });
        }
    });
    container(
        container(
            stack((
                label(|| "New Flutter Project").style(|s| s.font_bold()),
                label(|| "Project name"),
                name,
                label(|| "Parent directory (absolute path)"),
                text_input(wizard.parent, config),
                label(|| "Project type"),
                dyn_stack(
                    || vec!["app", "package", "plugin", "module", "package_ffi"],
                    |s| *s,
                    move |kind| {
                        label(move || {
                            format!(
                                "{} {kind}",
                                if template.get() == kind { "●" } else { "○" }
                            )
                        })
                        .on_click_stop(move |_| template.set(kind.into()))
                    },
                ),
                labeled_checkbox(
                    move || empty.get(),
                    || "Empty skeleton (apps); otherwise counter template",
                    move |checked| empty.set(checked),
                    config,
                ),
                label(|| "Platforms (apps and plugins; packages choose their native targets)"),
                dyn_stack(
                    || PLATFORMS.to_vec(),
                    |s| *s,
                    move |platform| {
                        labeled_checkbox(
                            move || {
                                platforms.with(|items| items.iter().any(|p| p == platform))
                            },
                            move || platform,
                            move |checked| {
                                platforms.update(|items| {
                                    if checked {
                                        if !items.iter().any(|p| p == platform) {
                                            items.push(platform.into());
                                        }
                                    } else {
                                        items.retain(|p| p != platform);
                                    }
                                });
                            },
                            config,
                        )
                    },
                ),
                label(move || message.get())
                    .style(|s| s.max_width(600.0).selectable(true)),
                label(|| "Create")
                    .on_click_stop(move |_| wizard.create(window.clone()))
                    .disabled(move || busy.get()),
                label(|| "Cancel")
                    .on_click_stop(move |_| active.set(false))
                    .disabled(move || busy.get()),
            ))
            .disabled(move || busy.get())
            .style(|s| s.flex_col().gap(8.0)),
        )
        .style(move |s| {
            s.width(640.0)
                .padding(24.0)
                .background(config.get().color(SparrowColor::PANEL_BACKGROUND))
        }),
    )
    .on_event_stop(floem::event::EventListener::PointerDown, |_| {})
    .style(move |s| {
        s.absolute()
            .z_index(200)
            .size_full()
            .items_center()
            .justify_center()
            .background(config.get().color(SparrowColor::SPARROW_DROPDOWN_SHADOW))
            .apply_if(!active.get(), |s| s.hide())
    })
}
