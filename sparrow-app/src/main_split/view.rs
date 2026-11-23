//! Editor split layout, resize handles and drop targets.

use std::{rc::Rc, sync::Arc};

use crate::{
    config::{SparrowConfig, color::SparrowColor},
    editor_tab::EditorTabData,
    id::{EditorTabId, SplitId},
    main_split::{SplitContent, SplitData, SplitDirection},
    panel::view::{ISLAND_RADIUS, island_corners},
    plugin::PluginData,
    window_tab::WindowTabData,
};
use floem::{
    IntoView, View,
    event::{Event, EventListener},
    peniko::kurbo::{Point, Rect},
    reactive::{
        ReadSignal, RwSignal, SignalGet, SignalUpdate, SignalWith, create_rw_signal,
    },
    style::{CursorStyle, FlexDirection, JustifyContent, Position},
    unit::PxPctAuto,
    views::{Decorators, container, dyn_stack, empty, stack, text},
};

use crate::editor::tab_view::editor_tab;

fn split_resize_border(
    splits: ReadSignal<im::HashMap<SplitId, RwSignal<SplitData>>>,
    editor_tabs: ReadSignal<im::HashMap<EditorTabId, RwSignal<EditorTabData>>>,
    split: ReadSignal<SplitData>,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    let content_rect = move |content: &SplitContent, tracked: bool| {
        if tracked {
            match content {
                SplitContent::EditorTab(editor_tab_id) => {
                    let editor_tab_data =
                        editor_tabs.with(|tabs| tabs.get(editor_tab_id).cloned());
                    if let Some(editor_tab_data) = editor_tab_data {
                        editor_tab_data.with(|editor_tab| editor_tab.layout_rect)
                    } else {
                        Rect::ZERO
                    }
                }
                SplitContent::Split(split_id) => {
                    if let Some(split) =
                        splits.with(|splits| splits.get(split_id).cloned())
                    {
                        split.with(|split| split.layout_rect)
                    } else {
                        Rect::ZERO
                    }
                }
            }
        } else {
            match content {
                SplitContent::EditorTab(editor_tab_id) => {
                    let editor_tab_data = editor_tabs
                        .with_untracked(|tabs| tabs.get(editor_tab_id).cloned());
                    if let Some(editor_tab_data) = editor_tab_data {
                        editor_tab_data
                            .with_untracked(|editor_tab| editor_tab.layout_rect)
                    } else {
                        Rect::ZERO
                    }
                }
                SplitContent::Split(split_id) => {
                    if let Some(split) =
                        splits.with_untracked(|splits| splits.get(split_id).cloned())
                    {
                        split.with_untracked(|split| split.layout_rect)
                    } else {
                        Rect::ZERO
                    }
                }
            }
        }
    };
    let direction = move |tracked: bool| {
        if tracked {
            split.with(|split| split.direction)
        } else {
            split.with_untracked(|split| split.direction)
        }
    };
    dyn_stack(
        move || {
            let data = split.get();
            data.children.into_iter().enumerate().skip(1)
        },
        |(index, (_, content))| (*index, content.id()),
        move |(index, (_, content))| {
            let drag_start: RwSignal<Option<Point>> = create_rw_signal(None);
            let view = empty();
            let view_id = view.id();
            view.on_event_stop(EventListener::PointerDown, move |event| {
                view_id.request_active();
                if let Event::PointerDown(pointer_event) = event {
                    drag_start.set(Some(pointer_event.pos));
                }
            })
            .on_event_stop(EventListener::PointerUp, move |_| {
                drag_start.set(None);
            })
            .on_event_stop(EventListener::PointerMove, move |event| {
                if let Event::PointerMove(pointer_event) = event {
                    if let Some(drag_start_point) = drag_start.get_untracked() {
                        let rects = split.with_untracked(|split| {
                            split
                                .children
                                .iter()
                                .map(|(_, c)| content_rect(c, false))
                                .collect::<Vec<Rect>>()
                        });
                        let direction = direction(false);
                        match direction {
                            SplitDirection::Vertical => {
                                let left = rects[index - 1].width();
                                let right = rects[index].width();
                                let shift = pointer_event.pos.x - drag_start_point.x;
                                let left = left + shift;
                                let right = right - shift;
                                let total_width =
                                    rects.iter().map(|r| r.width()).sum::<f64>();
                                split.with_untracked(|split| {
                                    for (i, (size, _)) in
                                        split.children.iter().enumerate()
                                    {
                                        if i == index - 1 {
                                            size.set(left / total_width);
                                        } else if i == index {
                                            size.set(right / total_width);
                                        } else {
                                            size.set(rects[i].width() / total_width);
                                        }
                                    }
                                })
                            }
                            SplitDirection::Horizontal => {
                                let up = rects[index - 1].height();
                                let down = rects[index].height();
                                let shift = pointer_event.pos.y - drag_start_point.y;
                                let up = up + shift;
                                let down = down - shift;
                                let total_height =
                                    rects.iter().map(|r| r.height()).sum::<f64>();
                                split.with_untracked(|split| {
                                    for (i, (size, _)) in
                                        split.children.iter().enumerate()
                                    {
                                        if i == index - 1 {
                                            size.set(up / total_height);
                                        } else if i == index {
                                            size.set(down / total_height);
                                        } else {
                                            size.set(
                                                rects[i].height() / total_height,
                                            );
                                        }
                                    }
                                })
                            }
                        }
                    }
                }
            })
            .style(move |s| {
                let rect = content_rect(&content, true);
                let is_dragging = drag_start.get().is_some();
                let direction = direction(true);
                s.position(Position::Absolute)
                    .apply_if(direction == SplitDirection::Vertical, |style| {
                        style.margin_left(rect.x0 as f32 - 0.0)
                    })
                    .apply_if(direction == SplitDirection::Horizontal, |style| {
                        style.margin_top(rect.y0 as f32 - 0.0)
                    })
                    .width(match direction {
                        SplitDirection::Vertical => PxPctAuto::Px(4.0),
                        SplitDirection::Horizontal => PxPctAuto::Pct(100.0),
                    })
                    .height(match direction {
                        SplitDirection::Vertical => PxPctAuto::Pct(100.0),
                        SplitDirection::Horizontal => PxPctAuto::Px(4.0),
                    })
                    .flex_direction(match direction {
                        SplitDirection::Vertical => FlexDirection::Row,
                        SplitDirection::Horizontal => FlexDirection::Column,
                    })
                    .apply_if(is_dragging, |s| {
                        s.cursor(match direction {
                            SplitDirection::Vertical => CursorStyle::ColResize,
                            SplitDirection::Horizontal => CursorStyle::RowResize,
                        })
                        .background(
                            config
                                .get()
                                .color(SparrowColor::EDITOR_LINK)
                                .multiply_alpha(0.75),
                        )
                    })
                    .hover(|s| {
                        s.cursor(match direction {
                            SplitDirection::Vertical => CursorStyle::ColResize,
                            SplitDirection::Horizontal => CursorStyle::RowResize,
                        })
                        .background(
                            config
                                .get()
                                .color(SparrowColor::EDITOR_LINK)
                                .multiply_alpha(0.75),
                        )
                    })
                    .pointer_events_auto()
            })
        },
    )
    .style(|s| {
        s.position(Position::Absolute)
            .size_full()
            .pointer_events_none()
    })
    .debug_name("Split Resize Border")
}

