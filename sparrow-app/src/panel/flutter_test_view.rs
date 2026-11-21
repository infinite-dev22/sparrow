//! Test tree and assertion details, backed by the machine-readable test stream.
use super::position::PanelPosition;
use crate::window_tab::WindowTabData;
use floem::{
    View,
    reactive::create_rw_signal,
    views::{Decorators, stack},
};
use std::{collections::BTreeSet, rc::Rc};

mod output;
mod toolbar;
mod tree;

use output::test_output;
use toolbar::test_toolbar;
use tree::test_tree;

pub fn flutter_test_panel(
    data: Rc<WindowTabData>,
    _position: PanelPosition,
) -> impl View {
    let collapsed = create_rw_signal(BTreeSet::<String>::new());
    stack((
        test_toolbar(data.clone()),
        stack((test_tree(&data, collapsed), test_output(&data)))
            .style(|s| s.flex_grow(1.0_f32).min_height(0.0).width_full()),
    ))
    .style(|s| s.size_full().flex_col())
    .debug_name("Flutter Test Panel")
}
