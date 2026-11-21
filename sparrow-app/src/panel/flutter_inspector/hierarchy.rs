//! Virtualized widget hierarchy and property pane layout.

use floem::prelude::SignalWith;
use std::{ops::Range, sync::Arc};

use floem::{
    IntoView, View,
    reactive::{ReadSignal, RwSignal, SignalGet, SignalUpdate},
    style::CursorStyle,
    views::{
        Decorators, VirtualVector, empty, label, stack, svg, virtual_stack,
    },
};

use crate::{
    config::{SparrowConfig, color::SparrowColor, icon::SparrowIcons},
    dart::vm_service::{FlutterInspectorData, InspectorNodeData},
    widgets::clickable_icon,
};
use crate::widgets::sparrow_scroll;
use super::properties::property_inspector_view;

pub(super) fn tree_and_properties_view(
    data: &FlutterInspectorData,
    config: ReadSignal<Arc<SparrowConfig>>,
    ui_line_height: floem::reactive::Memo<f64>,
) -> impl View + use<> {
    let root = data.root.read_only();
    let selected_node = data.selected_node;

    stack((
        // Top section: Widget Tree Hierarchy
        sparrow_scroll(
            virtual_stack(
                move || TreeList::new(root.get()),
                move |(i, _, node)| (*i, node.get_untracked().node.value_id.clone()),
                {
                    let sel = selected_node;
                    move |(_, depth, node)| inspector_row(depth, node, sel, config)
                },
            )
            .item_size_fixed(move || ui_line_height.get())
            .style(|s| s.absolute().flex_col().min_width_full()),
        )
        .style(move |s| {
            let c = config.get();
            s.width_pct(100.0)
                .flex_grow(1.0_f32)
                .border_bottom(1.0)
                .border_color(c.color(SparrowColor::SPARROW_BORDER))
        }),
        // Bottom section: Property Details Inspector
        property_inspector_view(selected_node, config, ui_line_height),
    ))
    .style(|s| s.flex_col().size_pct(100.0, 100.0))
}

fn inspector_row(
    depth: usize,
    node: RwSignal<InspectorNodeData>,
    selected_node: RwSignal<Option<RwSignal<InspectorNodeData>>>,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    let (type_name, description, has_children, open) = node.with_untracked(|n| {
        (
            n.node.type_name.clone(),
            short_description(&n.node.description),
            !n.children.is_empty(),
            n.open,
        )
    });
    let indent = (depth as f32) * 10.0;
    let node_copy = node;

    let is_selected = move || {
        selected_node.with(|sel| sel.map(|s| s == node_copy).unwrap_or(false))
    };

    stack((
        chevron(open, has_children, config)
            .style(move |s| s.margin_left(indent).margin_right(2.0)),
        svg(move || config.get().ui_svg(SparrowIcons::TYPE_HIERARCHY)).style(
            move |s| {
                let c = config.get();
                let size = c.ui.icon_size() as f32;
                s.size(size, size)
                    .margin_right(6.0)
                    .color(c.color(SparrowColor::SPARROW_ICON_ACTIVE))
            },
        ),
        label(move || description.clone()).style(|s| {
            s.selectable(false)
                .min_width(0.0)
                .flex_basis(0.0)
                .flex_grow(1.0f32)
                .text_ellipsis()
        }),
        label(move || type_name.clone()).style(move |s| {
            s.color(config.get().color(SparrowColor::EDITOR_DIM))
                .margin_right(10.0)
                .selectable(false)
                .text_ellipsis()
        }),
    ))
    .on_click_stop(move |_| {
        selected_node.set(Some(node_copy));
        if has_children {
            open.update(|o| *o = !*o);
        }
    })
    .style(move |s| {
        let c = config.get();
        s.padding_left(10.0)
            .items_center()
            .width_pct(100.0)
            .cursor(CursorStyle::Pointer)
            .apply_if(is_selected(), |s| {
                s.background(c.color(SparrowColor::PANEL_HOVERED_BACKGROUND))
            })
            .hover(|s| s.background(c.color(SparrowColor::PANEL_HOVERED_BACKGROUND)))
    })
}

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
        empty().style(|s| s.size(16.0, 16.0)).into_any()
    }
}

fn short_description(s: &str) -> String {
    let first_line = s.lines().next().unwrap_or(s);
    if first_line.chars().count() > 120 {
        let mut truncated: String = first_line.chars().take(117).collect();
        truncated.push_str("...");
        truncated
    } else {
        first_line.to_string()
    }
}

struct TreeList {
    rows: Vec<(usize, RwSignal<InspectorNodeData>)>,
}

impl TreeList {
    fn new(root: Option<RwSignal<InspectorNodeData>>) -> Self {
        let mut rows = Vec::new();
        if let Some(root) = root {
            flatten_node(&root, 0, &mut rows);
        }
        Self { rows }
    }
}

fn flatten_node(
    node: &RwSignal<InspectorNodeData>,
    depth: usize,
    out: &mut Vec<(usize, RwSignal<InspectorNodeData>)>,
) {
    out.push((depth, *node));
    let n = node.get();
    if n.open.get() {
        for child in &n.children {
            flatten_node(child, depth + 1, out);
        }
    }
}

impl VirtualVector<(usize, usize, RwSignal<InspectorNodeData>)> for TreeList {
    fn total_len(&self) -> usize {
        self.rows.len()
    }

    fn slice(
        &mut self,
        range: Range<usize>,
    ) -> impl Iterator<Item = (usize, usize, RwSignal<InspectorNodeData>)> {
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
