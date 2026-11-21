//! Scrollable source preview with an independent caret and viewport.
use std::rc::Rc;

use floem::{
    View,
    reactive::{SignalGet, create_rw_signal},
    views::Decorators,
};

use crate::{
    config::color::SparrowColor,
    editor::{EditorData, view::read_only_editor_content},
    window_tab::WindowTabData,
};

use super::Issue;

pub(super) fn editor_preview(window: Rc<WindowTabData>, issue: Issue) -> impl View {
    let config = window.common.config;
    let (doc, _) = window.main_split.get_doc(issue.path.clone(), None);
    let pending = !doc.loaded();
    let editor = EditorData::new_doc(
        floem::reactive::Scope::current(),
        doc,
        None,
        None,
        None,
        window.common.clone(),
    );
    editor.go_to_location(issue.location(true), pending, None);
    let editor = create_rw_signal(editor);
    read_only_editor_content(window, editor).style(move |s| {
        s.size_full()
            .background(config.get().color(SparrowColor::EDITOR_BACKGROUND))
    })
}
