use std::rc::Rc;

use floem::View;

use super::position::PanelPosition;
use crate::window_tab::WindowTabData;

mod breakpoints;
mod variables;

pub(super) use breakpoints::breakpoints_view;
pub(super) use variables::variables_view;

pub fn debug_panel(
    window_tab_data: Rc<WindowTabData>,
    position: PanelPosition,
) -> impl View {
    super::debug_session::panel(window_tab_data, position)
}

pub(super) fn inspector(
    window_tab_data: Rc<WindowTabData>,
    position: PanelPosition,
) -> impl View {
    super::debug_inspector::inspector(window_tab_data, position)
}
