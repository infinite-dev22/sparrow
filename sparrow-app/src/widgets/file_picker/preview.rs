//! Bounded, read-only previews for the file chooser.
use std::{
    collections::HashSet,
    fs::{self, File},
    io::{BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

use floem::{
    IntoView, View,
    peniko::{Blob, Image, ImageFormat},
    reactive::{Scope, SignalGet, SignalUpdate, create_effect, create_rw_signal},
    views::{
        Decorators, dyn_container, dyn_stack, img, label, stack, stack_from_iter,
        svg,
    },
};

use super::{Browser, Config, tree};
use crate::{
    config::color::SparrowColor,
    widgets::{ButtonKind, button, sparrow_scroll},
};

#[derive(Clone)]
enum Content {
    Loading,
    Text(String, bool),
    Image(Image),
    Svg(String),
    Message(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Media {
    Pdf,
    Video,
}

fn extension(path: &Path) -> String {
    path.extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

fn media(path: &Path) -> Option<Media> {
    match extension(path).as_str() {
        "pdf" => Some(Media::Pdf),
        "mp4" | "m4v" | "mov" | "mkv" | "webm" | "avi" | "mpeg" | "mpg" => {
            Some(Media::Video)
        }
        _ => None,
    }
}

fn load(path: &Path, step: u32) -> Result<Content, String> {
    let meta = fs::metadata(path).map_err(|error| error.to_string())?;
    if !meta.is_file() {
        return Ok(Content::Message(
            "Select a file in the folder to preview it.".into(),
        ));
    }
    if let Some(kind) = media(path) {
        return render_media(path, step, kind).map(Content::Image);
    }
    let ext = extension(path);
    if matches!(ext.as_str(), "jpeg" | "jpg" | "png" | "webp" | "ico") {
        return decode(path).map(Content::Image);
    }
    if ext == "svg" && meta.len() < 1_048_576 {
        return fs::read_to_string(path)
            .map(Content::Svg)
            .map_err(|error| error.to_string());
    }
    let file = File::open(path).map_err(|e| e.to_string())?;
    const LIMIT: u64 = 128 * 1024;
    let mut bytes = Vec::new();
    BufReader::new(file)
        .take(LIMIT)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    let truncated = meta.len() > LIMIT;
    if bytes.contains(&0) {
        return Ok(Content::Message(format!(
            "Preview unavailable for this file · {} bytes",
            meta.len()
        )));
    }
    match std::str::from_utf8(
        bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes),
    ) {
        Ok(text) => Ok(Content::Text(
            text.lines().take(600).collect::<Vec<_>>().join("\n"),
            truncated || text.lines().count() > 600,
        )),
        Err(error) if truncated && error.error_len().is_none() => {
            let text =
                std::str::from_utf8(&bytes[..error.valid_up_to()]).unwrap_or("");
            Ok(Content::Text(
                text.lines().take(600).collect::<Vec<_>>().join("\n"),
                true,
            ))
        }
        Err(_) => Ok(Content::Message(format!(
            "Preview unavailable for this file · {} bytes",
            meta.len()
        ))),
    }
}

fn decode(path: &Path) -> Result<Image, String> {
    let mut input = image::io::Reader::open(path)
        .and_then(|reader| reader.with_guessed_format())
        .map_err(|e| e.to_string())?;
    let mut limits = image::io::Limits::default();
    limits.max_image_width = Some(12_000);
    limits.max_image_height = Some(12_000);
    limits.max_alloc = Some(96 * 1024 * 1024);
    input.limits(limits);
    let pixels = input.decode().map_err(|e| e.to_string())?;
    let pixels = if pixels.width() > 1800 || pixels.height() > 1800 {
        pixels.thumbnail(1800, 1800)
    } else {
        pixels
    }
    .into_rgba8();
    let (w, h) = pixels.dimensions();
    Ok(Image::new(
        Blob::new(Arc::new(pixels.into_raw())),
        ImageFormat::Rgba8,
        w,
        h,
    ))
}

fn render_media(path: &Path, step: u32, media: Media) -> Result<Image, String> {
    let scratch = tempfile::tempdir().map_err(|e| e.to_string())?;
    let mut command = match media {
        Media::Pdf => {
            let mut command = Command::new("pdftoppm");
            let page = (step + 1).to_string();
            command
                .args([
                    "-f",
                    &page,
                    "-l",
                    &page,
                    "-singlefile",
                    "-scale-to",
                    "1300",
                    "-png",
                ])
                .arg(path)
                .arg(scratch.path().join("page"));
            command
        }
        Media::Video => {
            let mut command = Command::new("ffmpeg");
            command
                .args([
                    "-nostdin",
                    "-loglevel",
                    "error",
                    "-protocol_whitelist",
                    "file,pipe",
                    "-ss",
                    &step.to_string(),
                    "-i",
                ])
                .arg(path)
                .args([
                    "-frames:v",
                    "1",
                    "-vf",
                    "scale=960:960:force_original_aspect_ratio=decrease",
                    "-threads",
                    "1",
                ])
                .arg(scratch.path().join("page.png"));
            command
        }
    };
    let executable = command.get_program().to_string_lossy().into_owned();
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                format!(
                    "Install {executable} to preview this format inside Sparrow."
                )
            } else {
                e.to_string()
            }
        })?;
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            let image = scratch.path().join("page.png");
            if status.success() && image.exists() {
                return decode(&image);
            }
            return Err("This page or timestamp is unavailable.".into());
        }
        if Instant::now() >= until {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Preview timed out.".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn folder_rows(path: &Path, hidden: bool) -> Vec<tree::Entry> {
    let roots = tree::scan(path, &HashSet::new(), hidden, 100, &|_, _| true)
        .unwrap_or_default();
    let expand = roots
        .iter()
        .filter(|entry| entry.directory)
        .take(12)
        .map(|entry| entry.path.clone())
        .collect::<HashSet<_>>();
    tree::scan(path, &expand, hidden, 120, &|_, _| true).unwrap_or(roots)
}

pub(super) fn view(browser: Browser, config: Config) -> impl View {
    let selected = browser.preview;
    let hidden = browser.hidden;
    let revision = browser.revision;
    let folder = create_rw_signal(Vec::<tree::Entry>::new());
    let preview_path = create_rw_signal(selected.get_untracked());
    let content = create_rw_signal(Content::Loading);
    let step = create_rw_signal(0_u32);
    let scope = Scope::current();
    let folder_ticket = create_rw_signal(0_u64);
    create_effect(move |_| {
        let path = selected.get();
        let hidden = hidden.get();
        revision.get();
        let ticket = folder_ticket.get_untracked() + 1;
        folder_ticket.set(ticket);
        folder.set(Vec::new());
        preview_path.set(path.clone());
        if !path.is_dir() {
            return;
        }
        let ready = floem::ext_event::create_ext_action(scope, move |rows| {
            if folder_ticket.try_get_untracked() == Some(ticket) {
                folder.set(rows);
            }
        });
        std::thread::spawn(move || ready(folder_rows(&path, hidden)));
    });
    create_effect(move |_| {
        preview_path.get();
        step.set(0);
    });
    let file_ticket = create_rw_signal(0_u64);
    create_effect(move |_| {
        let path = preview_path.get();
        let at = step.get();
        revision.get();
        let ticket = file_ticket.get_untracked() + 1;
        file_ticket.set(ticket);
        content.set(Content::Loading);
        let ready = floem::ext_event::create_ext_action(scope, move |result| {
            if file_ticket.try_get_untracked() == Some(ticket) {
                content.set(result);
            }
        });
        std::thread::spawn(move || {
            ready(load(&path, at).unwrap_or_else(Content::Message));
        });
    });
    let folder_view = sparrow_scroll(
        dyn_stack(
            move || folder.get(),
            tree::Entry::clone,
            move |entry| {
                let name = entry.name.clone();
                let is_dir = entry.directory;
                let path = entry.path.clone();
                let row = stack((
                    label(move || if is_dir { "›" } else { "" })
                        .style(|s| s.width(11)),
                    svg(move || config.get().file_svg(&path).0)
                        .style(|s| s.size(14, 14)),
                    label(move || name.clone())
                        .style(|s| s.min_width(0).text_ellipsis()),
                ))
                .style(move |s| {
                    s.width_full()
                        .height(23)
                        .items_center()
                        .gap(4)
                        .padding_left(7.0 + entry.depth as f64 * 14.0)
                        .hover(|s| {
                            s.background(
                                config
                                    .get()
                                    .color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                            )
                        })
                });
                row.on_click_stop(move |_| {
                    if entry.directory {
                        // Expand the nested folder inside the preview only.
                        let path = entry.path.clone();
                        let ready = floem::ext_event::create_ext_action(
                            scope,
                            move |(path, children): (PathBuf, Vec<tree::Entry>)| {
                                if !path.starts_with(selected.get_untracked()) {
                                    return;
                                }
                                folder.update(|rows| {
                                    if let Some(index) =
                                        rows.iter().position(|row| row.path == path)
                                    {
                                        let end = index
                                            + 1
                                            + rows[index + 1..]
                                                .iter()
                                                .take_while(|row| {
                                                    row.depth > rows[index].depth
                                                })
                                                .count();
                                        if end > index + 1 {
                                            rows.drain(index + 1..end);
                                        } else {
                                            rows.splice(
                                                index + 1..index + 1,
                                                children,
                                            );
                                        }
                                    }
                                });
                            },
                        );
                        let dotfiles = hidden.get_untracked();
                        let depth = entry.depth;
                        std::thread::spawn(move || {
                            let mut children = tree::scan(
                                &path,
                                &HashSet::new(),
                                dotfiles,
                                100,
                                &|_, _| true,
                            )
                            .unwrap_or_default();
                            for child in &mut children {
                                child.depth = depth + 1;
                            }
                            ready((path, children));
                        });
                    } else {
                        preview_path.set(entry.path.clone());
                    }
                })
            },
        )
        .style(|s| s.flex_col().width_full()),
    )
    .style(move |s| {
        let base = selected.get();
        s.width_full()
            .min_height(0)
            .height(if preview_path.get() == base { 260 } else { 160 })
            .border(1)
            .border_radius(6)
            .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
            .apply_if(!base.is_dir(), |s| s.hide())
    });
    let viewer = dyn_container(
        move || content.get(),
        move |content| match content {
            Content::Image(frame) => {
                let width = f64::from(frame.width);
                let height = f64::from(frame.height);
                let available = create_rw_signal(floem::kurbo::Size::ZERO);
                let view = img(Vec::new);
                view.id().update_state(frame);
                stack((view.style(move |s| {
                    let size = available.get();
                    let scale =
                        (size.width / width).min(size.height / height).min(1.0);
                    s.width(width * scale)
                        .height(height * scale)
                        .flex_shrink(0.0_f32)
                }),))
                .style(|s| {
                    s.size_full()
                        .items_center()
                        .justify_center()
                        .min_width(0)
                        .min_height(0)
                })
                .on_resize(move |rect| {
                    if available.get_untracked() != rect.size() {
                        available.set(rect.size());
                    }
                })
                .into_any()
            }
            Content::Svg(source) => svg(source)
                .style(|s| s.size_full().min_width(0).min_height(0))
                .into_any(),
            Content::Text(text, truncated) => {
                let editor_fonts = config.get_untracked().editor.font_family.clone();
                let lines = text
                    .lines()
                    .enumerate()
                    .map(move |(index, line)| {
                        // Keep long lines intact and pan horizontally within the preview.
                        let line = line.replace('\t', "    ");
                        stack((
                            label(move || format!("{}", index + 1)).style(
                                move |s| {
                                    s.width(42).flex_shrink(0.0_f32).color(
                                        config.get().color(
                                            SparrowColor::PANEL_FOREGROUND_DIM,
                                        ),
                                    )
                                },
                            ),
                            label(move || line.clone())
                                .style(|s| s.flex_shrink(0.0_f32).selectable(true)),
                        ))
                        .style(|s| s.min_height(18).flex_shrink(0.0_f32))
                    })
                    .collect::<Vec<_>>();
                sparrow_scroll(
                    stack((
                        label(|| "Showing the start of this file")
                            .style(move |s| s.apply_if(!truncated, |s| s.hide())),
                        stack_from_iter(lines).style(move |s| {
                            s.flex_col()
                                .items_start()
                                .font_family(editor_fonts.clone())
                        }),
                    ))
                    .style(|s| {
                        s.flex_col().items_start().padding(8).min_width_full()
                    }),
                )
                .style(|s| s.size_full().min_height(0).min_width(0))
                .into_any()
            }
            Content::Loading => label(|| "Loading preview…")
                .style(|s| s.padding(8))
                .into_any(),
            Content::Message(message) => label(move || message.clone())
                .style(|s| s.padding(8).min_width(0).width_full())
                .into_any(),
        },
    )
    .style(move |s| {
        s.width_full()
            .flex_grow(1.0_f32)
            .flex_basis(0)
            .min_height(0)
            .min_width(0)
            .border(1)
            .border_radius(6)
            .background(config.get().color(SparrowColor::EDITOR_BACKGROUND))
            .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
    });
    stack((
        label(move || {
            let path = selected.get();
            path.file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy()
                .into_owned()
        })
        .style(|s| {
            s.font_size(16)
                .font_bold()
                .text_ellipsis()
                .min_width(0)
                .width_full()
        }),
        label(move || preview_path.get().display().to_string()).style(move |s| {
            s.text_ellipsis()
                .selectable(true)
                .width_full()
                .min_width(0)
                .font_size(11)
                .color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
        }),
        folder_view,
        stack((
            button(|| "Previous", ButtonKind::Outlined, config)
                .on_click_stop(move |_| {
                    let jump = if media(&preview_path.get_untracked())
                        == Some(Media::Video)
                    {
                        5
                    } else {
                        1
                    };
                    step.update(|at| *at = at.saturating_sub(jump));
                })
                .disabled(move || step.get() == 0)
                .style(|s| s.padding_horiz(7).height(23)),
            label(move || {
                if media(&preview_path.get()) == Some(Media::Video) {
                    format!("{} s", step.get())
                } else {
                    format!("Page {}", step.get() + 1)
                }
            }),
            button(|| "Next", ButtonKind::Outlined, config)
                .on_click_stop(move |_| {
                    let jump = if media(&preview_path.get_untracked())
                        == Some(Media::Video)
                    {
                        5
                    } else {
                        1
                    };
                    step.update(|at| *at = at.saturating_add(jump).min(86_400));
                })
                .style(|s| s.padding_horiz(7).height(23)),
        ))
        .style(move |s| {
            s.items_center()
                .gap(7)
                .apply_if(media(&preview_path.get()).is_none(), |s| s.hide())
        }),
        viewer,
    ))
    .style(|s| s.flex_col().gap(8).width_full().min_width(0).min_height(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_text_and_webp_previews() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("notes.txt");
        fs::write(&file, "row\n".repeat(800)).unwrap();
        assert!(
            matches!(load(&file, 0).unwrap(), Content::Text(text, true) if text.lines().count() == 600)
        );
        fs::write(&file, [0, 1, 2]).unwrap();
        assert!(matches!(load(&file, 0).unwrap(), Content::Message(_)));
        let file = folder.path().join("graphic.webp");
        image::RgbaImage::from_pixel(20, 10, image::Rgba([20, 40, 60, 255]))
            .save(&file)
            .unwrap();
        assert!(
            matches!(load(&file, 0).unwrap(), Content::Image(img) if img.width == 20 && img.height == 10)
        );
    }
}
