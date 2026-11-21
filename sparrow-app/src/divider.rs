//! Decorative separators for stacks and toolbars.

use std::sync::Arc;

use floem::{
    View,
    peniko::Color,
    reactive::{ReadSignal, SignalGet},
    style::AlignItems,
    views::{Decorators, empty},
};

use crate::config::{SparrowConfig, color::SparrowColor};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DividerVariant {
    Horizontal,
    Vertical,
}

/// A theme-aware rule that stretches across the cross axis of its parent stack.
/// Use a horizontal rule in a column and a vertical rule in a row.
pub struct Divider {
    config: ReadSignal<Arc<SparrowConfig>>,
    variant: DividerVariant,
    thickness: f32,
    color: Option<Color>,
    indent: f32,
    spacing: f32,
}

impl Divider {
    pub fn new(config: ReadSignal<Arc<SparrowConfig>>) -> Self {
        Self {
            config,
            variant: DividerVariant::Horizontal,
            thickness: 1.0,
            color: None,
            indent: 0.0,
            spacing: 0.0,
        }
    }

    pub fn variant(mut self, variant: DividerVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn thickness(mut self, pixels: f32) -> Self {
        self.thickness = pixels.max(0.0);
        self
    }

    /// Override the theme's separator color.
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Inset both ends of the rule.
    pub fn indent(mut self, pixels: f32) -> Self {
        self.indent = pixels.max(0.0);
        self
    }

    /// Leave space between the rule and the views it separates.
    pub fn spacing(mut self, pixels: f32) -> Self {
        self.spacing = pixels.max(0.0);
        self
    }

    pub fn into_view(self) -> impl View {
        empty()
            .style(move |s| {
                let color = self.color.unwrap_or_else(|| {
                    self.config.get().color(SparrowColor::SPARROW_BORDER)
                });
                let s = s
                    .background(color)
                    .align_self(Some(AlignItems::Stretch))
                    .flex_shrink(0.0_f32)
                    .pointer_events_none();
                match self.variant {
                    DividerVariant::Horizontal => s
                        .height(self.thickness)
                        .margin_horiz(self.indent)
                        .margin_vert(self.spacing),
                    DividerVariant::Vertical => s
                        .width(self.thickness)
                        .margin_vert(self.indent)
                        .margin_horiz(self.spacing),
                }
            })
            .debug_name("Divider")
    }
}
