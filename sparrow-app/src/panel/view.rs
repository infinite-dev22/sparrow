use std::{rc::Rc, sync::Arc};

use floem::{
    AnyView, IntoView, View,
    event::{Event, EventListener, EventPropagation},
    kurbo::{Point, Size},
    reactive::{
        ReadSignal, RwSignal, SignalGet, SignalUpdate, SignalWith, create_rw_signal,
    },
    style::{CursorStyle, Style},
    taffy::AlignItems,
    unit::PxPctAuto,
    views::{
        Decorators, container, dyn_stack, empty, h_stack, label, stack,
        stack_from_iter, svg, tab, text,
    },
};

use super::{
    debug_view::debug_panel,
    global_search_view::global_search_panel,
    kind::PanelKind,
    plugin_view::plugin_panel,
    position::{PanelContainerPosition, PanelPosition},
    problem_view::problem_panel,
    source_control_view::source_control_panel,
    terminal_view::terminal_panel,
};
use crate::{
    app::{clickable_icon, clickable_icon_base},
    config::{SparrowConfig, color::SparrowColor, icon::SparrowIcons},
    file_explorer::view::file_explorer_panel,
    panel::{
        call_hierarchy_view::show_hierarchy_panel, document_symbol::symbol_panel,
        implementation_view::implementation_panel,
        references_view::references_panel,
    },
    window_tab::{DragContent, WindowTabData},
};

/// Radius of the rounded "islands" (tool windows and editor area).
pub const ISLAND_RADIUS: f32 = 10.0;

fn island_corner(
    config: ReadSignal<Arc<SparrowConfig>>,
    path: &'static str,
    top: bool,
    left: bool,
) -> impl View {
    let svg_str = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10" width="10" height="10"><path d="{path}" fill="currentColor"/></svg>"#
    );
    svg(move || svg_str.clone()).style(move |s| {
        let color = config.get().color(SparrowColor::SPARROW_WINDOW_BACKGROUND);
        s.absolute()
            .size(ISLAND_RADIUS, ISLAND_RADIUS)
            .color(color)
            .apply_if(top, |s| s.inset_top(0.0))
            .apply_if(!top, |s| s.inset_bottom(0.0))
            .apply_if(left, |s| s.inset_left(0.0))
            .apply_if(!left, |s| s.inset_right(0.0))
    })
}

/// floem does not clip children to a rounded parent, so rounded corners are
/// faked by painting four quarter-circle masks in the window-frame colour on
/// top of the island's corners.
pub fn island_corners(config: ReadSignal<Arc<SparrowConfig>>) -> impl View {
    stack((
        island_corner(config, "M0 0H10A10 10 0 0 0 0 10Z", true, true),
        island_corner(config, "M0 0H10V10A10 10 0 0 0 0 0Z", true, false),
        island_corner(config, "M0 0V10H10A10 10 0 0 1 0 0Z", false, true),
        island_corner(config, "M10 0V10H0A10 10 0 0 0 10 0Z", false, false),
    ))
        .style(|s| s.absolute().size_pct(100.0, 100.0).z_index(40).pointer_events_none())
}

pub fn foldable_panel_section(
    header: impl View + 'static,
    child: impl View + 'static,
    open: RwSignal<bool>,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    stack((
        h_stack((
            clickable_icon_base(
                move || {
                    if open.get() {
                        SparrowIcons::PANEL_FOLD_DOWN
                    } else {
                        SparrowIcons::PANEL_FOLD_UP
                    }
                },
                None::<Box<dyn Fn()>>,
                || false,
                || false,
                config,
            ),
            header.style(|s| s.align_items(AlignItems::Center).padding_left(3.0)),
        ))
            .style(move |s| {
                s.padding_horiz(10.0)
                    .padding_vert(6.0)
                    .width_pct(100.0)
                    .items_center()
                    .cursor(CursorStyle::Pointer)
                    .background(config.get().color(SparrowColor::PANEL_BACKGROUND))
            })
            .on_click_stop(move |_| {
                open.update(|open| *open = !*open);
            }),
        child.style(move |s| s.apply_if(!open.get(), |s| s.hide())),
    ))
}

/// A builder for creating a foldable panel out of sections
pub struct PanelBuilder {
    views: Vec<AnyView>,
    config: ReadSignal<Arc<SparrowConfig>>,
    position: PanelPosition,
}
impl PanelBuilder {
    pub fn new(
        config: ReadSignal<Arc<SparrowConfig>>,
        position: PanelPosition,
    ) -> Self {
        Self {
            views: Vec::new(),
            config,
            position,
        }
    }

