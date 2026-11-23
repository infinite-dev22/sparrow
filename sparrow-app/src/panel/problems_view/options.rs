//! Problems panel display, docking, and sizing options.
use super::*;

pub(super) fn panel_options(
    window: &WindowTabData,
    options: Options,
    excluded: RwSignal<Vec<Issue>>,
) -> Menu {
    let inspection = window.inspections;
    let panel = &window.panel;
    let pinned = panel.section_open(PanelSection::ProblemsSidebar);
    let maximize = panel.clone();
    let bottom = panel
        .panel_position(&PanelKind::Problems)
        .is_some_and(|(_, position)| position.is_bottom());
    let mut dock = Menu::new("Move to");
    for (name, position) in [
        ("Left", PanelPosition::LeftTop),
        ("Right", PanelPosition::RightTop),
        ("Bottom", PanelPosition::BottomLeft),
    ] {
        let panel = panel.clone();
        dock = dock.entry(MenuItem::new(name).action(move || {
            panel.move_panel_to_position(PanelKind::Problems, &position)
        }));
    }
    let mut resize = Menu::new("Resize");
    for (name, delta) in [("Increase Size", 48.0), ("Decrease Size", -48.0)] {
        let panel = panel.clone();
        resize = resize.entry(MenuItem::new(name).action(move || {
            if let Some((_, position)) = panel.panel_position(&PanelKind::Problems) {
                panel.size.update(|size| {
                    let extent = if position.is_bottom() {
                        &mut size.bottom
                    } else if matches!(
                        position,
                        PanelPosition::LeftTop | PanelPosition::LeftBottom
                    ) {
                        &mut size.left
                    } else {
                        &mut size.right
                    };
                    let available = panel.available_size.get_untracked();
                    let limit = if position.is_bottom() {
                        available.height
                    } else {
                        available.width
                    };
                    *extent =
                        (*extent + delta).clamp(150.0, (limit * 0.8).max(150.0));
                });
            }
        }));
    }
    let sidebar = panel.clone();
    Menu::new("Problems")
        .entry(toggle_item(
            "Open Files with Single Click",
            options.single_click.get_untracked(),
            move || toggle(options.single_click),
        ))
        .entry(toggle_item(
            "Open Source in Preview Tab",
            options.preview_tab.get_untracked(),
            move || toggle(options.preview_tab),
        ))
        .entry(toggle_item(
            "Editor Preview",
            options.preview.get_untracked(),
            move || toggle(options.preview),
        ))
        .separator()
        .entry(
            MenuItem::new("Close Inspection Results")
                .action(move || inspection.close())
                .enabled(inspection.report.get_untracked() != Report::Closed),
        )
        .entry(
            MenuItem::new("Restore Excluded Findings")
                .action(move || excluded.set(Vec::new()))
                .enabled(excluded.with_untracked(|rows| !rows.is_empty())),
        )
        .entry(
            Menu::new("View Mode").entry(
                MenuItem::new(if panel.panel_bottom_maximized(false) {
                    "Restore"
                } else {
                    "Maximize"
                })
                .action(move || maximize.toggle_maximize(&PanelKind::Problems))
                .enabled(bottom),
            ),
        )
        .entry(dock)
        .entry(resize)
        .separator()
        .entry(
            MenuItem::new(if pinned.get_untracked() {
                "Remove from Sidebar"
            } else {
                "Pin to Sidebar"
            })
            .action(move || {
                if pinned.get_untracked() {
                    pinned.set(false);
                    sidebar.hide_panel(&PanelKind::Problems);
                } else {
                    pinned.set(true);
                }
            }),
        )
}
