//! Editor gutter layout, breakpoint controls, folding and inline actions.

use std::{collections::BTreeMap, ops::DerefMut, path::PathBuf, rc::Rc};

use floem::{
    View,
    event::{Event, EventListener, EventPropagation},
    peniko::kurbo::Rect,
    reactive::{
        Memo, RwSignal, SignalGet, SignalUpdate, SignalWith, create_memo,
        create_rw_signal,
    },
    style::CursorStyle,
    views::{
        Decorators, clip, container, dyn_stack, editor::view::ScreenLines, empty,
        label, stack, svg,
    },
};
use lsp_types::CodeLens;
use sparrow_core::buffer::rope_text::RopeText;
use sparrow_rpc::{
    dap_types::{DapId, SourceBreakpoint},
    plugin::PluginId,
};

use crate::editor::{DocSignal, EditorData, gutter::editor_gutter_view};
use crate::{
    command::InternalCommand,
    config::{color::SparrowColor, icon::SparrowIcons},
    debug::{DapData, SparrowBreakpoint},
    editor::gutter::FoldingDisplayItem,
    widgets::clickable_icon,
    window_tab::{CommonData, WindowTabData},
};

pub(crate) fn toggle_gutter_breakpoint(
    line: usize, doc: DocSignal,
    daps: RwSignal<im::HashMap<DapId, DapData>>,
    breakpoints: RwSignal<BTreeMap<PathBuf, BTreeMap<usize, SparrowBreakpoint>>>,
    common: Rc<CommonData>,
) {
        let doc = doc.get_untracked();
        let offset = doc.buffer.with_untracked(|b| b.offset_of_line(line));
        if let Some(path) = doc.content.get_untracked().path() {
            let path_breakpoints = breakpoints
                .try_update(|breakpoints| {
                    let breakpoints = breakpoints.entry(path.clone()).or_default();
                    if let std::collections::btree_map::Entry::Vacant(e) =
                        breakpoints.entry(line)
                    {
                        e.insert(SparrowBreakpoint {
                            id: None,
                            verified: false,
                            message: None,
                            line,
                            offset,
                            dap_line: None,
                            active: true,
                        });
                    } else {
                        let mut toggle_active = false;
                        if let Some(breakpoint) = breakpoints.get_mut(&line) {
                            if !breakpoint.active {
                                breakpoint.active = true;
                                toggle_active = true;
                            }
                        }
                        if !toggle_active {
                            breakpoints.remove(&line);
                        }
                    }
                    breakpoints.clone()
                })
                .unwrap();
            let source_breakpoints: Vec<SourceBreakpoint> = path_breakpoints
                .iter()
                .filter_map(|(_, b)| {
                    if b.active {
                        Some(SourceBreakpoint {
                            line: b.line + 1,
                            column: None,
                            condition: None,
                            hit_condition: None,
                            log_message: None,
                        })
                    } else {
                        None
                    }
                })
                .collect();
            let daps: Vec<DapId> =
                daps.with_untracked(|daps| daps.keys().cloned().collect());
            for dap_id in daps {
                common.proxy.dap_set_breakpoints(
                    dap_id,
                    path.to_path_buf(),
                    source_breakpoints.clone(),
                );
            }
        }
}

fn editor_gutter_breakpoint_view(
    common: Rc<CommonData>,
    icon_padding: f32,
) -> impl View {
    let hovered = create_rw_signal(false);
    let config = common.config;
    container(
        svg(move || "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 12 12\"><circle cx=\"6\" cy=\"6\" r=\"6\" fill=\"currentColor\"/></svg>".to_string()).style(
            move |s| {
                let config = config.get();
                let size = 12.0;
                s.size(size, size)
                    .color(config.color(SparrowColor::DEBUG_BREAKPOINT_HOVER))
                    .apply_if(!hovered.get(), |s| s.hide())
            },
        ),
    )
    .on_event_stop(EventListener::PointerEnter, move |_| {
        hovered.set(true);
    })
    .on_event_stop(EventListener::PointerLeave, move |_| {
        hovered.set(false);
    })
    .style(move |s| {
        let config = config.get();
        s.width(config.ui.icon_size() as f32 + icon_padding * 2.0)
            .height(config.editor.line_height() as f32)
            .justify_center()
            .items_center()
            .cursor(CursorStyle::Pointer)
    })
}

