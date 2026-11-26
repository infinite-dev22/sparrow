use std::{
    path::{Path, PathBuf},
    rc::Rc,
};

use floem::{
    IntoView, View, ViewId,
    event::{Event, EventListener},
    reactive::{RwSignal, SignalGet, SignalUpdate, SignalWith, create_effect},
    style::CursorStyle,
    views::editor::{core::register::Clipboard, text::SystemClipboard},
    views::{
        Decorators, dyn_container, empty, label, scroll::ScrollClass, stack,
        stack_from_iter, svg,
    },
};

use super::{Browser, Complete, Config, FileDialogOptions};
use crate::{
    config::{color::SparrowColor, icon::SparrowIcons},
    widgets::{
        ButtonKind, TextField, breadcrumb_item, button, clickable_icon,
        dropdown::Dropdown, horizontal_scroll, m3_from_config, tooltip_label,
    },
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    Location,
    Edit,
    Search,
}

#[derive(Clone, Copy)]
enum Direction {
    Back,
    Forward,
}

#[derive(Default)]
pub(super) struct History {
    back: Vec<PathBuf>,
    forward: Vec<PathBuf>,
}

impl History {
    pub(super) fn record(&mut self, current: PathBuf) {
        self.back.push(current);
        self.forward.clear();
    }

    fn has_destination(&self, direction: Direction) -> bool {
        match direction {
            Direction::Back => !self.back.is_empty(),
            Direction::Forward => !self.forward.is_empty(),
        }
    }

    fn step(&mut self, direction: Direction, current: PathBuf) -> Option<PathBuf> {
        let (source, destination) = match direction {
            Direction::Back => (&mut self.back, &mut self.forward),
            Direction::Forward => (&mut self.forward, &mut self.back),
        };
        while let Some(path) = source.pop() {
            if path.is_dir() {
                destination.push(current);
                return Some(path);
            }
        }
        None
    }
}

impl Browser {
    fn step_history(&self, direction: Direction) {
        let current = self.directory.get_untracked();
        if let Some(path) = self
            .history
            .try_update(|history| history.step(direction, current))
            .flatten()
        {
            self.open_directory(path);
        }
    }

    pub(super) fn edit_location(&self) {
        self.query.set(String::new());
        self.path
            .set(self.directory.get_untracked().display().to_string());
        self.mode.set(Mode::Edit);
    }
}

#[derive(Clone)]
struct HeaderAction {
    label: &'static str,
    run: Rc<dyn Fn()>,
}

impl HeaderAction {
    fn new(label: &'static str, run: impl Fn() + 'static) -> Self {
        Self {
            label,
            run: Rc::new(run),
        }
    }
}

fn action_menu(
    icon: &'static str,
    tooltip: &'static str,
    actions: Vec<HeaderAction>,
    config: Config,
) -> impl View {
    let initial = actions[0].clone();
    let height = actions.len() as f64 * 28.0 + 2.0;
    let menu = Dropdown::custom(
        move || initial.clone(),
        move |_| {
            svg(move || config.get().ui_svg(icon))
                .style(move |s| {
                    s.size(16, 16)
                        .color(config.get().color(SparrowColor::SPARROW_ICON_ACTIVE))
                })
                .into_any()
        },
        actions,
        move |action| {
            label(move || action.label)
                .style(move |s| {
                    let config = config.get();
                    s.height(28)
                        .width_full()
                        .padding_horiz(10)
                        .items_center()
                        .hover(|s| {
                            s.background(
                                config.color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                            )
                        })
                        .selected(|s| {
                            s.background(
                                config
                                    .color(SparrowColor::PANEL_SELECTION_BACKGROUND),
                            )
                        })
                })
                .into_any()
        },
    )
    .on_accept(|action| (action.run)())
    .keyboard_navigable()
    .style(move |s| {
        let config = config.get();
        let border = config.color(SparrowColor::SPARROW_BORDER);
        s.size(26, 26)
            .flex_shrink(0.0_f32)
            .items_center()
            .justify_center()
            .border_radius(6)
            .cursor(CursorStyle::Pointer)
            .hover(|s| {
                s.background(config.color(SparrowColor::PANEL_HOVERED_BACKGROUND))
            })
            .focus_visible(|s| {
                s.outline(1)
                    .outline_color(config.color(SparrowColor::EDITOR_CARET))
            })
            .class(ScrollClass, |s| {
                s.size(164, height)
                    .background(
                        config.color(SparrowColor::SPARROW_WINDOW_BACKGROUND),
                    )
                    .color(config.color(SparrowColor::EDITOR_FOREGROUND))
                    .border(1)
                    .border_color(border)
                    .border_radius(6)
            })
    });
    tooltip_label(config, menu, move || tooltip)
}

