//! Debugger expression input with evaluate and add-watch shortcuts.

use floem::{
    View, ViewId,
    event::{Event, EventPropagation},
    keyboard::{Key, NamedKey},
};
use std::rc::Rc;

// Intercept submission before the stock TextInput consumes Enter. All other
// events still use the shared input's editing, focus, IME and clipboard behavior.
pub(super) struct ExpressionInput {
    pub(super) input: floem::views::TextInput,
    pub(super) submit: Rc<dyn Fn(bool)>,
}
impl View for ExpressionInput {
    fn id(&self) -> ViewId {
        self.input.id()
    }
    fn update(
        &mut self,
        cx: &mut floem::context::UpdateCx,
        state: Box<dyn std::any::Any>,
    ) {
        self.input.update(cx, state);
    }
    fn style_pass(&mut self, cx: &mut floem::context::StyleCx) {
        self.input.style_pass(cx);
    }
    fn layout(
        &mut self,
        cx: &mut floem::context::LayoutCx,
    ) -> floem::taffy::tree::NodeId {
        self.input.layout(cx)
    }
    fn compute_layout(
        &mut self,
        cx: &mut floem::context::ComputeLayoutCx,
    ) -> Option<floem::kurbo::Rect> {
        self.input.compute_layout(cx)
    }
    fn paint(&mut self, cx: &mut floem::context::PaintCx) {
        self.input.paint(cx);
    }
    fn event_before_children(
        &mut self,
        cx: &mut floem::context::EventCx,
        event: &Event,
    ) -> EventPropagation {
        if let Event::KeyDown(key) = event {
            if key.key.logical_key == Key::Named(NamedKey::Enter) {
                (self.submit)(key.modifiers.control() && key.modifiers.shift());
                return EventPropagation::Stop;
            }
        }
        self.input.event_before_children(cx, event)
    }
}
