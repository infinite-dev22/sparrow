//! Editor tab strip and tab actions.

use std::rc::Rc;

use crate::{
    command::InternalCommand,
    config::{color::SparrowColor, icon::SparrowIcons, ui::TabSeparatorHeight},
    editor_tab::{EditorTabChild, EditorTabData},
    id::EditorTabId,
    main_split::SplitDirection,
    window_tab::{Focus, WindowTabData},
};
use floem::{
    View,
    event::{Event, EventListener, EventPropagation},
    peniko::kurbo::{Rect, Size},
    reactive::{
        ReadSignal, RwSignal, SignalGet, SignalUpdate, SignalWith, create_memo,
        create_rw_signal,
    },
    style::{AlignItems, CursorStyle},
    taffy::{
        Line,
        style_helpers::{self, auto, fr},
    },
    text::Style as FontStyle,
    unit::PxPctAuto,
    views::{
        Decorators, clip, container, dyn_stack, empty, label,
        scroll::{VerticalScrollAsHorizontal, scroll},
        stack, svg, text,
    },
};

use crate::widgets::clickable_icon;
use crate::widgets::tooltip::tooltip_tip;

use super::context_menu::tab_secondary_click;

/// The top bar of an Editor tab. Includes the tab forward/back buttons, the tab scroll bar and the new split and tab close all button.
pub(super) fn editor_tab_header(
    window_tab_data: Rc<WindowTabData>,
    active_editor_tab: ReadSignal<Option<EditorTabId>>,
    editor_tab: RwSignal<EditorTabData>,
    dragging: RwSignal<Option<(RwSignal<usize>, EditorTabId)>>,
) -> impl View {
    let main_split = window_tab_data.main_split.clone();
    let plugin = window_tab_data.plugin.clone();
    let editors = window_tab_data.main_split.editors;
    let diff_editors = window_tab_data.main_split.diff_editors;
    let focus = window_tab_data.common.focus;
    let config = window_tab_data.common.config;
    let internal_command = window_tab_data.common.internal_command;
    let tab_commands = window_tab_data.common.sparrow_command;
    let tab_focus = window_tab_data.main_split.active_editor_tab;
    let editor_tab_id =
        editor_tab.with_untracked(|editor_tab| editor_tab.editor_tab_id);

    let editor_tab_active =
        create_memo(move |_| editor_tab.with(|editor_tab| editor_tab.active));
    let items = move || {
        let editor_tab = editor_tab.get();
        for (i, (index, _, _)) in editor_tab.children.iter().enumerate() {
            if index.get_untracked() != i {
                index.set(i);
            }
        }
        editor_tab.children
    };
    let key = |(_, _, child): &(RwSignal<usize>, RwSignal<Rect>, EditorTabChild)| {
        child.id()
    };
    let is_focused = move || {
        if let Focus::Workbench = focus.get() {
            editor_tab.with_untracked(|e| Some(e.editor_tab_id))
                == active_editor_tab.get()
        } else {
            false
        }
    };

    let view_fn = move |(i, layout_rect, child): (
        RwSignal<usize>,
        RwSignal<Rect>,
        EditorTabChild,
    )| {
        let local_child = child.clone();
        let child_for_close = child.clone();
        let child_for_mouse_close = child.clone();
        let child_for_mouse_close_2 = child.clone();
        let main_split = main_split.clone();
        let plugin = plugin.clone();
        let child_view = {
            let info = child.view_info(editors, diff_editors, plugin, config);
            let hovered = create_rw_signal(false);

            use crate::config::ui::TabCloseButton;

            let tab_icon = container({
                svg("")
                    .update_value(move || info.with(|info| info.icon.clone()))
                    .style(move |s| {
                        let config = config.get();
                        let size = config.ui.icon_size() as f32;
                        s.size(size, size)
                            .apply_opt(info.with(|info| info.color), |s, c| {
                                s.color(c)
                            })
                            .apply_if(
                                !info.with(|info| info.is_pristine)
                                    && config.ui.tab_close_button
                                        == TabCloseButton::Off,
                                |s| {
                                    s.color(config.color(SparrowColor::SPARROW_WARN))
                                },
                            )
                    })
            })
            .style(|s| s.padding(4.));

            let tab_content = crate::widgets::tooltip::tooltip(
                label(move || info.with(|info| info.name.clone())).style(move |s| {
                    s.apply_if(
                        !info
                            .with(|info| info.confirmed)
                            .map(|confirmed| confirmed.get())
                            .unwrap_or(true),
                        |s| s.font_style(FontStyle::Italic),
                    )
                    .selectable(false)
                }),
                move || {
                    tooltip_tip(
                        config,
                        text(info.with(|info| {
                            info.path
                                .clone()
                                .map(|path| path.display().to_string())
                                .unwrap_or("local".to_string())
                        })),
                    )
                },
            );

            let tab_close_button = clickable_icon(
                move || {
                    if hovered.get() || info.with(|info| info.is_pristine) {
                        SparrowIcons::CLOSE
                    } else {
                        SparrowIcons::UNSAVED
                    }
                },
                move || {
                    let editor_tab_id =
                        editor_tab.with_untracked(|t| t.editor_tab_id);
                    internal_command.send(InternalCommand::EditorTabChildClose {
                        editor_tab_id,
                        child: child_for_close.clone(),
                    });
                },
                || false,
                || false,
                || "Close",
                config,
            )
            .on_event_stop(EventListener::PointerDown, |_| {})
            .on_event_stop(EventListener::PointerEnter, move |_| {
                hovered.set(true);
            })
            .on_event_stop(EventListener::PointerLeave, move |_| {
                hovered.set(false);
            });

            stack((
                tab_icon.style(move |s| {
                    let tab_close_button = config.get().ui.tab_close_button;
                    s.apply_if(tab_close_button == TabCloseButton::Left, |s| {
                        s.grid_column(Line {
                            start: style_helpers::line(3),
                            end: style_helpers::span(1),
                        })
                    })
                }),
                tab_content.style(move |s| {
                    let tab_close_button = config.get().ui.tab_close_button;
                    s.apply_if(tab_close_button == TabCloseButton::Left, |s| {
                        s.grid_column(Line {
                            start: style_helpers::line(2),
                            end: style_helpers::span(1),
                        })
                    })
                    .color(config.get().color(
                        if editor_tab_active.get() == i.get() {
                            SparrowColor::SPARROW_TAB_ACTIVE_FOREGROUND
                        } else {
                            SparrowColor::SPARROW_TAB_INACTIVE_FOREGROUND
                        },
                    ))
                    .apply_if(tab_close_button == TabCloseButton::Off, |s| {
                        s.padding_right(4.)
                    })
                }),
                tab_close_button.style(move |s| {
                    let tab_close_button = config.get().ui.tab_close_button;
                    s.apply_if(tab_close_button == TabCloseButton::Left, |s| {
                        s.grid_column(Line {
                            start: style_helpers::line(1),
                            end: style_helpers::span(1),
                        })
                    })
                    .apply_if(tab_close_button == TabCloseButton::Off, |s| s.hide())
                }),
            ))
            .style(move |s| {
                s.items_center()
                    .justify_center()
                    .padding_horiz(10.)
                    .gap(6.)
                    .grid()
                    .grid_template_columns(vec![auto(), fr(1_f32), auto()])
                    .apply_if(
                        config.get().ui.tab_separator_height
                            == TabSeparatorHeight::Full,
                        |s| s.height_full(),
                    )
            })
        };

        let confirmed = match local_child {
            EditorTabChild::Editor(editor_id) => {
                editors.editor_untracked(editor_id).map(|e| e.confirmed)
            }
            EditorTabChild::DiffEditor(diff_editor_id) => diff_editors
                .with_untracked(|diff_editors| {
                    diff_editors
                        .get(&diff_editor_id)
                        .map(|diff_editor_data| diff_editor_data.confirmed)
                }),
            _ => None,
        };

        let header_content_size = create_rw_signal(Size::ZERO);
        let drag_over_left: RwSignal<Option<bool>> = create_rw_signal(None);
        stack((
            child_view
                .on_double_click_stop(move |_| {
                    if let Some(confirmed) = confirmed {
                        confirmed.set(true);
                    }
                })
                .on_event(EventListener::PointerDown, move |event| {
                    if let Event::PointerDown(pointer_event) = event {
                        if pointer_event.button.is_auxiliary() {
                            let editor_tab_id =
                                editor_tab.with_untracked(|t| t.editor_tab_id);
                            internal_command.send(
                                InternalCommand::EditorTabChildClose {
                                    editor_tab_id,
                                    child: child_for_mouse_close.clone(),
                                },
                            );
                            EventPropagation::Stop
                        } else {
                            editor_tab.update(|editor_tab| {
                                editor_tab.active = i.get_untracked();
                            });
                            EventPropagation::Continue
                        }
                    } else {
                        EventPropagation::Continue
                    }
                })
                .on_secondary_click_stop(move |_| {
                    let editor_tab_id =
                        editor_tab.with_untracked(|t| t.editor_tab_id);

                    let index = i.get_untracked();
                    tab_focus.set(Some(editor_tab_id));
                    focus.set(Focus::Workbench);
                    editor_tab.update(|tab| tab.active = index);
                    let count = editor_tab.with_untracked(|tab| tab.children.len());
                    let path = match &child_for_mouse_close_2 {
                        EditorTabChild::Editor(id) => {
                            editors.editor_untracked(*id).and_then(|editor| {
                                editor.doc().content.with_untracked(|content| {
                                    content.path().cloned()
                                })
                            })
                        }
                        _ => None,
                    };
                    let root = path
                        .as_ref()
                        .and_then(|_| {
                            editors.editor_untracked(
                                match &child_for_mouse_close_2 {
                                    EditorTabChild::Editor(id) => *id,
                                    _ => unreachable!(),
                                },
                            )
                        })
                        .and_then(|editor| editor.common.workspace.path.clone());
                    tab_secondary_click(
                        internal_command,
                        tab_commands,
                        editor_tab_id,
                        child_for_mouse_close_2.clone(),
                        index,
                        count,
                        path,
                        root,
                    );
                })
                .on_event_stop(EventListener::DragStart, move |_| {
                    dragging.set(Some((i, editor_tab_id)));
                })
                .on_event_stop(EventListener::DragEnd, move |_| {
                    dragging.set(None);
                })
                .on_resize(move |rect| {
                    header_content_size.set(rect.size());
                })
                .draggable()
                .dragging_style(move |s| {
                    let config = config.get();
                    s.border_radius(6.0).background(
                        config
                            .color(SparrowColor::PANEL_BACKGROUND)
                            .multiply_alpha(0.7),
                    )
                })
                .style(|s| {
                    s.align_items(Some(AlignItems::Center)).flex_grow(1.0f32)
                }),
            empty()
                .style(move |s| {
                    let i = i.get();
                    let drag_over_left = drag_over_left.get();
                    s.absolute()
                        .margin_left(if i == 0 { 0.0 } else { -2.0 })
                        .height_full()
                        .width(
                            header_content_size.get().width as f32
                                + if i == 0 { 1.0 } else { 3.0 },
                        )
                        .apply_if(drag_over_left.is_none(), |s| s.hide())
                        .apply_if(drag_over_left.is_some(), |s| {
                            if let Some(drag_over_left) = drag_over_left {
                                if drag_over_left {
                                    s.border_left(3.0)
                                } else {
                                    s.border_right(3.0)
                                }
                            } else {
                                s
                            }
                        })
                        .border_color(
                            config
                                .get()
                                .color(SparrowColor::SPARROW_TAB_ACTIVE_UNDERLINE)
                                .multiply_alpha(0.5),
                        )
                })
                .debug_name("Active Tab Indicator"),
        ))
        .on_resize(move |rect| {
            layout_rect.set(rect);
        })
        .style(move |s| {
            let config = config.get();
            let active = editor_tab_active.get() == i.get();
            let border = config.color(SparrowColor::SPARROW_TAB_ACTIVE_BORDER);

            // Islands look: the active tab is a rounded pill with a thin blue outline,
            // inactive tabs are plain text.
            s.height((config.ui.header_height() as f32 - 8.0).max(24.0))
                .margin_horiz(2.0)
                .flex_col()
                .items_center()
                .justify_center()
                .border_radius(if config.ui.rounded_corners { 4.0 } else { 0.0 })
                .cursor(CursorStyle::Pointer)
                .background(config.color(if active {
                    SparrowColor::SPARROW_TAB_ACTIVE_BACKGROUND
                } else {
                    SparrowColor::SPARROW_TAB_INACTIVE_BACKGROUND
                }))
                .apply_if(active, |s| {
                    s.border(1.0).border_color(if is_focused() {
                        border
                    } else {
                        border.multiply_alpha(0.5)
                    })
                })
                .hover(|s| {
                    s.background(
                        config.color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                    )
                })
        })
        .debug_name("Tab and Active Indicator")
        .on_event_stop(EventListener::DragOver, move |event| {
            if dragging.with_untracked(|dragging| dragging.is_some()) {
                if let Event::PointerMove(pointer_event) = event {
                    let new_left = pointer_event.pos.x
                        < header_content_size.get_untracked().width / 2.0;
                    if drag_over_left.get_untracked() != Some(new_left) {
                        drag_over_left.set(Some(new_left));
                    }
                }
            }
        })
        .on_event(EventListener::Drop, move |event| {
            if let Some((from_index, from_editor_tab_id)) = dragging.get_untracked()
            {
                drag_over_left.set(None);
                if let Event::PointerUp(pointer_event) = event {
                    let left = pointer_event.pos.x
                        < header_content_size.get_untracked().width / 2.0;
                    let index = i.get_untracked();
                    let new_index = if left { index } else { index + 1 };
                    main_split.move_editor_tab_child(
                        from_editor_tab_id,
                        editor_tab_id,
                        from_index.get_untracked(),
                        new_index,
                    );
                }
                EventPropagation::Stop
            } else {
                EventPropagation::Continue
            }
        })
        .on_event_stop(EventListener::DragLeave, move |_| {
            drag_over_left.set(None);
        })
    };

    let content_size = create_rw_signal(Size::ZERO);
    let scroll_offset = create_rw_signal(Rect::ZERO);
    stack((
        stack({
            let size = create_rw_signal(Size::ZERO);
            (clip(empty().style(move |s| {
                let config = config.get();
                s.absolute()
                    .height_full()
                    .width(size.get().width as f32)
                    .background(config.color(SparrowColor::PANEL_BACKGROUND))
                    .box_shadow_blur(3.0)
                    .box_shadow_color(
                        config.color(SparrowColor::SPARROW_DROPDOWN_SHADOW),
                    )
            }))
            .style(move |s| {
                let scroll_offset = scroll_offset.get();
                s.absolute()
                    .width(size.get().width as f32 + 30.0)
                    .height_full()
                    .apply_if(scroll_offset.x0 == 0.0, |s| s.hide())
            }),)
        })
        .style(|s| s.flex_shrink(0f32)),
        container(
            scroll({
                dyn_stack(items, key, view_fn)
                    .on_resize(move |rect| {
                        let size = rect.size();
                        if content_size.get_untracked() != size {
                            content_size.set(size);
                        }
                    })
                    .debug_name("Horizontal Tab Stack")
                    .style(|s| s.height_full().items_center())
            })
            .on_scroll(move |rect| {
                scroll_offset.set(rect);
            })
            .ensure_visible(move || {
                let active = editor_tab_active.get();
                editor_tab
                    .with_untracked(|editor_tab| editor_tab.children[active].1)
                    .get_untracked()
            })
            .scroll_style(|s| s.hide_bars(true))
            .style(|s| {
                s.set(VerticalScrollAsHorizontal, true)
                    .absolute()
                    .size_full()
            }),
        )
        .style(|s| {
            s.height_full()
                .flex_grow(1.0f32)
                .flex_basis(0.)
                .min_width(10.)
        })
        .debug_name("Tab scroll"),
        stack({
            let size = create_rw_signal(Size::ZERO);
            (
                clip({
                    empty().style(move |s| {
                        let config = config.get();
                        s.absolute()
                            .height_full()
                            .margin_left(30.0)
                            .width(size.get().width as f32)
                            .background(config.color(SparrowColor::PANEL_BACKGROUND))
                            .box_shadow_blur(3.0)
                            .box_shadow_color(
                                config.color(SparrowColor::SPARROW_DROPDOWN_SHADOW),
                            )
                    })
                })
                .style(move |s| {
                    let content_size = content_size.get();
                    let scroll_offset = scroll_offset.get();
                    s.absolute()
                        .margin_left(-30.0)
                        .width(size.get().width as f32 + 30.0)
                        .height_full()
                        .apply_if(scroll_offset.x1 >= content_size.width, |s| {
                            s.hide()
                        })
                }),
                stack((
                    crate::markdown_preview::toolbar(window_tab_data.clone()),
                    clickable_icon(
                        || SparrowIcons::SPLIT_HORIZONTAL,
                        move || {
                            let editor_tab_id =
                                editor_tab.with_untracked(|t| t.editor_tab_id);
                            internal_command.send(InternalCommand::Split {
                                direction: SplitDirection::Vertical,
                                editor_tab_id,
                            });
                        },
                        || false,
                        || false,
                        || "Split Horizontally",
                        config,
                    )
                    .style(|s| s.margin_left(6.0)),
                ))
                .on_resize(move |rect| {
                    size.set(rect.size());
                })
                .style(|s| s.items_center().height_full()),
            )
        })
        .debug_name("Split/Close Panel Buttons")
        .style(move |s| {
            let content_size = content_size.get();
            let scroll_offset = scroll_offset.get();
            s.height_full()
                .flex_shrink(0f32)
                .margin_left(PxPctAuto::Auto)
                .apply_if(scroll_offset.x1 < content_size.width, |s| {
                    s.margin_left(0.)
                })
        }),
    ))
    .style(move |s| {
        let config = config.get();
        s.items_center()
            .padding_horiz(4.0)
            .max_width_full()
            .border_bottom(1.0)
            .border_color(config.color(SparrowColor::SPARROW_WINDOW_BACKGROUND))
            .background(config.color(SparrowColor::EDITOR_BACKGROUND))
            .height(config.ui.header_height() as i32)
    })
    .debug_name("Editor Tab Header")
}
