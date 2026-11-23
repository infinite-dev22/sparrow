//! Directory scanning and chooser tree interaction.
use std::{
    collections::{HashSet, VecDeque},
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use floem::{
    View,
    event::{Event, EventListener, EventPropagation},
    keyboard::{Key, Modifiers, NamedKey},
    reactive::{
        Scope, SignalGet, SignalUpdate, SignalWith, create_effect, create_memo,
        create_rw_signal,
    },
    style::TextColor,
    views::{Decorators, label, stack, svg, virtual_stack},
};

use super::{Browser, Complete, Config, FileDialogOptions};
use crate::{config::color::SparrowColor, widgets::sparrow_scroll};

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub(super) struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub directory: bool,
    pub expanded: bool,
    pub depth: usize,
}

pub(super) fn scan(
    directory: &Path,
    expanded: &HashSet<PathBuf>,
    hidden: bool,
    limit: usize,
    visible: &impl Fn(&Path, bool) -> bool,
) -> std::io::Result<Vec<Entry>> {
    fn walk(
        path: &Path,
        depth: usize,
        expanded: &HashSet<PathBuf>,
        hidden: bool,
        limit: usize,
        visible: &impl Fn(&Path, bool) -> bool,
        rows: &mut Vec<Entry>,
    ) -> std::io::Result<()> {
        let mut entries = Vec::new();
        for entry in fs::read_dir(path)? {
            let Ok(entry) = entry else {
                continue;
            };
            let name = entry.file_name().to_string_lossy().into_owned();
            if !hidden && name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            let directory = entry.file_type().is_ok_and(|kind| {
                kind.is_dir() || kind.is_symlink() && path.is_dir()
            });
            if visible(&path, directory) {
                entries.push(Entry {
                    expanded: directory && expanded.contains(&path),
                    directory,
                    path,
                    name,
                    depth,
                });
            }
        }
        entries.sort_unstable_by(|a, b| {
            b.directory
                .cmp(&a.directory)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                .then_with(|| a.path.cmp(&b.path))
        });
        for entry in entries {
            if rows.len() >= limit {
                break;
            }
            let child = entry.expanded && depth < 24;
            let path = entry.path.clone();
            rows.push(entry);
            if child {
                let _ =
                    walk(&path, depth + 1, expanded, hidden, limit, visible, rows);
            }
        }
        Ok(())
    }
    let mut rows = Vec::new();
    walk(directory, 0, expanded, hidden, limit, visible, &mut rows)?;
    Ok(rows)
}

const ROW_LIMIT: usize = 3_000;

fn search(
    root: &Path,
    query: &str,
    hidden: bool,
    visible: &impl Fn(&Path, bool) -> bool,
    cancelled: &impl Fn() -> bool,
) -> std::io::Result<Vec<Entry>> {
    let query = query.to_lowercase();
    let root_entries = fs::read_dir(root)?;
    let mut folders = VecDeque::new();
    let mut entries = Some(root_entries);
    let mut rows = Vec::new();
    let mut visited = 0;
    'scan: while let Some(folder) = entries.take().or_else(|| {
        while let Some(path) = folders.pop_front() {
            if cancelled() {
                return None;
            }
            if let Ok(folder) = fs::read_dir(path) {
                return Some(folder);
            }
        }
        None
    }) {
        for entry in folder {
            if cancelled() || rows.len() >= ROW_LIMIT || visited >= 20_000 {
                break 'scan;
            }
            visited += 1;
            let Ok(entry) = entry else { continue };
            if !hidden && entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let directory = kind.is_dir() || kind.is_symlink() && path.is_dir();
            if kind.is_dir() {
                folders.push_back(path.clone());
            }
            let relative = path.strip_prefix(root).unwrap_or(&path);
            if entry
                .file_name()
                .to_string_lossy()
                .to_lowercase()
                .contains(&query)
                && visible(&path, directory)
            {
                rows.push(Entry {
                    name: relative.to_string_lossy().into_owned(),
                    path,
                    directory,
                    expanded: false,
                    depth: 0,
                });
            }
        }
    }
    rows.sort_unstable_by(|a, b| {
        b.directory
            .cmp(&a.directory)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.path.cmp(&b.path))
    });
    Ok(rows)
}