pub(super) fn header(
    title: String,
    browser: Browser,
    home: Option<PathBuf>,
    project: Option<PathBuf>,
    location: impl View + 'static,
    show_preview: RwSignal<bool>,
    finish: Complete,
    config: Config,
) -> impl View {
    let back = browser.clone();
    let history = browser.history;
    let forward = browser.clone();
    let desktop = directories::UserDirs::new()
        .and_then(|dirs| dirs.desktop_dir().map(Path::to_path_buf))
        .or_else(|| home.as_ref().map(|path| path.join("Desktop")))
        .filter(|path| path.is_dir());
    let go_project = browser.clone();
    let has_project = project.is_some();
    let mut places = Vec::new();
    for (label, destination) in [("Home", home), ("Desktop", desktop)] {
        if let Some(path) = destination {
            let browser = browser.clone();
            places.push(HeaderAction::new(label, move || {
                browser.enter(path.clone())
            }));
        }
    }
    let refresh = browser.revision;
    places.push(HeaderAction::new("Refresh", move || {
        refresh.update(|count| *count += 1)
    }));
    let mode = browser.mode;
    let query = browser.query;
    let hidden = browser.hidden;
    let new_folder = browser.new_folder;
    let name = browser.folder_name;
    stack((
        empty().style(move |s| s.absolute().inset_left(0).inset_top(0).height_full()
            .width_pct(if show_preview.get() { 42.5 } else { 100.0 })
            .pointer_events_none()
            .background(config.get().color(SparrowColor::SPARROW_WINDOW_BACKGROUND))),
        clickable_icon(
            || SparrowIcons::SEARCH,
            move || {
                if mode.get_untracked() == Mode::Search {
                    mode.set(Mode::Location);
                    query.set(String::new());
                } else {
                    mode.set(Mode::Search);
                }
            },
            move || mode.get() == Mode::Search,
            || false,
            || "Search this folder (Ctrl+F)",
            config,
        ),
        label(move || title.clone())
            .style(|s| s.font_bold().max_width(155).min_width(0).height_full().items_center().text_ellipsis().padding_horiz(6))
            .on_event_cont(EventListener::PointerDown, |event| {
                if matches!(event, Event::PointerDown(pointer) if pointer.button.is_primary()) {
                    floem::action::drag_window();
                }
            }),
        action_menu(SparrowIcons::MENU, "Places and folder actions", places, config),
        stack((
            clickable_icon(
                || SparrowIcons::TAB_PREVIOUS,
                move || back.step_history(Direction::Back),
                || false,
                move || !history.with(|history| history.has_destination(Direction::Back)),
                || "Back",
                config,
            ),
            clickable_icon(
                || SparrowIcons::TAB_NEXT,
                move || forward.step_history(Direction::Forward),
                || false,
                move || !history.with(|history| history.has_destination(Direction::Forward)),
                || "Forward",
                config,
            ),
        )).style(|s| s.flex_shrink(0.0_f32)),
        location.style(|s| s.flex_basis(0).flex_grow(1.0_f32).flex_shrink(1.0_f32).min_width(0)),
        clickable_icon(
            || SparrowIcons::DIRECTORY,
            move || {
                if let Some(path) = &project { go_project.enter(path.clone()); }
            },
            || false, move || !has_project, || "Current project folder", config,
        ).style(move |s| s.apply_if(!has_project, |s| s.hide())),
        clickable_icon(
            || SparrowIcons::ADD,
            move || {
                name.set(String::new());
                new_folder.set(true);
            },
            || false,
            || false,
            || "New folder",
            config,
        ),
        clickable_icon(
            || SparrowIcons::FILE_PICKER_HIDDEN,
            move || hidden.update(|show| *show = !*show),
            move || hidden.get(),
            || false,
            move || if hidden.get() { "Hide hidden files" } else { "Show hidden files" },
            config,
        ),
        stack((
            clickable_icon(
                || SparrowIcons::FILE_PICKER_LIST, move || show_preview.set(false),
                move || !show_preview.get(), || false, || "File tree only", config,
            ),
            clickable_icon(
                || SparrowIcons::SIDEBAR_RIGHT, move || show_preview.set(true),
                move || show_preview.get(), || false, || "File tree and preview", config,
            ),
        ))
        .style(move |s| s.flex_shrink(0.0_f32).padding(2).border_radius(7).background(
            config.get().color(SparrowColor::PANEL_HOVERED_BACKGROUND)
        )),
        clickable_icon(
            || SparrowIcons::WINDOW_CLOSE,
            move || finish(None), || false, || false, || "Close picker", config,
        ),
    ))
    .style(move |s| {
        let config = config.get();
        s.width_full().height(46).flex_shrink(0.0_f32).items_center()
            .padding_horiz(10).gap(6).border_bottom(1)
            .border_color(config.color(SparrowColor::SPARROW_BORDER))
            .background(config.color(SparrowColor::PANEL_BACKGROUND))
    })
}

