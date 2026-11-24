use floem::{
    IntoView, Renderer, View, ViewId,
    context::{PaintCx, UpdateCx},
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
            // TinySkia clears to white and presents an opaque softbuffer surface.
            if matches!(&**cx, ActiveRenderer::TinySkia(_)) {
                cx.fill(&bounds, self.matte, 0.0);
            }
            cx.save();
            cx.clip(&bounds.to_rounded_rect(DIALOG_RADIUS));
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