pub(super) fn entry_icon(
    path: PathBuf,
    directory: bool,
    config: Config,
) -> impl View {
    let icon = create_memo(move |_| {
        let config = config.get();
        let (source, tint) = if directory {
            config.folder_svg(&path)
        } else {
            config.file_svg(&path)
        };
        let tint = (tint.is_some() || source.contains("currentColor")).then(|| {
            config.color(if directory {
                SparrowColor::FILE_PICKER_FOLDER_ICON
            } else {
                SparrowColor::EDITOR_LINK
            })
        });
        (source, tint)
    });
    svg(move || icon.with(|(source, _)| source.clone())).style(move |s| {
        s.size(16, 16)
            .flex_shrink(0.0_f32)
            .set(TextColor, icon.with(|(_, tint)| *tint))
    })
}

pub(super) fn load(browser: Browser, options: Rc<FileDialogOptions>) {
    let generation = create_rw_signal(0_u64);
    let active = Arc::new(AtomicU64::new(0));
    let scope = Scope::current();
    create_effect(move |previous: Option<String>| {
        let directory = browser.directory.get();
        let expanded = browser.expanded.get();
        let hidden = browser.hidden.get();
        let query = browser.query.with(|query| query.trim().to_owned());
        browser.revision.get();
        if previous.as_ref().is_some_and(|previous| previous != &query) {
            browser.rows.set(Vec::new());
            browser.cursor.set(None);
            browser.selection.set(vec![directory.clone()]);
            browser.preview.set(directory.clone());
            browser.message.set(String::new());
            browser.overwrite.set(None);
        }
        browser.searching.set(!query.is_empty());
        let current_query = query.clone();
        let ticket = active.fetch_add(1, Ordering::Relaxed) + 1;
        let active = active.clone();
        generation.set(ticket);
        let types = options.types.clone();
        let directories = options.directories;
        let rows = browser.rows;
        let message = browser.message;
        let searching = browser.searching;
        let has_query = !query.is_empty();
        let start = move || {
            if generation.try_get_untracked() != Some(ticket) {
                return;
            }
            let ready = floem::ext_event::create_ext_action(
                scope,
                move |result: std::io::Result<Vec<Entry>>| {
                    if generation.try_get_untracked() != Some(ticket) {
                        return;
                    }
                    searching.set(false);
                    match result {
                        Ok(value) => rows.set(value),
                        Err(error) => {
                            rows.set(Vec::new());
                            message
                                .set(format!("Could not read the folder. {error}"));
                        }
                    }
                },
            );
            std::thread::spawn(move || {
                let visible = |path: &Path, is_dir| {
                    if is_dir {
                        return true;
                    }
                    !directories
                        && (types.is_empty()
                            || path
                                .extension()
                                .and_then(|ext| ext.to_str())
                                .is_some_and(|ext| {
                                    types.iter().any(|spec| {
                                        spec.extensions
                                            .iter()
                                            .any(|e| e.eq_ignore_ascii_case(ext))
                                    })
                                }))
                };
                let result = if query.is_empty() {
                    scan(&directory, &expanded, hidden, ROW_LIMIT, &visible)
                } else {
                    search(&directory, &query, hidden, &visible, &|| {
                        active.load(Ordering::Relaxed) != ticket
                    })
                };
                ready(result);
            });
        };
        if has_query {
            floem::action::exec_after(Duration::from_millis(140), move |_| start());
        } else {
            start();
        }
        current_query
    });
}

fn toggle(browser: &Browser, path: PathBuf) {
    browser.expanded.update(|paths| {
        if !paths.remove(&path) {
            paths.insert(path);
        }
    });
}

