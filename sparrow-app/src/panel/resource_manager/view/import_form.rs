//! Resource imports and new Android resource values.

use crate::panel::resource_manager::{
    AssetImportPlan, Kind, apply_asset_import, create_value,
    preview_asset_import_batch,
};
use crate::{
    config::color::SparrowColor,
    widgets::{
        ButtonKind,
        file_picker::{FileDialogOptions, open_file},
        sparrow_scroll, text_input,
    },
    window_tab::WindowTabData,
};
use floem::{
    View,
    reactive::{
        RwSignal, Scope, SignalGet, SignalUpdate, SignalWith, create_effect,
        create_rw_signal,
    },
    views::{Decorators, label, stack},
};
use std::{
    path::{Path, PathBuf},
    rc::Rc,
};

#[derive(Clone, Copy)]
pub(super) struct ImportForm {
    import_open: RwSignal<bool>,
    create_kind: RwSignal<Option<Kind>>,
    source: RwSignal<String>,
    selected_sources: RwSignal<Vec<PathBuf>>,
    destination: RwSignal<String>,
    error: RwSignal<String>,
    preview: RwSignal<String>,
    pending_plan: RwSignal<Option<AssetImportPlan>>,
    importing: RwSignal<bool>,
}

impl ImportForm {
    pub(super) fn new() -> Self {
        Self {
            import_open: create_rw_signal(false),
            create_kind: create_rw_signal(None),
            source: create_rw_signal(String::new()),
            selected_sources: create_rw_signal(Vec::new()),
            destination: create_rw_signal(String::from("assets/")),
            error: create_rw_signal(String::new()),
            preview: create_rw_signal(String::new()),
            pending_plan: create_rw_signal(None),
            importing: create_rw_signal(false),
        }
    }

    pub(super) fn show(self, kind: Option<Kind>, source: &str, destination: &str) {
        self.create_kind.set(kind);
        self.source.set(source.into());
        self.selected_sources.set(Vec::new());
        self.destination.set(destination.into());
        self.error.set(String::new());
        self.preview.set(String::new());
        self.pending_plan.set(None);
        self.import_open.set(true);
    }

