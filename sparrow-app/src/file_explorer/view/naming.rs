//! Project path creation, rename, and copy popup.

use std::path::Path;

use floem::{
    View,
    event::EventListener,
    reactive::{SignalGet, SignalWith, create_effect, create_memo},
    views::{Decorators, container, label, stack},
};
use sparrow_rpc::file::{Naming, NamingState};

use crate::{
    config::color::SparrowColor,
    file_explorer::data::FileExplorerData,
    widgets::{ButtonKind, button, text_input::TextInputBuilder},
};

fn naming_description(naming: &Naming) -> (&'static str, &'static str, String) {
    match naming {
        Naming::None => ("", "", String::new()),
        Naming::NewNode(node) => (
            if node.is_dir {
                "New Directory"
            } else {
                "New File"
            },
            "Create",
            node.base_path.display().to_string(),
        ),
        Naming::Renaming(rename) => (
            "Rename",
            "Rename",
            format!(
                "Current name  {}",
                rename
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
            ),
        ),
        Naming::Duplicating(copy) => (
            if copy.move_on_finish { "Move" } else { "Copy" },
            if copy.move_on_finish { "Move" } else { "Copy" },
            copy.destination
                .as_deref()
                .or_else(|| copy.path.parent())
                .map(Path::display)
                .map(|path| path.to_string())
                .unwrap_or_default(),
        ),
    }
}

pub(crate) fn file_naming_popup(data: FileExplorerData) -> impl View {
    let config = data.common.config;
    let naming = data.naming;
    let active = create_memo(move |_| naming.with(|state| state.state().is_some()));
    let busy = move || {
        naming.with(|state| matches!(state.state(), Some(NamingState::Pending)))
    };
    let describe = move || naming.with(naming_description);
    let editor = data.naming_editor_data.clone();
    let commands = data.clone();
    let submit = data.clone();
    let cancel = data.clone();
    let root = data.common.window_common.app_view_id;
    let input = TextInputBuilder::new()
        .key_focus(commands)
        .build_editor(editor)
        .placeholder(|| "Name".into())
        .style(move |s| {
            s.width_full()
                .height(33)
                .padding_horiz(8)
                .padding_vert(0)
                .border(1)
                .border_radius(5)
                .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
                .focus(|s| {
                    s.border_color(config.get().color(SparrowColor::EDITOR_CARET))
                })
        })
        .disabled(busy);
    let input_id = input.id();
    create_effect(move |was_open: Option<bool>| {
        let open = active.get();
        if open {
            input_id.request_focus();
        } else if was_open == Some(true) {
            root.get_untracked().request_focus();
        }
        open
    });
    let retry = create_memo(move |_| {
        naming.with(|state| state.state().is_some_and(NamingState::is_err))
    });
    create_effect(move |_| {
        if retry.get() {
            input_id.request_focus();
        }
    });

    let title = describe.clone();
    let action = describe.clone();
    let popup = stack((
        label(move || title().0).style(|s| s.font_bold().font_size(15)),
        label(move || describe().2).style(move |s| {
            s.width_full()
                .min_width(0)
                .text_ellipsis()
                .color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
        }),
        input,
        label(move || {
            naming.with(|state| {
                state
                    .state()
                    .and_then(NamingState::err)
                    .unwrap_or_default()
                    .to_string()
            })
        })
        .style(move |s| {
            s.width_full()
                .color(
                    config
                        .get()
                        .color(SparrowColor::ERROR_LENS_ERROR_FOREGROUND),
                )
                .apply_if(!retry.get(), |s| s.hide())
        }),
        stack((
            button(|| "Cancel", ButtonKind::Plain, config)
                .on_click_stop(move |_| cancel.cancel_naming())
                .disabled(busy),
            button(
                move || if busy() { "Working…" } else { action().1 },
                ButtonKind::Primary,
                config,
            )
            .on_click_stop(move |_| submit.finish_naming())
            .disabled(busy),
        ))
        .style(|s| s.width_full().justify_end().gap(8)),
    ))
    .style(move |s| {
        let c = config.get();
        s.flex_col()
            .width(370)
            .max_width_pct(92.0)
            .padding(14)
            .gap(9)
            .border(1)
            .border_radius(9)
            .background(c.color(SparrowColor::PANEL_BACKGROUND))
            .border_color(c.color(SparrowColor::SPARROW_BORDER))
            .color(c.color(SparrowColor::EDITOR_FOREGROUND))
            .box_shadow_blur(18)
            .box_shadow_color(c.color(SparrowColor::SPARROW_DROPDOWN_SHADOW))
    });

    container(popup)
        .on_event_stop(EventListener::PointerDown, |_| {})
        .style(move |s| {
            s.absolute()
                .size_full()
                .items_center()
                .justify_center()
                .z_index(100)
                .apply_if(!active.get(), |s| s.hide())
        })
        .debug_name("Project path dialog")
}
