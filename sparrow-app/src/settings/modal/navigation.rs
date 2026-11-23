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
    style::Style,
    views::{Decorators, dyn_stack, label, stack, svg},
};

use super::{PAGES, Page, select_page, tokens};
use floem::reactive::ReadSignal;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[derive(Clone, Copy)]
pub(super) struct Navigation {
    pub(super) page: RwSignal<Page>,
    pub(super) displayed_page: Memo<Page>,
    pub(super) history: RwSignal<Vec<Page>>,
    pub(super) index: RwSignal<usize>,
    pub(super) expanded: RwSignal<BTreeSet<Page>>,
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
) -> (impl View + use<A>, ViewId) {
    let Navigation {
        page,
        displayed_page,
        history,
        index,
        expanded,
        matching,
        shown_pages,
        modifications,
    } = state;
    let [icon_width, icon_height] =
        tokens::size("navigation", "disclosure_icon_size");
    let tree_focus = cx.create_rw_signal(None::<ViewId>);
    let rows = cx.create_rw_signal(BTreeMap::<Page, ViewId>::new());
    let tree = dyn_stack(
        || PAGES.to_vec(),
        |(page, ..)| *page,
        move |(p, _, title, _)| {
            let depth = p.ancestors().count();
            let has_children = p.children().next().is_some();
            let row = stack((
                svg(move || {
                    let expanded = matching.with(Option::is_some)
                        || expanded.with(|expanded| expanded.contains(&p));
                    config.get().ui_svg(if expanded {
                        SparrowIcons::ITEM_OPENED
                    } else {
                        SparrowIcons::ITEM_CLOSED
                    })
                })
                .on_click_stop(move |_| {
                    if matching.with_untracked(Option::is_some) || !has_children {
                        select_page(p, page, history, index);
                    } else if expanded
                        .with_untracked(|expanded| expanded.contains(&p))
                    {
                        if page
                            .get_untracked()
                            .ancestors()
                            .any(|ancestor| ancestor == p)
                        {
                            select_page(p, page, history, index);
                        }
                        expanded.update(|expanded| {
                            expanded.remove(&p);
                        });
                    } else {
                        expanded.update(|expanded| {
                            expanded.insert(p);
                        });
                    }
                    if let Some(id) = tree_focus.get_untracked() {
                        id.request_focus();
                    }
                })
                .style(move |s| {
                    let color = if has_children {
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
                    let modified = displayed_page.get() != p
                        && modifications.with(|items| items.contains(&p));
                    s.min_width(0)
                        .text_ellipsis()
                        .flex_grow(1.0_f32)
                        .apply_if(depth == 0, |s| s.font_bold())
                        .apply_if(modified, |s| modified_label(s, &config))
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
                let selected = displayed_page.get() == p;
                tree_row_layout(s, depth)
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
            });
            rows.update(|rows| {
                rows.insert(p, row.id());
            });
            row
        },
    )
    .style(|s| s.flex_col().width_full());
    let tree_id = tree.id();
    tree_focus.set(Some(tree_id));
    let tree = keyboard
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
                let is_expanded =
                    expanded.with_untracked(|expanded| expanded.contains(&current));
                match key {
                    Key::Named(NamedKey::ArrowRight) => {
                        let child = current.children().next();
                        if child.is_some() && !is_expanded {
                            expanded.update(|expanded| {
                                expanded.insert(current);
                            });
                            None
                        } else {
                            child
                        }
                    }
                    Key::Named(NamedKey::ArrowLeft) if is_expanded => {
                        expanded.update(|expanded| {
                            expanded.remove(&current);
                        });
                        None
                    }
                    Key::Named(NamedKey::ArrowLeft) => current.parent(),
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
        });
    let scroll = crate::widgets::sparrow_scroll(tree).scroll_to_view(move || {
        let current = displayed_page.get();
        if !shown_pages.with(|pages| pages.contains(&current)) {
            return None;
        }
        rows.with(|rows| rows.get(&current).copied())
    });
    (scroll, tree_id)
}

fn tree_row_layout(style: Style, depth: usize) -> Style {
    let indent = tokens::length("navigation", "tree_depth_step");
    let base = tokens::length("navigation", "tree_left_base");
    style
        .height(25.0)
        .width_full()
        .padding_left(base + depth as f64 * indent)
        .gap(tokens::length("navigation", "tree_icon_gap"))
        .items_center()
}

fn modified_label(style: Style, config: &SparrowConfig) -> Style {
    let color = match config.color.color_preference {
        ThemeColorPreference::Dark => {
            tokens::color("dark", "Tree.modifiedItemForeground")
        }
        ThemeColorPreference::Light => {
            tokens::color("light", "Tree.modifiedItemForeground")
        }
        _ => config.color(SparrowColor::EDITOR_LINK),
    };
    style.color(color)
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
    fn settings_layout_tree_uses_verified_indent_gap_and_square_corners() {
        use floem::{
            style::{BorderTopLeftRadius, ColGap, PaddingLeft, Width},
            unit::{PxPct, PxPctAuto},
        };

        let root = tree_row_layout(Style::new(), 0);
        let child = tree_row_layout(Style::new(), 1);
        assert_eq!(root.get(PaddingLeft), PxPct::Px(3.0));
        assert_eq!(child.get(PaddingLeft), PxPct::Px(19.0));
        assert_eq!(child.get(ColGap), PxPct::Px(5.0));
        assert_eq!(child.get(Width), PxPctAuto::Pct(100.0));
        assert_eq!(child.get(BorderTopLeftRadius), PxPct::Px(0.0));
    }

    #[test]
    fn settings_layout_modified_tree_label_uses_verified_theme_color() {
        use floem::{peniko::Color, style::TextColor};

        let mut config = SparrowConfig::default();
        for (preference, rgb) in [
            (ThemeColorPreference::Dark, [0x71, 0xA1, 0xFE]),
            (ThemeColorPreference::Light, [0x2F, 0x5E, 0xB9]),
        ] {
            config.color.color_preference = preference.clone();
            assert_eq!(
                modified_label(Style::new(), &config).get(TextColor),
                Some(Color::from_rgb8(rgb[0], rgb[1], rgb[2])),
                "{preference:?}",
            );
        }
    }

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
