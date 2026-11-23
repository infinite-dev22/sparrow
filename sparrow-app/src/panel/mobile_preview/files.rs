//! Files explicitly imported into this simulated device.
use std::{path::PathBuf, rc::Rc};

use floem::{
    View,
    file::FileDialogOptions,
    file_action::{open_file, save_as},
    reactive::{SignalGet, SignalUpdate},
    views::{Decorators, dyn_stack, label, stack},
};

use crate::widgets::{ButtonKind, button};

use super::{screen::Config, session::Session};

pub(super) fn view(session: Session, config: Config) -> impl View {
    let storage = session.files.clone();
    let revision = session.files_revision;
    let status = session.status;
    let items = storage.clone();
    let importer = Rc::new(move || {
        let storage = storage.clone();
        open_file(
            FileDialogOptions::new()
                .title("Import files to simulated device")
                .multi_selection(),
            move |info| {
                let Some(info) = info else {
                    return;
                };
                let result = (|| -> std::io::Result<()> {
                    let mut storage = storage.borrow_mut();
                    if storage.is_none() {
                        *storage = Some(tempfile::tempdir()?);
                    }
                    let root = storage.as_ref().unwrap().path();
                    for source in info.path() {
                        if !source.is_file() {
                            continue;
                        }
                        let Some(name) = source.file_name() else {
                            continue;
                        };
                        let mut destination = root.join(name);
                        let mut sequence = 2;
                        while destination.exists() {
                            destination = root.join(format!(
                                "{sequence}-{}",
                                name.to_string_lossy()
                            ));
                            sequence += 1;
                        }
                        std::fs::copy(source, destination)?;
                    }
                    Ok(())
                })();
                revision.update(|n| *n += 1);
                if let Err(error) = result {
                    status.set(format!("Could not import files: {error}"));
                }
            },
        );
    });
    stack((
        label(|| "Device files").style(|s| s.font_bold()),
        button(|| "Import files…", ButtonKind::Outlined, config)
            .on_click_stop(move |_| importer())
            .style(|s| s.padding(5.0)),
        label(|| "This device contains only the files you import.")
            .style(|s| s.font_size(11.0).width_full()),
        dyn_stack(
            move || {
                revision.get();
                let mut paths = items
                    .borrow()
                    .as_ref()
                    .and_then(|storage| std::fs::read_dir(storage.path()).ok())
                    .into_iter()
                    .flatten()
                    .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                    .collect::<Vec<_>>();
                paths.sort();
                paths
            },
            PathBuf::clone,
            move |path| {
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                let destination_name = name.clone();
                stack((
                    label(move || name.clone()).style(|s| {
                        s.min_width(0).text_ellipsis().flex_grow(1.0_f32)
                    }),
                    button(|| "Export…", ButtonKind::Plain, config).on_click_stop(
                        move |_| {
                            let path = path.clone();
                            save_as(
                                FileDialogOptions::new()
                                    .default_name(destination_name.clone()),
                                move |info| {
                                    if let Some(target) = info
                                        .as_ref()
                                        .and_then(|info| info.path().first())
                                        && let Err(error) =
                                            std::fs::copy(&path, target)
                                    {
                                        status.set(format!(
                                            "Could not export file: {error}"
                                        ));
                                    }
                                },
                            );
                        },
                    ),
                ))
                .style(|s| s.items_center().gap(5.0).width_full())
            },
        )
        .style(|s| s.flex_col().width_full().gap(3.0)),
    ))
    .style(|s| {
        s.flex_col()
            .width_full()
            .gap(8.0)
            .items_start()
            .padding(12.0)
    })
}
