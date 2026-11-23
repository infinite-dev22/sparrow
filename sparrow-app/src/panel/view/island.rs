//! Corner masks for panels and the editor surface.

use std::sync::Arc;

use floem::{
    View,
    kurbo::Point,
    reactive::{ReadSignal, RwSignal, SignalGet, SignalUpdate, create_rw_signal},
    views::{Decorators, stack, svg},
};

use crate::config::{SparrowConfig, color::SparrowColor};

/// Radius of the rounded "islands" (tool windows and editor area).
pub const ISLAND_RADIUS: f32 = 8.0;

fn island_corner(
    config: ReadSignal<Arc<SparrowConfig>>,
    path: &'static str,
    accent: RwSignal<Option<[u8; 3]>>,
    top: bool,
    left: bool,
) -> impl View {
    let svg_str = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10" width="10" height="10"><path d="{path}" fill="currentColor"/></svg>"#
    );
    let origin = create_rw_signal(Point::ZERO);
    svg(move || svg_str.clone())
        .on_move(move |point| origin.set(point))
        .style(move |s| {
            let color = crate::project_appearance::backdrop_color(
                config.get().color(SparrowColor::SPARROW_WINDOW_BACKGROUND),
                accent.get(),
                origin.get()
                    + (
                        f64::from(ISLAND_RADIUS) / 2.0,
                        f64::from(ISLAND_RADIUS) / 2.0,
                    ),
            );
            s.absolute()
                .size(ISLAND_RADIUS, ISLAND_RADIUS)
                .color(color)
                .apply_if(top, |s| s.inset_top(0.0))
                .apply_if(!top, |s| s.inset_bottom(0.0))
                .apply_if(left, |s| s.inset_left(0.0))
                .apply_if(!left, |s| s.inset_right(0.0))
        })
}

/// floem does not clip children to a rounded parent, so rounded corners are
/// faked by painting four quarter-circle masks matching the shared backdrop
/// at each island corner (including the project glow).
pub fn island_corners(
    config: ReadSignal<Arc<SparrowConfig>>,
    accent: RwSignal<Option<[u8; 3]>>,
) -> impl View {
    stack((
        island_corner(config, "M0 0H10A10 10 0 0 0 0 10Z", accent, true, true),
        island_corner(config, "M0 0H10V10A10 10 0 0 0 0 0Z", accent, true, false),
        island_corner(config, "M0 0V10H10A10 10 0 0 1 0 0Z", accent, false, true),
        island_corner(config, "M10 0V10H0A10 10 0 0 0 10 0Z", accent, false, false),
    ))
    .style(|s| {
        s.absolute()
            .size_pct(100.0, 100.0)
            .z_index(40)
            .pointer_events_none()
    })
}
