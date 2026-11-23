//! Project and branch chips in the title bar.

use floem::{
    View,
    kurbo::Point,
    peniko::Color,
    reactive::{ReadSignal, RwSignal, SignalGet, SignalUpdate},
    style::CursorStyle,
    views::{Decorators, container, drag_window_area, empty, label, stack, svg},
};
use std::{rc::Rc, sync::Arc};

use crate::{
    config::{SparrowConfig, color::SparrowColor, icon::SparrowIcons},
    window_tab::WindowTabData,
};

pub(super) fn workspace_controls(
    window_tab_data: Rc<WindowTabData>,
    _window_tabs: ReadSignal<im::Vector<(RwSignal<usize>, Rc<WindowTabData>)>>,
    branch: impl Fn() -> String + Copy + 'static,
    config: ReadSignal<Arc<SparrowConfig>>,
    project_chip_origin: RwSignal<Point>,
    branch_chip_origin: RwSignal<Point>,
) -> impl View {
    let workspace = window_tab_data.workspace.clone();
    let _main_split = window_tab_data.main_split.clone();
    let _workbench_command = window_tab_data.common.workbench_command;
    let _window_command = window_tab_data.common.window_common.window_command;

    // Copy out everything the closures need so none of them captures the Rc.
    let _current_tab_id = window_tab_data.window_tab_id;
    let branch_menu_visible = window_tab_data.branch_menu_visible;
    let project_menu_visible = window_tab_data.project_menu_visible;

    let initial_workspace = workspace.clone();
    let project_avatar = container(
        label(move || {
            initial_workspace
                .display()
                .map(|name| project_initials(&name))
                .unwrap_or_else(|| "S".to_string())
        })
        .style(|s| s.font_size(10.0).font_bold().selectable(false)),
    )
    .style(|s| {
        s.size(18.0, 18.0)
            .items_center()
            .justify_center()
            .border_radius(5.0)
            .margin_right(7.0)
            .color(Color::WHITE)
            .background(Color::from_rgba8(0x6E, 0x65, 0xF2, 0xFF)) // AS Blue
    });

    // THE PROJECT CHIP WITH NATIVE POPOUT MENU
    let project_chip = container(
        stack((
            project_avatar,
            label(move || {
                if let Some(s) = workspace.display() {
                    s
                } else {
                    "Open Folder".to_string()
                }
            })
            .style(|s| s.selectable(false)),
            svg(move || config.get().ui_svg(SparrowIcons::DROPDOWN_ARROW)).style(
                move |s| {
                    let size = config.get().ui.icon_size() as f32;
                    s.size(size, size).margin_left(4.0).color(
                        config.get().color(SparrowColor::SPARROW_ICON_INACTIVE),
                    )
                },
            ),
        ))
        .style(|s| s.items_center()),
    )
    .on_move(move |point| {
        if project_chip_origin.get_untracked() != point {
            project_chip_origin.set(point);
        }
    })
    .on_click_stop(move |_| {
        branch_menu_visible.set(false); // only one overlay at a time
        project_menu_visible.update(|v| *v = !*v);
    })
    .style(move |s| {
        let config = config.get();
        s.height(28.0)
            .padding_horiz(10.0)
            .items_center()
            .border_radius(6.0)
            .hover(|s| {
                s.cursor(CursorStyle::Pointer)
                    .background(config.color(SparrowColor::PANEL_HOVERED_BACKGROUND))
            })
    });

    // Branch Chip
    let branch_chip = stack((
        svg(move || config.get().ui_svg(SparrowIcons::SCM)).style(move |s| {
            let config = config.get();
            let size = config.ui.icon_size() as f32;
            s.size(size, size)
                .color(config.color(SparrowColor::SPARROW_ICON_ACTIVE))
        }),
        label(branch).style(|s| s.margin_left(6.0).selectable(false)),
        svg(move || config.get().ui_svg(SparrowIcons::DROPDOWN_ARROW)).style(
            move |s| {
                let config = config.get();
                let size = config.ui.icon_size() as f32;
                s.size(size, size)
                    .margin_left(4.0)
                    .color(config.color(SparrowColor::SPARROW_ICON_INACTIVE))
            },
        ),
    ))
    .on_move(move |point| {
        if branch_chip_origin.get_untracked() != point {
            branch_chip_origin.set(point);
        }
    })
    .on_click_stop(move |_| {
        // Toggle the branch switcher overlay
        project_menu_visible.set(false);
        branch_menu_visible.update(|v| *v = !*v);
    })
    .style(move |s| {
        let config = config.get();
        s.height(28.0)
            .padding_horiz(10.0)
            .margin_left(4.0)
            .items_center()
            .border_radius(6.0)
            .apply_if(branch().is_empty(), |s| s.hide())
            .hover(|s| {
                s.cursor(CursorStyle::Pointer)
                    .background(config.color(SparrowColor::PANEL_HOVERED_BACKGROUND))
            })
    });

    stack((
        project_chip,
        branch_chip,
        drag_window_area(empty())
            .style(|s| s.height_pct(100.0).flex_basis(0.0).flex_grow(1.0f32)),
    ))
    .style(|s| {
        s.flex_basis(0)
            .flex_grow(1.0f32)
            .height_pct(100.0)
            .items_center()
    })
    .debug_name("Middle of Top Bar")
}

fn project_initials(name: &str) -> String {
    let words: Vec<_> = name
        .split(|c: char| c == '_' || c == '-' || c.is_whitespace())
        .filter(|word| !word.is_empty())
        .collect();
    let mut initials = words
        .first()
        .and_then(|word| word.chars().next())
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "S".into());
    if words.len() > 1 {
        if let Some(last) = words.last().and_then(|word| word.chars().next()) {
            initials.extend(last.to_uppercase());
        }
    }
    initials
}
#[cfg(test)]
mod toolbar_tests {
    use super::*;
    #[test]
    fn project_initials_match_name_segments() {
        assert_eq!(project_initials("liquid_glass_kit"), "LK");
        assert_eq!(project_initials("app"), "A");
        assert_eq!(project_initials(""), "S");
    }
}
