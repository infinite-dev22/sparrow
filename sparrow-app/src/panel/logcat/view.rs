use super::{Pane, State};
use crate::panel::kind::PanelKind;
use crate::widgets::divider::{Divider, DividerVariant};
use crate::{config::icon::SparrowIcons, widgets::clickable_icon};
use floem::{
    IntoView, View,
    reactive::{SignalGet, SignalUpdate, SignalWith},
    style::CursorStyle,
    views::{Decorators, dyn_container, dyn_stack, label, stack},
};

mod history;
mod pane;
mod row;
mod toolbar;
mod variant;

use pane::pane_view;
use variant::variant_bar;

pub(super) fn panel(state: State) -> impl View {
    let config = state.window.common.config;
    let observed = state.clone();
    crate::ui_test_guard::observe_logcat(move || {
        serde_json::json!({
            "active":observed.active.get(),"split":observed.split.get(),
            "run_configuration":observed.window.run_configs.with(|c|c.get(observed.window.active_run_config_idx.get()).map(|c|c.name.clone())),
            "packages":observed.packages.get(),"variant":observed.variant.get(),"variants":observed.model.with(|m|m.variants.clone()),"gradle_syncing":observed.syncing.get(),"gradle_error":observed.gradle_error.get(),
            "panes":observed.panes.with(|panes|panes.iter().map(|p|serde_json::json!({
                "id":p.id,"query":p.query.get(),"serial":p.serial.get(),"paused":p.paused.get(),
                "follow":p.follow.get(),"wrap":p.wrap.get(),"compact":p.compact.get(),
                "records":p.data.with(|b|b.entries.len()),"snapshot_records":p.frozen.with(|r|r.len()),"last_id":p.data.with(|b|b.entries.back().map(|e|e.id)),"snapshot_last_id":p.frozen.with(|r|r.last().map(|e|e.id)),
                "error":p.error.get(),"query_valid":p.parsed.with(|q|q.is_ok()),"visible_count":p.visible_count.get(),"visible_packages":p.visible_packages.get(),
                "file":p.file_path.get(),"loading":p.loading.get(),"format":p.format.get(),"markers":p.data.with(|b|b.entries.iter().filter(|e|e.marker).map(|e|e.message.clone()).collect::<Vec<_>>()),
                "devices":p.catalog.with(|c|c.devices.iter().map(|d|serde_json::json!({"serial":d.serial,"ready":d.ready})).collect::<Vec<_>>())
            })).collect::<Vec<_>>())
        })
    });

    let tabs_state = state.clone();
    let add = state.clone();
    let open_capture = state.clone();
    let cleanup = state.clone();
    let tabs = crate::widgets::tab_strip(|strip| {
        dyn_stack(move || tabs_state.panes.get(), |p| p.id, {
            let state = state.clone();
            move |pane: Pane| {
                let id = pane.id;
                let close = state.clone();
                let active = state.active;
                let content = stack((
                    label(move || {
                        if let Some(path) = pane.file_path.get() {
                            return path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string();
                        }
                        if id == 0 {
                            "Logcat".into()
                        } else {
                            format!("Logcat {}", id + 1)
                        }
                    })
                    .style(|s| s.text_ellipsis().max_width(160.0)),
                    crate::widgets::tab_close_button(
                        move || close.remove(id),
                        "Close Logcat tab",
                        config,
                    ),
                ));
                strip.button(
                    content,
                    move || active.get() == id,
                    move || active.set(id),
                    config,
                )
            }
        })
        .style(|s| s.items_center())
    });
    let header = super::super::chrome::header_with_actions(
        state.window.clone(),
        PanelKind::Logcat,
        "Logcat",
        tabs,
        Some(stack((
            clickable_icon(
                || SparrowIcons::ADD,
                move || add.add(),
                || false,
                || false,
                || "New Logcat tab",
                config,
            ),
            clickable_icon(
                || SparrowIcons::DIRECTORY,
                move || open_capture.show_import.set(true),
                || false,
                || false,
                || "Open capture file…",
                config,
            ),
        ))
        .style(|s| s.items_center().gap(2.0)).into_any()),
    );
    let import_state = state.clone();
    let load = state.clone();
    let browse = state.clone();
    let cancel = state.show_import;
    let import_bar = stack((
        crate::widgets::TextField::new(state.import_path)
            .colors(move || crate::widgets::m3_from_config(config))
            .placeholder("Capture path (.log, .txt, .json or bugreport .zip)")
            .on_submit(move |path| {
                if !path.trim().is_empty() {
                    import_state.import(std::path::PathBuf::from(path.trim()));
                }
            })
            .into_view()
            .style(|s| {
                s.width(floem::unit::PxPctAuto::Auto)
                    .flex_grow(1.0_f32)
                    .flex_basis(0)
                    .min_width(0.0)
            }),
        label(|| "Open")
            .on_click_stop(move |_| {
                let path = load.import_path.get_untracked();
                if !path.trim().is_empty() {
                    load.import(std::path::PathBuf::from(path.trim()));
                }
            })
            .style(|s| s.padding_horiz(8.0).cursor(CursorStyle::Pointer)),
        label(|| "Browse…")
            .on_click_stop(move |_| {
                let state = browse.clone();
                crate::widgets::file_picker::open_file(
                    crate::widgets::file_picker::FileDialogOptions::new()
                        .title("Open Logcat capture"),
                    config,
                    move |file| {
                        if let Some(mut file) = file {
                            if let Some(path) = file.path.pop() {
                                state.import(path);
                            }
                        }
                    },
                );
            })
            .style(|s| s.padding_horiz(8.0).cursor(CursorStyle::Pointer)),
        clickable_icon(
            || SparrowIcons::CLOSE,
            move || cancel.set(false),
            || false,
            || false,
            || "Close capture path",
            config,
        ),
    ))
    .style(move |s| {
        s.width_full()
            .height(38.0)
            .items_center()
            .gap(6.0)
            .padding_horiz(10.0)
            .flex_shrink(0.0_f32)
            .apply_if(!state.show_import.get(), |s| s.hide())
    });
    let current = state.clone();
    let body_state = state.clone();
    let body = dyn_container(
        move || {
            (
                current.active.get(),
                current.split.get(),
                current
                    .panes
                    .with(|p| p.iter().map(|p| p.id).collect::<Vec<_>>()),
            )
        },
        move |(active, split, _)| {
            let panes = body_state.panes.get_untracked();
            let Some(pane) = panes.iter().find(|p| p.id == active).cloned() else {
                return label(|| "Use + to open a Logcat tab.")
                    .style(|s| s.padding(16.0))
                    .into_any();
            };
            if split {
                if let Some(other) = panes.into_iter().find(|p| p.id != active) {
                    return stack((
                        pane_view(body_state.clone(), pane).style(|s| {
                            s.width_pct(50.0).min_width(0.0).height_full()
                        }),
                        Divider::new(config)
                            .variant(DividerVariant::Vertical)
                            .into_view(),
                        pane_view(body_state.clone(), other).style(|s| {
                            s.flex_grow(1.0_f32)
                                .flex_basis(0)
                                .min_width(0.0)
                                .height_full()
                        }),
                    ))
                    .style(|s| s.size_full())
                    .into_any();
                }
            }
            pane_view(body_state.clone(), pane).into_any()
        },
    )
    .style(|s| {
        s.flex_grow(1.0_f32)
            .flex_basis(0)
            .min_height(0.0)
            .width_full()
    });
    let variants = variant_bar(state.clone());
    stack((header, import_bar, variants, body))
        .on_cleanup(move || {
            cleanup
                .sync_cancel
                .store(true, std::sync::atomic::Ordering::Release);
            cleanup.save();
            for pane in cleanup.panes.get_untracked() {
                pane.close();
            }
        })
        .style(|s| s.size_full().flex_col().min_width(0.0))
        .debug_name("Logcat")
}
