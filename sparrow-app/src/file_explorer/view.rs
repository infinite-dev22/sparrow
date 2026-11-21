use std::rc::Rc;

use floem::{
    View,
    reactive::SignalGet,
    views::{Decorators, container, stack},
};

use crate::{
    panel::{data::PanelSection, position::PanelPosition, view::PanelBuilder},
    window_tab::WindowTabData,
};

mod naming;
mod open_editors;
mod project_header;
mod tree;

use open_editors::open_editors_view;
use project_header::project_header;
use tree::file_explorer_view;

pub fn file_explorer_panel(
    window_tab_data: Rc<WindowTabData>,
    position: PanelPosition,
) -> impl View {
    let config = window_tab_data.common.config;
    let data = window_tab_data.file_explorer.clone();
    let source_control = window_tab_data.source_control.clone();
    let (project_info, project_watch) =
        super::project::load(window_tab_data.common.clone());
    let library_watch = project_watch.libraries();
    let header = project_header(window_tab_data.clone());
    let open_editors = PanelBuilder::new(config, position)
        .add_height_style(
            "Open Editors",
            150.0,
            container(open_editors_view(window_tab_data.clone()))
                .style(|s| s.size_full()),
            window_tab_data.panel.section_open(PanelSection::OpenEditor),
            move |s| s.apply_if(!config.get().ui.open_editors_visible, |s| s.hide()),
        )
        .build();
    stack((
        header,
        open_editors,
        container(file_explorer_view(
            data,
            source_control,
            project_info,
            window_tab_data,
            library_watch,
        ))
        .style(|s| {
            s.flex_grow(1.0_f32)
                .flex_basis(0.0)
                .min_height(0.0)
                .width_full()
        }),
    ))
    .style(|s| s.flex_col().size_full())
    .on_cleanup(move || project_watch.stop())
    .debug_name("File Explorer Panel")
}