fn split_border(
    splits: ReadSignal<im::HashMap<SplitId, RwSignal<SplitData>>>,
    editor_tabs: ReadSignal<im::HashMap<EditorTabId, RwSignal<EditorTabData>>>,
    split: ReadSignal<SplitData>,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    let direction = move || split.with(|split| split.direction);
    dyn_stack(
        move || split.get().children.into_iter().skip(1),
        |(_, content)| content.id(),
        move |(_, content)| {
            container(empty().style(move |s| {
                let direction = direction();
                s.width(match direction {
                    SplitDirection::Vertical => PxPctAuto::Px(1.0),
                    SplitDirection::Horizontal => PxPctAuto::Pct(100.0),
                })
                .height(match direction {
                    SplitDirection::Vertical => PxPctAuto::Pct(100.0),
                    SplitDirection::Horizontal => PxPctAuto::Px(1.0),
                })
                .background(config.get().color(SparrowColor::SPARROW_BORDER))
            }))
            .style(move |s| {
                let rect = match &content {
                    SplitContent::EditorTab(editor_tab_id) => {
                        let editor_tab_data = editor_tabs
                            .with(|tabs| tabs.get(editor_tab_id).cloned());
                        if let Some(editor_tab_data) = editor_tab_data {
                            editor_tab_data.with(|editor_tab| editor_tab.layout_rect)
                        } else {
                            Rect::ZERO
                        }
                    }
                    SplitContent::Split(split_id) => {
                        if let Some(split) =
                            splits.with(|splits| splits.get(split_id).cloned())
                        {
                            split.with(|split| split.layout_rect)
                        } else {
                            Rect::ZERO
                        }
                    }
                };
                let direction = direction();
                s.position(Position::Absolute)
                    .apply_if(direction == SplitDirection::Vertical, |style| {
                        style.margin_left(rect.x0 as f32 - 2.0)
                    })
                    .apply_if(direction == SplitDirection::Horizontal, |style| {
                        style.margin_top(rect.y0 as f32 - 2.0)
                    })
                    .width(match direction {
                        SplitDirection::Vertical => PxPctAuto::Px(4.0),
                        SplitDirection::Horizontal => PxPctAuto::Pct(100.0),
                    })
                    .height(match direction {
                        SplitDirection::Vertical => PxPctAuto::Pct(100.0),
                        SplitDirection::Horizontal => PxPctAuto::Px(4.0),
                    })
                    .flex_direction(match direction {
                        SplitDirection::Vertical => FlexDirection::Row,
                        SplitDirection::Horizontal => FlexDirection::Column,
                    })
                    .justify_content(Some(JustifyContent::Center))
            })
        },
    )
    .style(|s| {
        s.position(Position::Absolute)
            .size_full()
            .pointer_events_none()
    })
    .debug_name("Split Border")
}

