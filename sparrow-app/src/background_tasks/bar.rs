//! Paint-only progress animation: no layout or SVG parsing on animation frames.
use crate::config::{SparrowConfig, color::SparrowColor};
use floem::{
    Renderer, View, ViewId,
    peniko::{Color, kurbo::Rect},
    reactive::{Memo, ReadSignal, RwSignal, SignalGet, create_effect, create_memo},
    views::Decorators,
};
use std::{sync::Arc, time::Instant};

pub(super) fn progress_bar(
    percentage: impl Fn() -> Option<u32> + 'static,
    phase: RwSignal<f64>,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    let target = create_memo(move |_| percentage().map(|p| p.min(100)));
    let id = ViewId::new();
    create_effect(move |_| {
        target.get();
        phase.get();
        config.get();
        id.request_paint();
    });
    ProgressBar {
        id,
        target,
        phase,
        config,
        displayed: 0.0,
        last: Instant::now(),
        known: false,
    }
    .style(|s| s.pointer_events_none())
}
struct ProgressBar {
    id: ViewId,
    target: Memo<Option<u32>>,
    phase: RwSignal<f64>,
    config: ReadSignal<Arc<SparrowConfig>>,
    displayed: f64,
    last: Instant,
    known: bool,
}
fn approach(value: f64, target: f64, seconds: f64) -> f64 {
    let next = value + (target - value) * (1.0 - (-seconds / 0.12).exp());
    if (target - next).abs() < 0.05 {
        target
    } else {
        next
    }
}
fn gradient_alpha(position: f64) -> f64 {
    (std::f64::consts::PI * position.clamp(0.0, 1.0))
        .sin()
        .powi(2)
}
impl View for ProgressBar {
    fn id(&self) -> ViewId {
        self.id
    }
    fn paint(&mut self, cx: &mut floem::context::PaintCx) {
        let rect = self.id.get_content_rect();
        if rect.width() <= 0.0 || rect.height() <= 0.0 {
            return;
        }
        let config = self.config.get_untracked();
        let accent = config.color(SparrowColor::EDITOR_LINK);
        let track = config.color(SparrowColor::PANEL_HOVERED_BACKGROUND);
        let rounded = rect.to_rounded_rect(rect.height() / 2.0);
        cx.fill(&rounded, track, 0.0);
        let now = Instant::now();
        let seconds = now.duration_since(self.last).as_secs_f64();
        self.last = now;
        if let Some(target) = self.target.get_untracked() {
            if !self.known {
                self.displayed = 0.0;
            }
            self.known = true;
            self.displayed =
                approach(self.displayed, target as f64, seconds.min(0.1));
            let fill = Rect::new(
                rect.x0,
                rect.y0,
                rect.x0 + rect.width() * self.displayed / 100.0,
                rect.y1,
            );
            cx.fill(&fill.to_rounded_rect(rect.height() / 2.0), accent, 0.0);
        } else {
            self.known = false;
            let phase = (self.phase.get_untracked() / 1.6).fract();
            let width = rect.width() * 0.65;
            let left = rect.x0 - width + phase * (rect.width() + width);
            // Small solid strips work on every renderer, including software and
            // Wayland backends with incomplete gradient-brush support.
            let color = accent.to_rgba8();
            cx.save();
            cx.clip(&rounded);
            for index in 0..64 {
                let t = (index as f64 + 0.5) / 64.0;
                let alpha = gradient_alpha(t);
                let highlight = 0.22 * alpha;
                let channel =
                    |c: u8| (c as f64 + (255.0 - c as f64) * highlight) as u8;
                let color = Color::from_rgba8(
                    channel(color.r),
                    channel(color.g),
                    channel(color.b),
                    (255.0 * alpha) as u8,
                );
                let x = left + width * index as f64 / 64.0;
                cx.fill(
                    &Rect::new(x, rect.y0, x + width / 64.0 + 0.1, rect.y1),
                    color,
                    0.0,
                );
            }
            cx.restore();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn determinate_motion_is_smooth_and_frame_rate_independent() {
        let once = approach(0.0, 75.0, 0.1);
        let twice = approach(approach(0.0, 75.0, 0.05), 75.0, 0.05);
        assert!((once - twice).abs() < 0.0001);
        assert!(once > 0.0 && once < 75.0);
        assert_eq!(approach(75.0, 0.0, 10.0), 0.0);
        assert!(gradient_alpha(0.0) < 0.001);
        assert!(gradient_alpha(1.0) < 0.001);
        assert!(gradient_alpha(0.5) > 0.999);
    }
}
