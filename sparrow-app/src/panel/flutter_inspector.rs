//! Live widget inspector panel — reads the running Flutter app's widget
//! tree via the Dart VM service, allows property inspection, and offers
//! quick-toggle runtime controls.

use std::{ops::Range, rc::Rc, sync::Arc};
use floem::prelude::SignalWith;

use floem::{
    IntoView, View,
    reactive::{create_rw_signal, ReadSignal, RwSignal, SignalGet, SignalUpdate},
    style::CursorStyle,
    views::{
        Decorators, VirtualVector, empty, label, scroll, stack, svg,
        virtual_stack,
    },
};

use super::position::PanelPosition;
use crate::{
    app::clickable_icon,
    config::{SparrowConfig, color::SparrowColor, icon::SparrowIcons},
    dart::vm_service::{
        FlutterInspectorData, InspectorNodeData, InspectorState,
    },
    window_tab::WindowTabData,
};

pub fn flutter_inspector_panel(
    window_tab_data: Rc<WindowTabData>,
    _position: PanelPosition,
) -> impl View {
    let data = window_tab_data.flutter_inspector.clone();
    let config = window_tab_data.common.config;
    let ui_line_height = window_tab_data.common.ui_line_height;
    let state = data.state.read_only();

    // VM runtime toggle flags
    let debug_paint = create_rw_signal(false);
    let perf_overlay = create_rw_signal(false);
    let slow_animations = create_rw_signal(false);
    let repaint_rainbow = create_rw_signal(false);

    let client = data.client.clone();

    stack((
        status_bar(&data, config, state),
        // Quick toggle action bar
        toggle_bar(
            client,
            debug_paint,
            perf_overlay,
            slow_animations,
            repaint_rainbow,
            state,
            config,
        ),
        // Split view: Left/Top is Widget Hierarchy, Right/Bottom is Property Inspector
        tree_and_properties_view(&data, config, ui_line_height),
    ))
    .style(|s| s.absolute().size_full().flex_col())
    .debug_name("Flutter Inspector Panel")
}

fn status_bar(
    data: &FlutterInspectorData,
    config: ReadSignal<Arc<SparrowConfig>>,
    state: ReadSignal<InspectorState>,
) -> impl View + use<> {
    let data_refresh = data.clone();
    let data_disconnect = data.clone();

    stack((
        label(move || match state.get() {
            InspectorState::Disconnected => "Not connected".to_string(),
            InspectorState::Connecting => "Connecting…".to_string(),
            InspectorState::Connected => {
                let url = data_refresh
                    .vm_url
                    .get_untracked()
                    .map(|(_, u)| shorten_url(&u))
                    .unwrap_or_default();
                format!("Live · {url}")
            }
        })
        .style(move |s| {
            let c = config.get();
            s.color(c.color(SparrowColor::PANEL_FOREGROUND_DIM))
                .font_size(11.0)
                .margin_left(10.0)
                .selectable(false)
        })
        .into_any(),
        empty().style(|s| s.flex_grow(1.0f32)),
        clickable_icon(
            || SparrowIcons::IMAGE_LOADING,
            move || {
                // Reconnect / refresh from scratch
                if let Some((_, url)) = data_refresh.vm_url.get_untracked() {
                    data_refresh.client.connect(url);
                }
            },
            || false,
            || false,
            || "Refresh Widget Tree",
            config,
        ),
        clickable_icon(
            || SparrowIcons::CLOSE,
            move || data_disconnect.disconnect(),
            || false,
            move || state.get() == InspectorState::Disconnected,
            || "Disconnect",
            config,
        )
        .style(|s| s.margin_right(4.0)),
    ))
    .style(move |s| {
        let c = config.get();
        s.height(26.0)
            .items_center()
            .border_bottom(1.0)
            .border_color(c.color(SparrowColor::SPARROW_BORDER))
    })
    .debug_name("Flutter Inspector Status")
}

