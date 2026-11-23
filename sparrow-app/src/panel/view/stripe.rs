//! Tool window stripe, icon activation, overflow and docking.

use crate::widgets::popup_menu::PopupMenuExt;
use crate::widgets::popup_menu::{Menu, MenuItem};
use std::rc::Rc;

use floem::{
    View,
    event::EventListener,
    reactive::{SignalGet, SignalUpdate, SignalWith},
    style::CursorStyle,
    views::{Decorators, container, dyn_stack, empty, label, stack},
};

use crate::panel::{kind::PanelKind, position::PanelPosition};
use crate::{
    config::color::SparrowColor,
    window_tab::{DragContent, WindowTabData},
};

/// Android Studio style tool window stripe: a narrow vertical icon bar on the
/// outer edge of the window. The top of the stripe holds the side's first
/// position, the bottom holds its second position plus the bottom-panel
/// position on the same side, so every panel icon keeps a home.
pub fn tool_window_stripe(
    window_tab_data: Rc<WindowTabData>,
    is_left: bool,
) -> impl View {
    let config = window_tab_data.common.config;
    let overflow_window = window_tab_data.clone();
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
        label(|| "···")
            .style(move |s| {
                s.size(28.0, 28.0)
                    .margin_vert(2.0)
                    .items_center()
                    .justify_center()
                    .font_size(16.0)
                    .cursor(CursorStyle::Pointer)
                    .apply_if(!is_left, |s| s.hide())
                    .hover(|s| {
                        s.background(
                            config
                                .get()
                                .color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                        )
                        .border_radius(6.0)
                    })
            })
            .ide_popout_menu(move || {
                use strum::IntoEnumIterator;
                let mut menu = Menu::new("Tool Windows");
                let mut kinds: Vec<_> = PanelKind::iter()
                    .filter(|kind| kind.canonical() == *kind)
                    .collect();
                kinds.sort_by_key(|kind| kind.title());
                for kind in kinds {
                    let window = overflow_window.clone();
                    menu = menu.entry(
                        MenuItem::new(kind.title())
                            .action(move || window.show_panel(kind)),
                    );
                }
                menu
            }),
        empty().style(|s| s.flex_grow(1.0f32)),
        stripe_picker(window_tab_data.clone(), lower),
        stripe_picker(window_tab_data, bottom),
    ))
    .style(move |s| {
        let config = config.get();
        s.flex_col()
            .items_center()
            .width(config.ui.tool_window_stripe_width() as f32)
            .height_pct(100.0)
            .flex_shrink(0.0f32)
            .padding_vert(2.0)
            .background(floem::peniko::Color::TRANSPARENT) // Reveal the shared project backdrop.
            .apply_if(is_left, |s| s.margin_right(-1.0)) // Tighten gap
            .apply_if(!is_left, |s| s.margin_left(-1.0))
    })
    .debug_name("Tool Window Stripe")
}

fn stripe_picker(
    window_tab_data: Rc<WindowTabData>,
    position: PanelPosition,
) -> impl View {
    let panel = window_tab_data.panel.clone();
    let panels = panel.panels;
    let problems_pinned = panel.section_open(crate::panel::data::PanelSection::ProblemsSidebar);
    let config = window_tab_data.common.config;
    let dragging = window_tab_data.common.dragging;
    dyn_stack(
        move || {
            let active = panel.active_panel_at_position(&position, true);
            panel
                .panels
                .with(|panels| panels.get(&position).cloned().unwrap_or_default())
                .into_iter()
                .filter(|kind| {
                    active == Some((*kind, true))
                        || (kind.pinned() && (*kind != PanelKind::Problems || problems_pinned.get()))
                })
                .collect::<Vec<_>>()
        },
        |p| *p,
        move |p| {
            let window_tab_data = window_tab_data.clone();
            let tooltip = p.title();
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
            let button_active = is_active.clone();
            container(
                crate::widgets::icon_button(
                    move || icon,
                    move || tooltip,
                    config,
                    move || crate::widgets::IconButtonStyle {
                        size: Some(28.0),
                        icon_size: Some(16.0),
                        radius: Some(6.0),
                        foreground: button_active().then_some(floem::peniko::Color::WHITE),
                        background: button_active().then(|| config.get().color(
                            SparrowColor::SPARROW_BUTTON_PRIMARY_BACKGROUND,
                        )),
                    },
                )
                .on_click_stop(move |_| window_tab_data.toggle_panel_visual(p))
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
                s.size(28.0, 28.0)
                    .items_center()
                    .justify_center()
                    .margin_vert(2.0)
                    .border_radius(6.0)
                    .apply_if(is_active(), |s| {
                        s.background(config.color(SparrowColor::SPARROW_BUTTON_PRIMARY_BACKGROUND))
                    })
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
