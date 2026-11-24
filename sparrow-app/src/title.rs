use floem::{
    View,
    kurbo::Point,
    peniko::Color,
    reactive::{ReadSignal, RwSignal, SignalGet, SignalUpdate},
    views::{Decorators, stack},
};
use std::rc::Rc;

use crate::window_tab::WindowTabData;

mod actions;
mod branch_switcher;
mod device_selector;
mod leading;
mod project_switcher;
mod window_controls;
mod workspace;
pub mod run_config_menu;

pub use branch_switcher::branch_switcher;
pub use device_selector::device_selector_overlay;
pub use project_switcher::project_switcher;
pub use window_controls::window_controls_view;

use actions::run_actions;
use leading::application_controls;
use workspace::workspace_controls;

pub fn title(
    window_tab_data: Rc<WindowTabData>,
    window_tabs: ReadSignal<im::Vector<(RwSignal<usize>, Rc<WindowTabData>)>>,
    project_chip_origin: RwSignal<Point>,
    branch_chip_origin: RwSignal<Point>,
    device_chip_origin: RwSignal<Point>,
) -> impl View {
    let sparrow_command = window_tab_data.common.sparrow_command;
    let workbench_command = window_tab_data.common.workbench_command;
    let window_command = window_tab_data.common.window_common.window_command;
    let latest_release = window_tab_data.common.window_common.latest_release;
    let proxy_status = window_tab_data.common.proxy_status;
    let num_window_tabs = window_tab_data.common.window_common.num_window_tabs;
    let window_maximized = window_tab_data.common.window_common.window_maximized;
    let title_height = window_tab_data.title_height;
    let update_in_progress = window_tab_data.update_in_progress;
    let config = window_tab_data.common.config;
    let branch = window_tab_data.source_control.branch;
    let branch_text = move || branch.get();
    stack((
        application_controls(
            window_tab_data.clone(),
            window_tab_data.workspace.clone(),
            sparrow_command,
            workbench_command,
            config,
            proxy_status,
            num_window_tabs,
        ),
        workspace_controls(
            window_tab_data.clone(),
            window_tabs,
            branch_text,
            config,
            project_chip_origin,
            branch_chip_origin,
        ),
        run_actions(
            window_command,
            workbench_command,
            latest_release,
            update_in_progress,
            num_window_tabs,
            window_maximized,
            config,
            window_tab_data.flutter_hot_reload.clone(),
            window_tab_data.device_menu_visible,
            device_chip_origin,
            window_tab_data.clone(),
        ),
    ))
    .on_resize(move |rect| {
        let height = rect.height();
        if height != title_height.get_untracked() {
            title_height.set(height);
        }
    })
    .style(move |s| {
        let config = config.get();
        s.width_pct(100.0)
            .height(config.ui.header_height().max(34) as f32)
            .flex_shrink(0.0_f32)
            .items_center()
            .background(Color::TRANSPARENT)
    })
    .debug_name("Title / Top Bar")
}