fn toggle_bar(
    client: crate::dart::vm_service::VmServiceClient,
    debug_paint: RwSignal<bool>,
    perf_overlay: RwSignal<bool>,
    slow_animations: RwSignal<bool>,
    repaint_rainbow: RwSignal<bool>,
    state: ReadSignal<InspectorState>,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    stack((
        quick_toggle_icon(
            || SparrowIcons::FLUTTER_INSPECTOR,
            debug_paint,
            {
                let c = client.clone();
                move |v| c.set_debug_paint(None, v)
            },
            "Debug Paint",
            state,
            config,
        ),
        quick_toggle_icon(
            || SparrowIcons::FLUTTER_PERFORMANCE,
            perf_overlay,
            {
                let c = client.clone();
                move |v| c.set_performance_overlay(None, v)
            },
            "Performance Overlay",
            state,
            config,
        ),
        quick_toggle_icon(
            || SparrowIcons::FLUTTER_HOT_RELOAD,
            slow_animations,
            {
                let c = client.clone();
                move |v| c.set_slow_animations(None, v)
            },
            "Slow Animations (5x)",
            state,
            config,
        ),
        quick_toggle_icon(
            || SparrowIcons::SYMBOL_COLOR,
            repaint_rainbow,
            {
                let c = client.clone();
                move |v| c.set_repaint_rainbow(None, v)
            },
            "Repaint Rainbow",
            state,
            config,
        ),
        empty().style(|s| s.flex_grow(1.0_f32)),
        clickable_icon(
            || SparrowIcons::FLUTTER_HOT_RESTART,
            {
                let c = client.clone();
                move || c.reassemble(None)
            },
            || false,
            move || state.get() != InspectorState::Connected,
            || "Reassemble UI Subtree",
            config,
        )
        .style(|s| s.margin_right(4.0)),
    ))
    .style(move |s| {
        let c = config.get();
        s.height(24.0)
            .items_center()
            .padding_horiz(4.0)
            .border_bottom(1.0)
            .border_color(c.color(SparrowColor::SPARROW_BORDER))
            .background(c.color(SparrowColor::PANEL_HOVERED_BACKGROUND))
    })
}

fn quick_toggle_icon(
    icon: impl Fn() -> &'static str + 'static,
    flag: RwSignal<bool>,
    on_toggle: impl Fn(bool) + 'static,
    tooltip: &'static str,
    state: ReadSignal<InspectorState>,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    clickable_icon(
        icon,
        move || {
            flag.update(|v| {
                *v = !*v;
                on_toggle(*v);
            });
        },
        move || flag.get(),
        move || state.get() != InspectorState::Connected,
        move || tooltip,
        config,
    )
}

fn tree_and_properties_view(
    data: &FlutterInspectorData,
    config: ReadSignal<Arc<SparrowConfig>>,
    ui_line_height: floem::reactive::Memo<f64>,
) -> impl View + use<> {
    let root = data.root.read_only();
    let selected_node = data.selected_node;

    stack((
        // Top section: Widget Tree Hierarchy
        scroll(
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
        svg(move || config.get().ui_svg(SparrowIcons::TYPE_HIERARCHY)).style(move |s| {
            let c = config.get();
            let size = c.ui.icon_size() as f32;
            s.size(size, size)
                .margin_right(6.0)
                .color(c.color(SparrowColor::SPARROW_ICON_ACTIVE))
        }),
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
            .hover(|s| {
                s.background(c.color(SparrowColor::PANEL_HOVERED_BACKGROUND))
            })
    })
}

fn property_inspector_view(
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

fn shorten_url(url: &str) -> String {
    url.trim_start_matches("http://")
        .trim_end_matches('/')
        .split('/')
        .next()
        .unwrap_or(url)
        .to_string()
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

    fn slice(&mut self, range: Range<usize>) -> impl Iterator<Item = (usize, (String, String))> {
        let start = range.start.min(self.items.len());
        let end = range.end.min(self.items.len());
        self.items[start..end].iter().cloned()
    }
}