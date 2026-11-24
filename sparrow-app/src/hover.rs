pub(crate) mod view;

use floem::{
    peniko::kurbo::Rect,
    reactive::{RwSignal, Scope},
    views::editor::id::EditorId,
};

use crate::markdown::MarkdownContent;

#[derive(Clone)]
pub struct HoverData {
    pub request: RwSignal<u64>,
    pub active: RwSignal<bool>,
    pub offset: RwSignal<usize>,
    pub editor_id: RwSignal<EditorId>,
    pub diagnostics: RwSignal<Vec<String>>,
    pub suggestion: RwSignal<Option<String>>,
    pub content: RwSignal<Vec<MarkdownContent>>,
    pub layout_rect: RwSignal<Rect>,
}

impl HoverData {
    pub fn new(cx: Scope) -> Self {
        Self {
            request: cx.create_rw_signal(0),
            active: cx.create_rw_signal(false),
            offset: cx.create_rw_signal(0),
            diagnostics: cx.create_rw_signal(Vec::new()),
            suggestion: cx.create_rw_signal(None),
            content: cx.create_rw_signal(Vec::new()),
            editor_id: cx.create_rw_signal(EditorId::next()),
            layout_rect: cx.create_rw_signal(Rect::ZERO),
        }
    }
}
