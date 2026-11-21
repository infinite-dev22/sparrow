//! Analyzer code action review, diff and apply controls.

use crate::{
    code_action::CodeActionStatus, config::color::SparrowColor,
    window_tab::WindowTabData,
};
use floem::{
    View,
    event::EventListener,
    reactive::{SignalGet, SignalUpdate, SignalWith},
    views::{Decorators, dyn_stack, empty, label, stack},
};
use lapce_xi_rope::Rope;
use lsp_types::CodeActionOrCommand;
use sparrow_core::buffer::diff::{DiffLines, rope_diff};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, atomic::AtomicU64},
};
use crate::widgets::sparrow_scroll;
use super::{Preview, PreviewFile, install};

#[derive(Clone)]
struct DiffRow {
    text: String,
    kind: u8,
}
fn diff_rows(file: &PreviewFile) -> Vec<DiffRow> {
    let before: Vec<_> = file.before.lines().collect();
    let after: Vec<_> = file.after.lines().collect();
    let mut rows = Vec::new();
    for diff in rope_diff(
        Rope::from(file.before.as_str()),
        Rope::from(file.after.as_str()),
        0,
        Arc::new(AtomicU64::new(0)),
        Some(3),
    )
    .unwrap_or_default()
    {
        match diff {
            DiffLines::Left(range) => {
                for i in range {
                    rows.push(DiffRow {
                        text: format!(
                            "{:>4}    − {}",
                            i + 1,
                            before.get(i).unwrap_or(&"")
                        ),
                        kind: 1,
                    });
                }
            }
            DiffLines::Right(range) => {
                for i in range {
                    rows.push(DiffRow {
                        text: format!(
                            "     {:>4} + {}",
                            i + 1,
                            after.get(i).unwrap_or(&"")
                        ),
                        kind: 2,
                    });
                }
            }
            DiffLines::Both(info) => {
                for (n, (left, right)) in info.left.zip(info.right).enumerate() {
                    if let Some(skip) = &info.skip {
                        if skip.contains(&n) {
                            if n == skip.start {
                                rows.push(DiffRow {
                                    text: "          …".into(),
                                    kind: 0,
                                });
                            }
                            continue;
                        }
                    }
                    rows.push(DiffRow {
                        text: format!(
                            "{:>4} {:>4}   {}",
                            left + 1,
                            right + 1,
                            before.get(left).unwrap_or(&"")
                        ),
                        kind: 0,
                    });
                }
            }
        }
    }
    rows
}
pub fn view(window: Rc<WindowTabData>) -> impl View {
    let config = window.common.config;
    let data = window.code_action;
    let initial = data.get_untracked();
    let status = initial.status;
    let active = initial.active;
    let preview = initial.preview;
    let show = initial.show_preview;
    let scope = Rc::new(RefCell::new(None));
    install(window.clone(), scope.clone());
    let preview_view = stack((
        label(|| "Preview").style(|s| {
            s.height(28.0)
                .padding_horiz(10.0)
                .items_center()
                .font_bold()
        }),
        label(move || match preview.get() {
            Preview::Empty => String::new(),
            Preview::Loading => "Loading preview…".into(),
            Preview::Blocked(error) => error,
            Preview::Ready(p) if p.files.is_empty() => {
                "This action does not provide a source preview.".into()
            }
            _ => String::new(),
        })
        .style(move |s| {
            s.padding(12.0)
                .max_width(600.0)
                .apply_if(matches!(preview.get(),Preview::Empty) || matches!(preview.get(),Preview::Ready(p) if !p.files.is_empty()),|s|s.hide())
                .color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
        }),
        sparrow_scroll(
            dyn_stack(
                move || match preview.get() {
                    Preview::Ready(p) => p.files,
                    _ => vec![],
                },
                |f| (f.path.clone(), f.revision, f.after.clone()),
                move |file| {
                    let name = file.path.display().to_string();
                    let rows = diff_rows(&file);
                    stack((
                        label(move || name.clone()).style(move |s| {
                            s.padding_horiz(10.0)
                                .height(26.0)
                                .items_center()
                                .color(config.get().color(SparrowColor::EDITOR_LINK))
                        }),
                        dyn_stack(
                            move || rows.clone().into_iter().enumerate(),
                            |(i, _)| *i,
                            move |(_, row)| {
                                label(move || row.text.clone()).style(move |s| {
                                    s.min_width_full()
                                        .height(20.0)
                                        .padding_horiz(8.0)
                                        .items_center()
                                        .font_family(
                                            config.get().editor.font_family.clone(),
                                        )
                                        .font_size(12.0)
                                        .apply_if(row.kind == 1, |s| {
                                            s.background(
                                                floem::peniko::Color::from_rgba8(
                                                    200, 60, 60, 35,
                                                ),
                                            )
                                        })
                                        .apply_if(row.kind == 2, |s| {
                                            s.background(
                                                floem::peniko::Color::from_rgba8(
                                                    70, 160, 80, 35,
                                                ),
                                            )
                                        })
                                })
                            },
                        )
                        .style(|s| s.flex_col().min_width_full()),
                    ))
                    .style(|s| s.flex_col().min_width_full())
                },
            )
            .style(|s| s.flex_col().min_width_full()),
        )
        .style(|s| s.flex_grow(1.0_f32).min_height(0.0).width_full()),
    ))
    .style(move |s| {
        s.flex_col()
            .width(600.0)
            .min_width(0.0)
            .height_full()
            .border_left(1.0)
            .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
            .apply_if(!show.get(), |s| s.hide())
    });
    let apply_data = data;
    let close_data = data;
    let retry_window = window.clone();
    let list=sparrow_scroll(stack((label(move ||data.with(|d|match &d.request_state {crate::code_action::ActionRequestState::Loading=>"Finding quick fixes…".to_string(),crate::code_action::ActionRequestState::Failed(error)=>error.clone(),crate::code_action::ActionRequestState::Ready=>"No quick fixes available".to_string()})).style(move |s|s.padding(10.0).apply_if(!data.with(|d|d.filtered_items.is_empty()),|s|s.hide())),dyn_stack(move ||data.with(|d|d.filtered_items.clone().into_iter().enumerate().collect::<Vec<_>>()),move |(i,_)|(data.with_untracked(|d|d.request_id),*i),move |(i,item)|{
        let title=item.title().replace('\n'," ");let disabled=matches!(&item.item,CodeActionOrCommand::CodeAction(a) if a.disabled.is_some());
        label(move ||title.clone()).on_click_stop(move |_|{let d=data.get_untracked();d.active.set(i);})
            .on_double_click_stop(move |_|data.get_untracked().select())
            .on_event_stop(EventListener::PointerEnter,move |_|active.set(i))
            .style(move |s|s.height(26.0).width_full().padding_horiz(10.0).items_center().text_ellipsis().border_radius(4.0).apply_if(disabled,|s|s.color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))).apply_if(active.get()==i,|s|s.background(config.get().color(SparrowColor::COMPLETION_CURRENT))))
    }).style(|s|s.flex_col().width_full()))).style(|s|s.flex_col().width_full())).ensure_visible(move ||floem::peniko::kurbo::Rect::new(0.0,active.get() as f64*26.0,1.0,(active.get()+1) as f64*26.0)).style(|s|s.width(360.0).height_full());
    stack((
        stack((list, preview_view))
            .style(|s| s.height(360.0).min_height(0.0).width_full()),
        stack((
            crate::widgets::labeled_checkbox(
                move || show.get(),
                || "Preview",
                move |checked| show.set(checked),
                config,
            ),
            empty().style(|s| s.flex_grow(1.0_f32)),
            label(|| "Retry")
                .on_click_stop(move |_| {
                    data.get_untracked().cancel();
                    if let Some(editor) =
                        retry_window.main_split.active_editor.get_untracked()
                    {
                        editor.show_code_actions(false);
                    }
                })
                .style(move |s| {
                    s.padding_horiz(12.0)
                        .color(config.get().color(SparrowColor::EDITOR_LINK))
                        .apply_if(
                            data.with(|d| {
                                matches!(
                                    d.request_state,
                                    crate::code_action::ActionRequestState::Loading
                                ) || !d.filtered_items.is_empty()
                            }),
                            |s| s.hide(),
                        )
                }),
            label(|| "Cancel")
                .on_click_stop(move |_| close_data.get_untracked().cancel())
                .style(|s| s.padding_horiz(12.0)),
            label(|| "Apply  ↵")
                .on_click_stop(move |_| apply_data.get_untracked().select())
                .style(move |s| {
                    s.padding_horiz(12.0).color(config.get().color(
                        if matches!(preview.get(), Preview::Ready(_)) {
                            SparrowColor::EDITOR_LINK
                        } else {
                            SparrowColor::PANEL_FOREGROUND_DIM
                        },
                    ))
                }),
        ))
        .style(|s| s.height(32.0).items_center().padding_horiz(10.0)),
    ))
    .on_resize(move |rect| data.update(|d| d.layout_rect = rect))
    .on_event_stop(EventListener::PointerDown, |_| {})
    .on_event_stop(EventListener::PointerMove, |_| {})
    .on_cleanup(move || {
        if let Some(scope) = scope.borrow_mut().take() {
            scope.dispose();
        }
    })
    .style(move |s| {
        let origin = window.code_action_origin();
        s.absolute()
            .margin_left(origin.x as f32)
            .margin_top(origin.y as f32)
            .width(if show.get() { 960.0 } else { 360.0 })
            .max_width(
                (window.common.workbench_size.get().width - 24.0).max(360.0) as f32
            )
            .flex_col()
            .border(1.0)
            .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
            .border_radius(6.0)
            .background(config.get().color(SparrowColor::COMPLETION_BACKGROUND))
            .font_size(13.0)
            .apply_if(status.get() == CodeActionStatus::Inactive, |s| s.hide())
    })
}
