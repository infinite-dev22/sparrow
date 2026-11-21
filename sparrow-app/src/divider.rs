use floem::IntoView;
use floem::peniko::Color;
use floem::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DividerVariant {
    Horizontal,
    Vertical,
}

pub struct Divider {
    variant: DividerVariant,
    height: f32,
    width: f32,
    color: Color,
    indent: f32,
}

impl Divider {
    pub fn new() -> Self {
        Self {
            variant: DividerVariant::Horizontal,
            height: 1.0,
            width: 1.0,
            color: Color::from_rgb8(200, 200, 200),
            indent: 0.0,
        }
    }

    pub fn variant(&mut self, variant: DividerVariant) -> &mut Self {
        self.variant = variant;
        self
    }

    pub fn height(&mut self, height: f32) -> &mut Self {
        self.height = height;
        self
    }

    pub fn width(&mut self, width: f32) -> &mut Self {
        self.width = width;
        self
    }

    pub fn color(&mut self, color: Color) -> &mut Self {
        self.color = color;
        self
    }

    pub fn indent(&mut self, indent: f32) -> &mut Self {
        self.indent = indent;
        self
    }

    pub fn into_view(self) -> impl IntoView {
        let variant = self.variant;
        let height = self.height;
        let width = self.width;
        let color = self.color;
        let indent = self.indent;

        let view = empty().style(move |s| {
            let base = s.background(color);

            match variant {
                DividerVariant::Horizontal => {
                    base.width_full()
                        .height(height)
                        .margin_vert(indent)
                }
                DividerVariant::Vertical => {
                    base.height_full()
                        .width(width)
                        .margin_horiz(indent)
                }
            }
        });

        view.into_any()
    }
}