    fn add_general(
        mut self,
        name: &'static str,
        height: Option<PxPctAuto>,
        view: impl View + 'static,
        open: RwSignal<bool>,
        style: impl Fn(Style) -> Style + 'static,
    ) -> Self {
        let position = self.position;
        let view = foldable_panel_section(
            text(name).style(move |s| s.selectable(false)),
            view,
            open,
            self.config,
        )
            .style(move |s| {
                let s = s.width_full().flex_col();
                // Use the manual height if given, otherwise if we're open behave flex,
                // otherwise, do nothing so that there's no height
                let s = if open.get() {
                    if let Some(height) = height {
                        s.height(height)
                    } else {
                        s.flex_grow(1.0f32).flex_basis(0.0)
                    }
                } else if position.is_bottom() {
                    s.flex_grow(0.3f32).flex_basis(0.0)
                } else {
                    s
                };

                style(s)
            });
        self.views.push(view.into_any());
        self
    }

    /// Add a view to the panel
    pub fn add(
        self,
        name: &'static str,
        view: impl View + 'static,
        open: RwSignal<bool>,
    ) -> Self {
        self.add_general(name, None, view, open, std::convert::identity)
    }

    /// Add a view to the panel with a custom style applied to the overall header+section-content
    pub fn add_style(
        self,
        name: &'static str,
        view: impl View + 'static,
        open: RwSignal<bool>,
        style: impl Fn(Style) -> Style + 'static,
    ) -> Self {
        self.add_general(name, None, view, open, style)
    }

    /// Add a view to the panel with a custom height that is only used when the panel is open
    pub fn add_height(
        self,
        name: &'static str,
        height: impl Into<PxPctAuto>,
        view: impl View + 'static,
        open: RwSignal<bool>,
    ) -> Self {
        self.add_general(
            name,
            Some(height.into()),
            view,
            open,
            std::convert::identity,
        )
    }

    /// Add a view to the panel with a custom height that is only used when the panel is open
    /// and a custom style applied to the overall header+section-content
    pub fn add_height_style(
        self,
        name: &'static str,
        height: impl Into<PxPctAuto>,
        view: impl View + 'static,
        open: RwSignal<bool>,
        style: impl Fn(Style) -> Style + 'static,
    ) -> Self {
        self.add_general(name, Some(height.into()), view, open, style)
    }

    /// Add a view to the panel with a custom height that is only used when the panel is open
    pub fn add_height_pct(
        self,
        name: &'static str,
        height: f64,
        view: impl View + 'static,
        open: RwSignal<bool>,
    ) -> Self {
        self.add_general(
            name,
            Some(PxPctAuto::Pct(height)),
            view,
            open,
            std::convert::identity,
        )
    }

    /// Build the panel into a view
    pub fn build(self) -> impl View {
        stack_from_iter(self.views).style(move |s| {
            s.width_full()
                .apply_if(!self.position.is_bottom(), |s| s.flex_col())
        })
    }
}