fn editor_gutter_breakpoints(
    window_tab_data: Rc<WindowTabData>,
    e_data: RwSignal<EditorData>,
    icon_padding: f32,
) -> impl View {
    let breakpoints = window_tab_data.terminal.debug.breakpoints;
    let common = window_tab_data.common.clone();

    let (ed, config) = e_data
        .with_untracked(|e| (e.editor.clone(), e.common.config));
    let viewport = ed.viewport;
    let screen_lines = ed.screen_lines;

    let num_display_lines = create_memo(move |_| {
        let screen_lines = screen_lines.get();
        screen_lines.lines.len()
        // let viewport = viewport.get();
        // let line_height = config.get().editor.line_height() as f64;
        // (viewport.height() / line_height).ceil() as usize + 1
    });

    clip(
        stack((
            dyn_stack(
                move || {
                    let num = num_display_lines.get();
                    0..num
                },
                move |i| *i,
                move |_| editor_gutter_breakpoint_view(common.clone(), icon_padding),
            )
            .style(move |s| {
                s.absolute().inset_left(0.0).inset_top(0.0).flex_col().margin_top(
                    -(viewport.get().y0 % config.get().editor.line_height() as f64)
                        as f32,
                )
            })
            .debug_name("Breakpoint Stack"),
            dyn_stack(
                move || {
                    let e_data = e_data.get();
                    let doc = e_data.doc_signal().get();
                    let content = doc.content.get();
                    let breakpoints = if let Some(path) = content.path() {
                        breakpoints
                            .with(|b| b.get(path).cloned())
                            .unwrap_or_default()
                    } else {
                        Default::default()
                    };
                    breakpoints.into_iter()
                },
                move |(line, b)| (*line, b.active),
                move |(line, breakpoint)| {
                    let active = breakpoint.active;
                    container(
                        svg(move || {
                            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 12 12\"><circle cx=\"6\" cy=\"6\" r=\"6\" fill=\"currentColor\"/></svg>".to_string()
                        })
                        .style(move |s| {
                            let config = config.get();
                            let size = 12.0;
                            let color = if active {
                                SparrowColor::DEBUG_BREAKPOINT
                            } else {
                                SparrowColor::EDITOR_DIM
                            };
                            let color = config.color(color);
                            s.size(size, size).color(color)
                        }),
                    )
                    .style(move |s| {
                        let config = config.get();
                        let line_y = screen_lines
                            .with(|s| s.info_for_line(line))
                            .map(|l| l.y)
                            .unwrap_or_default();
                        s.absolute()
                            .width(config.ui.icon_size() as f32 + icon_padding * 2.0)
                            .height(config.editor.line_height() as f32)
                            .justify_center()
                            .items_center()
                            .margin_top(line_y as f32 - viewport.get().y0 as f32)
                    })
                },
            )
            .style(|s| s.absolute().size_pct(100.0, 100.0)),
        ))
        .style(|s| s.size_pct(100.0, 100.0)),
    )
    .style(move |s| {
        s.absolute().inset_left(0.0).inset_top(0.0)
            .width(config.get().ui.icon_size() as f32 + icon_padding * 2.0)
            .height_full()
            .z_index(2)
    })
}

