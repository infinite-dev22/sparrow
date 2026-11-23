//! Layout builder for foldable tool window sections.

use std::sync::Arc;

use floem::{
    AnyView, IntoView, View,
    reactive::{ReadSignal, RwSignal, SignalGet},
    style::Style,
    unit::PxPctAuto,
    views::{Decorators, stack_from_iter, text},
};

use crate::config::SparrowConfig;
use crate::panel::position::PanelPosition;

use super::foldable_panel_section;

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
            let s = if position.is_bottom() {
                // Bottom tools are columns: vertical section heights must not
                // become intrinsic widths that squeeze stacks and variables.
                s.height_full()
                    .min_width(0.0)
                    .flex_grow(1.0_f32)
                    .flex_basis(0.0)
            } else if open.get() {
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
