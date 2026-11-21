//! Virtualized debugger variables tree.

use std::rc::Rc;

use floem::{
    View,
    peniko::Color,
    reactive::{SignalGet, SignalWith},
    views::{Decorators, container, scroll, stack, svg, text, virtual_stack},
};

use crate::{
    config::{color::SparrowColor, icon::SparrowIcons},
    debug::DapVariable,
    window_tab::WindowTabData,
};

pub(in crate::panel) fn variables_view(
    window_tab_data: Rc<WindowTabData>,
) -> impl View {
    let terminal = window_tab_data.terminal.clone();
    let local_terminal = window_tab_data.terminal.clone();
    let ui_line_height = window_tab_data.common.ui_line_height;
    let config = window_tab_data.common.config;
    container(
        scroll(
            virtual_stack(
                move || {
                    let dap = terminal.get_active_dap(true);
                    dap.map(|dap| {
                        if !dap.stopped.get() {
                            return DapVariable::default();
                        }
                        let process_stopped = terminal
                            .get_terminal(&dap.term_id)
                            .and_then(|t| {
                                t.run_debug.with(|r| r.as_ref().map(|r| r.stopped))
                            })
                            .unwrap_or(
                                terminal.debug.active_dap.get() != Some(dap.dap_id),
                            );
                        if process_stopped {
                            return DapVariable::default();
                        }
                        let mut values = dap.variables.get();
                        if terminal.common.debugger_settings.get().sort_variables {
                            crate::debug_settings::sort_variables(&mut values);
                        }
                        values
                    })
                    .unwrap_or_default()
                },
                |node| {
                    (
                        node.item.name().to_string(),
                        node.item.value().map(|v| v.to_string()),
                        node.item.reference(),
                        node.expanded,
                        node.level,
                    )
                },
                move |node| {
                    let local_terminal = local_terminal.clone();
                    let level = node.level;
                    let reference = node.item.reference();
                    let name = node.item.name();
                    let ty = node.item.ty();
                    let type_exists = ty.map(|ty| !ty.is_empty()).unwrap_or(false);
                    let has_value = node.item.value().is_some();
                    stack((
                        svg(move || {
                            let config = config.get();
                            let svg_str = match node.expanded {
                                true => SparrowIcons::ITEM_OPENED,
                                false => SparrowIcons::ITEM_CLOSED,
                            };
                            config.ui_svg(svg_str)
                        })
                        .style(move |s| {
                            let config = config.get();
                            let size = config.ui.icon_size() as f32;

                            let color = if reference > 0 {
                                config.color(SparrowColor::SPARROW_ICON_ACTIVE)
                            } else {
                                Color::TRANSPARENT
                            };
                            s.size(size, size).margin_left(10.0).color(color)
                        }),
                        text(name),
                        text(": ").style(move |s| {
                            s.apply_if(!type_exists || reference == 0, |s| s.hide())
                        }),
                        text(node.item.ty().unwrap_or("")).style(move |s| {
                            s.color(config.get().style_color("type").unwrap())
                                .apply_if(!type_exists || reference == 0, |s| {
                                    s.hide()
                                })
                        }),
                        text(format!(" = {}", node.item.value().unwrap_or("")))
                            .style(move |s| s.apply_if(!has_value, |s| s.hide())),
                    ))
                    .on_click_stop(move |_| {
                        if reference > 0 {
                            let dap = local_terminal.get_active_dap(false);
                            if let Some(dap) = dap {
                                let process_stopped = local_terminal
                                    .get_terminal(&dap.term_id)
                                    .and_then(|t| {
                                        t.run_debug
                                            .with(|r| r.as_ref().map(|r| r.stopped))
                                    })
                                    .unwrap_or(
                                        local_terminal
                                            .debug
                                            .active_dap
                                            .get_untracked()
                                            != Some(dap.dap_id),
                                    );
                                if !process_stopped {
                                    dap.toggle_expand(
                                        node.parent.clone(),
                                        reference,
                                    );
                                }
                            }
                        }
                    })
                    .style(move |s| {
                        s.items_center()
                            .padding_right(10.0)
                            .padding_left((level * 10) as f32)
                            .min_width_pct(100.0)
                            .hover(|s| {
                                s.apply_if(reference > 0, |s| {
                                    s.background(config.get().color(
                                        SparrowColor::PANEL_HOVERED_BACKGROUND,
                                    ))
                                })
                            })
                    })
                },
            )
            .item_size_fixed(move || ui_line_height.get())
            .style(|s| s.flex_col().min_width_full()),
        )
        .style(|s| s.absolute().size_full()),
    )
    .style(|s| {
        s.width_full()
            .line_height(1.6)
            .flex_grow(1.0f32)
            .flex_basis(0)
    })
}
