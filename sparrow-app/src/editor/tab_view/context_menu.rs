//! Editor tab context menu and tab actions.

use crate::widgets::popup_menu::show_context_menu;
use crate::widgets::popup_menu::{Menu, MenuItem};
use std::path::PathBuf;

use crate::{
    command::{
        CommandKind, InternalCommand, SparrowCommand, SparrowWorkbenchCommand,
    },
    editor_tab::EditorTabChild,
    id::EditorTabId,
    listener::Listener,
    main_split::{SplitDirection, SplitMoveDirection, TabCloseKind},
};

pub(super) fn tab_secondary_click(
    internal_command: Listener<InternalCommand>,
    commands: Listener<SparrowCommand>,
    editor_tab_id: EditorTabId,
    child: EditorTabChild,
    index: usize,
    count: usize,
    path: Option<PathBuf>,
    workspace: Option<PathBuf>,
) {
    let mut menu = Menu::new("");
    let child_unmodified = child.clone();
    let child_other = child.clone();
    let child_right = child.clone();
    let child_left = child.clone();
    menu = menu
        .entry(MenuItem::new("Close").action(move || {
            internal_command.send(InternalCommand::EditorTabChildClose {
                editor_tab_id,
                child: child.clone(),
            });
        }))
        .entry(MenuItem::new("Close Other Tabs").enabled(count > 1).action(
            move || {
                internal_command.send(InternalCommand::EditorTabCloseByKind {
                    editor_tab_id,
                    child: child_other.clone(),
                    kind: TabCloseKind::CloseOther,
                });
            },
        ))
        .entry(MenuItem::new("Close All Tabs").action(move || {
            internal_command.send(InternalCommand::EditorTabClose { editor_tab_id });
        }))
        .entry(
            MenuItem::new("Close Tabs to the Right")
                .enabled(index + 1 < count)
                .action(move || {
                    internal_command.send(InternalCommand::EditorTabCloseByKind {
                        editor_tab_id,
                        child: child_right.clone(),
                        kind: TabCloseKind::CloseToRight,
                    });
                }),
        )
        .entry(
            MenuItem::new("Close Tabs to the Left")
                .enabled(index > 0)
                .action(move || {
                    internal_command.send(InternalCommand::EditorTabCloseByKind {
                        editor_tab_id,
                        child: child_left.clone(),
                        kind: TabCloseKind::CloseToLeft,
                    });
                }),
        );
    menu = menu
        .entry(MenuItem::new("Close Unmodified Tabs").action(move || {
            internal_command.send(InternalCommand::EditorTabCloseByKind {
                editor_tab_id,
                child: child_unmodified.clone(),
                kind: TabCloseKind::CloseUnmodified,
            });
        }))
        .separator();
    if let Some(path) = path {
        let copy_path = path.clone();
        menu = menu
            .entry(MenuItem::new("Copy Path/Reference...").action(move || {
                let path = copy_path.clone();
                let root = workspace.clone();
                floem::action::exec_after(std::time::Duration::ZERO, move |_| {
                    show_context_menu(crate::menus::copy_path_menu(path, root), None)
                });
            }))
            .separator();
        let is_dart = path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("dart"));
        for (label, command) in [
            ("Run", SparrowWorkbenchCommand::RunCurrentFile),
            ("Debug", SparrowWorkbenchCommand::DebugCurrentFile),
        ] {
            if is_dart {
                let target = path.clone();
                let name = target
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                menu = menu.entry(
                    MenuItem::new(format!("{label} '{name}'")).action(move || {
                        commands.send(SparrowCommand {
                            kind: CommandKind::Workbench(command.clone()),
                            data: serde_json::to_value(&target).ok(),
                        })
                    }),
                );
            }
        }
        menu = menu
            .entry(MenuItem::new("Rename File...").action(move || {
                commands.send(SparrowCommand {
                    kind: CommandKind::Workbench(
                        SparrowWorkbenchCommand::RenameActiveFile,
                    ),
                    data: serde_json::to_value(&path).ok(),
                })
            }))
            .separator();
    }
    for (label, direction) in [
        ("Split and Move Right", SplitMoveDirection::Right),
        ("Split and Move Down", SplitMoveDirection::Down),
    ] {
        menu =
            menu.entry(MenuItem::new(label).enabled(count > 1).action(move || {
                internal_command.send(InternalCommand::SplitAndMove {
                    editor_tab_id,
                    index,
                    direction,
                })
            }));
    }
    menu = menu
        .entry(MenuItem::new("Split Right").action(move || {
            internal_command.send(InternalCommand::Split {
                direction: SplitDirection::Vertical,
                editor_tab_id,
            });
        }))
        .entry(MenuItem::new("Split Down").action(move || {
            internal_command.send(InternalCommand::Split {
                direction: SplitDirection::Horizontal,
                editor_tab_id,
            });
        }));
    show_context_menu(menu, None);
}
