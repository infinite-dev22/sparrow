//! Keyboard and pointer navigation for the Settings page tree.

use crate::config::{SparrowConfig, color::SparrowColor};
use crate::widgets::modal_focus::{ModalFocus, list_destination};
use floem::{
    View,
    event::{Event, EventListener, EventPropagation},
    keyboard::{Key, NamedKey},
    reactive::{RwSignal, Scope, SignalGet, SignalUpdate, SignalWith},
    views::{Decorators, dyn_stack, label, stack},
};

use super::{PAGES, Page, navigate, visible_pages};
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
    let tree_focus = cx.create_rw_signal(None::<floem::ViewId>);
    let tree = dyn_stack(
        || PAGES.to_vec(),
        |(p, _, _)| *p as u8,
        move |(p, title, depth)| {
            stack((
                label(move || {
                    if p == Page::Overview {
                        if appearance_expanded.get() {
                            "⌄"
                        } else {
                            "›"
                        }
                    } else if p == Page::Languages {
                        if languages_expanded.get() {
                            "⌄"
                        } else {
                            "›"
                        }
                    } else {
                        ""
                    }
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
                .style(|s| s.width(16.0)),
                label(move || title).style(|s| s.flex_grow(1.0_f32)),
            ))
            .on_click_stop(move |_| {
                navigate(p, page, history, index, query);
                if let Some(id) = tree_focus.get_untracked() {
                    id.request_focus();
                }
            })
            .style(move |s| {
                s.height(25.0)
                    .width_full()
                    .padding_left((18 + depth * 16) as f32)
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
                    .apply_if(page.get() == p, |s| {
                        s.background(
                            config
                                .get()
                                .color(SparrowColor::SPARROW_TAB_ACTIVE_BACKGROUND),
                        )
                    })
                    .hover(|s| s.cursor(floem::style::CursorStyle::Pointer))
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