fn editor_gutter_code_lens_view(
    window_tab_data: Rc<WindowTabData>,
    line: usize,
    lens: (PluginId, usize, im::Vector<CodeLens>),
    screen_lines: RwSignal<ScreenLines>,
    viewport: RwSignal<Rect>,
    icon_padding: f32,
) -> impl View {
    let config = window_tab_data.common.config;
    let view = container(
        svg(move || config.get().ui_svg(SparrowIcons::START)).style(move |s| {
            let config = config.get();
            let size = config.ui.icon_size() as f32;
            s.size(size, size)
                .color(config.color(SparrowColor::SPARROW_ICON_ACTIVE))
        }),
    )
    .style(move |s| {
        let config = config.get();
        s.padding(4.0)
            .border_radius(6.0)
            .hover(|s| {
                s.cursor(CursorStyle::Pointer)
                    .background(config.color(SparrowColor::PANEL_HOVERED_BACKGROUND))
            })
            .active(|s| {
                s.background(
                    config.color(SparrowColor::PANEL_HOVERED_ACTIVE_BACKGROUND),
                )
            })
    })
    .on_click_stop({
        move |_| {
            let (plugin_id, offset, lens) = lens.clone();
            window_tab_data.show_code_lens(true, plugin_id, offset, lens);
        }
    });
    container(view).style(move |s| {
        let line_info = screen_lines.with(|s| s.info_for_line(line));
        let line_y = line_info.clone().map(|l| l.y).unwrap_or(-100.0);
        let rect = viewport.get();
        let config = config.get();
        let icon_size = config.ui.icon_size();
        let width = icon_size as f32 + icon_padding * 2.0;
        s.absolute()
            .width(width)
            .height(config.editor.line_height() as f32)
            .justify_center()
            .items_center()
            .margin_top(line_y as f32 - rect.y0 as f32)
    })
}

fn editor_gutter_folding_view(
    window_tab_data: Rc<WindowTabData>,
    screen_lines: RwSignal<ScreenLines>,
    viewport: RwSignal<Rect>,
    folding_display_item: FoldingDisplayItem,
) -> impl View {
    let config = window_tab_data.common.config;
    let view = container(
        svg(move || {
            let icon_str = match folding_display_item {
                FoldingDisplayItem::UnfoldStart(_) => SparrowIcons::FOLD_DOWN,
                FoldingDisplayItem::Folded(_) => SparrowIcons::FOLD_CLOSE,
                FoldingDisplayItem::UnfoldEnd(_) => SparrowIcons::FOLD_UP,
            };
            config.get().ui_svg(icon_str)
        })
        .style(move |s| {
            let config = config.get();
            let size = config.ui.icon_size() as f32;
            s.size(size, size)
                .color(config.color(SparrowColor::SPARROW_ICON_ACTIVE))
        }),
    )
    .style(move |s| {
        let config = config.get();
        s.padding(4.0)
            .border_radius(6.0)
            .hover(|s| {
                s.cursor(CursorStyle::Pointer)
                    .background(config.color(SparrowColor::PANEL_HOVERED_BACKGROUND))
            })
            .active(|s| {
                s.background(
                    config.color(SparrowColor::PANEL_HOVERED_ACTIVE_BACKGROUND),
                )
            })
    });
    container(view).style(move |s| {
        let line = folding_display_item.position().line;
        let line_info = screen_lines.with(|s| s.info_for_line(line as usize));
        let line_y = line_info.clone().map(|l| l.y).unwrap_or(-100.0);
        let rect = viewport.get();
        let config = config.get();
        let icon_size = config.ui.icon_size();
        let width = icon_size as f32 + 4.0;
        s.absolute()
            .width(width / 2.0)
            .height(config.editor.line_height() as f32)
            .justify_center()
            .items_center()
            .margin_top(line_y as f32 - rect.y0 as f32)
    })
}

