//! Open Editors list in the Project tool window.

use std::rc::Rc;

use floem::{
    View,
    event::EventListener,
    reactive::{RwSignal, SignalGet, SignalUpdate, SignalWith, create_rw_signal},
    style::CursorStyle,
    text::Style as FontStyle,
    views::{Decorators, container, dyn_stack, label, stack, svg},
};

use crate::{
    command::InternalCommand,
    config::{color::SparrowColor, icon::SparrowIcons},
    editor_tab::{EditorTabChild, EditorTabData},
    plugin::PluginData,
    widgets::clickable_icon,
    window_tab::WindowTabData,
};
use crate::widgets::sparrow_scroll;

pub(super) fn open_editors_view(window_tab_data: Rc<WindowTabData>) -> impl View {
    let diff_editors = window_tab_data.main_split.diff_editors;
    let editors = window_tab_data.main_split.editors;
    let editor_tabs = window_tab_data.main_split.editor_tabs;
    let config = window_tab_data.common.config;
    let internal_command = window_tab_data.common.internal_command;
    let active_editor_tab = window_tab_data.main_split.active_editor_tab;
    let plugin = window_tab_data.plugin.clone();

    let child_view = move |plugin: PluginData,
                           editor_tab: RwSignal<EditorTabData>,
                           child_index: RwSignal<usize>,
                           child: EditorTabChild| {
        let editor_tab_id =
            editor_tab.with_untracked(|editor_tab| editor_tab.editor_tab_id);
        let child_for_close = child.clone();
        let info = child.view_info(editors, diff_editors, plugin, config);
        let hovered = create_rw_signal(false);

        stack((
            clickable_icon(
                move || {
                    if hovered.get() || info.with(|info| info.is_pristine) {
                        SparrowIcons::CLOSE
                    } else {
                        SparrowIcons::UNSAVED
                    }
                },
                move || {
                    let editor_tab_id =
                        editor_tab.with_untracked(|t| t.editor_tab_id);
                    internal_command.send(InternalCommand::EditorTabChildClose {
                        editor_tab_id,
                        child: child_for_close.clone(),
                    });
                },
                || false,
                || false,
                || "Close",
                config,
            )
            .on_event_stop(EventListener::PointerEnter, move |_| {
                hovered.set(true);
            })
            .on_event_stop(EventListener::PointerLeave, move |_| {
                hovered.set(false);
            })
            .on_event_stop(EventListener::PointerDown, |_| {})
            .style(|s| s.margin_left(10.0)),
            container(svg(move || info.with(|info| info.icon.clone())).style(
                move |s| {
                    let size = config.get().ui.icon_size() as f32;
                    s.size(size, size)
                        .apply_opt(info.with(|info| info.color), |s, c| s.color(c))
                },
            ))
            .style(|s| s.padding_horiz(6.0)),
            label(move || info.with(|info| info.name.clone())).style(move |s| {
                s.apply_if(
                    !info
                        .with(|info| info.confirmed)
                        .map(|confirmed| confirmed.get())
                        .unwrap_or(true),
                    |s| s.font_style(FontStyle::Italic),
                )
            }),
        ))
        .style(move |s| {
            let config = config.get();
            s.items_center()
                .width_pct(100.0)
                .cursor(CursorStyle::Pointer)
                .apply_if(
                    active_editor_tab.get() == Some(editor_tab_id)
                        && editor_tab.with(|editor_tab| editor_tab.active)
                            == child_index.get(),
                    |s| {
                        s.background(
                            config.color(SparrowColor::PANEL_CURRENT_BACKGROUND),
                        )
                    },
                )
                .hover(|s| {
                    s.background(
                        config.color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                    )
                })
        })
        .on_event_cont(EventListener::PointerDown, move |_| {
            editor_tab.update(|editor_tab| {
                editor_tab.active = child_index.get_untracked();
            });
            active_editor_tab.set(Some(editor_tab_id));
        })
    };

    sparrow_scroll(
        dyn_stack(
            move || editor_tabs.get().into_iter().enumerate(),
            move |(index, (editor_tab_id, _))| (*index, *editor_tab_id),
            move |(index, (_, editor_tab))| {
                let plugin = plugin.clone();
                stack((
                    label(move || format!("Group {}", index + 1))
                        .style(|s| s.margin_left(10.0)),
                    dyn_stack(
                        move || editor_tab.get().children,
                        move |(_, _, child)| child.id(),
                        move |(child_index, _, child)| {
                            child_view(
                                plugin.clone(),
                                editor_tab,
                                child_index,
                                child,
                            )
                        },
                    )
                    .style(|s| s.flex_col().width_pct(100.0)),
                ))
                .style(|s| s.flex_col())
            },
        )
        .style(|s| s.flex_col().width_pct(100.0)),
    )
    .style(|s| s.absolute().size_full().line_height(1.0))
    .debug_name("Open Editors")
}
