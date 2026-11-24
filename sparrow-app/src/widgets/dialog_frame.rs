use floem::{
    IntoView, Renderer, View, ViewId,
    context::{PaintCx, UpdateCx},
    kurbo::Shape,
    peniko::Color,
    views::Decorators,
};
use std::{any::Any, ops::Deref};

pub(crate) const DIALOG_RADIUS: f64 = 8.0;

// Floem exposes its concrete renderer through PaintCx's Deref implementation.
type ActiveRenderer = <PaintCx<'static> as Deref>::Target;

struct DialogFrame {
    id: ViewId,
    matte: Color,
}

struct Activate;

impl View for DialogFrame {
    fn id(&self) -> ViewId {
        self.id
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        if let Some(size) = self.id.get_size() {
            let bounds = size.to_rect();
            let rounded = bounds.to_rounded_rect(DIALOG_RADIUS);
            // TinySkia clears to white and presents an opaque softbuffer surface.
            if matches!(&**cx, ActiveRenderer::TinySkia(_)) {
                cx.fill(&bounds, self.matte, 0.0);
                cx.paint_children(self.id);

                // Floem restores the inherited clip for every child, rebuilding
                // a surface-sized TinySkia mask each time. Paint the outside of
                // the rounded rectangle once instead on the opaque surface.
                let mut corners = bounds.to_path(0.1);
                corners.extend(rounded.to_path(0.1).reverse_subpaths());
                cx.fill(&corners, self.matte, 0.0);
                return;
            }
            cx.save();
            cx.clip(&rounded);
            cx.paint_children(self.id);
            cx.restore();
        }
    }

    fn update(&mut self, _: &mut UpdateCx, value: Box<dyn Any>) {
        if value.is::<Activate>() {
            floem::action::focus_window();
        }
    }
}

pub(crate) fn dialog_frame(
    child: impl IntoView + 'static,
    matte: Color,
) -> impl View {
    let id = ViewId::new();
    id.set_children([child]);
    DialogFrame { id, matte }.style(|s| s.size_full())
}

pub(crate) fn activate_frame(id: ViewId) {
    id.update_state(Activate);
}