pub fn panel_container_view(
    window_tab_data: Rc<WindowTabData>,
    position: PanelContainerPosition,
) -> impl View {
    let panel = window_tab_data.panel.clone();
    let config = window_tab_data.common.config;
    let dragging = window_tab_data.common.dragging;
    let current_size = create_rw_signal(Size::ZERO);
    let available_size = window_tab_data.panel.available_size;
    let is_dragging_panel = move || {
        dragging
            .with(|d| d.as_ref().map(|d| d.is_panel()))
            .unwrap_or(false)
    };
    let drop_view = {
        let panel = panel.clone();
        move |position: PanelPosition| {
            let panel = panel.clone();
            let dragging_over = create_rw_signal(false);
            empty()
                .on_event(EventListener::DragEnter, move |_| {
                    if is_dragging_panel() {
                        dragging_over.set(true);
                        EventPropagation::Stop
                    } else {
                        EventPropagation::Continue
                    }
                })
                .on_event(EventListener::DragLeave, move |_| {
                    if is_dragging_panel() {
                        dragging_over.set(false);
                        EventPropagation::Stop
                    } else {
                        EventPropagation::Continue
                    }
                })
                .on_event(EventListener::Drop, move |_| {
                    if let Some(DragContent::Panel(kind)) = dragging.get_untracked()
                    {
                        dragging_over.set(false);
                        panel.move_panel_to_position(kind, &position);
                        EventPropagation::Stop
                    } else {
                        EventPropagation::Continue
                    }
                })
                .style(move |s| {
                    s.size_pct(100.0, 100.0).apply_if(dragging_over.get(), |s| {
                        s.background(
                            config
                                .get()
                                .color(SparrowColor::EDITOR_DRAG_DROP_BACKGROUND),
                        )
                    })
                })
        }
    };

    let resize_drag_view = {
        let panel = panel.clone();
        let panel_size = panel.size;
        move |position: PanelContainerPosition| {
            panel.panel_info();
            let view = empty();
            let view_id = view.id();
            let drag_start: RwSignal<Option<Point>> = create_rw_signal(None);
            view.on_event_stop(EventListener::PointerDown, move |event| {
                view_id.request_active();
                if let Event::PointerDown(pointer_event) = event {
                    drag_start.set(Some(pointer_event.pos));
                }
            })
                .on_event_stop(EventListener::PointerMove, move |event| {
                    if let Event::PointerMove(pointer_event) = event {
                        if let Some(drag_start_point) = drag_start.get_untracked() {
                            let current_size = current_size.get_untracked();
                            let available_size = available_size.get_untracked();
                            match position {
                                PanelContainerPosition::Left => {
                                    let new_size = current_size.width
                                        + pointer_event.pos.x
                                        - drag_start_point.x;
                                    let current_panel_size = panel_size.get_untracked();
                                    let new_size = new_size
                                        .max(150.0)
                                        .min(available_size.width - 150.0 - 150.0);
                                    if new_size != current_panel_size.left {
                                        panel_size.update(|size| {
                                            size.left = new_size;
                                            size.right = size.right.min(
                                                available_size.width - new_size - 150.0,
                                            )
                                        })
                                    }
                                }
                                PanelContainerPosition::Bottom => {
                                    let new_size = current_size.height
                                        - (pointer_event.pos.y - drag_start_point.y);
                                    let maximized = panel.panel_bottom_maximized(false);
                                    if (maximized
                                        && new_size < available_size.height - 50.0)
                                        || (!maximized
                                        && new_size > available_size.height - 50.0)
                                    {
                                        panel.toggle_bottom_maximize();
                                    }

                                    let new_size = new_size
                                        .max(100.0)
                                        .min(available_size.height - 100.0);
                                    let current_size =
                                        panel_size.with_untracked(|s| s.bottom);
                                    if current_size != new_size {
                                        panel_size.update(|size| {
                                            size.bottom = new_size;
                                        })
                                    }
                                }
                                PanelContainerPosition::Right => {
                                    let new_size = current_size.width
                                        - (pointer_event.pos.x - drag_start_point.x);
                                    let current_panel_size = panel_size.get_untracked();
                                    let new_size = new_size
                                        .max(150.0)
                                        .min(available_size.width - 150.0 - 150.0);
                                    if new_size != current_panel_size.right {
                                        panel_size.update(|size| {
                                            size.right = new_size;
                                            size.left = size.left.min(
                                                available_size.width - new_size - 150.0,
                                            )
                                        })
                                    }
                                }
                            }
                        }
                    }
                })
                .on_event_stop(EventListener::PointerUp, move |_| {
                    drag_start.set(None);
                })
                .style(move |s| {
                    let is_dragging = drag_start.get().is_some();
                    let current_size = current_size.get();
                    let config = config.get();
                    s.absolute()
                        .apply_if(position == PanelContainerPosition::Bottom, |s| {
                            s.width_pct(100.0).height(4.0).margin_top(-2.0)
                        })
                        .apply_if(position == PanelContainerPosition::Left, |s| {
                            s.width(4.0)
                                .margin_left(current_size.width as f32 - 2.0)
                                .height_pct(100.0)
                        })
                        .apply_if(position == PanelContainerPosition::Right, |s| {
                            s.width(4.0).margin_left(-2.0).height_pct(100.0)
                        })
                        .apply_if(is_dragging, |s| {
                            s.background(config.color(SparrowColor::EDITOR_CARET))
                                .apply_if(
                                    position == PanelContainerPosition::Bottom,
                                    |s| s.cursor(CursorStyle::RowResize),
                                )
                                .apply_if(
                                    position != PanelContainerPosition::Bottom,
                                    |s| s.cursor(CursorStyle::ColResize),
                                )
                                .z_index(2)
                        })
                        .hover(|s| {
                            s.background(config.color(SparrowColor::EDITOR_CARET))
                                .apply_if(
                                    position == PanelContainerPosition::Bottom,
                                    |s| s.cursor(CursorStyle::RowResize),
                                )
                                .apply_if(
                                    position != PanelContainerPosition::Bottom,
                                    |s| s.cursor(CursorStyle::ColResize),
                                )
                                .z_index(2)
                        })
                })
        }
    };

    let is_bottom = position.is_bottom();
    stack((
        panel_view(window_tab_data.clone(), position.first()),
        panel_view(window_tab_data.clone(), position.second()),
        resize_drag_view(position),
        island_corners(config),
        stack((drop_view(position.first()), drop_view(position.second()))).style(
            move |s| {
                let is_dragging_panel = is_dragging_panel();
                s.absolute()
                    .size_pct(100.0, 100.0)
                    .apply_if(!is_bottom, |s| s.flex_col())
                    .apply_if(!is_dragging_panel, |s| s.pointer_events_none())
            },
        ),
    ))
        .on_resize(move |rect| {
            let size = rect.size();
            if size != current_size.get_untracked() {
                current_size.set(size);
            }
        })
        .style(move |s| {
            let size = panel.size.with(|s| match position {
                PanelContainerPosition::Left => s.left,
                PanelContainerPosition::Bottom => s.bottom,
                PanelContainerPosition::Right => s.right,
            });
            let is_maximized = panel.panel_bottom_maximized(true);
            let config = config.get();
            s.apply_if(!panel.is_container_shown(&position, true), |s| s.hide())
                .apply_if(position == PanelContainerPosition::Bottom, |s| {
                    s.width_pct(100.0)
                        .apply_if(!is_maximized, |s| s.height(size as f32))
                        .apply_if(is_maximized, |s| s.flex_grow(1.0f32))
                        .background(config.color(SparrowColor::PANEL_BACKGROUND))
                })
                .apply_if(position == PanelContainerPosition::Left, |s| {
                    s.width(size as f32)
                        .height_pct(100.0)
                        .background(config.color(SparrowColor::PANEL_BACKGROUND))
                })
                .apply_if(position == PanelContainerPosition::Right, |s| {
                    s.width(size as f32)
                        .height_pct(100.0)
                        .background(config.color(SparrowColor::PANEL_BACKGROUND))
                })
                .apply_if(!is_bottom, |s| s.flex_col())
                .border_radius(ISLAND_RADIUS)
                .color(config.color(SparrowColor::PANEL_FOREGROUND))
        })
        .debug_name(format!("{:?} Panel Container View", position))
}

