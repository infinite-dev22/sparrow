//! Paint-only plots rendered in Sparrow's theme.

use floem::{
    Renderer, View, ViewId,
    peniko::kurbo::{Line, Point, Rect, Stroke},
    reactive::create_effect,
    views::Decorators,
};

use super::Config;
use crate::config::color::SparrowColor;

pub(super) fn plot(
    series: impl Fn(bool) -> Vec<[f64; 2]> + 'static,
    budget: Option<f64>,
    config: Config,
) -> impl View {
    use floem::reactive::SignalGet;
    let id = ViewId::new();
    let series = std::rc::Rc::new(series);
    let watch = series.clone();
    create_effect(move |_| {
        config.get();
        watch(true);
        id.request_paint();
    });
    let view = Plot {
        id,
        series: Box::new(move || series(false)),
        budget,
        config,
    };
    view.style(|s| s.height(112.0).width_full().min_width(0))
}

struct Plot {
    id: ViewId,
    series: Box<dyn Fn() -> Vec<[f64; 2]>>,
    budget: Option<f64>,
    config: Config,
}

impl View for Plot {
    fn id(&self) -> ViewId {
        self.id
    }

    fn paint(&mut self, cx: &mut floem::context::PaintCx) {
        use floem::reactive::SignalGet;
        let bounds = self.id.get_content_rect();
        if bounds.width() <= 0.0 || bounds.height() <= 0.0 {
            return;
        }
        let config = self.config.get_untracked();
        let samples = (self.series)();
        let primary = config.color(SparrowColor::EDITOR_LINK);
        let secondary = config.color(SparrowColor::SOURCE_CONTROL_ADDED);
        let grid = config.color(SparrowColor::SPARROW_BORDER);
        let ceiling = samples
            .iter()
            .flatten()
            .copied()
            .fold(self.budget.unwrap_or(1.0), f64::max)
            * 1.12;
        let y = |value: f64| bounds.y1 - value / ceiling * bounds.height();
        for fraction in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let axis = bounds.y1 - fraction * bounds.height();
            cx.stroke(
                &Line::new((bounds.x0, axis), (bounds.x1, axis)),
                grid,
                &Stroke::new(1.0),
            );
        }
        if let Some(budget) = self.budget {
            let axis = y(budget);
            cx.stroke(
                &Line::new((bounds.x0, axis), (bounds.x1, axis)),
                config.color(SparrowColor::SPARROW_WARN),
                &Stroke::new(1.0),
            );
            let width = bounds.width() / samples.len().max(40) as f64;
            let bar = (width * 0.32).max(0.5);
            let left = bounds.x1 - width * samples.len() as f64;
            for (i, [build, raster]) in samples.iter().copied().enumerate() {
                let x = left + i as f64 * width;
                cx.fill(&Rect::new(x, y(build), x + bar, bounds.y1), primary, 0.0);
                cx.fill(
                    &Rect::new(x + bar, y(raster), x + 2.0 * bar, bounds.y1),
                    secondary,
                    0.0,
                );
            }
        } else {
            let scale =
                bounds.width() / samples.len().saturating_sub(1).max(20) as f64;
            let left = bounds.x1 - scale * samples.len().saturating_sub(1) as f64;
            for (index, color) in [primary, secondary].into_iter().enumerate() {
                for (i, pair) in samples.windows(2).enumerate() {
                    let p = Point::new(left + i as f64 * scale, y(pair[0][index]));
                    let q = Point::new(p.x + scale, y(pair[1][index]));
                    cx.stroke(&Line::new(p, q), color, &Stroke::new(1.5));
                }
            }
        }
    }
}
