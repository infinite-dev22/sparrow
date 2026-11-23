use floem::{
    IntoView, Renderer, View, ViewId,
    context::{PaintCx, UpdateCx},
    peniko::Color,
    views::Decorators,
};
use std::any::Any;

pub(crate) const DIALOG_RADIUS: f64 = 8.0;

struct DialogFrame {
    id: ViewId,
    matte: Option<Color>,
}

struct Activate;

impl View for DialogFrame {
    fn id(&self) -> ViewId {
        self.id
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        if let Some(size) = self.id.get_size() {
            if let Some(matte) = self.matte {
                cx.fill(&size.to_rect(), matte, 0.0);
            }
            cx.save();
            cx.clip(&size.to_rect().to_rounded_rect(DIALOG_RADIUS));
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
    // The TinySkia surface clears to opaque white and presents through softbuffer.
    let matte = std::env::var("FLOEM_FORCE_TINY_SKIA")
        .is_ok_and(|value| value == "1")
        .then_some(matte);
    DialogFrame { id, matte }.style(|s| s.size_full())
}

pub(crate) fn activate_frame(id: ViewId) {
    id.update_state(Activate);
}
