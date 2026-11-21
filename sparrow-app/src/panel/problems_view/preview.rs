//! Scrollable source preview with an independent caret and viewport.
use std::rc::Rc;

use floem::{
    View,
    reactive::{Scope, SignalGet, SignalUpdate, create_rw_signal},
    views::Decorators,
};

use crate::{
    config::color::SparrowColor, editor::view::read_only_editor_content,
    window_tab::WindowTabData,
};

use super::{Issue, colors, rank};

pub(super) fn editor_preview(window: Rc<WindowTabData>, issue: Issue) -> impl View {
    let config = window.common.config;
    let severity = rank(&issue.diagnostic);
    let (doc, _) = window.main_split.get_doc(issue.path.clone(), None);
    let pending = !doc.loaded();
    let scope = Scope::current().create_child();
    let editors = window.main_split.editors;
    // Document layout looks up the editor in the workspace registry.
    let editor =
        editors.make_from_doc(scope, doc, None, None, None, window.common.clone());
    let id = editor.id();
    editor.editor.read_only.set(true);
    editor.go_to_location(issue.location(true), pending, None);
    read_only_editor_content(window, create_rw_signal(editor))
        .style(move |s| {
            let config = config.get();
            s.size_full()
                .border_top(2.0)
                .border_color(colors::severity_color(&config, severity))
                .background(config.color(SparrowColor::EDITOR_BACKGROUND))
        })
        .on_cleanup(move || {
            editors.remove(id);
            scope.dispose();
        })
}
