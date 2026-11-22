//! Read-only source excerpts used by search results.

use sparrow_core::buffer::rope_text::RopeText;
use std::rc::Rc;

use floem::{
    IntoView, View,
    reactive::{Scope, SignalGet, SignalUpdate, SignalWith, create_rw_signal},
    views::{Decorators, empty, label, stack},
};

use crate::{
    config::color::SparrowColor, editor::view::read_only_editor_content,
    widgets::Divider, window_tab::WindowTabData,
};

use super::Hit;

pub(crate) fn view(window: Rc<WindowTabData>, hit: Option<Hit>) -> impl View {
    let Some(hit) = hit else {
        return empty().into_any();
    };
    let config = window.common.config;
    let relative = window
        .workspace
        .path
        .as_deref()
        .and_then(|root| hit.path.strip_prefix(root).ok())
        .unwrap_or(&hit.path)
        .display()
        .to_string();
    let (doc, _) = window.main_split.get_doc(hit.path.clone(), None);
    let pending = !doc.loaded();
    let scope = Scope::current().create_child();
    let editors = window.main_split.editors;
    let mut common = (*window.common).clone();
    common.find = crate::find::Find::new(scope);
    let editor =
        editors.make_from_doc(scope, doc, None, None, None, Rc::new(common));
    let id = editor.id();
    editor.editor.read_only.set(true);
    editor.go_to_location(hit.location(), pending, None);
    if !hit.span.is_empty() {
        let highlighted = editor.on_screen_find;
        let offset = hit.offset;
        let length = hit.span.len();
        let expected = hit.text[hit.span.clone()].to_owned();
        let doc = editor.doc();
        scope.create_effect(move |_| {
            if doc.loaded.get() {
                let end = offset + length;
                let matches = doc.buffer.with(|buffer| {
                    end <= buffer.len()
                        && buffer.text().is_codepoint_boundary(offset)
                        && buffer.text().is_codepoint_boundary(end)
                        && buffer.text().slice_to_cow(offset..end) == expected
                });
                highlighted.set(crate::editor::OnScreenFind {
                    active: matches,
                    pattern: expected.clone(),
                    regions: if matches {
                        vec![sparrow_core::selection::SelRegion::new(
                            offset, end, None,
                        )]
                    } else {
                        Vec::new()
                    },
                });
            }
        });
    }
    stack((
        Divider::new(config).into_view(),
        label(move || relative.clone()).style(move |s| {
            s.width_full()
                .height(25)
                .items_center()
                .padding_horiz(8)
                .text_ellipsis()
                .color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
        }),
        read_only_editor_content(window, create_rw_signal(editor)).style(move |s| {
            s.width_full()
                .flex_grow(1.0_f32)
                .flex_basis(0)
                .min_height(0)
                .background(config.get().color(SparrowColor::EDITOR_BACKGROUND))
        }),
    ))
    .style(|s| s.flex_col().size_full().min_height(0))
    .on_cleanup(move || {
        editors.remove(id);
        scope.dispose();
    })
    .into_any()
}