fn panel_view(
    window_tab_data: Rc<WindowTabData>,
    position: PanelPosition,
) -> impl View {
    let panel = window_tab_data.panel.clone();
    let panels = move || {
        panel
            .panels
            .with(|p| p.get(&position).cloned().unwrap_or_default())
    };
    let active_fn = move || {
        panel
            .styles
            .with(|s| s.get(&position).map(|s| s.active).unwrap_or(0))
    };
    tab(
        active_fn,
        panels,
        |p| *p,
        move |kind| {
            let view = match kind {
                PanelKind::Terminal => {
                    terminal_panel(window_tab_data.clone()).into_any()
                }
                PanelKind::FileExplorer => {
                    file_explorer_panel(window_tab_data.clone(), position).into_any()
                }
                PanelKind::SourceControl => {
                    source_control_panel(window_tab_data.clone(), position)
                        .into_any()
                }
                PanelKind::Plugin => {
                    plugin_panel(window_tab_data.clone(), position).into_any()
                }
                PanelKind::Search => {
                    global_search_panel(window_tab_data.clone(), position).into_any()
                }
                PanelKind::Problem => {
                    problem_panel(window_tab_data.clone(), position).into_any()
                }
                PanelKind::Debug => {
                    debug_panel(window_tab_data.clone(), position).into_any()
                }
                PanelKind::CallHierarchy => {
                    show_hierarchy_panel(window_tab_data.clone(), position)
                        .into_any()
                }
                PanelKind::DocumentSymbol => {
                    symbol_panel(window_tab_data.clone(), position).into_any()
                }
                PanelKind::References => {
                    references_panel(window_tab_data.clone(), position).into_any()
                }
                PanelKind::Implementation => {
                    implementation_panel(window_tab_data.clone(), position)
                        .into_any()
                }
            };
            view.style(|s| s.size_pct(100.0, 100.0))
        },
    )
        .style(move |s| {
            s.size_pct(100.0, 100.0).apply_if(
                !panel.is_position_shown(&position, true)
                    || panel.is_position_empty(&position, true),
                |s| s.hide(),
            )
        })
}

