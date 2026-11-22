//! Keyboard and pointer navigation for the Settings page tree.

use crate::config::{SparrowConfig, color::SparrowColor, icon::SparrowIcons};
use crate::widgets::modal_focus::{ModalFocus, list_destination};
use floem::{
    View,
    event::{Event, EventListener, EventPropagation},
    keyboard::{Key, NamedKey},
    reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith},
    views::{Decorators, dyn_stack, label, stack, svg},
};

use super::{PAGES, Page, navigate, tokens, visible_pages};
use floem::reactive::ReadSignal;
use std::sync::Arc;

pub(super) fn navigation_tree(
    cx: Scope,
    keyboard: &ModalFocus,
    config: ReadSignal<Arc<SparrowConfig>>,
    page: RwSignal<Page>,
    history: RwSignal<Vec<Page>>,
    index: RwSignal<usize>,
    query: RwSignal<String>,
    appearance_expanded: RwSignal<bool>,
    languages_expanded: RwSignal<bool>,
) -> impl View + use<> {
    let [icon_width, icon_height] =
        tokens::size("navigation", "disclosure_icon_size");
    let indent = tokens::length("navigation", "tree_depth_step");
    let base = tokens::length("navigation", "tree_left_base");
    let gap = tokens::length("navigation", "tree_icon_gap");
    let tree_focus = cx.create_rw_signal(None::<floem::ViewId>);
    let tree = dyn_stack(
        || PAGES.to_vec(),
        |(p, _, _)| *p as u8,
        move |(p, title, depth)| {
            stack((
                svg(move || {
                    let expanded = match p {
                        Page::Overview => appearance_expanded.get(),
                        Page::Languages => languages_expanded.get(),
                        _ => false,
                    };
                    config.get().ui_svg(if expanded {
                        SparrowIcons::ITEM_OPENED
                    } else {
                        SparrowIcons::ITEM_CLOSED
                    })
                })
                .on_click_stop(move |_| {
                    if p == Page::Overview {
                        let collapsing = appearance_expanded.get_untracked();
                        if collapsing
                            && matches!(
                                page.get_untracked(),
                                Page::Appearance | Page::System
                            )
                        {
                            navigate(p, page, history, index, query);
                        }
                        appearance_expanded.set(!collapsing);
                    } else if p == Page::Languages {
                        let collapsing = languages_expanded.get_untracked();
                        if collapsing
                            && matches!(
                                page.get_untracked(),
                                Page::Flutter | Page::Dart
                            )
                        {
                            navigate(p, page, history, index, query);
                        }
                        languages_expanded.set(!collapsing);
                    }
                    if let Some(id) = tree_focus.get_untracked() {
                        id.request_focus();
                    }
                })
                .style(move |s| {
                    let color = if matches!(p, Page::Overview | Page::Languages) {
                        config.get().color(SparrowColor::EDITOR_FOREGROUND)
                    } else {
                        floem::peniko::Color::TRANSPARENT
                    };
                    s.size(icon_width, icon_height)
                        .color(color)
                        .flex_shrink(0.0_f32)
                }),
                label(move || title).style(move |s| {
                    s.min_width(0)
                        .text_ellipsis()
                        .flex_grow(1.0_f32)
                        .apply_if(depth == 0, |s| s.font_bold())
                }),
            ))
            .on_click_stop(move |_| {
                navigate(p, page, history, index, query);
                if let Some(id) = tree_focus.get_untracked() {
                    id.request_focus();
                }
            })
            .style(move |s| {
                let config = config.get();
                let selected = page.get() == p;
                s.height(25.0)
                    .width_full()
                    .padding_left(base + depth as f64 * indent)
                    .gap(gap)
                    .items_center()
                    .apply_if(
                        depth > 0
                            && !(if matches!(p, Page::Flutter | Page::Dart) {
                                languages_expanded.get()
                            } else {
                                appearance_expanded.get()
                            })
                            && query.with(|q| q.is_empty()),
                        |s| s.hide(),
                    )
                    .apply_if(selected, |s| {
                        s.background(
                            config.color(SparrowColor::PANEL_SELECTION_BACKGROUND),
                        )
                    })
                    .hover(|s| {
                        s.cursor(floem::style::CursorStyle::Pointer).apply_if(
                            !selected,
                            |s| {
                                s.background(
                                    config.color(
                                        SparrowColor::PANEL_HOVERED_BACKGROUND,
                                    ),
                                )
                            },
                        )
                    })
            })
        },
    )
    .style(|s| s.flex_col().width_full());
    let tree_id = tree.id();
    tree_focus.set(Some(tree_id));
    keyboard
        .control(tree)
        .style(|s| s.focus(|s| s.background(floem::peniko::Color::TRANSPARENT)))
        .on_event(EventListener::KeyDown, move |event| {
            let Event::KeyDown(key) = event else {
                return EventPropagation::Continue;
            };
            if !key.modifiers.is_empty() {
                return EventPropagation::Continue;
            }
            let current = page.get_untracked();
            let pages = visible_pages(
                appearance_expanded.get_untracked(),
                languages_expanded.get_untracked(),
                !query.with_untracked(|q| q.is_empty()),
            );
            let position = pages.iter().position(|p| *p == current).unwrap_or(0);
            let destination =
                list_destination(&key.key.logical_key, position, pages.len())
                    .map(|i| pages[i]);
            let destination = match (&key.key.logical_key, current) {
                (Key::Named(NamedKey::ArrowRight), Page::Overview) => {
                    if appearance_expanded.get_untracked() {
                        Some(Page::Appearance)
                    } else {
                        appearance_expanded.set(true);
                        None
                    }
                }
                (Key::Named(NamedKey::ArrowRight), Page::Languages) => {
                    if languages_expanded.get_untracked() {
                        Some(Page::Flutter)
                    } else {
                        languages_expanded.set(true);
                        None
                    }
                }
                (Key::Named(NamedKey::ArrowLeft), Page::Overview) => {
                    appearance_expanded.set(false);
                    None
                }
                (Key::Named(NamedKey::ArrowLeft), Page::Languages) => {
                    languages_expanded.set(false);
                    None
                }
                (
                    Key::Named(NamedKey::ArrowLeft),
                    Page::Appearance | Page::System,
                ) => Some(Page::Overview),
                (Key::Named(NamedKey::ArrowLeft), Page::Flutter | Page::Dart) => {
                    Some(Page::Languages)
                }
                _ => destination,
            };
            if let Some(destination) = destination {
                navigate(destination, page, history, index, query);
            }
            if matches!(
                key.key.logical_key,
                Key::Named(
                    NamedKey::ArrowUp
                        | NamedKey::ArrowDown
                        | NamedKey::ArrowLeft
                        | NamedKey::ArrowRight
                        | NamedKey::Home
                        | NamedKey::End
                )
            ) {
                EventPropagation::Stop
            } else {
                EventPropagation::Continue
            }
        })
}
