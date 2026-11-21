//! Command palette layout and result list.

use std::{ops::Range, rc::Rc};

use crate::{
    command::{CommandKind, SparrowCommand},
    config::color::SparrowColor,
    palette::{
        PaletteStatus,
        item::{PaletteItem, PaletteItemContent},
    },
    window_tab::WindowTabData,
};
use floem::{
    View,
    event::EventListener,
    peniko::kurbo::{Point, Rect, Size},
    reactive::{ReadSignal, SignalGet, SignalUpdate, SignalWith},
    style::{AlignItems, CursorStyle, Display, Position},
    unit::PxPctAuto,
    views::{
        Decorators, VirtualVector, container,
        scroll::{PropagatePointerWheel, scroll},
        stack, text, virtual_stack,
    },
};

mod input;
mod item;
mod preview;

use input::palette_input;
use item::palette_item;
use preview::palette_preview;

struct PaletteItems(im::Vector<PaletteItem>);

impl VirtualVector<(usize, PaletteItem)> for PaletteItems {
    fn total_len(&self) -> usize {
        self.0.len()
    }

    fn slice(
        &mut self,
        range: Range<usize>,
    ) -> impl Iterator<Item = (usize, PaletteItem)> {
        let start = range.start;
        Box::new(
            self.0
                .slice(range)
                .into_iter()
                .enumerate()
                .map(move |(i, item)| (i + start, item)),
        )
    }
}

fn palette_content(
    window_tab_data: Rc<WindowTabData>,
    layout_rect: ReadSignal<Rect>,
) -> impl View {
    let items = window_tab_data.palette.filtered_items;
    let keymaps = window_tab_data
        .palette
        .keypress
        .get_untracked()
        .command_keymaps;
    let index = window_tab_data.palette.index.read_only();
    let clicked_index = window_tab_data.palette.clicked_index.write_only();
    let config = window_tab_data.common.config;
    let run_id = window_tab_data.palette.run_id;
    let input = window_tab_data.palette.input.read_only();
    let palette_item_height = 25.0;
    let workspace = window_tab_data.workspace.clone();
    stack((
        scroll({
            let workspace = workspace.clone();
            virtual_stack(
                move || PaletteItems(items.get()),
                move |(i, _item)| {
                    (run_id.get_untracked(), *i, input.get_untracked().input)
                },
                move |(i, item)| {
                    let workspace = workspace.clone();
                    let keymap = {
                        let cmd_kind = match &item.content {
                            PaletteItemContent::PaletteHelp { cmd } => {
                                Some(CommandKind::Workbench(cmd.clone()))
                            }
                            PaletteItemContent::Command {
                                cmd: SparrowCommand { kind, .. },
                            } => Some(kind.clone()),
                            _ => None,
                        };

                        cmd_kind
                            .and_then(|kind| keymaps.get(kind.str()))
                            .and_then(|maps| maps.first())
                    };
                    container(palette_item(
                        workspace,
                        i,
                        item,
                        index,
                        palette_item_height,
                        config,
                        keymap,
                    ))
                    .on_click_stop(move |_| {
                        clicked_index.set(Some(i));
                    })
                    .style(move |s| {
                        s.width_full().cursor(CursorStyle::Pointer).hover(|s| {
                            s.background(
                                config
                                    .get()
                                    .color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                            )
                        })
                    })
                },
            )
            .item_size_fixed(move || palette_item_height)
            .style(|s| s.width_full().flex_col())
        })
        .ensure_visible(move || {
            Size::new(1.0, palette_item_height)
                .to_rect()
                .with_origin(Point::new(
                    0.0,
                    index.get() as f64 * palette_item_height,
                ))
        })
        .style(|s| {
            s.width_full()
                .min_height(0.0)
                .set(PropagatePointerWheel, false)
        }),
        text("No matching results").style(move |s| {
            s.display(if items.with(|items| items.is_empty()) {
                Display::Flex
            } else {
                Display::None
            })
            .padding_horiz(10.0)
            .align_items(Some(AlignItems::Center))
            .height(palette_item_height as f32)
        }),
    ))
    .style(move |s| {
        s.flex_col()
            .width_full()
            .min_height(0.0)
            .max_height((layout_rect.get().height() * 0.45 - 36.0).round() as f32)
            .padding_bottom(5.0)
            .padding_bottom(5.0)
    })
}

pub(crate) fn palette(window_tab_data: Rc<WindowTabData>) -> impl View {
    let layout_rect = window_tab_data.layout_rect.read_only();
    let palette_data = window_tab_data.palette.clone();
    let status = palette_data.status.read_only();
    let config = palette_data.common.config;
    let has_preview = palette_data.has_preview.read_only();
    container(
        stack((
            palette_input(window_tab_data.clone()),
            palette_content(window_tab_data.clone(), layout_rect),
            palette_preview(window_tab_data.clone()),
        ))
        .on_event_stop(EventListener::PointerDown, move |_| {})
        .style(move |s| {
            let config = config.get();
            s.width(config.ui.palette_width() as f64)
                .max_width_full()
                .max_height(if has_preview.get() {
                    PxPctAuto::Auto
                } else {
                    PxPctAuto::Pct(100.0)
                })
                .height(if has_preview.get() {
                    PxPctAuto::Px(layout_rect.get().height() - 10.0)
                } else {
                    PxPctAuto::Auto
                })
                .margin_top(4.0)
                .border(1.0)
                .border_radius(6.0)
                .border_color(config.color(SparrowColor::SPARROW_BORDER))
                .flex_col()
                .background(config.color(SparrowColor::PALETTE_BACKGROUND))
                .pointer_events_auto()
        }),
    )
    .style(move |s| {
        s.display(if status.get() == PaletteStatus::Inactive {
            Display::None
        } else {
            Display::Flex
        })
        .position(Position::Absolute)
        .size_full()
        .flex_col()
        .items_center()
        .pointer_events_none()
    })
    .debug_name("Palette Layer")
}
