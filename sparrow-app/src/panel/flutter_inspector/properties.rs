//! Inspector property values for the selected widget.

use floem::prelude::SignalWith;
use std::{ops::Range, sync::Arc};

use floem::{
    View,
    reactive::{ReadSignal, RwSignal, SignalGet},
    views::{Decorators, VirtualVector, label, scroll, stack, virtual_stack},
};

use crate::{
    config::{SparrowConfig, color::SparrowColor},
    dart::vm_service::InspectorNodeData,
};

pub(super) fn property_inspector_view(
    selected_node: RwSignal<Option<RwSignal<InspectorNodeData>>>,
    config: ReadSignal<Arc<SparrowConfig>>,
    ui_line_height: floem::reactive::Memo<f64>,
) -> impl View {
    stack((
        // Header
        label(|| "PROPERTY INSPECTION".to_string()).style(move |s| {
            let c = config.get();
            s.font_size(10.0)
                .font_bold()
                .color(c.color(SparrowColor::EDITOR_DIM))
                .padding_left(10.0)
                .padding_vert(4.0)
                .selectable(false)
                .border_bottom(1.0)
                .border_color(c.color(SparrowColor::SPARROW_BORDER))
        }),
        // Properties content
        scroll(
            virtual_stack(
                move || {
                    let props = selected_node.with(|sel| {
                        if let Some(n) = sel {
                            n.with(|d| d.node.properties.clone())
                        } else {
                            Vec::new()
                        }
                    });
                    PropertyList::new(props)
                },
                move |(i, (k, _))| (*i, k.clone()),
                move |(_, (name, val))| property_row(name, val, config),
            )
            .item_size_fixed(move || ui_line_height.get())
            .style(|s| s.flex_col().min_width_full()),
        )
        .style(|s| s.size_pct(100.0, 100.0)),
    ))
    .style(move |s| {
        let c = config.get();
        s.height(160.0)
            .min_height(100.0)
            .flex_col()
            .width_pct(100.0)
            .background(c.color(SparrowColor::SPARROW_WINDOW_BACKGROUND))
    })
}

fn property_row(
    name: String,
    value: String,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    stack((
        label(move || name.clone()).style(move |s| {
            let c = config.get();
            s.font_size(11.0)
                .font_bold()
                .color(c.color(SparrowColor::EDITOR_FOREGROUND))
                .width(130.0)
                .min_width(80.0)
                .text_ellipsis()
                .selectable(true)
        }),
        label(move || value.clone()).style(move |s| {
            let c = config.get();
            s.font_size(11.0)
                .color(c.color(SparrowColor::SPARROW_WARN))
                .flex_grow(1.0_f32)
                .text_ellipsis()
                .selectable(true)
        }),
    ))
    .style(|s| s.padding_horiz(10.0).items_center().width_pct(100.0))
}

struct PropertyList {
    items: Vec<(usize, (String, String))>,
}

impl PropertyList {
    fn new(props: Vec<(String, String)>) -> Self {
        Self {
            items: props.into_iter().enumerate().collect(),
        }
    }
}

impl VirtualVector<(usize, (String, String))> for PropertyList {
    fn total_len(&self) -> usize {
        self.items.len()
    }

    fn slice(
        &mut self,
        range: Range<usize>,
    ) -> impl Iterator<Item = (usize, (String, String))> {
        let start = range.start.min(self.items.len());
        let end = range.end.min(self.items.len());
        self.items[start..end].iter().cloned()
    }
}
