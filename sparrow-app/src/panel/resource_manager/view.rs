//! Resource browser, asset previews and editing forms.

use super::{Inventory, Kind, Resource, scan};
use crate::panel::kind::PanelKind;
use crate::widgets::{dropdown_with_label, sparrow_scroll, text_input};
use crate::{
    command::InternalCommand,
    config::{color::SparrowColor, icon::SparrowIcons},
    editor::location::{EditorLocation, EditorPosition},
    widgets::clickable_icon,
    widgets::popup_menu::{Menu, MenuItem, PopupMenuExt},
    window_tab::WindowTabData,
};
use floem::{
    View,
    reactive::{
        Scope, SignalGet, SignalUpdate, SignalWith, create_effect, create_memo,
        create_rw_signal,
    },
    views::{Decorators, dyn_container, dyn_stack, empty, label, stack},
};
use std::{rc::Rc, time::Duration};

mod categories;
mod import_form;
mod rename_form;
mod thumbnail;

use categories::resource_categories;
use import_form::ImportForm;
use rename_form::RenameForm;
use thumbnail::resource_thumbnail;

pub fn panel(window: Rc<WindowTabData>) -> impl View {
    let config = window.common.config;
    let root = window
        .workspace
        .path
        .clone()
        .filter(|_| window.workspace.kind.is_local());
    let inventory = create_rw_signal(Inventory::default());
    let refresh = create_rw_signal(0u64);
    let generation = create_rw_signal(0u64);
    let busy = create_rw_signal(false);
    let query = create_rw_signal(String::new());
    let module = create_rw_signal(String::new());
    let kind = create_rw_signal(Kind::Asset);
    let grid = create_rw_signal(false);
    let size = create_rw_signal(48.0f32);
    let selected = create_rw_signal(None::<Resource>);
    let rename_form = RenameForm::new();
    let import = ImportForm::new();
    let alive = Rc::new(std::cell::Cell::new(true));
    let live = alive.clone();
    let scope = Scope::current();
    let load_root = root.clone();
    let visible = window.panel.clone();
    create_effect(move |_| {
        refresh.get();
        visible.styles.get();
        visible.panels.get();
        if !visible.is_panel_visible(&PanelKind::ResourceManager)
            || busy.get_untracked()
        {
            return;
        }
        busy.set(true);
        generation.update(|n| *n += 1);
        let epoch = generation.get_untracked();
        let root = load_root.clone();
        let live = live.clone();
        let send =
            floem::ext_event::create_ext_action(scope, move |result: Inventory| {
                if !live.get() {
                    return;
                }
                busy.set(false);
                if generation.get_untracked() == epoch {
                    inventory.set(result);
                }
                let live = live.clone();
                floem::action::exec_after(Duration::from_secs(2), move |_| {
                    if live.get() {
                        refresh.update(|n| *n += 1);
                    }
                });
            });
        std::thread::spawn(move || {
            send(match root {
                Some(root) => match scan(&root) {
                    Ok(resources) => Inventory {
                        resources,
                        error: String::new(),
                    },
                    Err(error) => Inventory {
                        error,
                        ..Default::default()
                    },
                },
                None => Inventory {
                    error: "Open a local Flutter project to browse resources."
                        .into(),
                    ..Default::default()
                },
            });
        });
    });
    let modules = create_memo(move |_| {
        inventory.with(|data| {
            data.resources
                .iter()
                .map(|resource| resource.module.clone())
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
        })
    });
    let filtered = create_memo(move |_| {
        inventory.with(|data| {
            data.resources
                .iter()
                .filter(|resource| {
                    resource.kind == kind.get()
                        && (module.with(|m| m.is_empty() || *m == resource.module))
                        && resource
                            .name
                            .to_lowercase()
                            .contains(&query.get().to_lowercase())
                })
                .cloned()
                .collect::<Vec<_>>()
        })
    });
    let internal = window.common.internal_command;
    let file_explorer = window.file_explorer.clone();
    let reference = window.clone();
    let import_root = root.clone();
    stack((
        crate::panel::chrome::header(
            window.clone(),
            PanelKind::ResourceManager,
            "Resource Manager",
            empty(),
        ),
        stack((
            clickable_icon(
                || SparrowIcons::ADD,
                || {},
                || false,
                || false,
                || "Add resource",
                config,
            )
            .ide_popout_menu(move || {
                Menu::new("")
                    .entry(MenuItem::new("Import Flutter asset…").action(
                        move || {
                            import.show(None, "", "assets/");
                        },
                    ))
                    .entry(MenuItem::new("Import Android resource…").action(
                        move || {
                            import.show(
                                None,
                                "",
                                "android/app/src/main/res/drawable/",
                            );
                        },
                    ))
                    .entry(MenuItem::new("New color value…").action(move || {
                        import.show(Some(Kind::Color), "#FFFFFFFF", "accent");
                    }))
                    .entry(MenuItem::new("New string value…").action(move || {
                        import.show(Some(Kind::String), "", "label");
                    }))
            }),
            clickable_icon(
                || SparrowIcons::IMAGE_LOADING,
                move || refresh.update(|n| *n += 1),
                || false,
                move || busy.get(),
                || "Refresh resources",
                config,
            ),
            dyn_container(
                move || {
                    let mut choices = vec![String::new()];
                    choices.extend(modules.get());
                    choices
                },
                move |choices| {
                    dropdown_with_label(
                        module,
                        choices,
                        config,
                        |name| {
                            if name.is_empty() {
                                "All modules".into()
                            } else {
                                name.clone()
                            }
                        },
                        |_| {},
                    )
                    .style(|s| s.max_width(160.0))
                },
            ),
            text_input(query, config)
                .placeholder("Search resources")
                .style(|s| s.flex_grow(1.0_f32).min_width(30.0)),
        ))
        .style(|s| {
            s.width_full()
                .height(34.0)
                .items_center()
                .gap(6.0)
                .padding_horiz(6.0)
        }),
        resource_categories(kind, config),
        label(move || {
            format!(
                "{} ({})",
                module.with(|m| if m.is_empty() {
                    "Project resources".to_string()
                } else {
                    m.clone()
                }),
                filtered.with(|r| r.len())
            )
        })
        .style(move |s| {
            s.width_full()
                .height(38.0)
                .padding_horiz(10.0)
                .items_center()
                .border_bottom(1.0)
                .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
        }),
        label(move || inventory.with(|data| data.error.clone())).style(move |s| {
            s.padding(10.0)
                .width_full()
                .apply_if(inventory.with(|d| d.error.is_empty()), |s| s.hide())
        }),
        label(move || {
            if busy.get() {
                "Loading resources…"
            } else {
                "No resources match this category or search."
            }
        })
        .style(move |s| {
            s.padding(16.0)
                .width_full()
                .apply_if(!filtered.with(|r| r.is_empty()), |s| s.hide())
        }),
        sparrow_scroll(
            dyn_stack(
                move || filtered.get(),
                |r| (r.module.clone(), r.kind, r.name.clone(), r.variants.clone()),
                move |resource| {
                    let chosen = resource.clone();
                    let menu_resource = resource.clone();
                    let explorer = file_explorer.clone();
                    let reference = reference.clone();
                    let info = resource.clone();
                    let check = (
                        resource.module.clone(),
                        resource.kind,
                        resource.name.clone(),
                    );
                    stack((
                        resource_thumbnail(
                            resource.variants.first().cloned(),
                            size,
                            config,
                        ),
                        stack((
                            label(move || info.name.clone())
                                .style(|s| s.text_ellipsis().min_width(0.0)),
                            label(move || {
                                format!(
                                    "{} · {} · {} version{}",
                                    resource.module,
                                    resource.kind.title(),
                                    resource.variants.len(),
                                    if resource.variants.len() == 1 {
                                        ""
                                    } else {
                                        "s"
                                    }
                                )
                            })
                            .style(move |s| {
                                s.font_size(11.0).color(
                                    config.get().color(SparrowColor::EDITOR_DIM),
                                )
                            }),
                        ))
                        .style(|s| s.flex_col().min_width(0.0).flex_grow(1.0_f32)),
                    ))
                    .on_click_stop(move |_| selected.set(Some(chosen.clone())))
                    .on_double_click_stop({
                        let resource = menu_resource.clone();
                        move |_| {
                            if let Some(variant) = resource.variants.first() {
                                internal.send(InternalCommand::JumpToLocation {
                                    location: EditorLocation {
                                        path: variant.path.clone(),
                                        position: Some(EditorPosition::Position(
                                            lsp_types::Position::new(
                                                variant.line,
                                                0,
                                            ),
                                        )),
                                        scroll_offset: None,
                                        ignore_unconfirmed: false,
                                        same_editor_tab: false,
                                    },
                                });
                            }
                        }
                    })
                    .ide_context_menu(move || {
                        let variant = menu_resource.variants.first().cloned();
                        let open = variant.clone();
                        let reveal = variant.clone();
                        let rename = variant.clone();
                        let explorer = explorer.clone();
                        let renamed = explorer.clone();
                        let reference = reference.clone();
                        let rename_window = reference.clone();
                        let rename_target = menu_resource.clone();
                        let usages = reference.clone();
                        let find_name = menu_resource.name.clone();
                        let value = if menu_resource.kind == Kind::Asset {
                            menu_resource.name.clone()
                        } else {
                            format!(
                                "@{}/{}",
                                menu_resource
                                    .kind
                                    .title()
                                    .to_lowercase()
                                    .replace(' ', "")
                                    .trim_end_matches('s'),
                                menu_resource.name
                            )
                        };
                        Menu::new("")
                            .entry(MenuItem::new("Open definition").action(
                                move || {
                                    if let Some(variant) = &open {
                                        internal.send(
                                            InternalCommand::JumpToLocation {
                                                location: EditorLocation {
                                                    path: variant.path.clone(),
                                                    position: Some(
                                                        EditorPosition::Position(
                                                            lsp_types::Position::new(
                                                                variant.line,
                                                                0,
                                                            ),
                                                        ),
                                                    ),
                                                    scroll_offset: None,
                                                    ignore_unconfirmed: false,
                                                    same_editor_tab: false,
                                                },
                                            },
                                        );
                                    }
                                },
                            ))
                            .entry(MenuItem::new("Find usages").action(move || {
                                usages.global_search.set_pattern(find_name.clone());
                                usages.show_panel(PanelKind::Search);
                            }))
                            .entry(
                                MenuItem::new("Copy resource reference")
                                    .action(move || crate::menus::copy_text(&value)),
                            )
                            .entry(MenuItem::new("Reveal in Project").action(
                                move || {
                                    if let Some(variant) = &reveal {
                                        reference
                                            .show_panel(PanelKind::FileExplorer);
                                        explorer.reveal_in_file_tree(
                                            variant.path.clone(),
                                        );
                                    }
                                },
                            ))
                            .entry(
                                MenuItem::new(
                                    if menu_resource.module.starts_with("android/") {
                                        "Rename resource…"
                                    } else {
                                        "Rename file…"
                                    },
                                )
                                .action(move || {
                                    if rename_target.module.starts_with("android/") {
                                        rename_form.show(rename_target.clone());
                                        return;
                                    }
                                    if let Some(variant) = &rename {
                                        rename_window
                                            .show_panel(PanelKind::FileExplorer);
                                        renamed.rename_path(variant.path.clone());
                                    }
                                }),
                            )
                    })
                    .style(move |s| {
                        s.padding(10.0)
                            .gap(10.0)
                            .items_center()
                            .border_bottom(1.0)
                            .border_color(
                                config.get().color(SparrowColor::SPARROW_BORDER),
                            )
                            .apply_if(grid.get(), |s| s.width(170.0).flex_col())
                            .apply_if(!grid.get(), |s| s.width_full())
                            .apply_if(
                                selected.with(|r| {
                                    r.as_ref().is_some_and(|r| {
                                        (r.module.clone(), r.kind, r.name.clone())
                                            == check
                                    })
                                }),
                                |s| {
                                    s.background(config.get().color(
                                        SparrowColor::PANEL_HOVERED_BACKGROUND,
                                    ))
                                },
                            )
                    })
                },
            )
            .style(move |s| {
                s.width_full()
                    .flex_wrap(floem::taffy::FlexWrap::Wrap)
                    .apply_if(!grid.get(), |s| s.flex_col())
            }),
        )
        .style(|s| s.flex_grow(1.0_f32).min_height(0.0).width_full()),
        dyn_stack(
            move || {
                selected
                    .get()
                    .map(|resource| resource.variants)
                    .unwrap_or_default()
            },
            |v| (v.path.clone(), v.line),
            move |variant| {
                let path = variant.path.clone();
                label(move || {
                    format!(
                        "{}  {}",
                        variant.qualifier,
                        variant
                            .path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                    )
                })
                .on_click_stop(move |_| {
                    internal.send(InternalCommand::JumpToLocation {
                        location: EditorLocation {
                            path: path.clone(),
                            position: Some(EditorPosition::Position(
                                lsp_types::Position::new(variant.line, 0),
                            )),
                            scroll_offset: None,
                            ignore_unconfirmed: false,
                            same_editor_tab: false,
                        },
                    })
                })
                .style(move |s| {
                    s.height(24.0)
                        .width_full()
                        .padding_horiz(12.0)
                        .items_center()
                        .color(config.get().color(SparrowColor::EDITOR_LINK))
                })
            },
        )
        .style(|s| s.flex_col().width_full().max_height(120.0)),
        stack((
            crate::widgets::tab_button(
                label(|| "List"),
                move || !grid.get(),
                move || grid.set(false),
                config,
            ),
            crate::widgets::tab_button(
                label(|| "Grid"),
                move || grid.get(),
                move || grid.set(true),
                config,
            ),
            empty().style(|s| s.flex_grow(1.0_f32)),
            crate::widgets::button(
                || "−",
                crate::widgets::ButtonKind::Outlined,
                config,
            )
            .on_click_stop(move |_| size.update(|v| *v = (*v - 8.0).max(24.0)))
            .disabled(move || size.get() <= 24.0)
            .style(|s| s.size(24.0, 24.0)),
            crate::widgets::button(
                || "+",
                crate::widgets::ButtonKind::Outlined,
                config,
            )
            .on_click_stop(move |_| size.update(|v| *v = (*v + 8.0).min(128.0)))
            .disabled(move || size.get() >= 128.0)
            .style(|s| s.size(24.0, 24.0)),
        ))
        .style(move |s| {
            s.width_full()
                .height(30.0)
                .items_center()
                .gap(4.0)
                .padding_horiz(8.0)
                .border_top(1.0)
                .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
        }),
        rename_form.view(window.clone(), selected, refresh, scope),
        import.view(window, import_root, module, refresh, scope),
    ))
    .on_cleanup(move || alive.set(false))
    .style(|s| s.absolute().size_full().flex_col().font_size(13.0))
}
