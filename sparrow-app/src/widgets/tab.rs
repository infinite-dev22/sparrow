use std::sync::Arc;

use floem::{
    IntoView, View, ViewId,
    kurbo::Rect,
    peniko::Color,
    reactive::{
        ReadSignal, RwSignal, SignalGet, SignalTrack, SignalUpdate, create_effect,
        create_rw_signal,
    },
    style::CursorStyle,
    views::{Decorators, Scroll, button},
};

use crate::config::{SparrowConfig, color::SparrowColor};

/// A selectable tab label. The caller owns selection and switches the content.
pub fn tab_button(
    content: impl IntoView + 'static,
    selected: impl Fn() -> bool + 'static,
    select: impl FnMut() + 'static,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    super::button::native_button_keyboard(button(content).action(select)).style(
        move |s| {
            let config = config.get();
            let selected = selected();
            s.height(24.0)
                .items_center()
                .padding_horiz(7.0)
                .margin_right(4.0)
                .flex_shrink(0.0_f32)
                .border_radius(4.0)
                .border(1.0)
                .border_color(if selected {
                    config.color(SparrowColor::EDITOR_LINK)
                } else {
                    Color::TRANSPARENT
                })
                .apply_if(selected, |s| {
                    s.background(
                        config.color(SparrowColor::PANEL_SELECTION_BACKGROUND),
                    )
                })
                .focus_visible(|s| {
                    s.border_color(config.color(SparrowColor::EDITOR_CARET))
                })
                .cursor(CursorStyle::Pointer)
        },
    )
}

#[derive(Clone, Copy)]
pub struct TabStrip {
    active: RwSignal<Option<(ViewId, Rect)>>,
}

impl TabStrip {
    pub fn button(
        self,
        content: impl IntoView + 'static,
        selected: impl Fn() -> bool + Clone + 'static,
        select: impl FnMut() + 'static,
        config: ReadSignal<Arc<SparrowConfig>>,
    ) -> impl View {
        self.item(
            tab_button(content, selected.clone(), select, config),
            selected,
        )
    }

    /// Register a tab that is a direct child of the strip's content row.
    pub fn item<V: View + 'static>(
        self,
        view: V,
        selected: impl Fn() -> bool + 'static,
    ) -> V {
        self.item_with_bounds(view, selected, create_rw_signal(Rect::ZERO))
    }

    pub(crate) fn item_with_bounds<V: View + 'static>(
        self,
        view: V,
        selected: impl Fn() -> bool + 'static,
        bounds: RwSignal<Rect>,
    ) -> V {
        let id = view.id();
        let clear = move || {
            if self
                .active
                .try_get_untracked()
                .flatten()
                .is_some_and(|(active, _)| active == id)
            {
                self.active.set(None);
            }
        };
        create_effect(move |_| {
            if selected() {
                let rect = bounds.get();
                let target = Some((id, Rect::new(rect.x0, 0.0, rect.x1, 0.0)));
                if rect.width() > 0.0 && self.active.get_untracked() != target {
                    self.active.set(target);
                }
            } else {
                clear();
            }
        });
        view.style(|s| s.flex_shrink(0.0_f32))
            .on_resize(move |rect| bounds.set(rect))
            .on_cleanup(clear)
    }
}

/// Scrolls an intrinsic-width row only when it exceeds the available width.
/// Active tabs are fully revealed on selection, layout changes, and viewport resize.
pub fn tab_strip<V: View + 'static>(build: impl FnOnce(TabStrip) -> V) -> Scroll {
    let active = create_rw_signal(None);
    let width = create_rw_signal(0.0);
    let content = build(TabStrip { active });
    super::horizontal_scroll(content.style(|s| s.items_center()))
        .scroll_style(|s| s.handle_thickness(3).track_thickness(3))
        .ensure_visible(move || {
            width.track();
            active.get().map_or(Rect::ZERO, |(_, bounds)| bounds)
        })
        .on_resize(move |rect| {
            if width.get_untracked() != rect.width() {
                width.set(rect.width());
            }
        })
        .style(|s| s.min_width(0.0).flex_shrink(1.0_f32))
}