fn editor_gutter_code_lens(
    window_tab_data: Rc<WindowTabData>,
    doc: DocSignal,
    screen_lines: RwSignal<ScreenLines>,
    viewport: RwSignal<Rect>,
    icon_padding: f32,
) -> impl View {
    let config = window_tab_data.common.config;

    dyn_stack(
        move || {
            let doc = doc.get();
            doc.code_lens.get()
        },
        move |(line, _)| (*line, doc.with_untracked(|doc| doc.rev())),
        move |(line, lens)| {
            editor_gutter_code_lens_view(
                window_tab_data.clone(),
                line,
                lens,
                screen_lines,
                viewport,
                icon_padding,
            )
        },
    )
    .style(move |s| {
        let config = config.get();
        let width = config.ui.icon_size() as f32 + icon_padding * 2.0;
        s.absolute()
            .width(width)
            .height_full()
            .margin_left(width - 8.0)
    })
    .debug_name("CodeLens Stack")
}

pub(super) fn editor_gutter_folding_range(
    window_tab_data: Rc<WindowTabData>,
    doc: DocSignal,
    screen_lines: RwSignal<ScreenLines>,
    viewport: RwSignal<Rect>,
) -> impl View {
    let config = window_tab_data.common.config;
    dyn_stack(
        move || doc.get().folding_ranges.get().to_display_items(),
        move |item| *item,
        move |item| {
            editor_gutter_folding_view(
                window_tab_data.clone(),
                screen_lines,
                viewport,
                item,
            )
            .on_click_stop({
                move |_| {
                    doc.get_untracked().folding_ranges.update(|x| match item {
                        FoldingDisplayItem::UnfoldStart(pos)
                        | FoldingDisplayItem::Folded(pos) => {
                            x.0.iter_mut().find_map(|mut range| {
                                let range = range.deref_mut();
                                if range.start == pos {
                                    range.status.click();
                                    Some(())
                                } else {
                                    None
                                }
                            });
                        }
                        FoldingDisplayItem::UnfoldEnd(pos) => {
                            x.0.iter_mut().find_map(|mut range| {
                                let range = range.deref_mut();
                                if range.end == pos {
                                    range.status.click();
                                    Some(())
                                } else {
                                    None
                                }
                            });
                        }
                    })
                }
            })
        },
    )
    .style(move |s| {
        let config = config.get();
        let width = config.ui.icon_size() as f32;
        // hide for now
        s.width(width)
            .height_full()
            .margin_left(-width / 2.0)
            .hide()
    })
    .debug_name("Folding Range Stack")
}

pub(super) fn editor_gutter_code_actions(
    e_data: RwSignal<EditorData>,
    gutter_width: Memo<f64>,
    icon_padding: f32,
) -> impl View {
    let (ed, doc, config) = e_data
        .with_untracked(|e| (e.editor.clone(), e.doc_signal(), e.common.config));
    let viewport = ed.viewport;
    let cursor = ed.cursor;

    let code_action_vline = create_memo(move |_| {
        let doc = doc.get();
        let (offset, affinity) =
            cursor.with(|cursor| (cursor.offset(), cursor.affinity));
        let has_code_actions = doc
            .code_actions()
            .with(|c| c.get(&offset).map(|c| !c.1.is_empty()).unwrap_or(false));
        if has_code_actions {
            let vline = ed.vline_of_offset(offset, affinity);
            Some(vline)
        } else {
            None
        }
    });

    container(
        container(
            svg(move || config.get().ui_svg(SparrowIcons::LIGHTBULB)).style(
                move |s| {
                    let config = config.get();
                    let size = config.ui.icon_size() as f32;
                    s.size(size, size)
                        .color(config.color(SparrowColor::SPARROW_WARN))
                },
            ),
        )
        .on_click_stop(move |_| {
            e_data.get_untracked().show_code_actions(true);
        })
        .style(move |s| {
            let config = config.get();
            s.padding(4.0)
                .border_radius(6.0)
                .hover(|s| {
                    s.cursor(CursorStyle::Pointer).background(
                        config.color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                    )
                })
                .active(|s| {
                    s.background(
                        config.color(SparrowColor::PANEL_HOVERED_ACTIVE_BACKGROUND),
                    )
                })
        }),
    )
    .style(move |s| {
        let config = config.get();
        let viewport = viewport.get();
        let gutter_width = gutter_width.get();
        let code_action_vline = code_action_vline.get();
        let size = config.ui.icon_size() as f32;
        let line_height = config.editor.line_height();
        let margin_top = if let Some(vline) = code_action_vline {
            (vline.get() * line_height) as f32 - viewport.y0 as f32
        } else {
            0.0
        };
        let width = size + icon_padding * 2.0;
        s.absolute()
            .items_center()
            .justify_center()
            .margin_left(gutter_width as f32 - width + 1.0)
            .margin_top(margin_top)
            .width(width)
            .height(line_height as f32)
            .apply_if(code_action_vline.is_none(), |s| s.hide())
    })
    .debug_name("Code Action LightBulb")
}

