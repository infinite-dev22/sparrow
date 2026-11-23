//! Editor find and replace toolbar.

use floem::{
    View,
    event::EventListener,
    keyboard::Modifiers,
    reactive::{RwSignal, SignalGet, SignalUpdate, SignalWith, create_memo},
    style::CursorStyle,
    views::{Decorators, container, empty, label, stack},
};
use lapce_xi_rope::find::CaseMatching;

use crate::editor::EditorData;
use crate::{
    command::InternalCommand,
    config::{color::SparrowColor, icon::SparrowIcons},
    widgets::clickable_icon,
    widgets::text_input::TextInputBuilder,
    window_tab::Focus,
};

fn search_editor_view(
    find_editor: EditorData,
    find_focus: RwSignal<bool>,
    is_active: impl Fn(bool) -> bool + 'static + Copy,
    replace_focus: RwSignal<bool>,
) -> impl View {
    let config = find_editor.common.config;

    let case_matching = find_editor.common.find.case_matching;
    let whole_word = find_editor.common.find.whole_words;
    let is_regex = find_editor.common.find.is_regex;
    let visual = find_editor.common.find.visual;

    stack((
        TextInputBuilder::new()
            .is_focused(move || {
                is_active(true)
                    && visual.get()
                    && find_focus.get()
                    && !replace_focus.get()
            })
            .build_editor(find_editor)
            .on_event_cont(EventListener::PointerDown, move |_| {
                find_focus.set(true);
                replace_focus.set(false);
            })
            .style(|s| s.width_pct(100.0)),
        clickable_icon(
            || SparrowIcons::SEARCH_CASE_SENSITIVE,
            move || {
                let new = match case_matching.get_untracked() {
                    CaseMatching::Exact => CaseMatching::CaseInsensitive,
                    CaseMatching::CaseInsensitive => CaseMatching::Exact,
                };
                case_matching.set(new);
            },
            move || case_matching.get() == CaseMatching::Exact,
            || false,
            || "Case Sensitive",
            config,
        )
        .style(|s| s.padding_vert(4.0)),
        clickable_icon(
            || SparrowIcons::SEARCH_WHOLE_WORD,
            move || {
                whole_word.update(|whole_word| {
                    *whole_word = !*whole_word;
                });
            },
            move || whole_word.get(),
            || false,
            || "Whole Word",
            config,
        )
        .style(|s| s.padding_left(6.0)),
        clickable_icon(
            || SparrowIcons::SEARCH_REGEX,
            move || {
                is_regex.update(|is_regex| {
                    *is_regex = !*is_regex;
                });
            },
            move || is_regex.get(),
            || false,
            || "Use Regex",
            config,
        )
        .style(|s| s.padding_horiz(6.0)),
    ))
    .style(move |s| {
        let config = config.get();
        s.flex_grow(1.0_f32).flex_basis(0.0).min_width(0.0)
            .items_center()
            .border(1.0)
            .border_radius(6.0)
            .border_color(config.color(SparrowColor::SPARROW_BORDER))
            .background(config.color(SparrowColor::EDITOR_BACKGROUND))
    })
}

fn replace_editor_view(
    replace_editor: EditorData,
    replace_active: RwSignal<bool>,
    replace_focus: RwSignal<bool>,
    is_active: impl Fn(bool) -> bool + 'static + Copy,
    find_focus: RwSignal<bool>,
) -> impl View {
    let config = replace_editor.common.config;
    let visual = replace_editor.common.find.visual;

    stack((
        TextInputBuilder::new()
            .is_focused(move || {
                is_active(true)
                    && visual.get()
                    && find_focus.get()
                    && replace_active.get()
                    && replace_focus.get()
            })
            .build_editor(replace_editor)
            .on_event_cont(EventListener::PointerDown, move |_| {
                find_focus.set(true);
                replace_focus.set(true);
            })
            .style(|s| s.width_pct(100.0)),
        empty().style(move |s| {
            let config = config.get();
            let size = config.ui.icon_size() as f32 + 10.0;
            s.size(0.0, size).padding_vert(4.0)
        }),
    ))
    .style(move |s| {
        let config = config.get();
        s.flex_grow(1.0_f32).flex_basis(0.0).min_width(0.0)
            .items_center()
            .border(1.0)
            .border_radius(6.0)
            .border_color(config.color(SparrowColor::SPARROW_BORDER))
            .background(config.color(SparrowColor::EDITOR_BACKGROUND))
    })
}

