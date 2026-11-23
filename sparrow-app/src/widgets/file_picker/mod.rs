//! Sparrow's local file and folder chooser.
mod preview;
mod tree;

use std::{
    cell::RefCell,
    collections::HashSet,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

use floem::{
    IntoView, View,
    event::{Event, EventListener, EventPropagation},
    file::{FileInfo, FileSpec},
    keyboard::{Key, NamedKey},
    reactive::{
        ReadSignal, RwSignal, SignalGet, SignalUpdate, create_effect,
        create_rw_signal,
    },
    views::{Decorators, dyn_container, label, stack, stack_from_iter},
    window::WindowConfig,
};

use crate::{
    config::{SparrowConfig, color::SparrowColor},
    widgets::{
        ButtonKind, Divider, DividerVariant, button, horizontal_scroll, text_input,
    },
};

pub(crate) type Config = ReadSignal<Arc<SparrowConfig>>;
type Complete = Rc<dyn Fn(Option<FileInfo>)>;

#[derive(Default, Clone)]
pub(crate) struct FileDialogOptions {
    title: String,
    start: Option<PathBuf>,
    default_name: String,
    types: Vec<FileSpec>,
    directories: bool,
    mixed: bool,
    multiple: bool,
    save: bool,
}

impl FileDialogOptions {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }
    pub fn force_starting_directory(mut self, path: impl Into<PathBuf>) -> Self {
        self.start = Some(path.into());
        self
    }
    pub fn default_name(mut self, name: impl Into<String>) -> Self {
        self.default_name = name.into();
        self
    }
    pub fn allowed_types(mut self, types: Vec<FileSpec>) -> Self {
        self.types = types;
        self
    }
    pub fn select_directories(mut self) -> Self {
        self.directories = true;
        self
    }
    pub fn files_and_directories(mut self) -> Self {
        self.mixed = true;
        self
    }
    pub fn multi_selection(mut self) -> Self {
        self.multiple = true;
        self
    }

    fn format(&self, path: &Path) -> Option<FileSpec> {
        let extension = path.extension()?.to_str()?;
        self.types.iter().copied().find(|spec| {
            spec.extensions
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(extension))
        })
    }

    fn accepts(&self, path: &Path) -> bool {
        if path.is_dir() {
            return self.directories || self.mixed;
        }
        !self.directories
            && path.is_file()
            && (self.types.is_empty() || self.format(path).is_some())
    }

    fn save_path(&self, directory: &Path, name: &str) -> Option<PathBuf> {
        let path = Path::new(name);
        if name.trim().is_empty()
            || path.file_name().is_none()
            || name.ends_with(std::path::MAIN_SEPARATOR)
        {
            return None;
        }
        let mut target = directory.join(path);
        if target.extension().is_none()
            && let Some(extension) =
                self.types.first().and_then(|spec| spec.extensions.first())
        {
            target.set_extension(extension);
        }
        (!target.is_dir()
            && target.parent().is_some_and(Path::is_dir)
            && (self.types.is_empty() || self.format(&target).is_some()))
        .then_some(target)
    }
}

pub(crate) fn open_file(
    options: FileDialogOptions,
    config: Config,
    callback: impl Fn(Option<FileInfo>) + 'static,
) {
    show(options, config, callback);
}

pub(crate) fn save_as(
    mut options: FileDialogOptions,
    config: Config,
    callback: impl Fn(Option<FileInfo>) + 'static,
) {
    options.save = true;
    show(options, config, callback);
}

