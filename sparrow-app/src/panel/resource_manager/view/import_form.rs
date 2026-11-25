//! Resource imports and new Android resource values.

use crate::panel::resource_manager::{
    AssetImportConflictPolicy, AssetImportPlan, Kind, apply_asset_import,
    create_value, preview_asset_import_batch_with_policy,
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
    event::{Event, EventListener, EventPropagation},
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum PubGetState {
    Hidden,
    Offer,
    Running,
    Complete,
    Failed,
}

#[derive(Clone, Copy)]
pub(super) struct ImportForm {
    import_open: RwSignal<bool>,
    create_kind: RwSignal<Option<Kind>>,
    source: RwSignal<String>,
    selected_sources: RwSignal<Vec<PathBuf>>,
    destination: RwSignal<String>,
    conflict_policy: RwSignal<AssetImportConflictPolicy>,
    error: RwSignal<String>,
    preview: RwSignal<String>,
    pending_plan: RwSignal<Option<AssetImportPlan>>,
    importing: RwSignal<bool>,
    pub_get_state: RwSignal<PubGetState>,
}

fn source_selection_label(paths: &[PathBuf]) -> String {
    match paths {
        [path] => path.to_string_lossy().into_owned(),
        [] => String::new(),
        paths => format!("{} files selected", paths.len()),
    }
}

fn source_selection_matches(label: &str, paths: &[PathBuf]) -> bool {
    match paths {
        [path] => label == path.to_string_lossy(),
        [] => false,
        paths => label == format!("{} files selected", paths.len()),
    }
}

fn source_paths_from_input(source: &str, selected: &[PathBuf]) -> Vec<PathBuf> {
    if source_selection_matches(source, selected) {
        selected.to_vec()
    } else {
        source
            .lines()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .collect()
    }
}

fn append_dropped_source(
    paths: &mut Vec<PathBuf>,
    path: PathBuf,
) -> Result<bool, String> {
    if !path.is_file() {
        return Err(format!("Drop a file, not a folder: {}", path.display()));
    }
    if paths.contains(&path) {
        return Ok(false);
    }
    paths.push(path);
    Ok(true)
}

impl ImportForm {
    pub(super) fn new() -> Self {
        Self {
            import_open: create_rw_signal(false),
            create_kind: create_rw_signal(None),
            source: create_rw_signal(String::new()),
            selected_sources: create_rw_signal(Vec::new()),
            destination: create_rw_signal(String::from("assets/")),
            conflict_policy: create_rw_signal(AssetImportConflictPolicy::Skip),
            error: create_rw_signal(String::new()),
            preview: create_rw_signal(String::new()),
            pending_plan: create_rw_signal(None),
            importing: create_rw_signal(false),
            pub_get_state: create_rw_signal(PubGetState::Hidden),
        }
    }

    pub(super) fn show(self, kind: Option<Kind>, source: &str, destination: &str) {
        self.create_kind.set(kind);
        self.source.set(source.into());
        self.selected_sources.set(Vec::new());
        self.destination.set(destination.into());
        self.conflict_policy.set(AssetImportConflictPolicy::Skip);
        self.error.set(String::new());
        self.preview.set(String::new());
        self.pending_plan.set(None);
        self.pub_get_state.set(PubGetState::Hidden);
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
            conflict_policy,
            error,
            preview,
            pending_plan,
            importing,
            pub_get_state,
        } = self;
        let config = window.common.config;
        create_effect(move |_| {
            source.get();
            destination.get();
            conflict_policy.get();
            preview.set(String::new());
            pending_plan.set(None);
        });
        let pub_get_root = import_root.clone();
        let start_pub_get = move || {
            let Some(root) = pub_get_root.clone() else {
                error.set(
                    "Open a local Flutter project to resolve its packages.".into(),
                );
                pub_get_state.set(PubGetState::Failed);
                return;
            };
            pub_get_state.set(PubGetState::Running);
            error.set("Running flutter pub get…".into());
            let send =
                floem::ext_event::create_ext_action(
                    scope,
                    move |result| match result {
                        Ok(()) => {
                            error.set(
                                "flutter pub get completed successfully.".into(),
                            );
                            pub_get_state.set(PubGetState::Complete);
                        }
                        Err(message) => {
                            error.set(message);
                            pub_get_state.set(PubGetState::Failed);
                        }
                    },
                );
            crate::dart::run_pub_get(
                &root,
                Some(Box::new(move |result| send(result))),
            );
        };
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
                        let source_label = source_selection_label(&paths);
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
                label(|| "Drop one or more files anywhere in this dialog, or use Browse…")
                    .style(move |s| s.apply_if(create_kind.get().is_some(), |s| s.hide())),
                text_input(destination, config)
                .placeholder("Destination path / resource name")
                .disabled(move || importing.get()),
            stack((
                label(|| "If a name already exists:"),
                crate::widgets::radio_group(
                    conflict_policy,
                    [
                        AssetImportConflictPolicy::Skip,
                        AssetImportConflictPolicy::KeepBoth,
                    ],
                    config,
                ),
                label(|| "Keep both adds _2, _3, and so on before the extension; files are never replaced."),
            ))
            .style(move |s| {
                s.flex_col()
                    .gap(4.0)
                    .apply_if(create_kind.get().is_some(), |s| s.hide())
            }),
            sparrow_scroll(label(move || preview.get()))
                .style(move |s| s.width_full().max_height(180.0).apply_if(preview.get().is_empty(), |s| s.hide())),
            label(move || error.get()),
            crate::widgets::action_group((
                crate::widgets::compact_button(
                    move || match pub_get_state.get() {
                        PubGetState::Offer | PubGetState::Failed => "Not now",
                        PubGetState::Complete => "Close",
                        _ => "Cancel",
                    },
                    crate::widgets::ButtonKind::Outlined,
                    config,
                )
                    .disabled(move || importing.get() || pub_get_state.get() == PubGetState::Running)
                    .on_click_stop(move |_| {
                        pub_get_state.set(PubGetState::Hidden);
                        import_open.set(false);
                    }),
                crate::widgets::compact_button(
                    move || match pub_get_state.get() {
                        PubGetState::Offer => "Run flutter pub get",
                        PubGetState::Running => "Resolving dependencies…",
                        PubGetState::Complete => "Close",
                        PubGetState::Failed => "Retry flutter pub get",
                        PubGetState::Hidden => {
                            if create_kind.get().is_some() {
                                "Create"
                            } else if pending_plan.get().is_some() {
                                "Apply import"
                            } else {
                                "Preview changes"
                            }
                        }
                    },
                    crate::widgets::ButtonKind::Primary,
                    config,
                )
                    .disabled(move || importing.get() || pub_get_state.get() == PubGetState::Running)
                    .on_click_stop(move |_| {
                    match pub_get_state.get_untracked() {
                        PubGetState::Offer | PubGetState::Failed => {
                            start_pub_get();
                            return;
                        }
                        PubGetState::Running => return,
                        PubGetState::Complete => {
                            pub_get_state.set(PubGetState::Hidden);
                            import_open.set(false);
                            return;
                        }
                        PubGetState::Hidden => {}
                    }
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
                    let source_paths =
                        source_paths_from_input(&source_value, &selected_source_paths);
                    let create=create_kind.get_untracked();let selected_module=module.get_untracked();
                    let selected_plan = pending_plan.get_untracked();
                    if create.is_none() && selected_plan.is_none() {
                        importing.set(true);
                        error.set("Preparing import preview…".into());
                        let requested_source = source_value.clone();
                        let requested_destination = destination_value.clone();
                        let requested_sources = selected_source_paths.clone();
                        let requested_policy = conflict_policy.get_untracked();
                        let root = root.clone();
                        let current_source = source_signal;
                        let current_destination = destination_signal;
                        let current_sources = selected_sources;
                        let current_policy = conflict_policy;
                        let send = floem::ext_event::create_ext_action(
                            scope,
                            move |result: Result<AssetImportPlan, String>| {
                                importing.set(false);
                                if current_source.get_untracked() != requested_source
                                    || current_destination.get_untracked()
                                        != requested_destination
                                    || current_sources.get_untracked() != requested_sources
                                    || current_policy.get_untracked() != requested_policy
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
                            send(preview_asset_import_batch_with_policy(
                                &root,
                                &source_paths,
                                Path::new(&destination_value),
                                requested_policy,
                            ));
                        });
                        return;
                    }
                    importing.set(true);
                    error.set("Importing…".into());
                    let should_offer_pub_get = selected_plan
                        .as_ref()
                        .is_some_and(AssetImportPlan::changes_pubspec);
                    let send = floem::ext_event::create_ext_action(
                        scope,
                        move |result: Result<(), String>| {
                            importing.set(false);
                            match result {
                                Ok(()) => {
                                    if should_offer_pub_get {
                                        pub_get_state.set(PubGetState::Offer);
                                        error.set(
                                            "Import complete. Run flutter pub get to refresh package resolution, or choose Not now.".into(),
                                        );
                                    } else {
                                        import_open.set(false);
                                        error.set(String::new());
                                    }
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
        .on_event(EventListener::DroppedFile, move |event| {
            if !import_open.get_untracked() {
                return EventPropagation::Continue;
            }
            let Event::DroppedFile(file) = event else {
                return EventPropagation::Continue;
            };
            if create_kind.get_untracked().is_some() {
                error.set("File drops are available while importing resources.".into());
            } else if importing.get_untracked() {
                error.set("Wait for the current import operation to finish before adding files.".into());
            } else {
                let mut paths = source_paths_from_input(
                    &source.get_untracked(),
                    &selected_sources.get_untracked(),
                );
                match append_dropped_source(&mut paths, file.path.clone()) {
                    Ok(added) => {
                        source.set(source_selection_label(&paths));
                        selected_sources.set(paths);
                        preview.set(String::new());
                        pending_plan.set(None);
                        error.set(if added {
                            "File added. Preview the changes before applying the import.".into()
                        } else {
                            "That file is already selected.".into()
                        });
                    }
                    Err(message) => error.set(message),
                }
            }
            EventPropagation::Stop
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropped_sources_are_deduplicated_and_folders_are_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("image.png");
        std::fs::write(&file, b"image").unwrap();
        let mut sources = Vec::new();

        assert_eq!(append_dropped_source(&mut sources, file.clone()), Ok(true));
        assert_eq!(append_dropped_source(&mut sources, file.clone()), Ok(false));
        assert_eq!(sources, vec![file]);
        assert!(
            append_dropped_source(&mut sources, directory.path().to_path_buf())
                .is_err()
        );
    }

    #[test]
    fn source_selection_labels_round_trip_without_losing_manual_paths() {
        let first = PathBuf::from("/tmp/first.png");
        let second = PathBuf::from("/tmp/second.json");
        let paths = vec![first.clone(), second.clone()];

        let label = source_selection_label(&paths);
        assert_eq!(label, "2 files selected");
        assert!(source_selection_matches(&label, &paths));
        assert_eq!(source_paths_from_input(&label, &paths), paths);
        assert_eq!(
            source_selection_label(std::slice::from_ref(&first)),
            first.to_string_lossy()
        );
        assert_eq!(
            source_paths_from_input("/tmp/manual.png\n/tmp/other.json", &[]),
            vec![
                PathBuf::from("/tmp/manual.png"),
                PathBuf::from("/tmp/other.json")
            ]
        );
    }
}