fn editor_gutter_tests(
    window: Rc<WindowTabData>,
    e_data: RwSignal<EditorData>,
) -> impl View {
    let (ed, doc, config) = e_data
        .with_untracked(|e| (e.editor.clone(), e.doc_signal(), e.common.config));
    let screen_lines = ed.screen_lines;
    let viewport = ed.viewport;
    let internal = window.common.internal_command;
    // Cache discovery by buffer revision, rather than rescanning on scroll.
    let tests = create_memo(move |_| {
        let doc = doc.get();
        let path = doc.content.with(|c| c.path().cloned());
        let Some(path) = path.filter(|p| crate::dart::test_runner::is_test_file(p))
        else {
            return Vec::new();
        };
        let source = doc.buffer.with(|b| b.to_string());
        crate::dart::test_runner::discover_tests(&source)
            .into_iter()
            .map(|test| (path.clone(), test))
            .collect::<Vec<_>>()
    });
    dyn_stack(
        move || tests.get(),
        |(path, test)| (path.clone(), test.byte_range.0, test.name.clone()),
        move |(path, test)| {
            let line = test.line;
            let debug_path = path.clone();
            let debug_name = test.name.clone();
            stack((
                crate::widgets::tooltip_label(
                    config,
                    container(svg(move || config.get().ui_svg(SparrowIcons::START))
                        .style(move |s| {
                            let config = config.get();
                            let size = config.ui.icon_size() as f32;
                            s.size(size, size).color(config.color(SparrowColor::SOURCE_CONTROL_ADDED))
                        }))
                        .style(move |s| s.padding(4.0).border_radius(6.0)
                            .hover(|s| s.cursor(CursorStyle::Pointer)
                                .background(config.get().color(SparrowColor::PANEL_HOVERED_BACKGROUND))))
                        .on_click_stop(move |_| {
                            internal.send(InternalCommand::FlutterRunTest {
                                path: path.clone(), name: test.name.clone(), debug: false,
                            });
                        }),
                    || "Run Test / Group",
                ),
                clickable_icon(
                    || SparrowIcons::DEBUG,
                    move || {
                        internal.send(InternalCommand::FlutterRunTest {
                            path: debug_path.clone(),
                            name: debug_name.clone(),
                            debug: true,
                        })
                    },
                    || false,
                    || false,
                    || "Debug Test / Group",
                    config,
                ),
            ))
            .style(move |s| {
                let info = screen_lines.with(|s| s.info_for_line(line));
                let y = info.map(|i| i.y).unwrap_or(-100.0) - viewport.get().y0;
                s.absolute()
                    .height(config.get().editor.line_height() as f32)
                    .items_center()
                    .margin_top(y as f32)
            })
        },
    )
    .style(move |s| {
        s.absolute()
            .inset_right(0.0)
            .width(48.0)
            .height_full()
            .apply_if(tests.with(|t| t.is_empty()), |s| s.hide())
    })
    .debug_name("Dart Test Gutter")
}

