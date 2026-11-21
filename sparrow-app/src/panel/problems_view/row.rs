//! Problem rows and their context actions.
use super::*;

pub(super) fn row(
    window: Rc<WindowTabData>,
    issue: Issue,
    selection: RwSignal<Option<Issue>>,
    excluded: RwSignal<Vec<Issue>>,
    options: Options,
    view_id: RwSignal<Option<floem::ViewId>>,
) -> impl View {
    let config = window.common.config;
    let tab = window.inspections.tab;
    let line = issue.diagnostic.range.start.line + 1;
    let file = issue
        .path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let rank = rank(&issue.diagnostic);
    let summary = issue.summary().to_owned();
    let description = issue.diagnostic.message.clone();
    let chosen = issue.clone();
    let click_issue = issue.clone();
    let double_issue = issue.clone();
    let click_window = window.clone();
    let dbl_window = window.clone();
    let focus_row = move || {
        if let Some(id) = view_id.get_untracked() {
            id.request_focus();
        }
    };
    let view = stack((
        svg(move || {
            config.get().ui_svg(match rank {
                0 => SparrowIcons::ERROR,
                1 => SparrowIcons::WARNING,
                _ => SparrowIcons::LIGHTBULB,
            })
        })
        .style(move |s| {
            s.size(14.0, 14.0)
                .color(config.get().color(match rank {
                    0 => SparrowColor::SPARROW_ERROR,
                    1 => SparrowColor::SPARROW_WARN,
                    _ => SparrowColor::PANEL_FOREGROUND_DIM,
                }))
                .flex_shrink(0.0_f32)
        }),
        label(move || summary.clone()).style(|s| s.min_width(0.0).text_ellipsis()),
        label(move || {
            if tab.get() == Tab::Inspection {
                format!("{file}:{line}")
            } else {
                format!(":{line}")
            }
        })
        .style(move |s| {
            s.color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
                .flex_shrink(0.0_f32)
        }),
    ))
    .style(move |s| {
        s.height(23.0)
            .width_full()
            .min_width(0.0)
            .padding_left(27.0)
            .padding_right(7.0)
            .items_center()
            .gap(5.0)
            .border_radius(4.0)
            .cursor(CursorStyle::Pointer)
            .hover(|s| {
                s.background(
                    config.get().color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                )
            })
            .apply_if(selection.with(|item| item.as_ref() == Some(&chosen)), |s| {
                s.background(
                    config.get().color(SparrowColor::PANEL_SELECTION_BACKGROUND),
                )
            })
    })
    .on_click_stop(move |_| {
        selection.set(Some(click_issue.clone()));
        if options.single_click.get_untracked()
            && tab.get_untracked() != Tab::Inspection
            && !options.preview.get_untracked()
        {
            jump(
                &click_window,
                &click_issue,
                options.preview_tab.get_untracked(),
            );
        } else {
            click_window
                .common
                .focus
                .set(Focus::Panel(PanelKind::Problems));
            focus_row();
        }
    })
    .on_double_click_stop(move |_| {
        jump(
            &dbl_window,
            &double_issue,
            options.preview_tab.get_untracked(),
        )
    })
    .on_secondary_click_stop(move |_| {
        selection.set(Some(issue.clone()));
        window.common.focus.set(Focus::Panel(PanelKind::Problems));
        focus_row();
        let open = window.clone();
        let fixes = window.clone();
        let copy = issue.diagnostic.message.clone();
        let jump_issue = issue.clone();
        let fix_issue = issue.clone();
        let mut menu = Menu::new("Problem")
            .entry(MenuItem::new("Jump to Source").action(move || {
                jump(&open, &jump_issue, options.preview_tab.get_untracked())
            }))
            .entry(MenuItem::new("Show Quick Fixes…").action(move || {
                details::show_actions(
                    fixes.clone(),
                    fix_issue.clone(),
                    selection,
                    false,
                )
            }))
            .entry(
                MenuItem::new("Copy Problem Description")
                    .action(move || crate::menus::copy_text(&copy)),
            );
        if tab.get_untracked() == Tab::Inspection {
            let suppress = window.clone();
            let suppress_issue = issue.clone();
            let usages = window.clone();
            let usage_issue = issue.clone();
            let exclude_issue = issue.clone();
            menu = menu
                .separator()
                .entry(MenuItem::new("Find Usages").action(move || {
                    jump(&usages, &usage_issue, options.preview_tab.get_untracked());
                    if let Some(editor) =
                        usages.main_split.active_editor.get_untracked()
                    {
                        let scope = usages.scope.create_child();
                        let doc = editor.doc();
                        let window = (*usages).clone();
                        scope.create_effect(move |_| {
                            if doc.loaded.get() {
                                editor.find_reference(window.clone());
                                scope.dispose();
                            }
                        });
                    }
                }))
                .entry(MenuItem::new("Suppress…").action(move || {
                    details::show_actions(
                        suppress.clone(),
                        suppress_issue.clone(),
                        selection,
                        true,
                    )
                }))
                .entry(MenuItem::new("Exclude from Results").action(move || {
                    excluded.update(|issues| issues.push(exclude_issue.clone()))
                }));
        }
        if tab.get_untracked() == Tab::Inspection && window.workspace.kind.is_local()
        {
            if let Some(path) = window
                .workspace
                .path
                .as_ref()
                .map(|root| root.join("analysis_options.yaml"))
                .filter(|path| path.is_file())
            {
                let window = window.clone();
                menu = menu.separator().entry(
                    MenuItem::new("Open Analysis Options").action(move || {
                        window.main_split.jump_to_location(
                            EditorLocation {
                                path: path.clone(),
                                position: None,
                                scroll_offset: None,
                                ignore_unconfirmed: false,
                                same_editor_tab: false,
                            },
                            None,
                        );
                    }),
                );
            }
        }
        show_context_menu(menu, None);
    });
    crate::widgets::tooltip_label(config, view, move || description.clone())
}
