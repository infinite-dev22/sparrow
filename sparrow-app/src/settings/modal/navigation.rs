//! Keyboard and pointer navigation for the Settings page tree.

use crate::config::{
    SparrowConfig, color::SparrowColor, color_theme::ThemeColorPreference,
    icon::SparrowIcons,
};
use crate::widgets::modal_focus::{ModalFocus, list_destination};
use floem::{
    View, ViewId,
    event::{Event, EventListener, EventPropagation},
    keyboard::{Key, NamedKey},
    reactive::{Memo, RwSignal, Scope, SignalGet, SignalUpdate, SignalWith},
    views::{Decorators, dyn_stack, label, stack, svg},
};

use super::{PAGES, Page, select_page, tokens};
use floem::reactive::ReadSignal;
use std::sync::Arc;

#[derive(Clone, Copy)]
pub(super) struct Navigation {
    pub(super) page: RwSignal<Page>,
    pub(super) history: RwSignal<Vec<Page>>,
    pub(super) index: RwSignal<usize>,
    pub(super) appearance_expanded: RwSignal<bool>,
    pub(super) languages_expanded: RwSignal<bool>,
    pub(super) matching: Memo<Option<Vec<Page>>>,
    pub(super) shown_pages: Memo<Vec<Page>>,
    pub(super) modifications: Memo<Vec<Page>>,
}

pub(super) fn navigation_tree<A: Fn() + 'static>(
    cx: Scope,
    keyboard: &ModalFocus,
    config: ReadSignal<Arc<SparrowConfig>>,
    accept: A,
    state: Navigation,
) -> impl View + use<A> {
    let Navigation {
        page,
        history,
        index,
        appearance_expanded,
        languages_expanded,
        matching,
        shown_pages,
        modifications,
    } = state;
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
                    let expanded = matching.with(Option::is_some)
                        || match p {
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
                    if matching.with_untracked(Option::is_some) {
                        select_page(p, page, history, index);
                    } else if p == Page::Overview {
                        let collapsing = appearance_expanded.get_untracked();
                        if collapsing
                            && matches!(
                                page.get_untracked(),
                                Page::Appearance | Page::System
                            )
                        {
                            select_page(p, page, history, index);
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
                            select_page(p, page, history, index);
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
                    let config = config.get();
                    let modified = page.get() != p
                        && modifications.with(|items| items.contains(&p));
                    s.min_width(0)
                        .text_ellipsis()
                        .flex_grow(1.0_f32)
                        .apply_if(depth == 0, |s| s.font_bold())
                        .apply_if(modified, |s| {
                            let color = match config.color.color_preference {
                                ThemeColorPreference::Dark => tokens::color(
                                    "dark",
                                    "Tree.modifiedItemForeground",
                                ),
                                ThemeColorPreference::Light => tokens::color(
                                    "light",
                                    "Tree.modifiedItemForeground",
                                ),
                                _ => config.color(SparrowColor::EDITOR_LINK),
                            };
                            s.color(color)
                        })
                }),
            ))
            .on_click_stop(move |_| {
                select_page(p, page, history, index);
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
                    .apply_if(!shown_pages.with(|pages| pages.contains(&p)), |s| {
                        s.hide()
                    })
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
            let key_repeat = key.key.repeat;
            let key = &key.key.logical_key;
            if *key == Key::Named(NamedKey::Enter) {
                if !key_repeat {
                    accept();
                }
                return EventPropagation::Stop;
            }
            let pages = shown_pages.get_untracked();
            let vertical = tree_destination(key, current, &pages);
            let destination = if matching.with_untracked(Option::is_some) {
                match key {
                    Key::Named(NamedKey::ArrowLeft) => current.parent(),
                    Key::Named(NamedKey::ArrowRight) => pages
                        .iter()
                        .find(|page| page.parent() == Some(current))
                        .copied(),
                    _ => vertical,
                }
            } else {
                match (key, current) {
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
                    (Key::Named(NamedKey::ArrowLeft), page) => page.parent(),
                    _ => vertical,
                }
            };
            if let Some(destination) =
                destination.filter(|destination| pages.contains(destination))
            {
                select_page(destination, page, history, index);
            }
            if matches!(
                key,
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

fn tree_destination(key: &Key, current: Page, pages: &[Page]) -> Option<Page> {
    let Some(index) = pages.iter().position(|&page| page == current) else {
        return match key {
            Key::Named(NamedKey::ArrowDown | NamedKey::Home) => {
                pages.first().copied()
            }
            Key::Named(NamedKey::ArrowUp | NamedKey::End) => pages.last().copied(),
            _ => None,
        };
    };
    list_destination(key, index, pages.len()).map(|index| pages[index])
}

pub(super) fn search_navigation(
    event: &Event,
    tree: ViewId,
    shown_pages: Memo<Vec<Page>>,
    page: RwSignal<Page>,
    history: RwSignal<Vec<Page>>,
    index: RwSignal<usize>,
) -> EventPropagation {
    let Event::KeyDown(key) = event else {
        return EventPropagation::Continue;
    };
    if !key.modifiers.is_empty()
        || !matches!(
            key.key.logical_key,
            Key::Named(NamedKey::ArrowUp | NamedKey::ArrowDown)
        )
    {
        return EventPropagation::Continue;
    }
    let pages = shown_pages.get_untracked();
    if let Some(destination) =
        tree_destination(&key.key.logical_key, page.get_untracked(), &pages)
    {
        select_page(destination, page, history, index);
        tree.request_focus();
    }
    EventPropagation::Stop
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_enters_filtered_results_when_current_page_is_hidden() {
        let pages = [Page::Languages, Page::Dart];
        for (key, current, expected) in [
            (NamedKey::ArrowDown, Page::Editor, Page::Languages),
            (NamedKey::ArrowUp, Page::Editor, Page::Dart),
            (NamedKey::ArrowDown, Page::Languages, Page::Dart),
            (NamedKey::Home, Page::Dart, Page::Languages),
            (NamedKey::End, Page::Languages, Page::Dart),
        ] {
            assert_eq!(
                tree_destination(&Key::Named(key), current, &pages),
                Some(expected)
            );
        }
        assert_eq!(
            tree_destination(&Key::Named(NamedKey::ArrowDown), Page::Editor, &[]),
            None
        );
    }
}
