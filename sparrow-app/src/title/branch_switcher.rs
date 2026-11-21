//! Branch switcher popup and branch action rows.

use floem::reactive::Scope;
use floem::{
    View,
    kurbo::Point,
    reactive::{
        ReadSignal, RwSignal, SignalGet, SignalUpdate, SignalWith, create_effect,
        create_memo,
    },
    style::{CursorStyle, Display},
    text::Weight,
    views::{Decorators, container, dyn_stack, empty, label, stack, svg},
};
use std::{rc::Rc, sync::Arc};

use crate::command::CommandKind;
use crate::widgets::text_input::TextInputBuilder;
use crate::{
    command::{SparrowCommand, SparrowWorkbenchCommand},
    config::{SparrowConfig, color::SparrowColor, icon::SparrowIcons},
    widgets::divider::Divider,
    window_tab::WindowTabData,
};
use crate::widgets::sparrow_scroll;

pub fn branch_switcher(
    window_tab_data: Rc<WindowTabData>,
    origin: RwSignal<Point>,
) -> impl View {
    let visible = window_tab_data.branch_menu_visible;
    let config = window_tab_data.common.config;
    let source_control = window_tab_data.source_control.clone();
    let current_branch = source_control.branch.read_only();
    let branches = source_control.branches.read_only();
    let tags = source_control.tags.read_only();
    let sparrow_command = window_tab_data.common.sparrow_command;
    let workbench_command = window_tab_data.common.workbench_command;
    let title_height = window_tab_data.title_height;
    let window_origin = window_tab_data.common.window_origin;

    // Setup a local editor for the search input
    let cx = Scope::current();
    let search_editor = window_tab_data
        .main_split
        .editors
        .make_local(cx, window_tab_data.common.clone());
    let search_buffer = search_editor.doc_signal();

    // Clear search when opened
    create_effect({
        let value = search_editor.clone();
        move |_| {
            if visible.get() {
                value.doc().reload(lapce_xi_rope::Rope::from(""), true);
            }
        }
    });

    let close_menu = {
        let visible = window_tab_data.branch_menu_visible;
        move || visible.set(false)
    };

    let search_text = create_memo(move |_| {
        search_buffer.with(|d| d.buffer.with(|b| b.to_string().to_lowercase()))
    });

    // Section header label
    let section_label = move |text: &'static str| {
        label(move || text.to_string()).style(move |s| {
            s.padding_horiz(10.0)
                .padding_vert(4.0)
                .margin_top(4.0) // Give sections breathing room
                .font_size(11.0)
                .font_weight(Weight::BOLD)
                .color(config.get().color(SparrowColor::EDITOR_DIM))
        })
    };

    let divider = move |spacing| Divider::new(config).spacing(spacing).into_view();

    // Helper to draw a single branch/tag row
    let branch_row = move |name: String, is_current: bool| {
        let name_clone = name.clone();
        let cmd = sparrow_command;
        let close = close_menu;
        let config_get = config;

        container(
            stack((
                svg(move || config_get.get().ui_svg(SparrowIcons::SCM)).style(
                    move |s| {
                        let c = config_get.get();
                        let size = c.ui.icon_size() as f32 - 2.0;
                        s.size(size, size).margin_right(8.0).color(if is_current {
                            c.color(SparrowColor::EDITOR_FOREGROUND)
                        } else {
                            c.color(SparrowColor::EDITOR_DIM)
                        })
                    },
                ),
                label(move || name_clone.clone()).style(move |s| {
                    let c = config_get.get();
                    s.font_size(13.0)
                        .font_weight(if is_current {
                            Weight::BOLD
                        } else {
                            Weight::NORMAL
                        })
                        .color(c.color(SparrowColor::EDITOR_FOREGROUND))
                }),
            ))
            .style(|s| s.items_center()),
        )
        .on_click_stop(move |_| {
            if !is_current {
                cmd.send(SparrowCommand {
                    kind: CommandKind::Workbench(
                        SparrowWorkbenchCommand::CheckoutReference,
                    ),
                    data: Some(serde_json::json!(name)),
                });
            }
            close();
        })
        .style(move |s| {
            let c = config_get.get();
            s.width_full()
                .height(32.0)
                .padding_horiz(10.0)
                .items_center()
                .apply_if(is_current, |s| {
                    s.background(
                        c.color(SparrowColor::SPARROW_BUTTON_PRIMARY_BACKGROUND)
                            .multiply_alpha(0.3),
                    )
                })
                .hover(|s| {
                    s.background(c.color(SparrowColor::PANEL_HOVERED_BACKGROUND))
                        .cursor(CursorStyle::Pointer)
                })
        })
    };

    stack((
        // Background to catch outside clicks
        empty()
            .on_click_stop(move |_| close_menu())
            .style(|s| s.absolute().size_full()),
        // Menu Panel
        container(
            sparrow_scroll(
                stack((
                    // Search bar
                    container(
                        TextInputBuilder::new()
                            .build_editor(search_editor)
                            .placeholder(|| {
                                "Search for branches and actions".to_string()
                            })
                            .style(move |s| {
                                let c = config.get();
                                s.width_full()
                                    .min_height(32.0)
                                    .padding_horiz(8.0) // <-- Fixes text clipping
                                    .border(1.0)
                                    .border_radius(4.0)
                                    .border_color(
                                        c.color(SparrowColor::SPARROW_BORDER),
                                    )
                                    .background(
                                        c.color(SparrowColor::EDITOR_BACKGROUND),
                                    )
                            }),
                    )
                    .style(|s| s.padding(10.0).width_full()),
                    divider(0.0),
                    // Actions
                    stack((
                        simple_menu_item(
                            "Update Project...",
                            move || {
                                sparrow_command.send(SparrowCommand{kind:CommandKind::Workbench(SparrowWorkbenchCommand::GitCommand),data:Some(serde_json::json!(["pull","--ff-only"]))});
                                close_menu();
                            },
                            config,
                        ),
                        simple_menu_item(
                            "Commit...",
                            move || {
                                workbench_command.send(
                                    SparrowWorkbenchCommand::SourceControlCommit,
                                );
                                close_menu();
                            },
                            config,
                        ),
                        simple_menu_item("Push...", move || {sparrow_command.send(SparrowCommand{kind:CommandKind::Workbench(SparrowWorkbenchCommand::GitCommand),data:Some(serde_json::json!(["push"]))});}, config),
                    ))
                    .style(|s| s.flex_col().width_full()),
                    divider(6.0),
                    // Branch Actions
                    simple_menu_item("New Branch...", move || {sparrow_command.send(SparrowCommand{kind:CommandKind::Workbench(SparrowWorkbenchCommand::GitCommand),data:Some(serde_json::json!(["switch","-c","new-branch"]))});close_menu();}, config),
                    simple_menu_item(
                        "Checkout Tag or Revision...",
                        move || {workbench_command.send(SparrowWorkbenchCommand::PaletteSCMReferences);close_menu();},
                        config,
                    ),
                    divider(6.0),
                    // Recent
                    section_label("Recent"),
                    dyn_stack(
                        move || {
                            let query = search_text.get();
                            let current = current_branch.get();
                            if query.is_empty()
                                || current.to_lowercase().contains(&query)
                            {
                                vec![current]
                            } else {
                                vec![]
                            }
                        },
                        |name| name.clone(),
                        move |name| {
                            let is_current = name == current_branch.get_untracked();
                            branch_row(name, is_current)
                        },
                    ),
                    // Local Branches
                    section_label("Local"),
                    dyn_stack(
                        move || {
                            let query = search_text.get();
                            let current = current_branch.get();
                            branches
                                .get()
                                .into_iter()
                                .filter(|b| {
                                    !b.contains('/')
                                        && b.to_lowercase().contains(&query)
                                })
                                .map(|b| {
                                    let is_cur = b == current;
                                    (b, is_cur)
                                })
                                .collect::<Vec<_>>()
                        },
                        |(name, _)| name.clone(),
                        move |(name, is_current)| branch_row(name, is_current),
                    ),
                    // Remote Branches
                    section_label("Remote"),
                    dyn_stack(
                        move || {
                            let query = search_text.get();
                            branches
                                .get()
                                .into_iter()
                                .filter(|b| {
                                    b.contains('/')
                                        && b.to_lowercase().contains(&query)
                                })
                                .map(|b| (b, false))
                                .collect::<Vec<_>>()
                        },
                        |(name, _)| name.clone(),
                        move |(name, is_current)| branch_row(name, is_current),
                    ),
                    // Tags
                    section_label("Tags"),
                    dyn_stack(
                        move || {
                            let query = search_text.get();
                            tags.get()
                                .into_iter()
                                .filter(|t| t.to_lowercase().contains(&query))
                                .map(|t| (t, false))
                                .collect::<Vec<_>>()
                        },
                        |(name, _)| name.clone(),
                        move |(name, is_current)| branch_row(name, is_current),
                    ),
                ))
                .style(|s| s.flex_col().width_full()),
            )
            .style(|s| s.max_height(600.0).width_full()),
        )
        .on_click_stop(|_| {}) // Prevent clicks inside menu from closing it
        .style(move |s| {
            let c = config.get();
            let pt = origin.get() - window_origin.get().to_vec2();
            let th = title_height.get();

            s.absolute()
                .margin_left(pt.x as f32)
                .margin_top(th as f32 + 2.0)
                .width(450.0)
                .padding(0.0)
                .background(c.color(SparrowColor::PANEL_BACKGROUND))
                .border(1.0)
                .border_radius(6.0)
                .border_color(c.color(SparrowColor::SPARROW_BORDER))
                .box_shadow_blur(8.0)
                .box_shadow_color(c.color(SparrowColor::SPARROW_DROPDOWN_SHADOW))
        }),
    ))
    .style(move |s| {
        s.absolute()
            .size_full()
            .display(if visible.get() {
                Display::Flex
            } else {
                Display::None
            })
            .z_index(100)
    })
}

fn simple_menu_item(
    text: &'static str,
    on_click: impl Fn() + 'static,
    config: ReadSignal<Arc<SparrowConfig>>,
) -> impl View {
    label(move || text.to_string())
        .on_click_stop(move |_| on_click())
        .style(move |s| {
            let config = config.get();
            s.width_full()
                .padding_horiz(10.0)
                .padding_vert(6.0)
                .border_radius(4.0)
                .font_size(13.0)
                .hover(|s| {
                    s.background(
                        config.color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                    )
                    .cursor(CursorStyle::Pointer)
                })
        })
}
