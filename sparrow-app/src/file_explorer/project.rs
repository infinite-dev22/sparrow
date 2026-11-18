//! Flutter project roles and resolved Dart libraries; no synthetic dependencies.
use crate::{
    command::InternalCommand,
    config::{color::SparrowColor, icon::SparrowIcons},
    window_tab::CommonData,
};
use floem::{
    View,
    event::{Event, EventListener, EventPropagation},
    ext_event::create_ext_action,
    keyboard::{Key, NamedKey},
    reactive::{
        RwSignal, SignalGet, SignalUpdate, SignalWith, create_memo, create_rw_signal,
    },
    views::{Decorators, dyn_stack, label, stack, svg},
};
use sparrow_rpc::proxy::{PathRequest, ProxyResponse};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
    rc::Rc,
};

#[derive(Clone, Default, PartialEq)]
pub struct ProjectInfo {
    pub flutter: bool,
    pub modules: BTreeMap<PathBuf, String>,
    libraries: Vec<Entry>,
    revision: u64,
    error: Option<String>,
}
#[derive(Clone, PartialEq)]
struct Entry {
    path: PathBuf,
    name: String,
    dir: bool,
    level: usize,
    read_only: bool,
}
fn package_entries(
    config_path: &Path,
    source: &str,
    workspace: &Path,
) -> Vec<Entry> {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(source) else {
        return vec![];
    };
    let Ok(base) = url::Url::from_file_path(config_path) else {
        return vec![];
    };
    let mut entries = Vec::new();
    for package in json["packages"].as_array().into_iter().flatten() {
        let Some(name) = package["name"].as_str() else {
            continue;
        };
        let Some(uri) = package["rootUri"]
            .as_str()
            .and_then(|uri| base.join(uri).ok())
        else {
            continue;
        };
        let Ok(root) = uri.to_file_path() else {
            continue;
        };
        if root == workspace
            || root
                .canonicalize()
                .ok()
                .is_some_and(|p| workspace.canonicalize().ok().as_ref() == Some(&p))
        {
            continue;
        }
        // Library URIs are relative to the package directory, not package_config.json.
        let Ok(root_url) = url::Url::from_directory_path(&root) else {
            continue;
        };
        let Some(path) = root_url
            .join(package["packageUri"].as_str().unwrap_or("lib/"))
            .ok()
            .and_then(|u| u.to_file_path().ok())
        else {
            continue;
        };
        entries.push(Entry {
            read_only: path.components().any(|c| c.as_os_str() == ".pub-cache")
                || std::env::var_os("PUB_CACHE")
                    .is_some_and(|cache| path.starts_with(cache)),
            path,
            name: name.into(),
            dir: true,
            level: 2,
        });
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries
}
#[path = "project_watch.rs"]
mod watch;
pub use watch::LibraryWatch;

pub fn load(common: Rc<CommonData>) -> (RwSignal<ProjectInfo>, watch::ProjectWatch) {
    let info = create_rw_signal(ProjectInfo::default());
    let (tx, rx) = std::sync::mpsc::channel();
    let updates = floem::ext_event::create_signal_from_channel(rx);
    floem::reactive::create_effect(move |_| {
        if let Some(value) = updates.get() {
            info.set(value);
        }
    });
    let local_root = matches!(
        common.workspace.kind,
        crate::workspace::SparrowWorkspaceType::Local
    )
    .then(|| common.workspace.path.clone())
    .flatten();
    let worker = watch::ProjectWatch::start(
        local_root,
        crate::config::SparrowConfig::settings_file(),
        move |value| {
            let _ = tx.send(value);
        },
    );
    (info, worker)
}

pub fn node_label(path: &Path, info: &ProjectInfo) -> String {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    match info.modules.get(path) {
        Some(module) => format!("{name} [{module}]"),
        None => name.into_owned(),
    }
}
pub fn role(path: &Path, root: Option<&Path>) -> Option<&'static str> {
    let relative = path.strip_prefix(root?).ok()?;
    match relative.to_str()? {
        ".dart_tool" | "build" | "target" | ".gradle" | "out" => Some("excluded"),
        "lib" => Some("source"),
        "test" | "integration_test" => Some("test"),
        "android" | "ios" | "macos" | "linux" | "windows" | "web" => Some("platform"),
        _ => None,
    }
}
pub fn external_libraries(
    window: Rc<crate::window_tab::WindowTabData>,
    info: RwSignal<ProjectInfo>,
    library_watch: LibraryWatch,
) -> impl View {
    let common = window.common.clone();
    let config = common.config;
    let line_height = common.ui_line_height;
    let open = create_rw_signal(false);
    let packages_open = create_rw_signal(false);
    let expanded = create_rw_signal(HashSet::<PathBuf>::new());
    let children = create_rw_signal(HashMap::<PathBuf, Vec<Entry>>::new());
    let selected = create_rw_signal(None::<PathBuf>);
    let id = create_rw_signal(None::<floem::ViewId>);
    let errors = create_rw_signal(None::<String>);
    let pub_get=common.workbench_command;
    floem::reactive::create_effect(move |_| {
        library_watch.set_directories(expanded.get().into_iter().collect());
    });
    let entries = create_memo(move |_| {
        fn append(
            entry: Entry,
            expanded: &HashSet<PathBuf>,
            children: &HashMap<PathBuf, Vec<Entry>>,
            out: &mut Vec<Entry>,
        ) {
            let path = entry.path.clone();
            out.push(entry);
            if expanded.contains(&path) {
                if let Some(items) = children.get(&path) {
                    for child in items {
                        append(child.clone(), expanded, children, out)
                    }
                }
            }
        }
        let mut rows = Vec::new();
        if open.get() {
            let info = info.get();
            if packages_open.get() {
                for entry in info.libraries.iter().filter(|e| e.level == 2) {
                    append(
                        entry.clone(),
                        &expanded.get(),
                        &children.get(),
                        &mut rows,
                    );
                }
            }
            for entry in info.libraries.into_iter().filter(|e| e.level == 1) {
                append(entry, &expanded.get(), &children.get(), &mut rows);
            }
        }
        rows
    });
    let refresh_common = common.clone();
    floem::reactive::create_effect(move |_| {
        let value = info.get();
        let roots: Vec<_> = value.libraries.iter().map(|e| e.path.clone()).collect();
        let mut known = value.libraries.clone();
        children.with_untracked(|cache| {
            for rows in cache.values() {
                known.extend(rows.clone());
            }
        });
        expanded.update(|paths| {
            paths.retain(|p| roots.iter().any(|root| p.starts_with(root)))
        });
        if selected.with_untracked(|p| {
            p.as_ref()
                .is_some_and(|p| !roots.iter().any(|root| p.starts_with(root)))
        }) {
            selected.set(None);
        }
        children.set(HashMap::new());
        errors.set(None);
        for entry in known {
            if expanded.with_untracked(|paths| paths.contains(&entry.path)) {
                read_children(&refresh_common, entry, info, children, errors);
            }
        }
    });
    let toggle = {
        let common = common.clone();
        move |entry: Entry| {
            expanded.update(|set| {
                if !set.remove(&entry.path) {
                    set.insert(entry.path.clone());
                }
            });
            if expanded.with_untracked(|set| set.contains(&entry.path))
                && !children.with_untracked(|c| c.contains_key(&entry.path))
            {
                read_children(&common, entry, info, children, errors);
            }
        }
    };
    let click_toggle = toggle.clone();
    let keyboard_toggle = toggle.clone();
    let row_common = common.clone();
    let key_common = common.clone();
    let view = stack((
        stack((
            svg(move || {
                config.get().ui_svg(if open.get() {
                    SparrowIcons::ITEM_OPENED
                } else {
                    SparrowIcons::ITEM_CLOSED
                })
            })
            .style(|s| s.size(14.0, 14.0).margin_right(6.0)),
            svg(move || config.get().file_svg(Path::new("package_config.json")).0)
                .style(move |s| {
                    s.size(16.0, 16.0)
                        .margin_right(6.0)
                        .color(config.get().color(SparrowColor::SPARROW_ICON_ACTIVE))
                }),
            label(|| "External Libraries"),
        ))
        .on_click_stop(move |_| {
            open.update(|v| *v = !*v);
            if let Some(id) = id.get_untracked() {
                id.request_focus();
            }
        })
        .style(move |s| {
            s.height(line_height.get())
                .items_center()
                .padding_left(10.0)
        }),
        stack((
            svg(move || {
                config.get().ui_svg(if packages_open.get() {
                    SparrowIcons::ITEM_OPENED
                } else {
                    SparrowIcons::ITEM_CLOSED
                })
            })
            .style(|s| s.size(14.0, 14.0).margin_right(6.0)),
            svg(move || config.get().ui_svg(SparrowIcons::DART)).style(move |s| {
                s.size(16.0, 16.0)
                    .margin_right(6.0)
                    .color(config.get().color(SparrowColor::SPARROW_ICON_ACTIVE))
            }),
            label(move || {
                format!(
                    "Dart Packages ({})",
                    info.with(|v| v
                        .libraries
                        .iter()
                        .filter(|e| e.level == 2)
                        .count())
                )
            }),
        ))
        .on_click_stop(move |_| {
            packages_open.update(|v| *v = !*v);
            if let Some(id) = id.get_untracked() {
                id.request_focus();
            }
        })
        .style(move |s| {
            s.height(line_height.get())
                .items_center()
                .padding_left(20.0)
                .apply_if(!open.get(), |s| s.hide())
        }),
        dyn_stack(
            move || entries.get(),
            |e| (e.path.clone(), e.name.clone(), e.dir, e.level),
            move |entry| {
                let name = entry.name.clone();
                let path = entry.path.clone();
                let icon_path = path.clone();
                let style_path = path.clone();
                let click_entry = entry.clone();
                let double_entry = entry.clone();
                let common = row_common.clone();
                let double_common = common.clone();
                let toggle = click_toggle.clone();
                stack((
                    svg(move || {
                        config.get().ui_svg(
                            if expanded.with(|v| v.contains(&path)) {
                                SparrowIcons::ITEM_OPENED
                            } else {
                                SparrowIcons::ITEM_CLOSED
                            },
                        )
                    })
                    .style(move |s| {
                        s.size(14.0, 14.0)
                            .margin_right(6.0)
                            .apply_if(!entry.dir, |s| {
                                s.color(floem::peniko::Color::TRANSPARENT)
                            })
                    }),
                    svg(move || {
                        if entry.dir {
                            config.get().folder_svg(&icon_path).0
                        } else {
                            config.get().file_svg(&icon_path).0
                        }
                    })
                    .style(move |s| {
                        s.size(16.0, 16.0).margin_right(6.0).color(
                            config.get().color(SparrowColor::SPARROW_ICON_ACTIVE),
                        )
                    }),
                    label(move || name.clone()),
                ))
                .on_click_stop(move |_| {
                    selected.set(Some(click_entry.path.clone()));
                    if click_entry.dir {
                        toggle(click_entry.clone());
                    }
                    if let Some(id) = id.get_untracked() {
                        id.request_focus();
                    }
                })
                .on_double_click_stop(move |_| {
                    if !double_entry.dir {
                        double_common.internal_command.send(
                            InternalCommand::OpenLibraryFile {
                                path: double_entry.path.clone(),
                                read_only: double_entry.read_only,
                            },
                        );
                    }
                })
                .style(move |s| {
                    s.height(line_height.get())
                        .items_center()
                        .padding_left(10.0 + entry.level as f32 * 10.0)
                        .min_width_full()
                        .border_radius(4.0)
                        .hover(|s| {
                            s.background(
                                config
                                    .get()
                                    .color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                            )
                        })
                        .apply_if(
                            selected.with(|p| p.as_ref() == Some(&style_path)),
                            |s| {
                                s.background(
                                    config.get().color(
                                        SparrowColor::PANEL_CURRENT_BACKGROUND,
                                    ),
                                )
                            },
                        )
                })
            },
        )
        .style(|s| s.flex_col().min_width_full()),
        label(move || {
            errors
                .get()
                .or_else(|| info.get().error)
                .unwrap_or_default()
        })
        .on_click_stop(move |_| {
            if errors.get_untracked().is_none() && info.with_untracked(|v|v.error.is_some()) {
                pub_get.send(crate::command::SparrowWorkbenchCommand::DartPubGet);
            }
        })
        .style(move |s| {
            s.padding_left(30.0)
                .color(config.get().color(SparrowColor::SPARROW_ERROR))
                .apply_if(
                    errors.get().is_none() && info.with(|v| v.error.is_none()),
                    |s| s.hide(),
                )
        }),
    ))
    .style(|s| s.flex_col().min_width_full());
    id.set(Some(view.id()));
    view.keyboard_navigable()
        .on_event(EventListener::KeyDown, move |event| {
            if let Event::KeyDown(key) = event {
                if key.modifiers.is_empty() {
                    let rows = entries.get_untracked();
                    let current = selected
                        .get_untracked()
                        .and_then(|p| rows.iter().position(|r| r.path == p));
                    match key.key.logical_key {
                        Key::Named(NamedKey::ArrowDown | NamedKey::ArrowUp) => {
                            if !rows.is_empty() {
                                let index = if key.key.logical_key
                                    == Key::Named(NamedKey::ArrowDown)
                                {
                                    current
                                        .map(|i| (i + 1).min(rows.len() - 1))
                                        .unwrap_or(0)
                                } else {
                                    current.unwrap_or(0).saturating_sub(1)
                                };
                                selected.set(Some(rows[index].path.clone()));
                            }
                            return EventPropagation::Stop;
                        }
                        Key::Named(
                            NamedKey::Enter
                            | NamedKey::F4
                            | NamedKey::ArrowRight
                            | NamedKey::ArrowLeft,
                        ) => {
                            if let Some(index) = current {
                                let entry = rows[index].clone();
                                if entry.dir {
                                    let is_open = expanded
                                        .with_untracked(|e| e.contains(&entry.path));
                                    if key.key.logical_key
                                        != Key::Named(NamedKey::ArrowRight)
                                        || !is_open
                                    {
                                        if key.key.logical_key
                                            != Key::Named(NamedKey::ArrowLeft)
                                            || is_open
                                        {
                                            keyboard_toggle(entry);
                                        }
                                    }
                                } else if matches!(
                                    key.key.logical_key,
                                    Key::Named(NamedKey::Enter | NamedKey::F4)
                                ) {
                                    key_common.internal_command.send(
                                        InternalCommand::OpenLibraryFile {
                                            path: entry.path,
                                            read_only: entry.read_only,
                                        },
                                    );
                                }
                            }
                            return EventPropagation::Stop;
                        }
                        Key::Named(NamedKey::Escape) => {
                            key_common
                                .focus
                                .set(crate::window_tab::Focus::Workbench);
                            key_common.view_id.get_untracked().request_focus();
                            return EventPropagation::Stop;
                        }
                        _ => {}
                    }
                }
                if window.key_down(key) {
                    return EventPropagation::Stop;
                }
            }
            EventPropagation::Continue
        })
}
fn read_children(
    common: &Rc<CommonData>,
    entry: Entry,
    info: RwSignal<ProjectInfo>,
    children: RwSignal<HashMap<PathBuf, Vec<Entry>>>,
    errors: RwSignal<Option<String>>,
) {
    let generation = info.get_untracked().revision;
    let path = entry.path.clone();
    let level = entry.level + 1;
    let send = create_ext_action(common.scope, move |result| {
        // Ignore replies for the previous resolution, even when the path is reused.
        if info.try_get_untracked().map(|value| value.revision) != Some(generation) {
            return;
        }
        match result {
            Ok(ProxyResponse::ReadDirResponse { mut items, .. }) => {
                items.sort_by(|a, b| {
                    b.is_dir.cmp(&a.is_dir).then_with(|| a.path.cmp(&b.path))
                });
                children.update(|cache| {
                    cache.insert(
                        path,
                        items
                            .into_iter()
                            .map(|node| Entry {
                                name: node
                                    .path
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .into_owned(),
                                path: node.path,
                                dir: node.is_dir,
                                level,
                                read_only: entry.read_only,
                            })
                            .collect(),
                    );
                });
                errors.set(None);
            }
            _ => errors.set(Some(format!("Unable to read {}", path.display()))),
        }
    });
    common.proxy.read_dir(PathRequest::Path(entry.path), send);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cached_dependencies_are_read_only_and_local_packages_remain_editable() {
        let rows = package_entries(
            Path::new("/project/.dart_tool/package_config.json"),
            r#"{"packages":[{"name":"cached","rootUri":"file:///home/user/.pub-cache/hosted/pub.dev/cached-1.0.0"},{"name":"local","rootUri":"file:///project/local"}]}"#,
            Path::new("/project"),
        );
        assert!(rows[0].read_only);
        assert!(!rows[1].read_only);
    }
    #[test]
    fn resolves_relative_package_uris_and_excludes_project() {
        let rows = package_entries(
            Path::new("/project/.dart_tool/package_config.json"),
            r#"{"packages":[{"name":"app","rootUri":"../","packageUri":"lib/"},{"name":"dependency","rootUri":"file:///cache/dep","packageUri":"src/"},{"name":"local","rootUri":"../../local/","packageUri":"lib/"}]}"#,
            Path::new("/project"),
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].path, PathBuf::from("/cache/dep/src/"));
        assert_eq!(rows[1].path, PathBuf::from("/local/lib/"));
    }
    #[test]
    fn recognizes_only_top_level_flutter_roles() {
        assert_eq!(
            role(Path::new("/p/build"), Some(Path::new("/p"))),
            Some("excluded")
        );
        assert_eq!(role(Path::new("/p/lib/build"), Some(Path::new("/p"))), None);
    }
}