pub(super) fn location(
    browser: Browser,
    options: Rc<FileDialogOptions>,
    finish: Complete,
    tree_id: ViewId,
    home: Option<PathBuf>,
    config: Config,
) -> impl View {
    let mode = browser.mode;
    let edit = browser.clone();
    let go = browser.clone();
    let search = browser.query;
    let input = TextField::new(browser.path)
        .colors(move || m3_from_config(config))
        .placeholder("Enter a path")
        .on_submit(move |_| {
            go.message.set(String::new());
            go.path_entry(&options, &finish);
            if go
                .message
                .try_get_untracked()
                .is_some_and(|message| message.is_empty())
            {
                mode.set(Mode::Location);
                tree_id.request_focus();
            }
        })
        .into_view()
        .style(move |s| s.apply_if(mode.get() != Mode::Edit, |s| s.hide()));
    let input_id = input.id();
    let search_input = TextField::new(search)
        .colors(move || m3_from_config(config))
        .placeholder("Search this folder")
        .on_submit(move |_| tree_id.request_focus())
        .into_view()
        .style(move |s| s.apply_if(mode.get() != Mode::Search, |s| s.hide()));
    let search_id = search_input.id();
    create_effect(move |_| match mode.get() {
        Mode::Edit => input_id.request_focus(),
        Mode::Search => search_id.request_focus(),
        Mode::Location => tree_id.request_focus(),
    });

    let directory = browser.directory;
    let path_editor = browser.clone();
    let crumbs = horizontal_scroll(dyn_container(
        move || directory.get(),
        move |directory| {
            let parts = directory
                .ancestors()
                .map(Path::to_path_buf)
                .collect::<Vec<_>>();
            stack_from_iter(parts.into_iter().rev().enumerate().map(
                |(index, path)| {
                    let current = path == directory;
                    let name = if home.as_ref() == Some(&path) {
                        "Home".into()
                    } else {
                        path.file_name()
                            .unwrap_or(path.as_os_str())
                            .to_string_lossy()
                            .into_owned()
                    };
                    let go = browser.clone();
                    let edit = browser.clone();
                    breadcrumb_item(
                        index == 0,
                        button(move || name.clone(), ButtonKind::Plain, config)
                            .on_click_stop(move |_| {
                                if current {
                                    edit.edit_location();
                                } else {
                                    go.enter(path.clone());
                                }
                            })
                            .style(move |s| {
                                s.height(24)
                                    .padding_horiz(6)
                                    .apply_if(current, |s| s.font_bold())
                            }),
                        config,
                    )
                    .style(|s| s.items_center().flex_shrink(0.0_f32))
                },
            ))
            .into_any()
        },
    ))
    .scroll_to_percent(move || {
        directory.with(|_| ());
        100.0
    })
    .scroll_style(|s| s.hide_bars(true))
    .style(|s| s.max_width_pct(91.0).min_width(0).height(28).items_center());
    let address = stack((
        crumbs,
        empty().style(|s| s.flex_grow(1.0_f32).height_full()),
    ))
    .on_click_stop(move |_| edit.edit_location())
    .style(|s| {
        s.items_center()
            .height_full()
            .flex_grow(1.0_f32)
            .flex_basis(0)
            .min_width(0)
            .cursor(CursorStyle::Text)
    });
    let bar = stack((
        address,
        action_menu(
            SparrowIcons::WINDOW_MORE,
            "Path actions",
            vec![
                HeaderAction::new("Edit path", move || path_editor.edit_location()),
                HeaderAction::new("Copy folder path", move || {
                    directory.with_untracked(|path| {
                        SystemClipboard::new().put_string(path.to_string_lossy());
                    });
                }),
            ],
            config,
        ),
    ))
    .style(move |s| {
        let config = config.get();
        s.items_center()
            .width_full()
            .height(32)
            .padding_horiz(4)
            .border_radius(8)
            .background(config.color(SparrowColor::PANEL_HOVERED_BACKGROUND))
            .apply_if(mode.get() != Mode::Location, |s| s.hide())
    });
    stack((bar, input, search_input))
        .style(|s| s.flex_col().width_full().flex_shrink(0.0_f32))
}

#[cfg(test)]
mod tests {
    use super::{Direction, History};

    #[test]
    fn directory_history_supports_back_forward_and_new_destinations() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first");
        let second = root.path().join("second");
        let third = root.path().join("third");
        for path in [&first, &second, &third] {
            std::fs::create_dir(path).unwrap();
        }

        let mut history = History::default();
        assert!(!history.has_destination(Direction::Back));
        assert!(!history.has_destination(Direction::Forward));
        history.record(first.clone());
        history.record(second.clone());
        assert_eq!(
            history.step(Direction::Back, third.clone()),
            Some(second.clone())
        );
        assert_eq!(
            history.step(Direction::Back, second.clone()),
            Some(first.clone())
        );
        assert_eq!(
            history.step(Direction::Forward, first.clone()),
            Some(second.clone())
        );
        assert!(history.has_destination(Direction::Forward));
        history.record(second.clone());
        assert!(!history.has_destination(Direction::Forward));

        std::fs::remove_dir(second).unwrap();
        assert_eq!(history.step(Direction::Back, third.clone()), Some(first));
        assert_eq!(
            history.step(Direction::Forward, root.path().join("first")),
            Some(third)
        );
    }
}