thread_local! {
    static LAST_DIRECTORY: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

fn home() -> Option<PathBuf> {
    ["HOME", "USERPROFILE"]
        .into_iter()
        .find_map(|key| std::env::var_os(key).map(PathBuf::from))
        .filter(|p| p.is_dir())
}

fn show(
    options: FileDialogOptions,
    config: Config,
    callback: impl Fn(Option<FileInfo>) + 'static,
) {
    let theme = config.get_untracked();
    let title = if options.title.is_empty() {
        "Choose a file".into()
    } else {
        options.title.clone()
    };
    let home = home();
    let start = options
        .start
        .as_ref()
        .filter(|path| path.is_dir())
        .cloned()
        .or_else(|| {
            LAST_DIRECTORY
                .with_borrow(Clone::clone)
                .filter(|path| path.is_dir())
        })
        .or_else(|| home.clone())
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    let callback = Rc::new(RefCell::new(Some(callback)));
    floem::new_window(
        move |id| {
            let finish: Complete = Rc::new(move |value| {
                let Some(callback) = callback.borrow_mut().take() else {
                    return;
                };
                if let Some(path) = value.as_ref().and_then(|file| file.path.first())
                {
                    let directory = if path.is_dir() {
                        Some(path.as_path())
                    } else {
                        path.parent()
                    };
                    LAST_DIRECTORY.with_borrow_mut(|last| {
                        *last = directory.map(Path::to_path_buf)
                    });
                }
                callback(value);
                floem::close_window(id);
            });
            let config = create_rw_signal(theme).read_only();
            dialog(Rc::new(options), start, home, config, finish)
        },
        Some(
            WindowConfig::default()
                .title(title)
                .size((1040.0, 680.0))
                .min_size((590.0, 390.0)),
        ),
    );
}

#[derive(Clone)]
struct Browser {
    directory: RwSignal<PathBuf>,
    expanded: RwSignal<HashSet<PathBuf>>,
    rows: RwSignal<Vec<tree::Entry>>,
    selection: RwSignal<Vec<PathBuf>>,
    cursor: RwSignal<Option<PathBuf>>,
    hidden: RwSignal<bool>,
    revision: RwSignal<u64>,
    path: RwSignal<String>,
    preview: RwSignal<PathBuf>,
    filename: RwSignal<String>,
    new_folder: RwSignal<bool>,
    folder_name: RwSignal<String>,
    overwrite: RwSignal<Option<PathBuf>>,
    message: RwSignal<String>,
}

impl Browser {
    fn new(path: PathBuf, name: String) -> Self {
        Self {
            directory: create_rw_signal(path.clone()),
            expanded: create_rw_signal(HashSet::new()),
            rows: create_rw_signal(Vec::new()),
            selection: create_rw_signal(vec![path.clone()]),
            cursor: create_rw_signal(None),
            hidden: create_rw_signal(false),
            revision: create_rw_signal(0),
            path: create_rw_signal(path.display().to_string()),
            preview: create_rw_signal(path),
            filename: create_rw_signal(name),
            new_folder: create_rw_signal(false),
            folder_name: create_rw_signal(String::new()),
            overwrite: create_rw_signal(None),
            message: create_rw_signal(String::new()),
        }
    }

    fn enter(&self, directory: PathBuf) {
        if !directory.is_dir() {
            return;
        }
        self.expanded.set(HashSet::new());
        self.rows.set(Vec::new());
        self.cursor.set(None);
        self.selection.set(vec![directory.clone()]);
        self.preview.set(directory.clone());
        self.path.set(directory.display().to_string());
        self.message.set(String::new());
        self.overwrite.set(None);
        self.directory.set(directory);
    }

    fn choose(
        &self,
        entry: &tree::Entry,
        extend: bool,
        options: &FileDialogOptions,
    ) {
        self.preview.set(entry.path.clone());
        self.cursor.set(Some(entry.path.clone()));
        if options.save && !entry.directory {
            self.filename.set(entry.name.clone());
        }
        if extend && options.multiple && options.accepts(&entry.path) {
            self.selection.update(|paths| {
                if let Some(index) = paths.iter().position(|p| p == &entry.path) {
                    paths.remove(index);
                } else {
                    paths.push(entry.path.clone());
                }
            });
        } else {
            self.selection.set(vec![entry.path.clone()]);
        }
        self.message.set(String::new());
        self.overwrite.set(None);
    }

    fn path_entry(&self, options: &FileDialogOptions, complete: &Complete) {
        let field = self.path.get_untracked();
        let field = field.trim();
        let path = if let Some(value) = field.strip_prefix("~/") {
            home()
                .map(|home| home.join(value))
                .unwrap_or_else(|| PathBuf::from(field))
        } else {
            self.directory.get_untracked().join(field)
        };
        if path.is_dir() {
            self.enter(path);
        } else if options.save && path.parent().is_some_and(Path::is_dir) {
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            self.enter(path.parent().unwrap().to_path_buf());
            self.filename.set(name);
        } else if options.accepts(&path) {
            if self.selection.get_untracked().as_slice() == [path.clone()] {
                self.submit(options, complete);
                return;
            }
            if let Some(parent) = path.parent() {
                self.enter(parent.to_path_buf());
            }
            self.path.set(path.display().to_string());
            self.preview.set(path.clone());
            self.cursor.set(Some(path.clone()));
            self.selection.set(vec![path]);
        } else {
            self.message
                .set("Enter an existing path supported by this chooser.".into());
        }
    }

    fn save_directory(&self) -> PathBuf {
        self.selection
            .get_untracked()
            .first()
            .and_then(|path| {
                if path.is_dir() {
                    Some(path.clone())
                } else {
                    path.parent().map(Path::to_path_buf)
                }
            })
            .unwrap_or_else(|| self.directory.get_untracked())
    }

    fn create_folder(&self) {
        let name = self.folder_name.get_untracked();
        let name = name.trim();
        let mut components = Path::new(name).components();
        if !matches!(components.next(), Some(std::path::Component::Normal(_)))
            || components.next().is_some()
        {
            self.message.set("Enter a single folder name.".into());
            return;
        }
        let folder = self.save_directory().join(name);
        match std::fs::create_dir(&folder) {
            Ok(()) => {
                self.new_folder.set(false);
                self.enter(folder);
            }
            Err(error) => self
                .message
                .set(format!("Could not create the folder. {error}")),
        }
    }

    fn submit(&self, options: &FileDialogOptions, finish: &Complete) {
        let paths = if options.save {
            let Some(path) = options
                .save_path(&self.save_directory(), &self.filename.get_untracked())
            else {
                self.message
                    .set("Enter a valid filename in an existing folder.".into());
                return;
            };
            if path.symlink_metadata().is_ok()
                && self.overwrite.get_untracked().as_ref() != Some(&path)
            {
                self.overwrite.set(Some(path));
                return;
            }
            vec![path]
        } else {
            let paths: Vec<_> = self
                .selection
                .get_untracked()
                .into_iter()
                .filter(|p| options.accepts(p))
                .collect();
            if paths.is_empty() {
                self.message.set(
                    if options.directories {
                        "Choose a folder."
                    } else {
                        "Choose a supported file."
                    }
                    .into(),
                );
                return;
            }
            paths
        };
        let format = paths.first().and_then(|path| options.format(path));
        finish(Some(FileInfo {
            path: paths,
            format,
        }));
    }
}

fn dialog(
    options: Rc<FileDialogOptions>,
    start: PathBuf,
    home: Option<PathBuf>,
    config: Config,
    finish: Complete,
) -> impl View {
    let browser = Browser::new(start, options.default_name.clone());
    tree::load(browser.clone(), options.clone());
    let tree = tree::view(browser.clone(), options.clone(), config, finish.clone());
    let right = preview::view(browser.clone(), config);
    let controls = toolbar(browser.clone(), home, config);
    let path = browser.path;
    let go = browser.clone();
    let go_options = options.clone();
    let go_finish = finish.clone();
    let entry = text_input(path, config)
        .placeholder("Enter a path")
        .on_enter(move || go.path_entry(&go_options, &go_finish));
    let filename = browser.filename;
    let overwrite = browser.overwrite;
    create_effect(move |_| {
        filename.get();
        overwrite.set(None);
    });
    let add_name = browser.clone();
    let name_options = options.clone();
    let name_finish = finish.clone();
    let save = options.save;
    let name_field = text_input(filename, config)
        .placeholder("Filename")
        .on_enter(move || add_name.submit(&name_options, &name_finish))
        .style(move |s| s.width_full().apply_if(!save, |s| s.hide()));
    let new_folder = browser.new_folder;
    let folder_name = browser.folder_name;
    let create = browser.clone();
    let enter_create = browser.clone();
    let folder_input = text_input(folder_name, config)
        .placeholder("New folder name")
        .on_enter(move || enter_create.create_folder())
        .style(|s| s.flex_grow(1.0_f32).min_width(0));
    let folder_input_id = folder_input.id();
    create_effect(move |_| {
        if new_folder.get() {
            folder_input_id.request_focus();
        }
    });
    let folder_field = stack((
        folder_input,
        button(|| "Create", ButtonKind::Primary, config)
            .on_click_stop(move |_| create.create_folder())
            .style(|s| s.padding_horiz(12).height(27)),
        button(|| "Dismiss", ButtonKind::Plain, config)
            .on_click_stop(move |_| new_folder.set(false)),
    ))
    .style(move |s| {
        s.width_full()
            .items_center()
            .gap(6)
            .apply_if(!new_folder.get(), |s| s.hide())
    });
    let cancel = finish.clone();
    let confirm = finish.clone();
    let submit = browser.clone();
    let closed = finish.clone();
    let escape = finish.clone();
    let status = browser.message;
    stack((
        controls,
        folder_field,
        stack((
            stack((entry, tree)).style(|s| {
                s.flex_col()
                    .flex_grow(0.85_f32)
                    .flex_basis(0)
                    .min_width(0)
                    .min_height(0)
                    .gap(9)
            }),
            Divider::new(config)
                .variant(DividerVariant::Vertical)
                .into_view()
                .style(|s| s.width(1).height_full()),
            right.style(|s| {
                s.flex_grow(1.15_f32)
                    .flex_basis(0)
                    .min_width(0)
                    .min_height(0)
            }),
        ))
        .style(|s| {
            s.width_full()
                .flex_grow(1.0_f32)
                .flex_basis(0)
                .min_height(0)
                .gap(14)
        }),
        name_field,
        stack((
            label(move || {
                if let Some(path) = overwrite.get() {
                    format!(
                        "Replace the existing file {}?",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    )
                } else {
                    status.get()
                }
            })
            .style(move |s| {
                s.flex_grow(1.0_f32)
                    .min_width(0)
                    .color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
            }),
            button(
                move || {
                    if overwrite.get().is_some() {
                        "Replace"
                    } else if save {
                        "Save"
                    } else {
                        "Open"
                    }
                },
                ButtonKind::Primary,
                config,
            )
            .on_click_stop(move |_| submit.submit(&options, &confirm))
            .style(|s| s.padding_horiz(16).height(30)),
            button(|| "Cancel", ButtonKind::Outlined, config)
                .on_click_stop(move |_| cancel(None))
                .style(|s| s.padding_horiz(13).height(30)),
        ))
        .style(|s| s.width_full().items_center().gap(8)),
    ))
    .style(move |s| {
        let config = config.get();
        s.size_full()
            .flex_col()
            .padding(14)
            .gap(10)
            .background(config.color(SparrowColor::PANEL_BACKGROUND))
            .color(config.color(SparrowColor::EDITOR_FOREGROUND))
            .font_size(config.ui.font_size() as f32)
    })
    .on_event_cont(EventListener::WindowClosed, move |_| closed(None))
    .on_event(EventListener::KeyUp, move |event| {
        if let Event::KeyUp(event) = event
            && event.key.logical_key == Key::Named(NamedKey::Escape)
        {
            escape(None);
            return EventPropagation::Stop;
        }
        EventPropagation::Continue
    })
}

fn toolbar(browser: Browser, home: Option<PathBuf>, config: Config) -> impl View {
    let go_home = browser.clone();
    let up = browser.clone();
    let directory = browser.directory;
    let refresh = browser.revision;
    let hidden = browser.hidden;
    let crumbs = browser.clone();
    let desktop = home
        .as_ref()
        .map(|path| path.join("Desktop"))
        .filter(|path| path.is_dir());
    let has_desktop = desktop.is_some();
    let go_desktop = browser.clone();
    let new_folder = browser.new_folder;
    let name = browser.folder_name;
    stack((
        button(|| "Home", ButtonKind::Outlined, config)
            .on_click_stop(move |_| {
                if let Some(path) = &home {
                    go_home.enter(path.clone());
                }
            })
            .style(|s| s.padding_horiz(9).height(26)),
        button(|| "Desktop", ButtonKind::Plain, config)
            .on_click_stop(move |_| {
                if let Some(path) = &desktop {
                    go_desktop.enter(path.clone());
                }
            })
            .style(move |s| s.apply_if(!has_desktop, |s| s.hide())),
        button(|| "New folder", ButtonKind::Plain, config).on_click_stop(
            move |_| {
                name.set(String::new());
                new_folder.set(true);
            },
        ),
        button(|| "Up", ButtonKind::Outlined, config)
            .on_click_stop(move |_| {
                if let Some(parent) = directory.get_untracked().parent() {
                    up.enter(parent.to_path_buf());
                }
            })
            .style(|s| s.padding_horiz(9).height(26)),
        button(|| "Refresh", ButtonKind::Plain, config)
            .on_click_stop(move |_| refresh.update(|count| *count += 1)),
        button(
            move || {
                if hidden.get() {
                    "Hide hidden"
                } else {
                    "Show hidden"
                }
            },
            ButtonKind::Plain,
            config,
        )
        .on_click_stop(move |_| hidden.update(|hidden| *hidden = !*hidden)),
        horizontal_scroll(dyn_container(
            move || directory.get(),
            move |dir| {
                let crumbs = crumbs.clone();
                let parts =
                    dir.ancestors().map(Path::to_path_buf).collect::<Vec<_>>();
                stack_from_iter(parts.into_iter().rev().map(move |path| {
                    let title = path.file_name().map_or_else(
                        || path.display().to_string(),
                        |name| name.to_string_lossy().into_owned(),
                    );
                    let crumbs = crumbs.clone();
                    button(move || title.clone(), ButtonKind::Plain, config)
                        .on_click_stop(move |_| crumbs.enter(path.clone()))
                        .style(|s| s.padding_horiz(6).flex_shrink(0.0_f32))
                }))
                .into_any()
            },
        ))
        .style(|s| s.min_width(0).flex_grow(1.0_f32).height(27)),
    ))
    .style(|s| s.width_full().items_center().gap(8))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chooser_preserves_folder_file_filters_and_save_paths() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("Main.DART");
        let image = root.path().join("reference.webp");
        std::fs::write(&file, "void main() {}").unwrap();
        std::fs::write(&image, []).unwrap();
        let options = FileDialogOptions::new().allowed_types(vec![FileSpec {
            name: "Dart",
            extensions: &["dart"],
        }]);
        assert!(options.accepts(&file));
        assert!(!options.accepts(&image));
        assert!(!options.accepts(root.path()));
        assert!(options.clone().files_and_directories().accepts(root.path()));
        assert!(!options.clone().select_directories().accepts(&file));
        assert_eq!(
            options.save_path(root.path(), "New"),
            Some(root.path().join("New.dart"))
        );
        assert!(
            options
                .save_path(root.path(), "missing/path.dart")
                .is_none()
        );
        assert!(options.save_path(root.path(), "../").is_none());
    }
}