    pub(super) fn view(
        self,
        window: Rc<WindowTabData>,
        import_root: Option<PathBuf>,
        module: RwSignal<String>,
        refresh: RwSignal<u64>,
        scope: Scope,
    ) -> impl View {
        let Self {
            import_open,
            create_kind,
            source,
            selected_sources,
            destination,
            error,
            preview,
            pending_plan,
            importing,
        } = self;
        let config = window.common.config;
        create_effect(move |_| {
            source.get();
            destination.get();
            preview.set(String::new());
            pending_plan.set(None);
        });
        let browse_source = source;
        let browse_preview = preview;
        let browse = crate::widgets::compact_button(
            || "Browse…",
            ButtonKind::Outlined,
            config,
        )
        .on_click_stop(move |_| {
            let source = browse_source;
            let selected_sources = selected_sources;
            let preview = browse_preview;
            let pending_plan = pending_plan;
            open_file(
                FileDialogOptions::new()
                    .title("Choose resource files")
                    .multi_selection(),
                config,
                move |info| {
                    if let Some(paths) = info.map(|info| info.path().clone())
                        && !paths.is_empty()
                    {
                        let source_label = if paths.len() == 1 {
                            paths[0].to_string_lossy().into_owned()
                        } else {
                            format!("{} files selected", paths.len())
                        };
                        selected_sources.set(paths);
                        source.set(source_label);
                        preview.set(String::new());
                        pending_plan.set(None);
                    }
                },
            );
        })
        .disabled(move || importing.get())
        .style(move |s| s.apply_if(create_kind.get().is_some(), |s| s.hide()));
        stack((
            label(move ||match create_kind.get(){Some(Kind::Color)=>"New color resource",Some(Kind::String)=>"New string resource",_=>"Import resource"}).style(|s| s.font_bold()),
            stack((
                text_input(source, config)
                    .placeholder("Source file path / resource value; Browse supports multiple files")
                    .disabled(move || importing.get())
                    .style(|s| s.flex_grow(1.0_f32).min_width(0.0)),
                browse,
            ))
            .style(|s| s.width_full().items_center().gap(6.0)),
            text_input(destination, config)
                .placeholder("Destination path / resource name")
                .disabled(move || importing.get()),
            sparrow_scroll(label(move || preview.get()))
                .style(move |s| s.width_full().max_height(180.0).apply_if(preview.get().is_empty(), |s| s.hide())),
            label(move || error.get()),
            crate::widgets::action_group((
                crate::widgets::compact_button(|| "Cancel", crate::widgets::ButtonKind::Outlined, config)
                    .disabled(move || importing.get())
                    .on_click_stop(move |_| import_open.set(false)),
                crate::widgets::compact_button(move ||if create_kind.get().is_some(){"Create"}else if pending_plan.get().is_some(){"Apply import"}else{"Preview changes"}, crate::widgets::ButtonKind::Primary, config)
                    .disabled(move || importing.get())
                    .on_click_stop(move |_| {
                    let Some(root) = import_root.clone() else {
                        return;
                    };
                    let dirty = window.main_split.docs.with_untracked(|docs| {
                        docs.get(&root.join("pubspec.yaml"))
                            .is_some_and(|doc| !doc.is_pristine())
                    });
                    if dirty {
                        error.set(
                            "Save pubspec.yaml before importing an asset.".into(),
                        );
                        return;
                    }
                    if let Some(kind)=create_kind.get_untracked() {
                        let module=module.get_untracked();let module=if module.starts_with("android/"){module}else{"android/app".into()};
                        let target=root.join(module).join("src/main/res/values").join(if kind==Kind::Color{"colors.xml"}else{"strings.xml"});
                        if window.main_split.docs.with_untracked(|docs|docs.get(&target).is_some_and(|doc|!doc.is_pristine())){error.set("Save the resource definition before adding another value.".into());return;}
                    }
                    if importing.get_untracked() {
                        return;
                    }
                    let source_signal = source;
                    let destination_signal = destination;
                    let source_value = source_signal.get_untracked();
                    let destination_value = destination_signal.get_untracked();
                    let selected_source_paths = selected_sources.get_untracked();
                    let source_paths = if !selected_source_paths.is_empty()
                        && (source_value == format!("{} files selected", selected_source_paths.len())
                            || (selected_source_paths.len() == 1
                                && source_value
                                    == selected_source_paths[0].to_string_lossy().as_ref()))
                    {
                        selected_source_paths.clone()
                    } else {
                        source_value
                            .lines()
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .map(PathBuf::from)
                            .collect()
                    };
                    let create=create_kind.get_untracked();let selected_module=module.get_untracked();
                    let selected_plan = pending_plan.get_untracked();
                    if create.is_none() && selected_plan.is_none() {
                        importing.set(true);
                        error.set("Preparing import preview…".into());
                        let requested_source = source_value.clone();
                        let requested_destination = destination_value.clone();
                        let requested_sources = selected_source_paths.clone();
                        let root = root.clone();
                        let current_source = source_signal;
                        let current_destination = destination_signal;
                        let current_sources = selected_sources;
                        let send = floem::ext_event::create_ext_action(
                            scope,
                            move |result: Result<AssetImportPlan, String>| {
                                importing.set(false);
                                if current_source.get_untracked() != requested_source
                                    || current_destination.get_untracked()
                                        != requested_destination
                                    || current_sources.get_untracked() != requested_sources
                                {
                                    return;
                                }
                                match result {
                                    Ok(details) => {
                                        preview.set(details.details().to_string());
                                        pending_plan.set(Some(details));
                                        error.set("Review the proposed changes, then apply the import.".into());
                                    }
                                    Err(message) => error.set(message),
                                }
                            },
                        );
                        std::thread::spawn(move || {
                            send(preview_asset_import_batch(
                                &root,
                                &source_paths,
                                Path::new(&destination_value),
                            ));
                        });
                        return;
                    }
                    importing.set(true);
                    error.set("Importing…".into());
                    let send = floem::ext_event::create_ext_action(
                        scope,
                        move |result: Result<(), String>| {
                            importing.set(false);
                            match result {
                                Ok(()) => {
                                    import_open.set(false);
                                    error.set(String::new());
                                    preview.set(String::new());
                                    pending_plan.set(None);
                                    refresh.update(|n| *n += 1);
                                }
                                Err(message) => error.set(message),
                            }
                        },
                    );
                    std::thread::spawn(move || {
                        send(if let Some(plan) = selected_plan {
                            apply_asset_import(plan)
                        } else if let Some(kind) = create {
                            create_value(
                                &root,
                                &selected_module,
                                kind,
                                &destination_value,
                                &source_value,
                            )
                        } else {
                            Err("Preview the import before applying it.".into())
                        })
                    });
                }),
            )),
        ))
        .style(move |s| {
            s.absolute()
                .inset_left(12.0)
                .inset_right(12.0)
                .inset_top(100.0)
                .padding(16.0)
                .gap(12.0)
                .flex_col()
                .border(1.0)
                .border_radius(5.0)
                .background(config.get().color(SparrowColor::PANEL_BACKGROUND))
                .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
                .z_index(50)
                .apply_if(!import_open.get(), |s| s.hide())
        })
    }
}
