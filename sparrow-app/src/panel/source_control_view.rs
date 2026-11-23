use std::rc::Rc;

use floem::{
    View,
    event::{Event, EventListener},
    reactive::{SignalGet, SignalUpdate, SignalWith, create_memo, create_rw_signal},
    views::{Decorators, container, label, stack},
};
use sparrow_core::buffer::rope_text::RopeText;

use super::{kind::PanelKind, position::PanelPosition};
use crate::{
    config::{color::SparrowColor, icon::SparrowIcons},
    editor::view::editor_view,
    widgets::{CheckboxState, group_checkbox, labeled_checkbox},
    window_tab::{Focus, WindowTabData},
};

mod file_list;
#[path = "/tmp/sparrow-commit-portal.aYRa5i/checkbox_probe.rs"]
mod probe;

use crate::widgets::sparrow_scroll;
use file_list::file_diffs_view;

pub fn source_control_panel(
    window: Rc<WindowTabData>,
    _position: PanelPosition,
) -> impl View {
    let source = window.source_control.clone();
    probe::run(source.clone());
    let config = window.common.config;
    let focus = source.common.focus;
    let editor = create_rw_signal(source.editor.clone());
    let doc = source.editor.doc_signal();
    let viewport = source.editor.viewport();
    let origin = source.editor.window_origin();
    let debug = create_memo(move |_| None);
    let active = move |tracked| {
        if tracked {
            focus.get() == Focus::Panel(PanelKind::SourceControl)
        } else {
            focus.get_untracked() == Focus::Panel(PanelKind::SourceControl)
        }
    };
    let message_width = create_rw_signal(400.0f64);
    let empty_message =
        create_memo(move |_| doc.get().buffer.with(|b| b.is_empty()));
    let commit = source.clone();
    let push = source.clone();
    let amend_source = source.clone();
    let amend = source.amend;
    let busy = source.busy;
    let status = source.status;
    let untracked = source.untracked;
    let files = source.file_diffs;
    let tracked_open = create_rw_signal(true);
    let new_open = create_rw_signal(true);
    let refresh_source = source.clone();
    let commit_action = source.commit_action;
    let group = move |unversioned: bool, open: floem::reactive::RwSignal<bool>| {
        let selection = create_memo(move |_| {
            untracked.with(|untracked| {
                files.with(|items| {
                    items
                        .values()
                        .filter(|(diff, _)| {
                            untracked.contains(diff.path()) == unversioned
                        })
                        .fold((0, 0), |(total, selected), (_, checked)| {
                            (total + 1, selected + usize::from(*checked))
                        })
                })
            })
        });
        let count = move || selection.with(|(total, _)| *total);
        stack((
            stack((
                probe::watch(label(move || if open.get() { "⌄" } else { "›" })
                    .on_click_stop(move |_| open.update(|v| *v = !*v))
                    .style(|s| s.width(18.0)), if unversioned { "fold:untracked" } else { "fold:tracked" }),
                probe::watch(group_checkbox(
                    move || {
                        selection.with(|&(total, selected)| {
                            CheckboxState::from_selection(total, selected)
                        })
                    },
                    move |selected| {
                        untracked.with_untracked(|untracked| {
                            files.update(|items| {
                                for (diff, checked) in items.values_mut() {
                                    if untracked.contains(diff.path()) == unversioned
                                    {
                                        *checked = selected;
                                    }
                                }
                            });
                        });
                    },
                    config,
                ), if unversioned { "untracked" } else { "tracked" }),
                label(move || {
                    format!(
                        "{}  {} files",
                        if unversioned {
                            "Unversioned Files"
                        } else {
                            "Changes"
                        },
                        count()
                    )
                })
                .style(|s| s.margin_left(6.0)),
            ))
            .style(move |s| {
                s.height(24.0)
                    .width_full()
                    .items_center()
                    .padding_horiz(6.0)
                    .background(
                        config
                            .get()
                            .color(SparrowColor::PANEL_HOVERED_BACKGROUND)
                            .with_alpha(0.3),
                    )
            }),
            file_diffs_view(source.clone(), unversioned).style(move |s| {
                s.min_height(0.0)
                    .height((count() * 24) as f32)
                    .apply_if(!open.get(), |s| s.hide())
            }),
        ))
        .style(move |s| {
            s.width_full()
                .flex_col()
                .apply_if(count() == 0, |s| s.hide())
        })
    };
    let commit_button = crate::widgets::button(
        move || {
            if busy.get() {
                "Committing…"
            } else if amend.get() {
                "Amend Commit"
            } else {
                "Commit"
            }
        },
        crate::widgets::ButtonKind::Outlined,
        config,
    )
    .on_click_stop(move |_| commit.commit())
    .disabled(move || busy.get())
    .style(|s| s.padding_horiz(12.0).height(26.0));
    let commit_button_id = commit_button.id();
    commit_action.set(Some(commit_button_id));
    let push_button = crate::widgets::button(
        || "Commit and Push…",
        crate::widgets::ButtonKind::Outlined,
        config,
    )
    .on_click_stop(move |_| push.commit_with_options(true))
    .disabled(move || busy.get())
    .style(|s| s.padding_horiz(12.0).height(26.0));
    let message_view = stack((
        editor_view(editor.get_untracked(), debug, active)
            .style(move |s| s.width(message_width.get().max(40.0) as f32)),
        label(|| "Commit Message").style(move |s| {
            s.absolute()
                .color(config.get().color(SparrowColor::EDITOR_DIM))
                .apply_if(!empty_message.get(), |s| s.hide())
        }),
    ))
    .style(|s| s.absolute().min_size_pct(100.0, 100.0).padding(8.0));
    let message_id = message_view.id();
    let message_view = message_view
        .keyboard_navigable()
        .on_event_cont(EventListener::FocusGained, move |_| {
            focus.set(Focus::Panel(PanelKind::SourceControl));
        })
        .on_event_stop(EventListener::PointerDown, move |event| {
            focus.set(Focus::Panel(PanelKind::SourceControl));
            message_id.request_focus();
            if let Event::PointerDown(pointer) = event.clone().offset((8.0, 8.0)) {
                message_id.request_active();
                editor.get_untracked().pointer_down(&pointer);
            }
        })
        .on_event_stop(EventListener::PointerMove, move |event| {
            if let Event::PointerMove(pointer) = event.clone().offset((8.0, 8.0)) {
                editor.get_untracked().pointer_move(&pointer);
            }
        })
        .on_event_stop(EventListener::PointerUp, move |event| {
            if let Event::PointerUp(pointer) = event.clone().offset((8.0, 8.0)) {
                editor.get_untracked().pointer_up(&pointer);
            }
        });
    stack((
        super::chrome::header(
            window.clone(),
            PanelKind::SourceControl,
            "Commit",
            floem::views::empty(),
        ),
        stack((
            crate::widgets::clickable_icon(
                || SparrowIcons::IMAGE_LOADING,
                {
                    let source = refresh_source.clone();
                    move || source.refresh()
                },
                || false,
                || false,
                || "Refresh changes",
                config,
            ),
            label(|| "Show Git Log")
                .on_click_stop(move |_| window.show_panel(PanelKind::GitHistory))
                .style(move |s| {
                    s.color(config.get().color(SparrowColor::EDITOR_LINK))
                }),
        ))
        .style(|s| s.height(28.0).gap(8.0).items_center().padding_horiz(6.0)),
        sparrow_scroll(
            stack((group(false, tracked_open), group(true, new_open)))
                .style(|s| s.flex_col().width_full()),
        )
        .style(|s| s.flex_grow(1.0_f32).min_height(40.0).width_full()),
        labeled_checkbox(
            move || amend.get(),
            || "Amend last commit",
            move |checked| amend_source.set_amend(checked),
            config,
        )
        .style(move |s| {
            s.height(30.0)
                .padding_horiz(8.0)
                .items_center()
                .border_top(1.0)
                .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
        }),
        container(
            sparrow_scroll(message_view)
                .on_move(move |point| origin.set(point + (8.0, 8.0)))
                .on_scroll(move |rect| viewport.set(rect))
                .style(|s| s.absolute().size_full()),
        )
        .on_resize(move |rect| message_width.set((rect.width() - 16.0).max(40.0)))
        .style(move |s| {
            s.height_pct(36.0)
                .min_height(100.0)
                .width_full()
                .border(1.0)
                .border_radius(2.0)
                .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
        }),
        label(move || status.get()).style(move |s| {
            s.width_full()
                .max_height(72.0)
                .padding_horiz(8.0)
                .font_size(11.0)
                .apply_if(status.with(|s| s.is_empty()), |s| s.hide())
        }),
        stack((commit_button, push_button))
            .style(|s| s.height(36.0).gap(6.0).items_center().padding_horiz(6.0)),
    ))
    .style(|s| s.absolute().size_full().flex_col().font_size(13.0))
    .on_cleanup(move || {
        if commit_action.get_untracked() == Some(commit_button_id) {
            commit_action.set(None);
        }
    })
}
