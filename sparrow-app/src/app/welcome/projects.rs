use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    rc::Rc,
};

use floem::{
    IntoView, View,
    reactive::{RwSignal, SignalGet, SignalUpdate, SignalWith, create_rw_signal},
    views::{Decorators, dyn_container, label, stack, svg},
};
use serde::{Deserialize, Serialize};

use crate::{
    config::color::SparrowColor,
    widgets::{
        ButtonKind, button,
        file_picker::{self, FileDialogOptions},
        popup_menu::{Menu, MenuItem},
        text_input,
    },
};

type Config = crate::widgets::file_picker::Config;

#[derive(Clone, Default, Serialize, Deserialize)]
struct Appearance {
    group: Option<String>,
    icon: Option<String>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct Preferences {
    groups: BTreeSet<String>,
    projects: BTreeMap<String, Appearance>,
}

#[derive(Clone)]
pub(super) struct Projects {
    file: Rc<Option<PathBuf>>,
    preferences: RwSignal<Preferences>,
    editing_group: RwSignal<Option<String>>,
    group_name: RwSignal<String>,
}

impl Projects {
    pub(super) fn new() -> Self {
        let file = sparrow_core::directory::Directory::config_directory()
            .map(|directory| directory.join("recent-project-appearance.json"));
        let preferences = file
            .as_ref()
            .and_then(|file| std::fs::read(file).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self {
            file: Rc::new(file),
            preferences: create_rw_signal(preferences),
            editing_group: create_rw_signal(None),
            group_name: create_rw_signal(String::new()),
        }
    }

    fn change(&self, change: impl FnOnce(&mut Preferences)) {
        let mut next = self.preferences.get_untracked();
        change(&mut next);
        if let Some(path) = self.file.as_ref() {
            let save = serde_json::to_vec_pretty(&next)
                .map_err(std::io::Error::other)
                .and_then(|bytes| {
                    let temporary = path.with_extension("tmp");
                    std::fs::write(&temporary, bytes)?;
                    std::fs::rename(temporary, path)
                });
            if let Err(error) = save {
                tracing::error!(%error, "Could not save recent project appearance");
                return;
            }
        }
        self.preferences.set(next);
    }

    fn create_group(&self) {
        let group = self
            .group_name
            .with_untracked(|name| name.trim().to_owned());
        if group.is_empty() {
            return;
        }
        if let Some(project) = self.editing_group.get_untracked() {
            self.change(|settings| {
                settings.groups.insert(group.clone());
                settings.projects.entry(project).or_default().group = Some(group);
            });
            self.editing_group.set(None);
        }
    }

    pub(super) fn group_form(&self, config: Config) -> impl View + use<> {
        let add = self.clone();
        let submit = self.clone();
        let visible = self.editing_group;
        let input = text_input(self.group_name, config)
            .placeholder("Project group name")
            .on_enter(move || submit.create_group())
            .style(|s| s.flex_grow(1.0_f32).flex_basis(0).min_width(0));
        let id = input.id();
        floem::reactive::create_effect(move |_| {
            if visible.with(Option::is_some) {
                id.request_focus();
            }
        });
        stack((
            input,
            button(|| "Create Group", ButtonKind::Outlined, config)
                .on_click_stop(move |_| add.create_group())
                .style(|s| s.height(28).padding_horiz(8)),
            button(|| "Cancel", ButtonKind::Plain, config)
                .on_click_stop(move |_| visible.set(None))
                .style(|s| s.height(28).padding_horiz(8)),
        ))
        .style(move |s| {
            s.width_full()
                .gap(6)
                .items_center()
                .apply_if(visible.with(Option::is_none), |s| s.hide())
        })
    }

    pub(super) fn group(&self, project: &str) -> Option<String> {
        self.preferences.with(|settings| {
            settings
                .projects
                .get(project)
                .and_then(|appearance| appearance.group.clone())
        })
    }

    pub(super) fn badge(
        &self,
        project: String,
        initials: String,
        config: Config,
    ) -> impl View + use<> {
        let prefs = self.preferences;
        dyn_container(
            move || {
                prefs.with(|settings| {
                    settings
                        .projects
                        .get(&project)
                        .and_then(|appearance| appearance.icon.clone())
                })
            },
            move |icon| {
                if let Some(source) = icon {
                    svg(move || source.clone())
                        .style(|s| s.size(24, 24).set(floem::style::TextColor, None))
                        .into_any()
                } else {
                    let initials = initials.clone();
                    label(move || initials.clone()).into_any()
                }
            },
        )
        .style(move |s| {
            s.size(32, 32)
                .items_center()
                .justify_center()
                .border_radius(8)
                .background(
                    config.get().color(SparrowColor::PANEL_SELECTION_BACKGROUND),
                )
        })
    }

    pub(super) fn actions(
        &self,
        menu: Menu,
        project: String,
        config: Config,
    ) -> Menu {
        let mut groups = Menu::new("Move to Group");
        for name in self
            .preferences
            .with_untracked(|prefs| prefs.groups.clone())
        {
            let data = self.clone();
            let project = project.clone();
            groups = groups.entry(MenuItem::new(name.clone()).action(move || {
                data.change(|prefs| {
                    prefs.projects.entry(project.clone()).or_default().group =
                        Some(name.clone())
                });
            }));
        }
        let data = self.clone();
        let ungroup = project.clone();
        groups = groups.entry(MenuItem::new("Ungrouped").action(move || {
            data.change(|prefs| {
                prefs.projects.entry(ungroup.clone()).or_default().group = None
            });
        }));
        let group_editor = self.clone();
        let group_project = project.clone();
        let icon_editor = self.clone();
        menu.separator()
            .entry(MenuItem::new("New Project Group…").action(move || {
                group_editor.group_name.set(String::new());
                group_editor.editing_group.set(Some(group_project.clone()));
            }))
            .entry(groups)
            .entry(MenuItem::new("Set Custom Project Icon…").action(move || {
                let data = icon_editor.clone();
                let project = project.clone();
                file_picker::open_file(
                    FileDialogOptions::new()
                        .title("Choose an SVG project icon")
                        .allowed_types(vec![floem::file::FileSpec {
                            name: "SVG image",
                            extensions: &["svg"],
                        }]),
                    config,
                    move |file| {
                        use std::io::Read;
                        let Some(path) =
                            file.and_then(|file| file.path.into_iter().next())
                        else {
                            return;
                        };
                        let mut icon = String::new();
                        if std::fs::File::open(path)
                            .and_then(|file| {
                                file.take(128 * 1024 + 1).read_to_string(&mut icon)
                            })
                            .is_ok()
                            && !icon.is_empty()
                            && icon.len() <= 128 * 1024
                        {
                            data.change(|prefs| {
                                prefs
                                    .projects
                                    .entry(project.clone())
                                    .or_default()
                                    .icon = Some(icon)
                            });
                        }
                    },
                );
            }))
    }
}