fn split_list(
    split: ReadSignal<SplitData>,
    window_tab_data: Rc<WindowTabData>,
    plugin: PluginData,
    dragging: RwSignal<Option<(RwSignal<usize>, EditorTabId)>>,
) -> impl View {
    let main_split = window_tab_data.main_split.clone();
    let editor_tabs = main_split.editor_tabs.read_only();
    let active_editor_tab = main_split.active_editor_tab.read_only();
    let splits = main_split.splits.read_only();
    let config = main_split.common.config;
    let split_id = split.with_untracked(|split| split.split_id);

    let direction = move || split.with(|split| split.direction);
    let items = move || split.get().children.into_iter().enumerate();
    let key = |(_index, (_, content)): &(usize, (RwSignal<f64>, SplitContent))| {
        content.id()
    };
    let view_fn = {
        let main_split = main_split.clone();
        let window_tab_data = window_tab_data.clone();
        move |(_index, (split_size, content)): (
            usize,
            (RwSignal<f64>, SplitContent),
        )| {
            let plugin = plugin.clone();
            let child = match &content {
                SplitContent::EditorTab(editor_tab_id) => {
                    let editor_tab_data = editor_tabs
                        .with_untracked(|tabs| tabs.get(editor_tab_id).cloned());
                    if let Some(editor_tab_data) = editor_tab_data {
                        editor_tab(
                            window_tab_data.clone(),
                            plugin.clone(),
                            active_editor_tab,
                            editor_tab_data,
                            dragging,
                        )
                        .into_any()
                    } else {
                        text("empty editor tab").into_any()
                    }
                }
                SplitContent::Split(split_id) => {
                    if let Some(split) =
                        splits.with(|splits| splits.get(split_id).cloned())
                    {
                        split_list(
                            split.read_only(),
                            window_tab_data.clone(),
                            plugin.clone(),
                            dragging,
                        )
                        .into_any()
                    } else {
                        text("empty split").into_any()
                    }
                }
            };
            let local_main_split = main_split.clone();
            let local_local_main_split = main_split.clone();
            child
                .on_resize(move |rect| match &content {
                    SplitContent::EditorTab(editor_tab_id) => {
                        local_main_split.editor_tab_update_layout(
                            editor_tab_id,
                            None,
                            Some(rect),
                        );
                    }
                    SplitContent::Split(split_id) => {
                        let split_data =
                            splits.with(|splits| splits.get(split_id).cloned());
                        if let Some(split_data) = split_data {
                            split_data.update(|split| {
                                split.layout_rect = rect;
                            });
                        }
                    }
                })
                .on_move(move |point| match &content {
                    SplitContent::EditorTab(editor_tab_id) => {
                        local_local_main_split.editor_tab_update_layout(
                            editor_tab_id,
                            Some(point),
                            None,
                        );
                    }
                    SplitContent::Split(split_id) => {
                        let split_data =
                            splits.with(|splits| splits.get(split_id).cloned());
                        if let Some(split_data) = split_data {
                            split_data.update(|split| {
                                split.window_origin = point;
                            });
                        }
                    }
                })
                .style(move |s| s.flex_grow(split_size.get() as f32).flex_basis(0.0))
        }
    };
    container(
        stack((
            dyn_stack(items, key, view_fn).style(move |s| {
                s.flex_direction(match direction() {
                    SplitDirection::Vertical => FlexDirection::Row,
                    SplitDirection::Horizontal => FlexDirection::Column,
                })
                .size_full()
            }),
            split_border(splits, editor_tabs, split, config),
            split_resize_border(splits, editor_tabs, split, config),
        ))
        .style(|s| s.size_full()),
    )
    .on_cleanup(move || {
        if splits.with_untracked(|splits| splits.contains_key(&split_id)) {
            return;
        }
        split
            .with_untracked(|split_data| split_data.scope)
            .dispose();
    })
    .debug_name("Split List")
}

