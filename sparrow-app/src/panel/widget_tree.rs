//! Widget-tree panel: hierarchical view of every widget instantiation in the
//! active Dart file.

use std::{ops::Range, rc::Rc, sync::Arc};

use floem::{
    IntoView, View,
    reactive::{ReadSignal, RwSignal, SignalGet, SignalUpdate, SignalWith},
    style::CursorStyle,
    views::{
        Decorators, VirtualVector, empty, label, scroll, stack, svg,
        virtual_stack,
    },
};


use super::position::PanelPosition;
use crate::{
    app::clickable_icon,
    command::InternalCommand,
    config::{SparrowConfig, color::SparrowColor, icon::SparrowIcons},
    editor::location::{EditorLocation, EditorPosition},
    widget_tree::WidgetNodeData,
    window_tab::WindowTabData,
};

pub fn widget_tree_panel(
    window_tab_data: Rc<WindowTabData>,
    _position: PanelPosition,
) -> impl View {
    let data = window_tab_data.widget_tree.clone();
    let config = window_tab_data.common.config;
    let ui_line_height = window_tab_data.common.ui_line_height;
    let internal_command = window_tab_data.common.internal_command;
    let source_path = data.source_path.read_only();

    scroll(
        virtual_stack(
            move || WidgetList::new(data.roots.get()),
            move |(i, _, node)| (*i, node.get_untracked().info.class_name.clone()),
            move |(_, depth, node)| {
                widget_row(depth, node, config, internal_command, source_path)
            },
        )
            .item_size_fixed(move || ui_line_height.get())
            .style(|s| s.absolute().flex_col().min_width_full()),
    )
        .style(|s| s.absolute().size_full())
        .debug_name("Widget Tree Panel")
}

fn widget_row(
    depth: usize,
    node: RwSignal<WidgetNodeData>,
    config: ReadSignal<Arc<SparrowConfig>>,
    internal_command: crate::listener::Listener<InternalCommand>,
    source_path: ReadSignal<Option<std::path::PathBuf>>,
) -> impl View {
    let (kind, class_name, has_children, jump_target, open) = node.with_untracked(|n| {
        (
            n.info.kind(),
            n.info.class_name.clone(),
            !n.children.is_empty(),
            n.info.source_range.or(n.info.instantiation_range),
            n.open,
        )
    });
    let indent = (depth as f32) * 10.0;
    let kind_label = kind.label();

    stack((
        chevron(open, has_children, config)
            .style(move |s| s.margin_left(indent).margin_right(2.0)),
        svg(move || config.get().ui_svg(kind.icon())).style(move |s| {
            let c = config.get();
            let size = c.ui.icon_size() as f32;
            s.size(size, size)
                .margin_right(6.0)
                .color(c.color(SparrowColor::SPARROW_ICON_ACTIVE))
        }),
        label(move || class_name.clone()).style(|s| {
            s.selectable(false)
                .min_width(0.0)
                .flex_basis(0.0)
                .flex_grow(1.0f32)
                .text_ellipsis()
        }),
        label(move || kind_label).style(move |s| {
            s.color(config.get().color(SparrowColor::EDITOR_DIM))
                .margin_right(10.0)
                .selectable(false)
        }),
    ))
        .on_click_stop(move |_| {
            if let (Some((start, _)), Some(path)) =
                (jump_target, source_path.get_untracked())
            {
                internal_command.send(InternalCommand::JumpToLocation {
                    location: EditorLocation {
                        path,
                        position: Some(EditorPosition::Offset(start)),
                        scroll_offset: None,
                        ignore_unconfirmed: false,
                        same_editor_tab: false,
                    },
                });
            }
        })
        .style(move |s| {
            s.padding_left(10.0)
                .items_center()
                .width_pct(100.0)
                .cursor(CursorStyle::Pointer)
                .hover(|s| {
                    s.background(
                        config
                            .get()
                            .color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                    )
                })
        })
}

/// Chevron that toggles a node's `open` signal. For leaves, renders an
/// invisible 16×16 spacer so text stays aligned with siblings.
fn chevron(
    open: RwSignal<bool>,
    has_children: bool,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    if has_children {
        clickable_icon(
            move || {
                if open.get() {
                    SparrowIcons::ITEM_OPENED
                } else {
                    SparrowIcons::ITEM_CLOSED
                }
            },
            move || open.update(|o| *o = !*o),
            || false,
            || false,
            || "",
            config,
        )
            .style(|s| s.size(16.0, 16.0).padding(0.0))
            .into_any()
    } else {
        empty()
            .style(|s| s.size(16.0, 16.0))
            .into_any()
    }
}

/// Flattened-view vector for `virtual_stack`.
struct WidgetList {
    rows: Vec<(usize, RwSignal<WidgetNodeData>)>,
}

impl WidgetList {
    fn new(roots: Vec<RwSignal<WidgetNodeData>>) -> Self {
        let mut rows = Vec::new();
        flatten(&roots, 0, &mut rows);
        Self { rows }
    }
}

fn flatten(
    nodes: &[RwSignal<WidgetNodeData>],
    depth: usize,
    out: &mut Vec<(usize, RwSignal<WidgetNodeData>)>,
) {
    for node in nodes {
        out.push((depth, *node));
        // `.get()` (not `_untracked`) so collapsing/expanding a node rebuilds
        // the list.
        let n = node.get();
        if n.open.get() {
            flatten(&n.children, depth + 1, out);
        }
    }
}

impl VirtualVector<(usize, usize, RwSignal<WidgetNodeData>)> for WidgetList {
    fn total_len(&self) -> usize {
        self.rows.len()
    }

    fn slice(
        &mut self,
        range: Range<usize>,
    ) -> impl Iterator<Item = (usize, usize, RwSignal<WidgetNodeData>)> {
        let start = range.start;
        self.rows
            .iter()
            .skip(start)
            .take(range.len())
            .enumerate()
            .map(|(i, (depth, node))| (i + start, *depth, *node))
            .collect::<Vec<_>>()
            .into_iter()
    }
}