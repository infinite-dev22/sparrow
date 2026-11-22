//! Workbench layout and tool window docking.

use std::rc::Rc;

use crate::{
    panel::{
        position::PanelContainerPosition,
        view::{panel_container_view, tool_window_stripe},
    },
    window_tab::WindowTabData,
};
use floem::{
    View,
    reactive::{SignalGet, SignalUpdate},
    views::{Decorators, stack},
};

use super::window_messages::window_message_view;
use crate::main_split::view::main_split;

pub(super) fn workbench(window_tab_data: Rc<WindowTabData>) -> impl View {
    let workbench_size = window_tab_data.common.workbench_size;
    let main_split_width = window_tab_data.main_split.width;
    let panel = window_tab_data.panel.clone();
    stack((
        tool_window_stripe(window_tab_data.clone(), true),
        stack((
            stack((
                panel_container_view(
                    window_tab_data.clone(),
                    PanelContainerPosition::Left,
                ),
                main_split(window_tab_data.clone()).on_resize(move |rect| {
                    let width = rect.width();
                    if main_split_width.get_untracked() != width {
                        main_split_width.set(width);
                    }
                }),
                panel_container_view(
                    window_tab_data.clone(),
                    PanelContainerPosition::Right,
                ),
            ))
            .style(move |s| {
                let bottom_maximized = panel.panel_bottom_maximized(true)
                    && panel.is_container_shown(&PanelContainerPosition::Bottom, true);
                s.width_full()
                    .flex_grow(1.0_f32)
                    .flex_basis(0.0)
                    .min_height(0.0)
                    .gap(3.0)
                    .apply_if(bottom_maximized, |s| s.hide())
            }),
            panel_container_view(
                window_tab_data.clone(),
                PanelContainerPosition::Bottom,
            ),
        ))
        .style(|s| {
            s.height_full()
                .flex_col()
                .flex_grow(1.0_f32)
                .flex_basis(0.0)
                .min_width(0.0)
                .gap(3.0)
        }),
        tool_window_stripe(window_tab_data.clone(), false),
        window_message_view(window_tab_data.messages, window_tab_data.common.config),
    ))
    .on_resize(move |rect| {
        let size = rect.size();
        if size != workbench_size.get_untracked() {
            workbench_size.set(size);
        }
    })
    .style(move |s| s.size_full().gap(0.0).padding_bottom(2.0))
    .debug_name("Workbench")
}