pub(crate) fn main_split(window_tab_data: Rc<WindowTabData>) -> impl View {
    let root_split = window_tab_data.main_split.root_split;
    let root_split = window_tab_data
        .main_split
        .splits
        .get_untracked()
        .get(&root_split)
        .unwrap()
        .read_only();
    let config = window_tab_data.main_split.common.config;
    let plugin = window_tab_data.plugin.clone();
    let dragging: RwSignal<Option<(RwSignal<usize>, EditorTabId)>> =
        create_rw_signal(None);
    let split = split_list(
        root_split,
        window_tab_data.clone(),
        plugin.clone(),
        dragging,
    )
    .style(move |s| {
        let config = config.get();
        s.border_color(config.color(SparrowColor::SPARROW_BORDER))
            .background(config.color(SparrowColor::EDITOR_BACKGROUND))
            .size_full()
    })
    .debug_name("Main Split");

    let tabs = window_tab_data.main_split.editor_tabs;
    let has_files = move || {
        tabs.with(|tabs| {
            tabs.values()
                .any(|tab| tab.with(|tab| !tab.children.is_empty()))
        })
    };
    let split = split.style(move |s| s.apply_if(!has_files(), |s| s.hide()));
    let workbench = window_tab_data.common.workbench_command;
    let keypress = window_tab_data.common.keypress;
    let hints = [
        (
            "Search Everywhere",
            crate::command::SparrowWorkbenchCommand::SearchEverywhere,
        ),
        (
            "Go to File",
            crate::command::SparrowWorkbenchCommand::Palette,
        ),
        (
            "Open File",
            crate::command::SparrowWorkbenchCommand::OpenFile,
        ),
    ];
    let empty_editor = stack((
        floem::views::stack_from_iter(hints.into_iter().map(|(title, command)| {
            let id: &'static str = command.clone().into();
            let shortcut = move || {
                keypress.with(|keys| {
                    keys.command_keymaps
                        .get(id)
                        .and_then(|maps| maps.first())
                        .map(|map| {
                            map.key
                                .iter()
                                .map(|press| press.label())
                                .collect::<Vec<_>>()
                                .join(" ")
                        })
                        .unwrap_or_else(|| {
                            if title == "Search Everywhere" {
                                "Shift Shift".into()
                            } else {
                                String::new()
                            }
                        })
                })
            };
            stack((
                floem::views::label(move || title).style(move |s| {
                    s.color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
                }),
                floem::views::label(shortcut).style(move |s| {
                    s.color(config.get().color(SparrowColor::EDITOR_FOREGROUND))
                }),
            ))
            .on_click_stop(move |_| workbench.send(command.clone()))
            .style(|s| s.gap(10).items_center())
        }))
        .style(|s| s.flex_col().gap(12)),
        floem::views::label(|| "Drop files here to open them").style(move |s| {
            s.color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
        }),
    ))
    .style(move |s| {
        s.size_full()
            .flex_col()
            .justify_center()
            .items_center()
            .gap(18)
            .apply_if(has_files(), |s| s.hide())
    });

    // Islands look: the editor area is a rounded island.
    stack((
        split,
        empty_editor,
        island_corners(config, window_tab_data.common.project_color),
    ))
    .style(move |s| {
        let config = config.get();
        s.background(config.color(SparrowColor::EDITOR_BACKGROUND))
            .border_radius(ISLAND_RADIUS)
            .width_full()
            .flex_grow(1.0f32)
            .flex_basis(0.0)
    })
    .debug_name("Editor Island")
}
