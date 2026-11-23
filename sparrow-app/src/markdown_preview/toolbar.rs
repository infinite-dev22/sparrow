//! Markdown source, split and preview controls.

use super::is_markdown;
use crate::{config::color::SparrowColor, window_tab::WindowTabData};
use floem::{
    View,
    reactive::{SignalGet, SignalUpdate, SignalWith, create_memo},
    views::{Decorators, stack},
};
use std::rc::Rc;

pub fn toolbar(window: Rc<WindowTabData>) -> impl View {
    let config = window.common.config;
    let active = window.main_split.active_editor;
    let views = window.common.document_views;
    let path = create_memo(move |_| {
        active.get().and_then(|editor| {
            editor
                .doc_signal()
                .get()
                .content
                .with(|c| c.path().filter(|p| is_markdown(p)).cloned())
        })
    });
    let [source, split, preview] = [
        ("source", "≡", "Editor only"),
        ("split", "◫", "Editor and Preview"),
        ("preview", "▧", "Preview only"),
    ]
    .map(|(choice, icon, title)| {
        crate::widgets::tooltip_label(
            config,
            crate::widgets::button(
                move || icon,
                crate::widgets::ButtonKind::Plain,
                config,
            )
            .on_click_stop(move |_| {
                if let Some(path) = path.get_untracked() {
                    views.update(|views| {
                        views.insert(path, choice.into());
                    });
                }
            })
            .style(move |s| {
                s.size(26.0, 24.0)
                    .justify_center()
                    .items_center()
                    .border_radius(3.0)
                    .apply_if(
                        path.with(|p| {
                            p.as_ref().is_some_and(|p| {
                                views.with(|v| {
                                    v.get(p).map(String::as_str).unwrap_or("source")
                                        == choice
                                })
                            })
                        }),
                        |s| {
                            s.background(
                                config
                                    .get()
                                    .color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                            )
                        },
                    )
            }),
            move || title,
        )
    });
    stack((source, split, preview)).style(move |s| {
        s.height_full()
            .items_center()
            .gap(2.0)
            .padding_horiz(4.0)
            .apply_if(path.get().is_none(), |s| s.hide())
    })
}