pub(super) fn view(
    browser: Browser,
    options: Rc<FileDialogOptions>,
    config: Config,
    finish: Complete,
) -> impl View {
    let rows = browser.rows;
    let selected = browser.selection;
    let cursor = browser.cursor;
    let scroll_target = create_rw_signal(floem::kurbo::Rect::ZERO);
    let for_rows = browser.clone();
    let options_for_rows = options.clone();
    let accept = finish.clone();
    let tree = virtual_stack(
        move || rows.read_only(),
        Entry::clone,
        move |entry| {
            let preview = for_rows.clone();
            let down = for_rows.clone();
            let open = for_rows.clone();
            let options = options_for_rows.clone();
            let open_options = options.clone();
            let finish = accept.clone();
            let path = entry.path.clone();
            let check = path.clone();
            let pointer_entry = entry.clone();
            let expanded = entry.expanded;
            let is_folder = entry.directory;
            let name = entry.name.clone();
            let query = for_rows.query;
            let row = stack((
                label(move || {
                    if !is_folder || query.with(|value| !value.trim().is_empty()) {
                        ""
                    } else if expanded {
                        "⌄"
                    } else {
                        "›"
                    }
                })
                .style(|s| s.width(18).flex_shrink(0.0_f32))
                .on_click_stop(move |_| {
                    if is_folder && query.with(|value| value.trim().is_empty()) {
                        toggle(&preview, path.clone());
                    }
                }),
                entry_icon(entry.path.clone(), is_folder, config),
                label(move || name.clone())
                    .style(|s| s.min_width(0).text_ellipsis().selectable(false)),
            ))
            .style(move |s| {
                let config = config.get();
                let selected = selected.with(|paths| paths.contains(&check));
                s.width_full()
                    .min_width(0)
                    .height(24)
                    .items_center()
                    .padding_left(6.0 + entry.depth as f64 * 14.0)
                    .gap(4)
                    .border_radius(4)
                    .apply_if(!selected, |s| {
                        s.hover(|s| {
                            s.background(
                                config.color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                            )
                        })
                    })
                    .apply_if(selected, |s| {
                        s.background(
                            config.color(SparrowColor::PANEL_SELECTION_BACKGROUND),
                        )
                    })
            })
            .on_event_cont(EventListener::PointerDown, move |event| {
                if let Event::PointerDown(pointer) = event
                    && pointer.button.is_primary()
                {
                    let extend = pointer
                        .modifiers
                        .intersects(Modifiers::CONTROL | Modifiers::META);
                    down.choose(&pointer_entry, extend, &options);
                }
            })
            .on_double_click_stop(move |_| {
                if entry.directory {
                    open.enter(entry.path.clone());
                } else {
                    open.submit(&open_options, &finish);
                }
            });
            row
        },
    )
    .item_size_fixed(|| 24.0)
    .style(|s| s.width_full().flex_col());

    let view = sparrow_scroll(tree)
        .ensure_visible(move || scroll_target.get())
        .keyboard_navigable()
        .style(move |s| {
            s.width_full()
                .flex_basis(0)
                .flex_grow(1.0_f32)
                .min_height(0)
                .border(1)
                .border_radius(6)
                .background(config.get().color(SparrowColor::FILE_PICKER_CANVAS))
                .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
        });
    let id = view.id();
    let focus = id;
    floem::action::exec_after(std::time::Duration::from_millis(35), move |_| {
        focus.request_focus()
    });
    view.on_event_cont(EventListener::PointerDown, move |_| id.request_focus())
        .on_event(EventListener::KeyDown, move |e| {
            let Event::KeyDown(event) = e else {
                return EventPropagation::Continue;
            };
            let rows = rows.get_untracked();
            let index = cursor
                .get_untracked()
                .and_then(|path| rows.iter().position(|entry| entry.path == path));
            let current = index.and_then(|index| rows.get(index));
            let searching = browser.query.with(|value| !value.trim().is_empty());
            match event.key.logical_key {
                Key::Named(
                    NamedKey::ArrowDown
                    | NamedKey::ArrowUp
                    | NamedKey::Home
                    | NamedKey::End,
                ) => {
                    let next = match event.key.logical_key {
                        Key::Named(NamedKey::ArrowUp) => {
                            index.unwrap_or(0).saturating_sub(1)
                        }
                        Key::Named(NamedKey::ArrowDown) => index
                            .map_or(0, |i| i + 1)
                            .min(rows.len().saturating_sub(1)),
                        Key::Named(NamedKey::End) => rows.len().saturating_sub(1),
                        _ => 0,
                    };
                    if let Some(entry) = rows.get(next) {
                        browser.choose(entry, false, &options);
                        scroll_target.set(floem::kurbo::Rect::new(
                            0.0,
                            next as f64 * 24.0,
                            0.0,
                            (next + 1) as f64 * 24.0,
                        ));
                    }
                }
                Key::Named(NamedKey::ArrowRight) if !searching => {
                    if let Some(entry) =
                        current.filter(|e| e.directory && !e.expanded)
                    {
                        toggle(&browser, entry.path.clone());
                    }
                }
                Key::Named(NamedKey::ArrowLeft) if !searching => {
                    if let Some(entry) = current.filter(|e| e.expanded) {
                        toggle(&browser, entry.path.clone());
                    } else if let Some(parent) =
                        browser.directory.get_untracked().parent()
                    {
                        browser.enter(parent.to_path_buf());
                    }
                }
                Key::Named(NamedKey::Enter) => {
                    if let Some(entry) =
                        current.filter(|e| e.directory && !options.accepts(&e.path))
                    {
                        browser.enter(entry.path.clone());
                    } else {
                        browser.submit(&options, &finish);
                    }
                }
                _ => return EventPropagation::Continue,
            }
            EventPropagation::Stop
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expanded_tree_orders_folders_and_follows_hidden_setting() {
        let root = tempfile::tempdir().unwrap();
        let nested = root.path().join("src");
        fs::create_dir(&nested).unwrap();
        fs::write(nested.join("main.dart"), "").unwrap();
        fs::write(root.path().join(".secret"), "").unwrap();
        fs::write(root.path().join("README"), "").unwrap();
        let rows = scan(
            root.path(),
            &HashSet::from([nested]),
            false,
            100,
            &|_, _| true,
        )
        .unwrap();
        assert_eq!(
            rows.iter()
                .map(|row| (&row.name[..], row.depth))
                .collect::<Vec<_>>(),
            [("src", 0), ("main.dart", 1), ("README", 0)]
        );
        assert!(
            scan(root.path(), &HashSet::new(), true, 100, &|_, _| true)
                .unwrap()
                .iter()
                .any(|row| row.name == ".secret")
        );
    }
    #[test]
    fn search_matches_nested_names_and_respects_hidden_files_and_filters() {
        let root = tempfile::tempdir().unwrap();
        let public = root.path().join("src");
        let hidden = root.path().join(".private");
        fs::create_dir(&public).unwrap();
        fs::create_dir(&hidden).unwrap();
        fs::write(public.join("Main.DART"), "").unwrap();
        fs::write(public.join("main.txt"), "").unwrap();
        fs::write(hidden.join("main.dart"), "").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.path(), public.join("cycle")).unwrap();

        let visible = |path: &Path, directory| {
            directory
                || path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("dart"))
        };
        let visible_rows =
            search(root.path(), "MaIn", false, &visible, &|| false).unwrap();
        assert_eq!(visible_rows.len(), 1);
        assert_eq!(visible_rows[0].path, public.join("Main.DART"));
        assert_eq!(visible_rows[0].depth, 0);
        let all_rows =
            search(root.path(), "MaIn", true, &visible, &|| false).unwrap();
        assert_eq!(all_rows.len(), 2);
        assert!(
            all_rows
                .iter()
                .any(|row| row.path == hidden.join("main.dart"))
        );
        assert!(
            search(root.path(), "main", true, &visible, &|| true)
                .unwrap()
                .is_empty()
        );
    }
}
