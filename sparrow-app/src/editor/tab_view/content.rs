//! Active view inside an editor tab group.

use std::rc::Rc;

use crate::{
    config::color::SparrowColor,
    editor::{diff::diff_show_more_section_view, view::editor_container_view},
    editor_tab::{EditorTabChild, EditorTabData},
    id::EditorTabId,
    keymap::keymap_view,
    plugin::{PluginData, plugin_info_view},
    settings::{settings_view, theme_color_settings_view},
    window_tab::{Focus, WindowTabData},
};
use floem::{
    IntoView, View,
    event::EventListener,
    reactive::{
        ReadSignal, RwSignal, SignalGet, SignalUpdate, SignalWith, create_effect,
        create_rw_signal,
    },
    style::Style,
    views::{Decorators, container, stack, tab, text},
};

pub(super) fn editor_tab_content(
    window_tab_data: Rc<WindowTabData>,
    plugin: PluginData,
    active_editor_tab: ReadSignal<Option<EditorTabId>>,
    editor_tab: RwSignal<EditorTabData>,
) -> impl View {
    let main_split = window_tab_data.main_split.clone();
    let common = main_split.common.clone();
    let workspace = common.workspace.clone();
    let editors = main_split.editors;
    let diff_editors = main_split.diff_editors;
    let config = common.config;
    let focus = common.focus;
    let items = move || {
        editor_tab
            .get()
            .children
            .into_iter()
            .map(|(_, _, child)| child)
    };
    let key = |child: &EditorTabChild| child.id();
    let view_fn = move |child| {
        let common = common.clone();
        let child = match child {
            EditorTabChild::Editor(editor_id) => {
                if let Some(editor_data) = editors.editor_untracked(editor_id) {
                    let editor_scope = editor_data.scope;
                    let editor_tab_id = editor_data.editor_tab_id;
                    let is_active = move |tracked: bool| {
                        editor_scope.track();
                        let focus = if tracked {
                            focus.get()
                        } else {
                            focus.get_untracked()
                        };
                        if let Focus::Workbench = focus {
                            let active_editor_tab = if tracked {
                                active_editor_tab.get()
                            } else {
                                active_editor_tab.get_untracked()
                            };
                            let editor_tab = if tracked {
                                editor_tab_id.get()
                            } else {
                                editor_tab_id.get_untracked()
                            };
                            editor_tab.is_some() && editor_tab == active_editor_tab
                        } else {
                            false
                        }
                    };
                    let editor_data = create_rw_signal(editor_data);
                    editor_container_view(
                        window_tab_data.clone(),
                        workspace.clone(),
                        is_active,
                        editor_data,
                    )
                    .into_any()
                } else {
                    text("empty editor").into_any()
                }
            }
            EditorTabChild::DiffEditor(diff_editor_id) => {
                let diff_editor_data = diff_editors.with_untracked(|diff_editors| {
                    diff_editors.get(&diff_editor_id).cloned()
                });
                if let Some(diff_editor_data) = diff_editor_data {
                    let focus_right = diff_editor_data.focus_right;
                    let diff_editor_tab_id = diff_editor_data.editor_tab_id;
                    let diff_editor_scope = diff_editor_data.scope;
                    let is_active = move |tracked: bool| {
                        let focus = if tracked {
                            focus.get()
                        } else {
                            focus.get_untracked()
                        };
                        if let Focus::Workbench = focus {
                            let active_editor_tab = if tracked {
                                active_editor_tab.get()
                            } else {
                                active_editor_tab.get_untracked()
                            };
                            let diff_editor_tab_id = if tracked {
                                diff_editor_tab_id.get()
                            } else {
                                diff_editor_tab_id.get_untracked()
                            };
                            Some(diff_editor_tab_id) == active_editor_tab
                        } else {
                            false
                        }
                    };
                    let left_viewport = diff_editor_data.left.viewport();
                    let left_scroll_to = diff_editor_data.left.scroll_to();
                    let right_viewport = diff_editor_data.right.viewport();
                    let right_scroll_to = diff_editor_data.right.scroll_to();
                    create_effect(move |_| {
                        let left_viewport = left_viewport.get();
                        if right_viewport.get_untracked() != left_viewport {
                            right_scroll_to
                                .set(Some(left_viewport.origin().to_vec2()));
                        }
                    });
                    create_effect(move |_| {
                        let right_viewport = right_viewport.get();
                        if left_viewport.get_untracked() != right_viewport {
                            left_scroll_to
                                .set(Some(right_viewport.origin().to_vec2()));
                        }
                    });
                    let left_editor =
                        create_rw_signal(diff_editor_data.left.clone());
                    let right_editor =
                        create_rw_signal(diff_editor_data.right.clone());
                    stack((
                        container(
                            editor_container_view(
                                window_tab_data.clone(),
                                workspace.clone(),
                                move |track| {
                                    is_active(track)
                                        && if track {
                                            !focus_right.get()
                                        } else {
                                            !focus_right.get_untracked()
                                        }
                                },
                                left_editor,
                            )
                            .debug_name("Left Editor"),
                        )
                        .on_event_cont(EventListener::PointerDown, move |_| {
                            focus_right.set(false);
                        })
                        .style(move |s| {
                            s.height_full()
                                .flex_grow(1.0f32)
                                .flex_basis(0.0)
                                .border_right(1.0)
                                .border_color(
                                    config.get().color(SparrowColor::SPARROW_BORDER),
                                )
                        }),
                        container(
                            editor_container_view(
                                window_tab_data.clone(),
                                workspace.clone(),
                                move |track| {
                                    is_active(track)
                                        && if track {
                                            focus_right.get()
                                        } else {
                                            focus_right.get_untracked()
                                        }
                                },
                                right_editor,
                            )
                            .debug_name("Right Editor"),
                        )
                        .on_event_cont(EventListener::PointerDown, move |_| {
                            focus_right.set(true);
                        })
                        .style(|s| {
                            s.height_full().flex_grow(1.0f32).flex_basis(0.0)
                        }),
                        diff_show_more_section_view(
                            &diff_editor_data.left,
                            &diff_editor_data.right,
                        ),
                    ))
                    .style(|s: Style| s.size_full())
                    .on_cleanup(move || {
                        diff_editor_scope.dispose();
                    })
                    .into_any()
                } else {
                    text("empty diff editor").into_any()
                }
            }
            EditorTabChild::Settings(_) => {
                settings_view(plugin.installed, editors, common).into_any()
            }
            EditorTabChild::ThemeColorSettings(_) => {
                theme_color_settings_view(editors, common).into_any()
            }
            EditorTabChild::Keymap(_) => keymap_view(editors, common).into_any(),
            EditorTabChild::Volt(_, id) => {
                plugin_info_view(plugin.clone(), id).into_any()
            }
        };
        child.style(|s| s.size_full())
    };
    let active = move || editor_tab.with(|t| t.active);

    tab(active, items, key, view_fn)
        .style(|s| s.size_full())
        .debug_name("Editor Tab Content")
}