pub fn panel_header(
    header: String,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    container(label(move || header.clone())).style(move |s| {
        s.padding_horiz(10.0)
            .padding_vert(6.0)
            .width_pct(100.0)
            .background(config.get().color(SparrowColor::EDITOR_BACKGROUND))
    })
}

/// Android Studio style tool window stripe: a narrow vertical icon bar on the
/// outer edge of the window. The top of the stripe holds the side's first
/// position, the bottom holds its second position plus the bottom-panel
/// position on the same side, so every panel icon keeps a home.
pub fn tool_window_stripe(
    window_tab_data: Rc<WindowTabData>,
    is_left: bool,
) -> impl View {
    let config = window_tab_data.common.config;
    let (top, lower, bottom) = if is_left {
        (
            PanelPosition::LeftTop,
            PanelPosition::LeftBottom,
            PanelPosition::BottomLeft,
        )
    } else {
        (
            PanelPosition::RightTop,
            PanelPosition::RightBottom,
            PanelPosition::BottomRight,
        )
    };
    stack((
        stripe_picker(window_tab_data.clone(), top),
        empty().style(|s| s.flex_grow(1.0f32)),
        stripe_picker(window_tab_data.clone(), lower),
        stripe_picker(window_tab_data, bottom),
    ))
        .style(move |s| {
            let config = config.get();
            s.flex_col()
                .items_center()
                .width(34.0)
                .height_pct(100.0)
                .flex_shrink(0.0f32)
                .padding_vert(4.0)
                .background(config.color(SparrowColor::SPARROW_WINDOW_BACKGROUND))
                .apply_if(is_left, |s| s.margin_right(-3.0))
                .apply_if(!is_left, |s| s.margin_left(-3.0))
        })
        .debug_name("Tool Window Stripe")
}

fn stripe_picker(
    window_tab_data: Rc<WindowTabData>,
    position: PanelPosition,
) -> impl View {
    let panel = window_tab_data.panel.clone();
    let panels = panel.panels;
    let config = window_tab_data.common.config;
    let dragging = window_tab_data.common.dragging;
    dyn_stack(
        move || {
            panel
                .panels
                .with(|panels| panels.get(&position).cloned().unwrap_or_default())
        },
        |p| *p,
        move |p| {
            let window_tab_data = window_tab_data.clone();
            let tooltip = match p {
                PanelKind::Terminal => "Terminal",
                PanelKind::FileExplorer => "File Explorer",
                PanelKind::SourceControl => "Source Control",
                PanelKind::Plugin => "Plugins",
                PanelKind::Search => "Search",
                PanelKind::Problem => "Problems",
                PanelKind::Debug => "Debug",
                PanelKind::CallHierarchy => "Call Hierarchy",
                PanelKind::DocumentSymbol => "Document Symbol",
                PanelKind::References => "References",
                PanelKind::Implementation => "Implementation",
            };
            let icon = p.svg_name();
            let is_active = {
                let window_tab_data = window_tab_data.clone();
                move || {
                    if let Some((active_panel, shown)) = window_tab_data
                        .panel
                        .active_panel_at_position(&position, true)
                    {
                        shown && active_panel == p
                    } else {
                        false
                    }
                }
            };
            container(
                clickable_icon(
                    || icon,
                    move || {
                        window_tab_data.toggle_panel_visual(p);
                    },
                    || false,
                    || false,
                    move || tooltip,
                    config,
                )
                    .draggable()
                    .on_event_stop(EventListener::DragStart, move |_| {
                        dragging.set(Some(DragContent::Panel(p)));
                    })
                    .on_event_stop(EventListener::DragEnd, move |_| {
                        dragging.set(None);
                    })
                    .dragging_style(move |s| {
                        let config = config.get();
                        s.border(1.0)
                            .border_radius(6.0)
                            .border_color(config.color(SparrowColor::SPARROW_BORDER))
                            .padding(6.0)
                            .background(
                                config
                                    .color(SparrowColor::PANEL_BACKGROUND)
                                    .multiply_alpha(0.7),
                            )
                    }),
            )
                .style(move |s| {
                    let config = config.get();
                    s.padding(1.0).margin_vert(2.0).border_radius(6.0).apply_if(
                        is_active(),
                        |s| s.background(config.color(SparrowColor::ACTIVITY_CURRENT)),
                    )
                })
        },
    )
        .style(move |s| {
            s.flex_col().items_center().apply_if(
                panels.with(|p| p.get(&position).map(|p| p.is_empty()).unwrap_or(true)),
                |s| s.hide(),
            )
        })
}