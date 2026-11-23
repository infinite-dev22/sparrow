//! Appearance-only theme management; no language-extension marketplace.
use super::{kind::PanelKind, position::PanelPosition};
use crate::widgets::{action_group, action_link, sparrow_scroll, text_input};
use crate::{command::SparrowWorkbenchCommand, window_tab::WindowTabData};
use floem::{
    View,
    reactive::{SignalGet, SignalWith, create_memo, create_rw_signal},
    views::{Decorators, dyn_stack, empty, label, stack},
};
use std::rc::Rc;

pub fn plugin_panel(
    window: Rc<WindowTabData>,
    _position: PanelPosition,
) -> impl View {
    let config = window.common.config;
    let plugin = window.plugin.clone();
    let installed = plugin.installed;
    let commands = window.common.workbench_command;
    let query = create_rw_signal(String::new());
    let rows = create_memo(move |_| {
        let q = query.get().to_lowercase();
        installed.with(|items| {
            items
                .values()
                .map(|v| v.meta.get())
                .filter(|m| m.display_name.to_lowercase().contains(&q))
                .collect::<Vec<_>>()
        })
    });
    stack((
        super::chrome::header(window, PanelKind::Plugin, "Themes", empty()),
        stack((
            action_group((
                action_link(
                    || "Color theme…",
                    move || commands.send(SparrowWorkbenchCommand::ChangeColorTheme),
                    config,
                ),
                action_link(
                    || "Icon theme…",
                    move || commands.send(SparrowWorkbenchCommand::ChangeIconTheme),
                    config,
                ),
                action_link(|| "Reload local themes", move || plugin.reload_themes(), config),
            ))
            .style(|s| s.width_full().padding(16.0)),
            floem::views::container(
                text_input(query, config).placeholder("Filter installed themes"),
            )
            .style(|s| s.margin_horiz(16.0).min_width(0.0)),
            label(|| "Built-in themes are available from the theme selectors. Additional local themes appear below.")
                .style(|s| s.padding(16.0).width_full()),
            sparrow_scroll(
                dyn_stack(
                    move || rows.get(),
                    |theme| theme.id(),
                    move |theme| {
                        stack((
                            label(move || theme.display_name.clone()).style(|s| s.font_bold()),
                            label(move || theme.description.clone()).style(|s| s.margin_top(6.0)),
                        ))
                        .style(|s| s.flex_col().width_full().padding(16.0))
                    },
                )
                .style(|s| s.flex_col().width_full()),
            )
            .style(|s| s.width_full().flex_grow(1.0_f32).min_height(0.0)),
        ))
        .style(|s| s.flex_col().width_full().min_height(0.0).flex_grow(1.0_f32)),
    ))
    .style(|s| s.absolute().size_full().flex_col())
}
