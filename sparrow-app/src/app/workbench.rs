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
    reactive::{SignalGet, SignalUpdate, SignalWith, create_memo},
    views::{Decorators, stack},
};

use super::window_messages::window_message_view;
use crate::main_split::view::main_split;

pub(super) fn workbench(window_tab_data: Rc<WindowTabData>) -> impl View {
    let workbench_size = window_tab_data.common.workbench_size;
    let main_split_width = window_tab_data.main_split.width;
    let panel = window_tab_data.panel.clone();
    let dock_width_scale = {
        let panel = panel.clone();
        let config = window_tab_data.common.config;
        create_memo(move |_| {
            let available = panel.available_size.get().width
                - 2.0 * config.get().ui.tool_window_stripe_width() as f64
                - 6.0;
            let total = panel.size.with(|size| {
                [
                    (PanelContainerPosition::Left, size.left),
                    (PanelContainerPosition::Right, size.right),
                ]
                .into_iter()
                .filter(|(position, _)| panel.is_container_shown(position, true))
                .map(|(_, width)| width.max(0.0))
                .sum::<f64>()
            });
            // Saved dimensions can exceed the current window after reopening or docking.
            fit_side_docks(available, total)
        })
    };
    stack((
        tool_window_stripe(window_tab_data.clone(), true),
        stack((
            stack((
                panel_container_view(
                    window_tab_data.clone(),
                    PanelContainerPosition::Left,
                    dock_width_scale,
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
                    dock_width_scale,
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
                dock_width_scale,
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
        window_message_view(window_tab_data.clone()),
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

fn fit_side_docks(available: f64, total: f64) -> f64 {
    let editor_width = (available / 3.0).clamp(0.0, 150.0);
    ((available - editor_width).max(0.0) / total.max(1.0)).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::fit_side_docks;

    #[test]
    fn large_saved_docks_leave_room_for_the_editor() {
        assert_eq!(fit_side_docks(725.0, 420.0), 1.0);
        let requested = 420.0 + 466.0;
        let editor_width = 725.0 - requested * fit_side_docks(725.0, requested);
        assert_eq!(editor_width, 150.0);
        assert_eq!(fit_side_docks(0.0, requested), 0.0);
    }
}