pub(super) fn editor_gutter(
    window_tab_data: Rc<WindowTabData>,
    e_data: RwSignal<EditorData>,
) -> impl View {
    let icon_padding = 6.0;

    let (ed, doc, config) = e_data
        .with_untracked(|e| (e.editor.clone(), e.doc_signal(), e.common.config));
    let viewport = ed.viewport;
    let scroll_delta = ed.scroll_delta;
    let screen_lines = ed.screen_lines;

    let gutter_rect = create_rw_signal(Rect::ZERO);
    let gutter_width = create_memo(move |_| gutter_rect.get().width());

    let previews = create_memo(move |_| {
        let doc = doc.get();
        if doc.content.with(|c|c.path().is_some_and(|p|crate::dart::is_dart_path(p))) {
            doc.buffer.with(|b|crate::dart::extract_swatches(&b.to_string()))
        } else { (Vec::new(),Vec::new()) }
    });
    let bookmarks = window_tab_data.common.bookmarks;
    let lanes = create_memo(move |_| {
        let doc = doc.get();
        let (colors,icons)=previews.get();
        let max_on_line=|lines:Vec<usize>| {
            let mut counts=std::collections::HashMap::<usize,usize>::new();
            for line in lines { *counts.entry(line).or_default()+=1; }
            counts.into_values().max().unwrap_or(0)
        };
        let has_bookmarks=doc.content.with(|c|c.path().is_some_and(|p|bookmarks.with(|items|items.get(p).is_some_and(|lines|!lines.is_empty()))));
        let tests=doc.content.with(|c|c.path().is_some_and(|p|crate::dart::test_runner::is_test_file(p)));
        crate::editor::gutter::GutterLanes::new(max_on_line(colors.iter().map(|c|c.line).collect()),
            max_on_line(icons.iter().map(|i|i.line).collect()),has_bookmarks,
            doc.code_lens.with(|lenses|!lenses.is_empty()),tests)
    });
    let gutter_padding_right=create_memo(move |_|lanes.get().padding);
    let number_width=create_memo(move |_|gutter_width.get()-gutter_padding_right.get() as f64);
    let click_doc = doc;
    let click_common = window_tab_data.common.clone();
    let click_daps = window_tab_data.terminal.debug.daps;
    let click_breakpoints = window_tab_data.terminal.debug.breakpoints;
    stack((
        stack((
            empty().style(|s| s.width(0.0)),
            label(move || {
                let doc = doc.get();
                doc.buffer.with(|b| b.last_line() + 1).to_string()
            }).style(move |s|s.font_family(config.get().editor.font_family.clone()).font_size(config.get().editor.font_size() as f32)),
            empty().style(move |s| s.width(gutter_padding_right.get())),
        ))
        .debug_name("Centered Last Line Count")
        .style(|s| s.height_pct(100.0).color(floem::peniko::Color::TRANSPARENT)),
        clip(
            stack((
                editor_gutter_code_lens(
                    window_tab_data.clone(),
                    doc,
                    screen_lines,
                    viewport,
                    icon_padding,
                ).style(move |s|s.margin_left((number_width.get()+lanes.get().run as f64) as f32)),
                editor_gutter_view(e_data.get_untracked(), gutter_padding_right, lanes, previews)
                    .on_resize(move |rect| {
                        gutter_rect.set(rect);
                    })
                    .on_event_stop(EventListener::PointerWheel, move |event| {
                        if let Event::PointerWheel(pointer_event) = event {
                            scroll_delta.set(pointer_event.delta);
                        }
                    })
                    .style(|s| s.size_pct(100.0, 100.0)),

                editor_gutter_tests(window_tab_data.clone(), e_data).style(|s|s.inset_right(6.0)),
            ))
            .style(|s| s.size_pct(100.0, 100.0)),
        )
        .style(move |s| s.absolute().size_pct(100.0, 100.0)),
        editor_gutter_breakpoints(window_tab_data.clone(), e_data, 3.0)
            .style(move |s|s.margin_left((number_width.get()+lanes.get().breakpoint as f64) as f32)),
    ))
    .style(move |s| s.height_pct(100.0).padding_left(4.0).border_right(1.0)
        .border_color(config.get().color(SparrowColor::SPARROW_BORDER)))
    .on_event(EventListener::PointerDown, move |event| {
        if let Event::PointerDown(pointer) = event {
            if !pointer.button.is_primary() { return EventPropagation::Continue; }
            let editor = e_data.get_untracked();
            let width = gutter_rect.get_untracked().width();
            if editor.kind.get_untracked().is_normal() && pointer.pos.x >= width - 7.0
                && pointer.pos.x <= width {
                let doc = editor.doc();
                let items = super::diff_markers::hunks(&doc, &doc.head_changes().get_untracked());
                let y = pointer.pos.y + viewport.get_untracked().y0;
                if let Some(index) = items.iter().position(|hunk| {
                    let (top, bottom) = super::diff_markers::gutter_span(&editor, hunk);
                    y >= top && y < bottom
                }) {
                    super::diff_markers::show_hunk_menu(&editor, &items, index);
                    return EventPropagation::Stop;
                }
            }
            let height = config.get_untracked().editor.line_height() as f64;
            let line = screen_lines.with_untracked(|lines|lines.iter_lines_y()
                .find(|(_,y)|pointer.pos.y>=*y-viewport.get_untracked().y0 && pointer.pos.y<*y-viewport.get_untracked().y0+height)
                .map(|(line,_)|line));
            if let Some(line)=line {
                let x=pointer.pos.x-number_width.get_untracked();
                let layout=lanes.get_untracked();
                if pointer.modifiers.alt() || (x>=layout.bookmark as f64 && x<layout.bookmark as f64+14.0) {
                    if let Some(path)=click_doc.get_untracked().content.with_untracked(|c|c.path().cloned()) {
                        bookmarks.update(|items|{
                            let lines=items.entry(path).or_default();
                            if !lines.remove(&line) { lines.insert(line); }
                        });
                    }
                    return EventPropagation::Stop;
                }
                if x>=layout.colors as f64 && x<layout.bookmark as f64 {
                    let slot=((x-layout.colors as f64)/14.0).floor() as usize;
                    if let Some(swatch)=previews.get_untracked().0.into_iter().filter(|color|color.line==line).nth(slot) {
                        let document=click_doc.get_untracked();let start=document.buffer.with_untracked(|buffer|buffer.offset_of_line(line)+swatch.col);
                        let end=start+swatch.source_text.len();let expected=swatch.source_text.clone();
                        let mut menu=crate::widgets::popup_menu::Menu::new("Color");
                        for (name,argb) in [("Amber",0xFFFFC107u32),("Blue",0xFF2196F3),("Green",0xFF4CAF50),("Red",0xFFF44336),("White",0xFFFFFFFF),("Black",0xFF000000),("Transparent",0x00000000)] {
                            let doc=document.clone();let expected=expected.clone();
                            menu=menu.entry(crate::widgets::popup_menu::MenuItem::new(format!("{name}  #{argb:08X}")).action(move ||{
                                if doc.buffer.with_untracked(|buffer|end<=buffer.len()&&buffer.slice_to_cow(start..end).as_ref()==expected) {
                                    let replacement=format!("Color(0x{argb:08X})");
                                    doc.do_raw_edit(&[(sparrow_core::selection::Selection::region(start,end),replacement.as_str())],sparrow_core::editor::EditType::Other);
                                }
                            }));
                        }
                        crate::widgets::popup_menu::show_context_menu(menu,None);
                    }
                    return EventPropagation::Stop;
                }
                if x>=layout.breakpoint as f64 && x<layout.icons as f64 {
                    toggle_gutter_breakpoint(line, click_doc, click_daps, click_breakpoints, click_common.clone());
                    return EventPropagation::Stop;
                }
            }
        }
        EventPropagation::Continue
    })
    .debug_name("Editor Gutter")
}