pub(super) fn find_view(
    editor: RwSignal<EditorData>,
    find_editor: EditorData,
    find_focus: RwSignal<bool>,
    replace_editor: EditorData,
    replace_active: RwSignal<bool>,
    replace_focus: RwSignal<bool>,
    is_active: impl Fn(bool) -> bool + 'static + Copy,
) -> impl View {
    let common = find_editor.common.clone();
    let config = common.config;
    let find_visual = common.find.visual;
    let replace_doc = replace_editor.doc_signal();
    let focus = common.focus;

    let find_pos = create_memo(move |_| {
        let visual = find_visual.get();
        if !visual {
            return (0, 0);
        }
        let editor = editor.get_untracked();
        let cursor = editor.cursor();
        let offset = cursor.with(|cursor| cursor.offset());
        let occurrences = editor.doc_signal().get().find_result.occurrences;
        occurrences.with(|occurrences| {
            for (i, region) in occurrences.regions().iter().enumerate() {
                if offset <= region.max() {
                    return (i + 1, occurrences.regions().len());
                }
            }
            (occurrences.regions().len(), occurrences.regions().len())
        })
    });

    container(
        stack((
            stack((
                clickable_icon(
                    move || {
                        if replace_active.get() {
                            SparrowIcons::ITEM_OPENED
                        } else {
                            SparrowIcons::ITEM_CLOSED
                        }
                    },
                    move || {
                        replace_active.update(|active| *active = !*active);
                    },
                    move || false,
                    || false,
                    || "Toggle Replace",
                    config,
                )
                .style(|s| s.padding_horiz(6.0)),
                search_editor_view(
                    find_editor,
                    find_focus,
                    is_active,
                    replace_focus,
                ),
                label(move || {
                    let (current, all) = find_pos.get();
                    if all == 0 {
                        "No Results".to_string()
                    } else {
                        format!("{current} of {all}")
                    }
                })
                .style(|s| s.margin_left(6.0).min_width(70.0)),
                clickable_icon(
                    || SparrowIcons::SEARCH_BACKWARD,
                    move || {
                        editor.get_untracked().search_backward(Modifiers::empty());
                    },
                    move || false,
                    || false,
                    || "Previous Match",
                    config,
                )
                .style(|s| s.padding_left(6.0)),
                clickable_icon(
                    || SparrowIcons::SEARCH_FORWARD,
                    move || {
                        editor.get_untracked().search_forward(Modifiers::empty());
                    },
                    move || false,
                    || false,
                    || "Next Match",
                    config,
                )
                .style(|s| s.padding_left(6.0)),
                clickable_icon(
                    || SparrowIcons::CLOSE,
                    move || {
                        editor.get_untracked().clear_search();
                    },
                    move || false,
                    || false,
                    || "Close",
                    config,
                )
                .style(|s| s.padding_horiz(6.0)),
            ))
            .style(|s| s.items_center().width_full()),
            stack((
                empty().style(move |s| {
                    let config = config.get();
                    let width = config.ui.icon_size() as f32 + 10.0 + 6.0 * 2.0;
                    s.width(width)
                }),
                replace_editor_view(
                    replace_editor,
                    replace_active,
                    replace_focus,
                    is_active,
                    find_focus,
                ),
                clickable_icon(
                    || SparrowIcons::SEARCH_REPLACE,
                    move || {
                        let text = replace_doc
                            .get_untracked()
                            .buffer
                            .with_untracked(|b| b.to_string());
                        editor.get_untracked().replace_next(&text);
                    },
                    move || false,
                    || false,
                    || "Replace Next",
                    config,
                )
                .style(|s| s.padding_left(6.0)),
                clickable_icon(
                    || SparrowIcons::SEARCH_REPLACE_ALL,
                    move || {
                        let text = replace_doc
                            .get_untracked()
                            .buffer
                            .with_untracked(|b| b.to_string());
                        editor.get_untracked().replace_all(&text);
                    },
                    move || false,
                    || false,
                    || "Replace All",
                    config,
                )
                .style(|s| s.padding_left(6.0)),
            ))
            .style(move |s| {
                s.items_center()
                    .margin_top(4.0)
                    .apply_if(!replace_active.get(), |s| s.hide())
            }),
        ))
        .style(move |s| {
            let config = config.get();
            s.width_full().padding_horiz(6.0)
                .background(config.color(SparrowColor::PANEL_BACKGROUND))
                .border_radius(0.0)
                .border_bottom(1.0)
                .border_color(config.color(SparrowColor::SPARROW_BORDER))
                .padding_vert(4.0)
                .cursor(CursorStyle::Default)
                .flex_col()
        })
        .on_event_stop(EventListener::PointerDown, move |_| {
            // Shift the editor tab focus to the editor the find search is attached to
            // So that if you have two tabs open side-by-side (and thus two find views),
            // clicking on one will shift the focus to the editor it's attached to
            let editor = editor.get_untracked();
            if let Some(editor_tab_id) = editor.editor_tab_id.get_untracked() {
                editor
                    .common
                    .internal_command
                    .send(InternalCommand::FocusEditorTab { editor_tab_id });
            }
            focus.set(Focus::Workbench);
            // Request focus on the app view, as our current method of dispatching pointer events
            // is from the app_view to the actual editor. That's also why this stops the pointer
            // event is stopped here, as otherwise our default handling would make it go through to
            // the editor.
            common
                .window_common
                .app_view_id
                .get_untracked()
                .request_focus();
        }),
    )
    .style(move |s| {
        s.width_full().flex_shrink(0.0_f32)
            .apply_if(!find_visual.get(), |s| s.hide())
    })
}
