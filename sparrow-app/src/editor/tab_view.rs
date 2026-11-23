//! Editor tab group layout and split drop targets.

use std::rc::Rc;

use crate::{
    command::InternalCommand,
    config::color::SparrowColor,
    editor_tab::EditorTabData,
    id::EditorTabId,
    main_split::SplitMoveDirection,
    plugin::PluginData,
    window_tab::{Focus, WindowTabData},
};
use floem::{
    View,
    event::{Event, EventListener, EventPropagation},
    peniko::kurbo::Size,
    reactive::{
        ReadSignal, RwSignal, SignalGet, SignalUpdate, SignalWith, create_rw_signal,
    },
    views::{Decorators, empty, stack},
};

mod content;
mod context_menu;
mod header;

use content::editor_tab_content;
use header::editor_tab_header;

#[derive(Clone, Copy, PartialEq, Eq)]
enum DragOverPosition {
    Top,
    Bottom,
    Left,
    Right,
    Middle,
}

pub(crate) fn editor_tab(
    window_tab_data: Rc<WindowTabData>,
    plugin: PluginData,
    active_editor_tab: ReadSignal<Option<EditorTabId>>,
    editor_tab: RwSignal<EditorTabData>,
    dragging: RwSignal<Option<(RwSignal<usize>, EditorTabId)>>,
) -> impl View {
    let main_split = window_tab_data.main_split.clone();
    let common = main_split.common.clone();
    let editor_tabs = main_split.editor_tabs;
    let editor_tab_id =
        editor_tab.with_untracked(|editor_tab| editor_tab.editor_tab_id);
    let config = common.config;
    let focus = common.focus;
    let internal_command = main_split.common.internal_command;
    let tab_size = create_rw_signal(Size::ZERO);
    let drag_over: RwSignal<Option<DragOverPosition>> = create_rw_signal(None);
    stack((
        editor_tab_header(
            window_tab_data.clone(),
            active_editor_tab,
            editor_tab,
            dragging,
        ),
        stack((
            editor_tab_content(
                window_tab_data.clone(),
                plugin.clone(),
                active_editor_tab,
                editor_tab,
            ),
            empty()
                .style(move |s| {
                    let pos = drag_over.get();
                    let width = match pos {
                        Some(pos) => match pos {
                            DragOverPosition::Top => 100.0,
                            DragOverPosition::Bottom => 100.0,
                            DragOverPosition::Left => 50.0,
                            DragOverPosition::Right => 50.0,
                            DragOverPosition::Middle => 100.0,
                        },
                        None => 100.0,
                    };
                    let height = match pos {
                        Some(pos) => match pos {
                            DragOverPosition::Top => 50.0,
                            DragOverPosition::Bottom => 50.0,
                            DragOverPosition::Left => 100.0,
                            DragOverPosition::Right => 100.0,
                            DragOverPosition::Middle => 100.0,
                        },
                        None => 100.0,
                    };
                    let size = tab_size.get_untracked();
                    let margin_left = match pos {
                        Some(pos) => match pos {
                            DragOverPosition::Top => 0.0,
                            DragOverPosition::Bottom => 0.0,
                            DragOverPosition::Left => 0.0,
                            DragOverPosition::Right => size.width / 2.0,
                            DragOverPosition::Middle => 0.0,
                        },
                        None => 0.0,
                    };
                    let margin_top = match pos {
                        Some(pos) => match pos {
                            DragOverPosition::Top => 0.0,
                            DragOverPosition::Bottom => size.height / 2.0,
                            DragOverPosition::Left => 0.0,
                            DragOverPosition::Right => 0.0,
                            DragOverPosition::Middle => 0.0,
                        },
                        None => 0.0,
                    };
                    s.absolute()
                        .size_pct(width, height)
                        .margin_top(margin_top as f32)
                        .margin_left(margin_left as f32)
                        .apply_if(pos.is_none(), |s| s.hide())
                        .background(
                            config
                                .get()
                                .color(SparrowColor::EDITOR_DRAG_DROP_BACKGROUND),
                        )
                })
                .debug_name("Drag Over Handle"),
            empty()
                .on_event_stop(EventListener::DragOver, move |event| {
                    if dragging.with_untracked(|dragging| dragging.is_some()) {
                        if let Event::PointerMove(pointer_event) = event {
                            let size = tab_size.get_untracked();
                            let pos = pointer_event.pos;
                            let new_drag_over = if pos.x < size.width / 4.0 {
                                DragOverPosition::Left
                            } else if pos.x > size.width * 3.0 / 4.0 {
                                DragOverPosition::Right
                            } else if pos.y < size.height / 4.0 {
                                DragOverPosition::Top
                            } else if pos.y > size.height * 3.0 / 4.0 {
                                DragOverPosition::Bottom
                            } else {
                                DragOverPosition::Middle
                            };
                            if drag_over.get_untracked() != Some(new_drag_over) {
                                drag_over.set(Some(new_drag_over));
                            }
                        }
                    }
                })
                .on_event_stop(EventListener::DragLeave, move |_| {
                    drag_over.set(None);
                })
                .on_event(EventListener::Drop, move |_| {
                    if let Some((from_index, from_editor_tab_id)) =
                        dragging.get_untracked()
                    {
                        if let Some(pos) = drag_over.get_untracked() {
                            match pos {
                                DragOverPosition::Top => {
                                    main_split.move_editor_tab_child_to_new_split(
                                        from_editor_tab_id,
                                        from_index.get_untracked(),
                                        editor_tab_id,
                                        SplitMoveDirection::Up,
                                    );
                                }
                                DragOverPosition::Bottom => {
                                    main_split.move_editor_tab_child_to_new_split(
                                        from_editor_tab_id,
                                        from_index.get_untracked(),
                                        editor_tab_id,
                                        SplitMoveDirection::Down,
                                    );
                                }
                                DragOverPosition::Left => {
                                    main_split.move_editor_tab_child_to_new_split(
                                        from_editor_tab_id,
                                        from_index.get_untracked(),
                                        editor_tab_id,
                                        SplitMoveDirection::Left,
                                    );
                                }
                                DragOverPosition::Right => {
                                    main_split.move_editor_tab_child_to_new_split(
                                        from_editor_tab_id,
                                        from_index.get_untracked(),
                                        editor_tab_id,
                                        SplitMoveDirection::Right,
                                    );
                                }
                                DragOverPosition::Middle => {
                                    main_split.move_editor_tab_child(
                                        from_editor_tab_id,
                                        editor_tab_id,
                                        from_index.get_untracked(),
                                        editor_tab.with_untracked(|editor_tab| {
                                            editor_tab.active + 1
                                        }),
                                    );
                                }
                            }
                        }
                        drag_over.set(None);
                        EventPropagation::Stop
                    } else {
                        EventPropagation::Continue
                    }
                })
                .on_resize(move |rect| {
                    tab_size.set(rect.size());
                })
                .style(move |s| {
                    s.absolute()
                        .size_full()
                        .apply_if(dragging.get().is_none(), |s| {
                            s.pointer_events_none()
                        })
                }),
        ))
        .debug_name("Editor Content and Drag Over")
        .style(|s| s.size_full()),
    ))
    .on_event_cont(EventListener::PointerDown, move |_| {
        if focus.get_untracked() != Focus::Workbench {
            focus.set(Focus::Workbench);
        }
        let editor_tab_id = editor_tab.with_untracked(|t| t.editor_tab_id);
        internal_command.send(InternalCommand::FocusEditorTab { editor_tab_id });
    })
    .on_cleanup(move || {
        if editor_tabs
            .with_untracked(|editor_tabs| editor_tabs.contains_key(&editor_tab_id))
        {
            return;
        }
        editor_tab
            .with_untracked(|editor_tab| editor_tab.scope)
            .dispose();
    })
    .style(|s| s.flex_col().size_full())
    .debug_name("Editor Tab (Content + Header)")
}
