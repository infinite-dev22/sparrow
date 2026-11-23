//! Directory scanning and chooser tree interaction.
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
};

use floem::{
    View,
    event::{Event, EventListener, EventPropagation},
    keyboard::{Key, Modifiers, NamedKey},
    reactive::{
        Scope, SignalGet, SignalUpdate, SignalWith, create_effect, create_rw_signal,
    },
    views::{Decorators, label, stack, svg, virtual_stack},
};

use super::{Browser, Complete, Config, FileDialogOptions};
use crate::{
    config::{color::SparrowColor, icon::SparrowIcons},
    widgets::sparrow_scroll,
};

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

pub(super) fn load(browser: Browser, options: Rc<FileDialogOptions>) {
    let generation = create_rw_signal(0_u64);
    let scope = Scope::current();
    create_effect(move |_| {
        let directory = browser.directory.get();
        let expanded = browser.expanded.get();
        let hidden = browser.hidden.get();
        browser.revision.get();
        let ticket = generation.get_untracked() + 1;
        generation.set(ticket);
        let types = options.types.clone();
        let directories = options.directories;
        let rows = browser.rows;
        let message = browser.message;
        let ready = floem::ext_event::create_ext_action(
            scope,
            move |result: std::io::Result<Vec<Entry>>| {
                if generation.try_get_untracked() != Some(ticket) {
                    return;
                }
                match result {
                    Ok(value) => rows.set(value),
                    Err(error) => {
                        rows.set(Vec::new());
                        message.set(format!("Could not read the folder. {error}"));
                    }
                }
            },
        );
        std::thread::spawn(move || {
            ready(scan(
                &directory,
                &expanded,
                hidden,
                3000,
                &|path, is_dir| {
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
                },
            ));
        });
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
            let icon = path.clone();
            let browse = if is_folder {
                SparrowIcons::DIRECTORY
            } else {
                SparrowIcons::FILE
            };
            let row = stack((
                label(move || {
                    if !is_folder {
                        ""
                    } else if expanded {
                        "⌄"
                    } else {
                        "›"
                    }
                })
                .style(|s| s.width(18).flex_shrink(0.0_f32))
                .on_click_stop(move |_| {
                    if is_folder {
                        toggle(&preview, path.clone());
                    }
                }),
                svg(move || {
                    if is_folder {
                        config.get().ui_svg(browse)
                    } else {
                        config.get().file_svg(&icon).0
                    }
                })
                .style(move |s| {
                    s.size(15, 15)
                        .flex_shrink(0.0_f32)
                        .color(config.get().color(SparrowColor::SPARROW_ICON_ACTIVE))
                }),
                label(move || name.clone())
                    .style(|s| s.min_width(0).text_ellipsis().selectable(false)),
            ))
            .style(move |s| {
                let config = config.get();
                s.width_full()
                    .min_width(0)
                    .height(24)
                    .items_center()
                    .padding_left(6.0 + entry.depth as f64 * 14.0)
                    .gap(4)
                    .border_radius(4)
                    .apply_if(selected.with(|paths| paths.contains(&check)), |s| {
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
                Key::Named(NamedKey::ArrowRight) => {
                    if let Some(entry) =
                        current.filter(|e| e.directory && !e.expanded)
                    {
                        toggle(&browser, entry.path.clone());
                    }
                }
                Key::Named(NamedKey::ArrowLeft) => {
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
}
