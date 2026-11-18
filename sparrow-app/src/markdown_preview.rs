//! Live Markdown document preview sharing the editor's unsaved buffer.
use crate::{
    command::InternalCommand,
    config::color::SparrowColor,
    editor::EditorData,
    markdown::{MarkdownContent, parse_document},
    window_tab::WindowTabData,
};
use floem::{
    IntoView, View,
    event::{Event, EventListener},
    reactive::{
        RwSignal, SignalGet, SignalUpdate, SignalWith, create_effect, create_memo,
        create_rw_signal,
    },
    views::{
        Decorators, dyn_stack, empty, img_from_path, label, rich_text, scroll, stack,
    },
};
use sparrow_core::buffer::rope_text::RopeText;
use std::{path::Path, rc::Rc};
pub fn is_markdown(path: &Path) -> bool {
    path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
        matches!(s.to_lowercase().as_str(), "md" | "markdown" | "mdown")
    })
}
pub fn mode(editor: &EditorData) -> String {
    let doc = editor.doc_signal().get();
    doc.content
        .with(|content| {
            content.path().and_then(|path| {
                editor
                    .common
                    .document_views
                    .with(|views| views.get(path).cloned())
            })
        })
        .unwrap_or_else(|| "source".into())
}
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
    let [source, split, preview] =
        [
            ("source", "≡", "Editor only"),
            ("split", "◫", "Editor and Preview"),
            ("preview", "▧", "Preview only"),
        ]
        .map(|(choice, icon, title)| {
            crate::app::tooltip_label(
                config,
                label(move || icon)
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
                                            v.get(p)
                                                .map(String::as_str)
                                                .unwrap_or("source")
                                                == choice
                                        })
                                    })
                                }),
                                |s| {
                                    s.background(config.get().color(
                                        SparrowColor::PANEL_HOVERED_BACKGROUND,
                                    ))
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
pub fn preview(
    window: Rc<WindowTabData>,
    editor: RwSignal<EditorData>,
) -> impl View {
    let config = window.common.config;
    let anchor_scroll = create_rw_signal(None::<floem::kurbo::Point>);
    let preview_height = create_rw_signal(1.0f64);
    let pointer_inside = create_rw_signal(false);
    let preview_percent = create_rw_signal(0.0f32);
    let width = create_rw_signal(600.0f32);
    let internal = window.common.internal_command;
    let source = create_memo(move |_| {
        let doc = editor.get().doc_signal().get();
        if doc
            .content
            .with(|content| content.path().is_some_and(|p| is_markdown(p)))
        {
            doc.buffer.with(|buffer| buffer.to_string())
        } else {
            String::new()
        }
    });
    let path = create_memo(move |_| {
        editor
            .get()
            .doc_signal()
            .get()
            .content
            .with(|content| content.path().cloned())
    });
    let rendered = create_rw_signal((0u64, Vec::<MarkdownContent>::new()));
    create_effect(move |_| {
        let mut blocks = parse_document(&source.get(), &config.get());
        for block in &mut blocks {
            if let MarkdownContent::LinkedText { layout, .. } = block {
                layout.set_size((width.get() - 48.0).max(80.0), f32::MAX);
            }
        }
        let revision = rendered.with_untracked(|(revision, _)| *revision) + 1;
        rendered.set((revision, blocks));
    });
    scroll(
        dyn_stack(
            move || {
                rendered.with(|(revision, blocks)| {
                    blocks
                        .iter()
                        .cloned()
                        .enumerate()
                        .map(|(index, block)| (*revision, index, block))
                        .collect::<Vec<_>>()
                })
            },
            |(revision, index, _)| (*revision, *index),
            move |(_, _, block)| match block {
                MarkdownContent::LinkedText { layout, links } => {
                    let hit_layout = layout.clone();
                    rich_text(move || layout.clone())
                        .on_event_stop(EventListener::PointerDown, move |event| {
                            if let Event::PointerDown(pointer) = event {
                                if pointer.button.is_primary() {
                                    let hit = hit_layout.hit_point(pointer.pos);
                                    if let Some((_, target)) =
                                        links.iter().find(|(range, _)| {
                                            range.contains(&hit.index)
                                        })
                                    {
                                        if let Ok(url) = url::Url::parse(target) {
                                            if matches!(
                                                url.scheme(),
                                                "http" | "https" | "mailto"
                                            ) {
                                                let _ = open::that(url.as_str());
                                            }
                                        } else if target.starts_with('#') {
                                            if let Some((range, _)) = links
                                                .iter()
                                                .find(|(range, anchor)| {
                                                    range.is_empty()
                                                        && anchor == target
                                                })
                                            {
                                                anchor_scroll.set(Some(
                                                    floem::kurbo::Point::new(
                                                        0.0,
                                                        hit_layout
                                                            .hit_position(
                                                                range.start,
                                                            )
                                                            .point
                                                            .y,
                                                    ),
                                                ));
                                            }
                                        } else {
                                            if let Some(parent) =
                                                path.get_untracked().and_then(|p| {
                                                    p.parent().map(Path::to_path_buf)
                                                })
                                            {
                                                let destination = parent.join(
                                                    target
                                                        .split('#')
                                                        .next()
                                                        .unwrap_or(target),
                                                );
                                                internal.send(
                                                    InternalCommand::OpenFile {
                                                        path: destination,
                                                    },
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        })
                        .style(|s| s.width_full().min_width(0.0))
                        .into_any()
                }
                MarkdownContent::Image { url, title } => {
                    let local = path
                        .get_untracked()
                        .and_then(|path| path.parent().map(|p| p.join(&url)));
                    if let Some(local) = local.filter(|path| path.is_file()) {
                        img_from_path(move || local.clone())
                            .style(|s| s.max_width_full().max_height(480.0))
                            .into_any()
                    } else {
                        label(move || {
                            if title.is_empty() {
                                format!("Image: {url}")
                            } else {
                                title.clone()
                            }
                        })
                        .style(move |s| {
                            s.color(config.get().color(SparrowColor::EDITOR_DIM))
                        })
                        .into_any()
                    }
                }
                MarkdownContent::Text(layout) => rich_text(move || layout.clone())
                    .style(|s| s.width_full())
                    .into_any(),
                MarkdownContent::Separator => empty()
                    .style(move |s| {
                        s.height(1.0).width_full().background(
                            config.get().color(SparrowColor::SPARROW_BORDER),
                        )
                    })
                    .into_any(),
            },
        )
        .on_resize(move |rect| preview_height.set(rect.height()))
        .style(|s| s.flex_col().width_full().padding(24.0).gap(12.0)),
    )
    .on_event_cont(EventListener::PointerEnter, move |_| {
        pointer_inside.set(true)
    })
    .on_event_cont(EventListener::PointerLeave, move |_| {
        pointer_inside.set(false)
    })
    .scroll_to_percent(move || {
        let data = editor.get();
        if pointer_inside.get() || mode(&data) == "preview" {
            return preview_percent.get();
        }
        let viewport = data.viewport().get();
        let doc = data.doc_signal().get();
        let total = doc.buffer.with(|buffer| buffer.last_line() + 1) as f64
            * config.get().editor.line_height() as f64;
        ((viewport.y0 / (total - viewport.height()).max(1.0)) * 100.0)
            .clamp(0.0, 100.0) as f32
    })
    .on_scroll(move |rect| {
        let data = editor.get_untracked();
        if !pointer_inside.get_untracked() || !data.doc().content.with_untracked(|c| c.path().is_some_and(|p| is_markdown(p))) || mode(&data) == "source" {
            return;
        }
        let ratio = (rect.y0
            / (preview_height.get_untracked() - rect.height()).max(1.0))
        .clamp(0.0, 1.0);
        let percent = (ratio * 100.0) as f32;
        if (preview_percent.get_untracked() - percent).abs() > 0.1 {
            preview_percent.set(percent);
        }
        let data = editor.get_untracked();
        if mode(&data) == "split" {
            let viewport = data.viewport().get_untracked();
            let doc = data.doc();
            let total = doc.buffer.with_untracked(|buffer| buffer.last_line() + 1)
                as f64
                * config.get_untracked().editor.line_height() as f64;
            data.scroll_to().set(Some(floem::kurbo::Vec2::new(
                viewport.x0,
                ratio * (total - viewport.height()).max(0.0),
            )));
        }
    })
    .scroll_to(move || anchor_scroll.get())
    .on_resize(move |rect| width.set(rect.width() as f32))
    .style(move |s| {
        s.height_full()
            .min_width(0.0)
            .flex_basis(0.0)
            .flex_grow(1.0_f32)
            .border_left(1.0)
            .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn markdown_extensions_are_case_insensitive() {
        for file in ["README.md", "guide.markdown", "doc.MD"] {
            assert!(is_markdown(Path::new(file)));
        }
        assert!(!is_markdown(Path::new("main.dart")));
    }
}
