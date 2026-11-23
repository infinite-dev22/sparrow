use crate::widgets::popup_menu::{Menu, MenuItem};
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::{
    io::{BufReader, IsTerminal, Read, Write},
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc,
        mpsc::{SyncSender, channel, sync_channel},
    },
};

use crate::debug::{debug_floating_toolbar, run_config_modal};
use crate::title::{branch_switcher, device_selector_overlay, project_switcher};
use crate::{
    about, alert,
    command::{
        CommandKind, InternalCommand, SparrowCommand, SparrowWorkbenchCommand,
    },
    config::{SparrowConfig, color::SparrowColor, watcher::ConfigWatcher},
    db::SparrowDb,
    editor::location::{EditorLocation, EditorPosition},
    listener::Listener,
    status::status,
    title::title,
    tracing::*,
    update::ReleaseInfo,
    window::{TabsInfo, WindowData, WindowInfo},
    window_tab::WindowTabData,
    workspace::{SparrowWorkspace, SparrowWorkspaceType},
};
use anyhow::{Context, Result, anyhow};
use clap::Parser;
use floem::{
    View,
    event::{Event, EventListener, EventPropagation},
    ext_event::{create_ext_action, create_signal_from_channel},
    peniko::kurbo::{Point, Size},
    prelude::SignalTrack,
    reactive::{
        ReadSignal, RwSignal, Scope, SignalGet, SignalUpdate, SignalWith,
        create_effect, create_rw_signal, provide_context, use_context,
    },
    views::{Decorators, drag_resize_window_area, empty, stack, tab},
    window::{ResizeDirection, WindowConfig, WindowId},
};
use notify::Watcher;
use serde::{Deserialize, Serialize};
use sparrow_core::command::MultiSelectionCommand;
use sparrow_core::{
    command::{EditCommand, FocusCommand},
    directory::Directory,
    meta,
    syntax::{Syntax, highlight::reset_highlight_configs},
};
use sparrow_rpc::{
    RpcMessage,
    core::{CoreMessage, CoreNotification},
    file::PathObject,
};
use tracing_subscriber::{filter::Targets, reload::Handle};

mod grammars;
pub(crate) mod logging;
mod log_viewer;
mod window_messages;
mod workbench;
mod workspace_tabs;

use crate::{
    completion::view::completion, editor::signature_help::signature_help,
    hover::view::hover, palette::view::palette, rename::view::rename,
};
use workbench::workbench;
use workspace_tabs::workspace_tab_header;

pub(crate) fn sparrow_window_config() -> WindowConfig {
    WindowConfig::default().application_id("dev.sparrow.sparrow")
}

#[derive(Parser)]
#[clap(name = "Sparrow", about = "Native Dart and Flutter IDE")]
#[clap(version=meta::VERSION)]
#[derive(Debug)]
struct Cli {
    #[clap(long,hide=true)]
    run_config_task: Option<String>,
    /// Launch new window even if sparrow is already running
    #[clap(short, long, action)]
    new: bool,
    /// Don't return instantly when opened in a terminal
    #[clap(short, long, action)]
    wait: bool,

    /// Additional local color/icon theme package directories.
    #[clap(
        long = "theme-path",
        alias = "plugin-path",
        value_name = "THEME_PATH",
        action
    )]
    plugin_path: Vec<PathBuf>,

    /// Paths to file(s) and/or folder(s) to open.
    /// When path is a file (that exists or not),
    /// it accepts `path:line:column` syntax
    /// to specify line and column at which it should open the file
    #[clap(value_parser = sparrow_proxy::cli::parse_file_line_column)]
    #[clap(value_hint = clap::ValueHint::AnyPath)]
    paths: Vec<PathObject>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppInfo {
    pub windows: Vec<WindowInfo>,
}

#[derive(Clone)]
pub enum AppCommand {
    SaveApp,
    NewWindow { folder: Option<PathBuf> },
    CloseWindow(WindowId),
    WindowGotFocus(WindowId),
    WindowClosed(WindowId),
}

#[derive(Clone)]
pub struct AppData {
    pub windows: RwSignal<im::HashMap<WindowId, WindowData>>,
    pub active_window: RwSignal<WindowId>,
    pub window_scale: RwSignal<f64>,
    pub app_command: Listener<AppCommand>,
    pub app_terminated: RwSignal<bool>,
    /// The latest release information
    pub latest_release: RwSignal<Arc<Option<ReleaseInfo>>>,
    pub watcher: Arc<notify::RecommendedWatcher>,
    pub tracing_handle: Handle<Targets>,
    pub config: RwSignal<Arc<SparrowConfig>>,
    /// Paths to extra plugins to load
    pub plugin_paths: Arc<Vec<PathBuf>>,
}

impl AppData {
    pub fn reload_config(&self) {
        let config = SparrowConfig::load(
            &SparrowWorkspace::default(),
            &[],
            &self.plugin_paths,
        );

        self.config.set(Arc::new(config));
        self.window_scale.set(self.config.get().ui.scale());

        let windows = self.windows.get_untracked();
        for (_, window) in windows {
            window.reload_config();
        }
    }

    pub fn active_window_tab(&self) -> Option<Rc<WindowTabData>> {
        if let Some(window) = self.active_window() {
            return window.active_window_tab();
        }
        None
    }

    fn active_window(&self) -> Option<WindowData> {
        let windows = self.windows.get_untracked();
        let active_window = self.active_window.get_untracked();
        windows
            .get(&active_window)
            .cloned()
            .or_else(|| windows.iter().next().map(|(_, window)| window.clone()))
    }

    fn default_window_config(&self) -> WindowConfig {
        sparrow_window_config()
            .apply_default_theme(false)
            .title("Sparrow")
    }

    pub fn new_window(&self, folder: Option<PathBuf>) {
        let config = self
            .active_window()
            .map(|window| {
                self.default_window_config()
                    .size(window.common.size.get_untracked())
                    .position(window.position.get_untracked() + (50.0, 50.0))
            })
            .or_else(|| {
                let db: Arc<SparrowDb> = use_context().unwrap();
                db.get_window().ok().map(|info| {
                    self.default_window_config()
                        .size(info.size)
                        .position(info.pos)
                })
            })
            .unwrap_or_else(|| {
                self.default_window_config().size(Size::new(800.0, 600.0))
            });
        let config = if cfg!(target_os = "macos")
            || self.config.get_untracked().core.custom_titlebar
        {
            config.show_titlebar(false)
        } else {
            config
        };
        let workspace = SparrowWorkspace {
            path: folder,
            ..Default::default()
        };
        let app_data = self.clone();
        floem::new_window(
            move |window_id| {
                app_data.app_view(
                    window_id,
                    WindowInfo {
                        size: Size::ZERO,
                        pos: Point::ZERO,
                        maximised: false,
                        tabs: TabsInfo {
                            active_tab: 0,
                            workspaces: vec![workspace],
                        },
                    },
                    vec![],
                )
            },
            Some(config),
        );
    }

    pub fn run_app_command(&self, cmd: AppCommand) {
        match cmd {
            AppCommand::SaveApp => {
                let db: Arc<SparrowDb> = use_context().unwrap();
                if let Err(err) = db.save_app(self) {
                    tracing::error!("{:?}", err);
                }
            }
            AppCommand::WindowClosed(window_id) => {
                if self.app_terminated.get_untracked() {
                    return;
                }
                let db: Arc<SparrowDb> = use_context().unwrap();
                if self.windows.with_untracked(|w| w.len()) == 1 {
                    if let Err(err) = db.insert_app(self.clone()) {
                        tracing::error!("{:?}", err);
                    }
                }
                let window_data = self
                    .windows
                    .try_update(|windows| windows.remove(&window_id))
                    .unwrap();
                if let Some(window_data) = window_data {
                    window_data.scope.dispose();
                }
                if let Err(err) = db.save_app(self) {
                    tracing::error!("{:?}", err);
                }
            }
            AppCommand::CloseWindow(window_id) => {
                floem::close_window(window_id);
            }
            AppCommand::NewWindow { folder } => {
                self.new_window(folder);
            }
            AppCommand::WindowGotFocus(window_id) => {
                self.active_window.set(window_id);
            }
        }
    }

    fn create_windows(
        &self,
        db: Arc<SparrowDb>,
        paths: Vec<PathObject>,
    ) -> floem::Application {
        let mut app = floem::Application::new();

        let mut initial_windows = 0;

        // Split user input into known existing directors and
        // file paths that exist or not
        let (dirs, files): (Vec<&PathObject>, Vec<&PathObject>) =
            paths.iter().partition(|p| p.is_dir);

        let files: Vec<PathObject> = files.into_iter().cloned().collect();
        let mut files = if files.is_empty() { None } else { Some(files) };

        if !dirs.is_empty() {
            // There were directories specified, so we'll load those as windows

            // Use the last opened window's size and position as the default
            let (size, mut pos) = db
                .get_window()
                .map(|i| (i.size, i.pos))
                .unwrap_or_else(|_| (Size::new(800.0, 600.0), Point::new(0.0, 0.0)));

            for dir in dirs {
                #[cfg(windows)]
                let workspace_type = if !std::env::var("WSL_DISTRO_NAME")
                    .unwrap_or_default()
                    .is_empty()
                    || !std::env::var("WSL_INTEROP").unwrap_or_default().is_empty()
                {
                    SparrowWorkspaceType::RemoteWSL(crate::workspace::WslHost {
                        host: String::new(),
                    })
                } else {
                    SparrowWorkspaceType::Local
                };
                #[cfg(not(windows))]
                let workspace_type = SparrowWorkspaceType::Local;

                let info = WindowInfo {
                    size,
                    pos,
                    maximised: false,
                    tabs: TabsInfo {
                        active_tab: 0,
                        workspaces: vec![SparrowWorkspace {
                            kind: workspace_type,
                            path: Some(dir.path.to_owned()),
                            last_open: 0,
                        }],
                    },
                };

                pos += (50.0, 50.0);

                let config = self
                    .default_window_config()
                    .size(info.size)
                    .position(info.pos);
                let config = if cfg!(target_os = "macos")
                    || self.config.get_untracked().core.custom_titlebar
                {
                    config.show_titlebar(false)
                } else {
                    config
                };
                let app_data = self.clone();
                let files = files.take().unwrap_or_default();
                app = app.window(
                    move |window_id| app_data.app_view(window_id, info, files),
                    Some(config),
                );
                initial_windows += 1;
            }
        } else if files.is_none() {
            // There were no dirs and no files specified, so we'll load the last windows
            match db.get_app() {
                Ok(app_info) => {
                    for info in app_info.windows {
                        let config = self
                            .default_window_config()
                            .size(info.size)
                            .position(info.pos);
                        let config = if cfg!(target_os = "macos")
                            || self.config.get_untracked().core.custom_titlebar
                        {
                            config.show_titlebar(false)
                        } else {
                            config
                        };
                        let app_data = self.clone();
                        app = app.window(
                            move |window_id| {
                                app_data.app_view(window_id, info, vec![])
                            },
                            Some(config),
                        );
                        initial_windows += 1;
                    }
                }
                Err(err) => {
                    tracing::error!("{:?}", err);
                }
            }
        }

        if initial_windows == 0 {
            let mut info = db.get_window().unwrap_or_else(|_| WindowInfo {
                size: Size::new(800.0, 600.0),
                pos: Point::ZERO,
                maximised: false,
                tabs: TabsInfo {
                    active_tab: 0,
                    workspaces: vec![SparrowWorkspace::default()],
                },
            });
            info.tabs = TabsInfo {
                active_tab: 0,
                workspaces: vec![SparrowWorkspace::default()],
            };
            let config = self
                .default_window_config()
                .size(info.size)
                .position(info.pos);
            let config = if cfg!(target_os = "macos")
                || self.config.get_untracked().core.custom_titlebar
            {
                config.show_titlebar(false)
            } else {
                config
            };
            let app_data = self.clone();
            app = app.window(
                move |window_id| {
                    app_data.app_view(
                        window_id,
                        info,
                        files.take().unwrap_or_default(),
                    )
                },
                Some(config),
            );
        }

        app
    }

    fn app_view(
        &self,
        window_id: WindowId,
        info: WindowInfo,
        files: Vec<PathObject>,
    ) -> impl View + use<> {
        let app_view_id = create_rw_signal(floem::ViewId::new());
        let window_data = WindowData::new(
            window_id,
            app_view_id,
            info,
            self.window_scale,
            self.latest_release.read_only(),
            self.plugin_paths.clone(),
            self.app_command,
        );

        {
            let cur_window_tab = window_data.active.get_untracked();
            let (_, window_tab) =
                &window_data.window_tabs.get_untracked()[cur_window_tab];
            for file in files {
                let position = file.linecol.map(|pos| {
                    EditorPosition::Position(lsp_types::Position {
                        line: pos.line.saturating_sub(1) as u32,
                        character: pos.column.saturating_sub(1) as u32,
                    })
                });

                window_tab.run_internal_command(InternalCommand::GoToLocation {
                    location: EditorLocation {
                        path: file.path.clone(),
                        position,
                        scroll_offset: None,
                        // Create a new editor for the file, so we don't change any current unconfirmed
                        // editor
                        ignore_unconfirmed: true,
                        same_editor_tab: false,
                    },
                });
            }
        }

        self.windows.update(|windows| {
            windows.insert(window_id, window_data.clone());
        });
        let window_size = window_data.common.size;
        let position = window_data.position;
        let window_scale = window_data.window_scale;
        let app_command = window_data.app_command;
        let config = window_data.config;
        // The KeyDown and PointerDown event handlers both need ownership of a WindowData object.
        let key_down_window_data = window_data.clone();
        let key_up_window_data = window_data.clone();
        let view = stack((
            workspace_tab_header(window_data.clone()),
            window(window_data.clone()),
            stack((
                drag_resize_window_area(ResizeDirection::West, empty()).style(|s| {
                    s.absolute().width(4.0).height_full().pointer_events_auto()
                }),
                drag_resize_window_area(ResizeDirection::North, empty()).style(
                    |s| s.absolute().width_full().height(4.0).pointer_events_auto(),
                ),
                drag_resize_window_area(ResizeDirection::East, empty()).style(
                    move |s| {
                        s.absolute()
                            .margin_left(window_size.get().width as f32 - 4.0)
                            .width(4.0)
                            .height_full()
                            .pointer_events_auto()
                    },
                ),
                drag_resize_window_area(ResizeDirection::South, empty()).style(
                    move |s| {
                        s.absolute()
                            .margin_top(window_size.get().height as f32 - 4.0)
                            .width_full()
                            .height(4.0)
                            .pointer_events_auto()
                    },
                ),
                drag_resize_window_area(ResizeDirection::NorthWest, empty()).style(
                    |s| s.absolute().width(20.0).height(4.0).pointer_events_auto(),
                ),
                drag_resize_window_area(ResizeDirection::NorthWest, empty()).style(
                    |s| s.absolute().width(4.0).height(20.0).pointer_events_auto(),
                ),
                drag_resize_window_area(ResizeDirection::NorthEast, empty()).style(
                    move |s| {
                        s.absolute()
                            .margin_left(window_size.get().width as f32 - 20.0)
                            .width(20.0)
                            .height(4.0)
                            .pointer_events_auto()
                    },
                ),
                drag_resize_window_area(ResizeDirection::NorthEast, empty()).style(
                    move |s| {
                        s.absolute()
                            .margin_left(window_size.get().width as f32 - 4.0)
                            .width(4.0)
                            .height(20.0)
                            .pointer_events_auto()
                    },
                ),
                drag_resize_window_area(ResizeDirection::SouthWest, empty()).style(
                    move |s| {
                        s.absolute()
                            .margin_top(window_size.get().height as f32 - 4.0)
                            .width(20.0)
                            .height(4.0)
                            .pointer_events_auto()
                    },
                ),
                drag_resize_window_area(ResizeDirection::SouthWest, empty()).style(
                    move |s| {
                        s.absolute()
                            .margin_top(window_size.get().height as f32 - 20.0)
                            .width(4.0)
                            .height(20.0)
                            .pointer_events_auto()
                    },
                ),
                drag_resize_window_area(ResizeDirection::SouthEast, empty()).style(
                    move |s| {
                        s.absolute()
                            .margin_left(window_size.get().width as f32 - 20.0)
                            .margin_top(window_size.get().height as f32 - 4.0)
                            .width(20.0)
                            .height(4.0)
                            .pointer_events_auto()
                    },
                ),
                drag_resize_window_area(ResizeDirection::SouthEast, empty()).style(
                    move |s| {
                        s.absolute()
                            .margin_left(window_size.get().width as f32 - 4.0)
                            .margin_top(window_size.get().height as f32 - 20.0)
                            .width(4.0)
                            .height(20.0)
                            .pointer_events_auto()
                    },
                ),
            ))
            .debug_name("Drag Resize Areas")
            .style(move |s| {
                s.absolute()
                    .size_full()
                    .apply_if(
                        cfg!(target_os = "macos")
                            || !config.get_untracked().core.custom_titlebar,
                        |s| s.hide(),
                    )
                    .pointer_events_none()
            }),
        ))
        .style(|s| s.flex_col().size_full());
        let view_id = view.id();
        app_view_id.set(view_id);

        view_id.request_focus();

        view.window_scale(move || window_scale.get())
            .keyboard_navigable()
            .on_event(EventListener::KeyUp, move |event| {
                // Floem sends keys directly to the focused field, without
                // bubbling to its modal. Escape clears text-input focus on
                // key-down; dismiss the topmost configuration modal on key-up.
                if let Event::KeyUp(key) = event {
                    if key.key.logical_key
                        == floem::keyboard::Key::Named(
                            floem::keyboard::NamedKey::Escape,
                        )
                    {
                        if crate::widgets::modal_focus::consume_popup_escape()
                            || crate::widgets::modal_focus::external_dialog_active()
                        {
                            return EventPropagation::Stop;
                        }
                        let active = key_up_window_data.active.get_untracked();
                        if let Some((_, tab)) = key_up_window_data
                            .window_tabs
                            .with_untracked(|tabs| tabs.get(active).cloned())
                        {
                            let modal = if tab.log_viewer_visible.get_untracked() {
                                Some(tab.log_viewer_visible)
                            } else if tab.settings_modal_visible.get_untracked()
                            {
                                if !tab.settings_search_query.with_untracked(String::is_empty) {
                                    tab.settings_search_query.set(String::new());
                                    if let Some(input) = tab.settings_search_input.get_untracked() {
                                        input.request_focus();
                                    }
                                    return EventPropagation::Stop;
                                }
                                Some(tab.settings_modal_visible)
                            } else if tab.run_config_modal_visible.get_untracked() {
                                Some(tab.run_config_modal_visible)
                            } else if tab.flutter_project.active.get_untracked()
                                && !tab.flutter_project.is_busy()
                            {
                                Some(tab.flutter_project.active)
                            } else if tab.file_refactor.active.get_untracked()&&!tab.file_refactor.is_busy(){Some(tab.file_refactor.active)
                            } else if tab.scaffold.active.get_untracked(){Some(tab.scaffold.active)
                            } else if tab.formatting.active.get_untracked()&&!tab.formatting.is_busy(){Some(tab.formatting.active)
                            } else if tab.search_everywhere.active.get_untracked() {
                                Some(tab.search_everywhere.active)
                            } else if tab.project_search.active.get_untracked()&&!tab.project_search.is_busy(){Some(tab.project_search.active)
                            } else if tab.git_actions.active.get_untracked()&&!tab.git_actions.is_busy(){Some(tab.git_actions.active)
                            } else {
                                None
                            };
                            if modal.is_none()&&tab.git_actions.active.get_untracked(){tab.git_actions.cancel_command();return EventPropagation::Stop;}
                            if let Some(modal) = modal {
                                modal.set(false);
                                view_id.request_focus();
                                return EventPropagation::Stop;
                            }
                        }
                    }
                }
                EventPropagation::Continue
            })
            .on_event(EventListener::KeyDown, move |event| {
                if let Event::KeyDown(key_event) = event
                    && let Some(tab) = key_down_window_data.active_window_tab()
                    && tab.key_down(key_event)
                {
                    // The commit message editor uses Sparrow's keymaps while
                    // retaining Floem focus for Tab navigation to its actions.
                    if tab.common.focus.get_untracked()
                        != crate::window_tab::Focus::Panel(
                            crate::panel::kind::PanelKind::SourceControl,
                        )
                    {
                        view_id.request_focus();
                    }
                    EventPropagation::Stop
                } else {
                    // Let Floem handle unbound keys, including focus navigation.
                    EventPropagation::Continue
                }
            })
            .on_event(EventListener::PointerDown, {
                let window_data = window_data.clone();
                move |event| {
                    if let Event::PointerDown(pointer_event) = event {
                        window_data.key_down(pointer_event);
                        EventPropagation::Stop
                    } else {
                        EventPropagation::Continue
                    }
                }
            })
            .on_event_stop(EventListener::WindowLostFocus, move |_| { crate::ui_test_guard::focus(false); })
            .on_event_stop(EventListener::WindowResized, move |event| {
                if let Event::WindowResized(size) = event {
                    window_size.set(*size);
                    crate::ui_test_guard::size(size.width, size.height);
                }
            })
            .on_event_stop(EventListener::WindowMoved, move |event| {
                if let Event::WindowMoved(point) = event {
                    position.set(*point);
                }
            })
            .on_event_stop(EventListener::WindowGotFocus, move |_| {
                crate::ui_test_guard::focus(true);
                app_command.send(AppCommand::WindowGotFocus(window_id));
            })
            .on_event_stop(EventListener::WindowClosed, move |_| {
                app_command.send(AppCommand::WindowClosed(window_id));
            })
            .on_event_stop(EventListener::DroppedFile, move |event: &Event| {
                if let Event::DroppedFile(file) = event {
                    if file.path.is_dir() {
                        app_command.send(AppCommand::NewWindow {
                            folder: Some(file.path.clone()),
                        });
                    } else if let Some(win_tab_data) =
                        window_data.active_window_tab()
                    {
                        win_tab_data.common.internal_command.send(
                            InternalCommand::GoToLocation {
                                location: EditorLocation {
                                    path: file.path.clone(),
                                    position: None,
                                    scroll_offset: None,
                                    ignore_unconfirmed: false,
                                    same_editor_tab: false,
                                },
                            },
                        )
                    }
                }
            })
            .debug_name("App View")
    }
}

pub use crate::widgets::{
    clickable_icon, clickable_icon_base, not_clickable_icon, tooltip_label,
};

fn window_tab(
    window_tab_data: Rc<WindowTabData>,
    window_tabs: ReadSignal<im::Vector<(RwSignal<usize>, Rc<WindowTabData>)>>,
) -> impl View {
    let source_control = window_tab_data.source_control.clone();
    let window_origin = window_tab_data.common.window_origin;
    let layout_rect = window_tab_data.layout_rect;
    let config = window_tab_data.common.config;
    let workbench_command = window_tab_data.common.workbench_command;
    let inspections = window_tab_data.inspections;
    let window_tab_scope = window_tab_data.scope;
    let hover_active = window_tab_data.common.hover.active;
    let title_menus = [
        window_tab_data.project_menu_visible,
        window_tab_data.branch_menu_visible,
        window_tab_data.device_menu_visible,
    ];
    let status_height = window_tab_data.status_height;
    let project_chip_origin = create_rw_signal(Point::ZERO);
    let branch_chip_origin = create_rw_signal(Point::ZERO);
    let device_chip_origin = create_rw_signal(Point::ZERO);
    let settings_modal = window_tab_data.settings_modal_visible;
    let log_viewer = window_tab_data.log_viewer_visible;
    let run_modal = window_tab_data.run_config_modal_visible;
    let project_modal = window_tab_data.flutter_project.active;
    let file_modal=window_tab_data.file_refactor.active;
    let scaffold_modal=window_tab_data.scaffold.active;
    let formatting_modal=window_tab_data.formatting.active;
    let search_modal=window_tab_data.project_search.active;
    let everywhere_modal = window_tab_data.search_everywhere.active;
    let git_modal=window_tab_data.git_actions.active;
    let symbol_modal=window_tab_data.rename.active;
    let interaction_layer_open=move ||symbol_modal.get()||file_modal.get()||scaffold_modal.get()||formatting_modal.get()||search_modal.get()||everywhere_modal.get()||git_modal.get();
    let modal_open =
        move || log_viewer.get() || settings_modal.get() || run_modal.get() || project_modal.get() || file_modal.get() || scaffold_modal.get() || formatting_modal.get() || search_modal.get() || everywhere_modal.get() || git_modal.get();

    let project_color = window_tab_data.common.project_color;
    let background_tasks_open = window_tab_data.common.background_tasks.open;
    let about_open = window_tab_data.about_data.visible;
    let alert_open = window_tab_data.alert_data.active;
    let view = stack((
        stack((
            crate::project_appearance::glow_view(config, project_color),
            title(
                window_tab_data.clone(),
                window_tabs,
                project_chip_origin,
                branch_chip_origin,
                device_chip_origin,
            ),
            workbench(window_tab_data.clone()),
            status(
                window_tab_data.clone(),
                source_control,
                workbench_command,
                status_height,
                config,
            ),
        ))
        .on_resize(move |rect| {
            layout_rect.set(rect);
        })
        .on_move(move |point| {
            window_origin.set(point);
        })
        .style(move |s| {
            s.size_full().flex_col().background(
                config.get().color(SparrowColor::SPARROW_WINDOW_BACKGROUND),
            )
        })
        .disabled(modal_open)
        .debug_name("Base Layer"),
        completion(window_tab_data.clone()).disabled(modal_open),
        signature_help(window_tab_data.clone()).disabled(modal_open),
        hover(window_tab_data.clone()).disabled(modal_open),
        crate::code_action_preview::view(window_tab_data.clone()).disabled(modal_open),
        stack((rename(window_tab_data.clone()).disabled(modal_open),crate::file_refactor::view(window_tab_data.clone()),crate::scaffold::view(window_tab_data.clone()),crate::formatting::view(window_tab_data.clone()),crate::project_search::view(window_tab_data.clone()),crate::search_everywhere::view(window_tab_data.clone()),crate::git_actions::view(window_tab_data.clone()))).style(move |s|s.absolute().size_full().apply_if(!interaction_layer_open(),|s|s.hide())),

        palette(window_tab_data.clone()).disabled(modal_open),
        stack((
            crate::background_tasks::panel(window_tab_data.clone()).disabled(modal_open),
            about::about_popup(window_tab_data.clone()),
            alert::alert_box(window_tab_data.alert_data.clone()),
        )).style(move |s| s.absolute().size_full().apply_if(
            !background_tasks_open.get() && !about_open.get() && !alert_open.get(),
            |s| s.hide(),
        )),
        crate::dart::project_wizard::view(window_tab_data.clone()),
        run_config_modal(window_tab_data.clone()),
        crate::settings::modal::view(window_tab_data.clone()),
        log_viewer::view(window_tab_data.clone()),
        debug_floating_toolbar(window_tab_data.clone()).disabled(modal_open),
        project_switcher(window_tab_data.clone(), window_tabs, project_chip_origin),
        branch_switcher(window_tab_data.clone(), branch_chip_origin),
        device_selector_overlay(window_tab_data.clone(), device_chip_origin),
    ))
    .on_cleanup(move || {
        inspections.cancel();
        window_tab_scope.dispose();
    })
    .on_event_cont(EventListener::PointerMove, move |event| {
        if let Event::PointerMove(pointer) = event {
            crate::widgets::popup_menu::pointer(pointer.pos);
        }
        if hover_active.get_untracked() {
            hover_active.set(false);
        }
    })
    .style(move |s| {
        let config = config.get();
        s.size_full()
            .color(config.color(SparrowColor::EDITOR_FOREGROUND))
            .background(config.color(SparrowColor::EDITOR_BACKGROUND))
            .font_size(config.ui.font_size() as f32)
            .line_height(1.4)
            .apply_if(!config.ui.font_family.is_empty(), |s| {
                s.font_family(config.ui.font_family.clone())
            })
            .class(floem::views::scroll::Handle, |s| {
                s.background(config.color(SparrowColor::SPARROW_SCROLL_BAR))
            })
    })
    .on_event_cont(EventListener::WindowLostFocus, {
        let view_id = window_tab_data.common.view_id;
        move |_| {
            crate::widgets::tooltip::dismiss_window(view_id.get_untracked());
            for menu in title_menus {
                menu.set(false);
            }
        }
    })
    .debug_name("Window Tab");

    let view_id = view.id();
    window_tab_data.common.view_id.set(view_id);
    view
}

fn window(window_data: WindowData) -> impl View {
    let window_tabs = window_data.window_tabs.read_only();
    let active = window_data.active.read_only();
    let items = move || window_tabs.get();
    let key = |(_, window_tab): &(RwSignal<usize>, Rc<WindowTabData>)| {
        window_tab.window_tab_id
    };
    let active = move || active.get();
    let window_focus = create_rw_signal(false);
    let ime_enabled = window_data.ime_enabled;
    let window_maximized = window_data.common.window_maximized;

    tab(active, items, key, move |(_, window_tab_data)| {
        window_tab(window_tab_data, window_tabs)
    })
    .window_title(move || {
        let active = active();
        let window_tabs = window_tabs.get();
        let workspace = window_tabs
            .get(active)
            .or_else(|| window_tabs.last())
            .and_then(|(_, window_tab)| window_tab.workspace.display());
        match workspace {
            Some(workspace) => format!("{workspace} - Sparrow"),
            None => "Sparrow".to_string(),
        }
    })
    .on_event_stop(EventListener::ImeEnabled, move |_| {
        ime_enabled.set(true);
    })
    .on_event_stop(EventListener::ImeDisabled, move |_| {
        ime_enabled.set(false);
    })
    .on_event_cont(EventListener::WindowGotFocus, move |_| {
        window_focus.set(true);
    })
    .on_event_cont(EventListener::WindowMaximizeChanged, move |event| {
        if let Event::WindowMaximizeChanged(maximized) = event {
            window_maximized.set(*maximized);
        }
    })
    .window_menu(move || {
        window_focus.track();
        let active = active();
        let window_tabs = window_tabs.get();
        let window_tab = window_tabs.get(active).or_else(|| window_tabs.last());
        if let Some((_, window_tab)) = window_tab {
            window_tab.common.keypress.track();
            let workbench_command = window_tab.common.workbench_command;
            let sparrow_command = window_tab.common.sparrow_command;
            window_menu(
                sparrow_command,
                workbench_command,
                crate::menus::MenuContext::from_window(window_tab),
            )
            .into_native()
        } else {
            Menu::new("Sparrow").into_native()
        }
    })
    .style(|s| s.size_full())
    .debug_name("Window")
}

pub fn launch() {
    let cli = Cli::parse();
    if let Some(json)=cli.run_config_task.as_deref(){let code=match crate::dart::run_config::execute_task_snapshot(json){Ok(code)=>code,Err(error)=>{eprintln!("{error}");1}};std::process::exit(code);}

    let (reload_handle, _guard) = logging::logging();
    if !cli.wait {
        logging::panic_hook();
    }

    tracing::info!(version = sparrow_core::meta::VERSION, process = std::process::id(), "Starting Sparrow");

    #[cfg(feature = "vendored-fonts")]
    {
        use floem::text::{FONT_SYSTEM, fontdb::Source};

        for bytes in [
            include_bytes!("../../extra/fonts/CupertinoIcons/CupertinoIcons.ttf").as_slice(),
            include_bytes!(
                "../../extra/fonts/MaterialIcons/MaterialIcons-Regular.otf"
            )
            .as_slice(),
            include_bytes!("../../extra/fonts/Inter/Inter-Regular.otf").as_slice(),
            include_bytes!("../../extra/fonts/Inter/Inter-SemiBold.otf").as_slice(),
            include_bytes!(
                "../../extra/fonts/JetBrainsMono/JetBrainsMono-Regular.ttf"
            )
            .as_slice(),
            include_bytes!(
                "../../extra/fonts/JetBrainsMono/JetBrainsMono-Italic.ttf"
            )
            .as_slice(),
            include_bytes!("../../extra/fonts/JetBrainsMono/JetBrainsMono-Medium.ttf")
                .as_slice(),
            include_bytes!("../../extra/fonts/JetBrainsMono/JetBrainsMono-MediumItalic.ttf")
                .as_slice(),
            include_bytes!("../../extra/fonts/JetBrainsMono/JetBrainsMono-Bold.ttf")
                .as_slice(),
            include_bytes!("../../extra/fonts/JetBrainsMono/JetBrainsMono-BoldItalic.ttf")
                .as_slice(),
        ] {
            FONT_SYSTEM
                .lock()
                .db_mut()
                .load_font_source(Source::Binary(Arc::new(bytes)));
        }

        const FONT_DEJAVU_SANS_REGULAR: &[u8] = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../extra/fonts/DejaVu/DejaVuSans.ttf"
        ));
        const FONT_DEJAVU_SANS_MONO_REGULAR: &[u8] = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../extra/fonts/DejaVu/DejaVuSansMono.ttf"
        ));

        FONT_SYSTEM
            .lock()
            .db_mut()
            .load_font_source(Source::Binary(Arc::new(FONT_DEJAVU_SANS_REGULAR)));
        FONT_SYSTEM
            .lock()
            .db_mut()
            .load_font_source(Source::Binary(Arc::new(
                FONT_DEJAVU_SANS_MONO_REGULAR,
            )));
    }

    let stdin = std::io::stdin();
    if !stdin.is_terminal() {
        trace!(TraceLevel::INFO, "Loading custom environment from shell");
        load_shell_env();
    }

    // small hack to unblock terminal if launched from it
    // launch it as a separate process that waits
    // if !cli.wait {
    //     let mut args = std::env::args().collect::<Vec<_>>();
    //     args.push("--wait".to_string());
    //     let mut cmd = std::process::Command::new(&args[0]);
    //     #[cfg(target_os = "windows")]
    //     cmd.creation_flags(windows::Win32::System::Threading::CREATE_NO_WINDOW);
    //
    //     let stderr_file_path =
    //         Directory::logs_directory().unwrap().join("stderr.log");
    //     let stderr_file = std::fs::OpenOptions::new()
    //         .write(true)
    //         .truncate(true)
    //         .create(true)
    //         .read(true)
    //         .open(stderr_file_path)
    //         .unwrap();
    //     let stderr = Stdio::from(stderr_file);
    //
    //     let stdout_file_path =
    //         Directory::logs_directory().unwrap().join("stdout.log");
    //     let stdout_file = std::fs::OpenOptions::new()
    //         .write(true)
    //         .truncate(true)
    //         .create(true)
    //         .read(true)
    //         .open(stdout_file_path)
    //         .unwrap();
    //     let stdout = Stdio::from(stdout_file);
    //
    //     if let Err(why) = cmd
    //         .args(&args[1..])
    //         .stderr(stderr)
    //         .stdout(stdout)
    //         .env("SPARROW_LOG", "sparrow_app::app=error,off")
    //         .spawn()
    //     {
    //         eprintln!("Failed to launch Sparrow: {why}");
    //         std::process::exit(1);
    //     };
    //     return;
    // }

    // If the cli is not requesting a new window, and we're not developing a plugin, we try to open
    // in the existing Sparrow process
    if !cli.new {
        match get_socket() {
            Ok(socket) => {
                if let Err(e) = try_open_in_existing_process(socket, &cli.paths) {
                    trace!(TraceLevel::ERROR, "failed to open path(s): {e}");
                };
                return;
            }
            Err(err) => {
                // Cold start on a fresh install, or a stale socket left
                // behind by a previous crash. Both mean "start a new
                // instance" — not an error. A stale `.sock` file is
                // cleaned up here so the next startup is quiet.
                let kind = err.downcast_ref::<std::io::Error>().map(|e| e.kind());
                match kind {
                    Some(
                        std::io::ErrorKind::NotFound
                        | std::io::ErrorKind::ConnectionRefused,
                    ) => {
                        trace!(
                            TraceLevel::TRACE,
                            "no existing Sparrow instance ({err:#})"
                        );
                        if let Some(path) = Directory::local_socket() {
                            if path.exists() {
                                if let Err(rm) = std::fs::remove_file(&path) {
                                    tracing::warn!(
                                        "could not remove stale socket {path:?}: {rm}"
                                    );
                                }
                            }
                        }
                    }
                    _ => {
                        // Genuinely unexpected — permission denied, disk
                        // full, filesystem error. Worth logging loudly.
                        tracing::error!("{err:?}");
                    }
                }
            }
        }
    }

    #[cfg(feature = "updater")]
    crate::update::cleanup();

    if let Err(err) = sparrow_proxy::register_sparrow_path() {
        tracing::error!("{:?}", err);
    }
    let db = match SparrowDb::new() {
        Ok(db) => Arc::new(db),
        Err(e) => {
            #[cfg(windows)]
            logging::error_modal(
                "Error",
                &format!("Failed to create SparrowDb: {e}"),
            );

            trace!(TraceLevel::ERROR, "Failed to create SparrowDb: {e}");
            std::process::exit(1);
        }
    };
    let scope = Scope::new();
    provide_context(db.clone());

    let window_scale = scope.create_rw_signal(1.0);
    let latest_release = scope.create_rw_signal(Arc::new(None));
    let app_command = Listener::new_empty(scope);

    let plugin_paths = Arc::new(cli.plugin_path);

    let (tx, rx) = channel();
    let mut watcher = notify::recommended_watcher(ConfigWatcher::new(tx))
        .context("Failed to spawn file watcher")
        .unwrap();
    if let Some(path) = SparrowConfig::settings_file() {
        if let Err(err) = watcher.watch(&path, notify::RecursiveMode::Recursive) {
            tracing::error!("{:?}", err);
        }
    }
    if let Some(path) = Directory::themes_directory() {
        if let Err(err) = watcher.watch(&path, notify::RecursiveMode::Recursive) {
            tracing::error!("{:?}", err);
        }
    }
    if let Some(path) = SparrowConfig::keymaps_file() {
        if let Err(err) = watcher.watch(&path, notify::RecursiveMode::Recursive) {
            tracing::error!("{:?}", err);
        }
    }
    if let Some(path) = Directory::plugins_directory() {
        if let Err(err) = watcher.watch(&path, notify::RecursiveMode::Recursive) {
            tracing::error!("{:?}", err);
        }
    }

    let windows = scope.create_rw_signal(im::HashMap::new());
    let config =
        SparrowConfig::load(&SparrowWorkspace::default(), &[], &plugin_paths);

    // Restore scale from config
    window_scale.set(config.ui.scale());

    let config = scope.create_rw_signal(Arc::new(config));
    let app_data = AppData {
        windows,
        active_window: scope.create_rw_signal(WindowId::from_raw(0)),
        window_scale,
        app_terminated: scope.create_rw_signal(false),
        watcher: Arc::new(watcher),
        latest_release,
        app_command,
        tracing_handle: reload_handle,
        config,
        plugin_paths,
    };

    let app = app_data.create_windows(db.clone(), cli.paths);

    {
        let app_data = app_data.clone();
        let notification = create_signal_from_channel(rx);
        create_effect(move |_| {
            if notification.get().is_some() {
                tracing::debug!("notification reload_config");
                app_data.reload_config();
            }
        });
    }

    {
        let cx = Scope::new();
        let app_data = app_data.clone();
        let send = create_ext_action(cx, move |updated| {
            if updated {
                trace!(
                    TraceLevel::INFO,
                    "grammar or query got updated, reset highlight configs"
                );
                reset_highlight_configs();
                for (_, window) in app_data.windows.get_untracked() {
                    for (_, tab) in window.window_tabs.get_untracked() {
                        for (_, doc) in tab.main_split.docs.get_untracked() {
                            doc.syntax.update(|syntax| {
                                *syntax = Syntax::from_language(syntax.language);
                            });
                            doc.trigger_syntax_change(None);
                        }
                    }
                }
            }
        });
        std::thread::Builder::new()
            .name("FindGrammar".to_owned())
            .spawn(move || {
                use self::grammars::*;
                let updated = match find_grammar_release() {
                    Ok(release) => {
                        let mut updated = false;
                        match fetch_grammars(&release) {
                            Err(e) => {
                                trace!(
                                    TraceLevel::ERROR,
                                    "failed to fetch grammars: {e}"
                                );
                            }
                            Ok(u) => updated |= u,
                        }
                        match fetch_queries(&release) {
                            Err(e) => {
                                trace!(
                                    TraceLevel::ERROR,
                                    "failed to fetch grammars: {e}"
                                );
                            }
                            Ok(u) => updated |= u,
                        }
                        updated
                    }
                    Err(e) => {
                        trace!(
                            TraceLevel::ERROR,
                            "failed to obtain release info: {e}"
                        );
                        false
                    }
                };
                send(updated);
            })
            .unwrap();
    }

    #[cfg(feature = "updater")]
    {
        let (tx, rx) = sync_channel(1);
        let notification = create_signal_from_channel(rx);
        let latest_release = app_data.latest_release;
        create_effect(move |_| {
            if let Some(release) = notification.get() {
                latest_release.set(Arc::new(Some(release)));
            }
        });
        std::thread::Builder::new()
            .name("SparrowUpdater".to_owned())
            .spawn(move || {
                loop {
                    if let Ok(release) = crate::update::get_latest_release() {
                        if let Err(err) = tx.send(release) {
                            tracing::error!("{:?}", err);
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_secs(60 * 60));
                }
            })
            .unwrap();
    }

    {
        let (tx, rx) = sync_channel(1);
        let notification = create_signal_from_channel(rx);
        let app_data = app_data.clone();
        create_effect(move |_| {
            if let Some(CoreNotification::OpenPaths { paths }) = notification.get() {
                if let Some(window_tab) = app_data.active_window_tab() {
                    window_tab.open_paths(&paths);
                    // focus window after open doc
                    floem::action::focus_window();
                }
            }
        });
        std::thread::Builder::new()
            .name("ListenLocalSocket".to_owned())
            .spawn(move || {
                if let Err(err) = listen_local_socket(tx) {
                    tracing::error!("{:?}", err);
                }
            })
            .unwrap();
    }

    {
        let app_data = app_data.clone();
        app_data.app_command.listen(move |command| {
            app_data.run_app_command(command);
        });
    }

    app.on_event(move |event| match event {
        floem::AppEvent::WillTerminate => {
            app_data.app_terminated.set(true);
            if let Err(err) = db.insert_app(app_data.clone()) {
                tracing::error!("{:?}", err);
            }
        }
        floem::AppEvent::Reopen {
            has_visible_windows,
        } => {
            if !has_visible_windows {
                app_data.new_window(None);
            }
        }
    })
    .run();
}

/// Uses a login shell to load the correct shell environment for the current user.
pub fn load_shell_env() {
    use std::process::Command;

    #[cfg(not(windows))]
    let shell = match std::env::var("SHELL") {
        Ok(s) => s,
        Err(error) => {
            // Shell variable is not set, so we can't determine the correct shell executable.
            trace!(
                TraceLevel::ERROR,
                "Failed to obtain shell environment: {error}"
            );
            return;
        }
    };

    #[cfg(windows)]
    let shell = "powershell";

    let mut command = Command::new(shell);

    #[cfg(not(windows))]
    command.args(["--login", "-c", "printenv"]);

    #[cfg(windows)]
    command.args([
        "-Command",
        "Get-ChildItem env: | ForEach-Object { \"{0}={1}\" -f $_.Name, $_.Value }",
    ]);

    #[cfg(windows)]
    command.creation_flags(windows::Win32::System::Threading::CREATE_NO_WINDOW);

    let env = match command.output() {
        Ok(output) => String::from_utf8(output.stdout).unwrap_or_default(),

        Err(error) => {
            trace!(
                TraceLevel::ERROR,
                "Failed to obtain shell environment: {error}"
            );
            return;
        }
    };

    env.split('\n')
        .filter_map(|line| line.split_once('='))
        .for_each(|(key, value)| unsafe {
            let value = value.trim_matches('\r');
            if let Ok(v) = std::env::var(key) {
                if v != value {
                    tracing::debug!(variable = key, "Shell environment overrides an existing variable");
                }
            };
            std::env::set_var(key, value);
        })
}

pub fn get_socket() -> Result<interprocess::local_socket::LocalSocketStream> {
    let local_socket = Directory::local_socket()
        .ok_or_else(|| anyhow!("can't get local socket folder"))?;
    let socket =
        interprocess::local_socket::LocalSocketStream::connect(local_socket)?;
    Ok(socket)
}

pub fn try_open_in_existing_process(
    mut socket: interprocess::local_socket::LocalSocketStream,
    paths: &[PathObject],
) -> Result<()> {
    let msg: CoreMessage = RpcMessage::Notification(CoreNotification::OpenPaths {
        paths: paths.to_vec(),
    });
    sparrow_rpc::stdio::write_msg(&mut socket, msg)?;

    let (tx, rx) = crossbeam_channel::bounded(1);
    std::thread::spawn(move || {
        let mut buf = [0; 100];
        let received = if let Ok(n) = socket.read(&mut buf) {
            &buf[..n] == b"received"
        } else {
            false
        };
        tx.send(received)
    });

    let received = rx.recv_timeout(std::time::Duration::from_millis(500))?;
    if !received {
        return Err(anyhow!("didn't receive response"));
    }

    Ok(())
}

fn listen_local_socket(tx: SyncSender<CoreNotification>) -> Result<()> {
    let local_socket = Directory::local_socket()
        .ok_or_else(|| anyhow!("can't get local socket folder"))?;
    if local_socket.exists() {
        if let Err(err) = std::fs::remove_file(&local_socket) {
            tracing::error!("{:?}", err);
        }
    }
    let socket =
        interprocess::local_socket::LocalSocketListener::bind(local_socket)?;

    for stream in socket.incoming().flatten() {
        let tx = tx.clone();
        std::thread::spawn(move || -> Result<()> {
            let mut reader = BufReader::new(stream);
            loop {
                let msg: Option<CoreMessage> =
                    sparrow_rpc::stdio::read_msg(&mut reader)?;

                if let Some(RpcMessage::Notification(msg)) = msg {
                    tx.send(msg)?;
                } else {
                    trace!(TraceLevel::ERROR, "Unhandled message: {msg:?}");
                }

                let stream_ref = reader.get_mut();
                if let Err(err) = stream_ref.write_all(b"received") {
                    tracing::error!("{:?}", err);
                }
                if let Err(err) = stream_ref.flush() {
                    tracing::error!("{:?}", err);
                }
            }
        });
    }
    Ok(())
}

pub fn window_menu(
    sparrow_command: Listener<SparrowCommand>,
    workbench_command: Listener<SparrowWorkbenchCommand>,
    context: crate::menus::MenuContext,
) -> Menu {
    use crate::command::SparrowWorkbenchCommand::*;

    let mut menu = Menu::new("");

    // ── FILE ────────────────────────────────────────────────────────
    menu = menu.entry(
        Menu::new("File")
            .entry(MenuItem::new("New...").action(move || {
                workbench_command.send(NewFile);
            }))
            .entry(MenuItem::new("Open...").action(move || {
                workbench_command.send(OpenFile);
            }))
            .entry(MenuItem::new("Open Folder...").action(move || {
                workbench_command.send(OpenFolder);
            }))
            .entry(MenuItem::new("Recent Projects").action(move || {
                workbench_command.send(PaletteWorkspace);
            }))
            .separator()
            .entry(MenuItem::new("Close Project").action(move || {
                workbench_command.send(CloseFolder);
            }))
            .entry(MenuItem::new("Settings...").action(move || {
                workbench_command.send(OpenSettings);
            }))
            .entry(MenuItem::new("Project Structure...").enabled(false))
            .entry(MenuItem::new("Plugins...").enabled(false))
            .separator()
            .entry(MenuItem::new("Save All").action(move || {
                workbench_command.send(SaveAll);
            }))
            .entry(MenuItem::new("Reload All from Disk").enabled(false))
            .entry(MenuItem::new("Invalidate Caches...").enabled(false))
            .separator()
            .entry(MenuItem::new("Exit").action(move || {
                workbench_command.send(Quit);
            })),
    );

    // ── EDIT ────────────────────────────────────────────────────────
    menu = menu.entry(
        Menu::new("Edit")
            .entry(MenuItem::new("Undo").action(move || {
                sparrow_command.send(SparrowCommand {
                    kind: CommandKind::Edit(EditCommand::Undo),
                    data: None,
                });
            }))
            .entry(MenuItem::new("Redo").action(move || {
                sparrow_command.send(SparrowCommand {
                    kind: CommandKind::Edit(EditCommand::Redo),
                    data: None,
                });
            }))
            .separator()
            .entry(MenuItem::new("Cut").action(move || {
                sparrow_command.send(SparrowCommand {
                    kind: CommandKind::Edit(EditCommand::ClipboardCut),
                    data: None,
                });
            }))
            .entry(MenuItem::new("Copy").action(move || {
                sparrow_command.send(SparrowCommand {
                    kind: CommandKind::Edit(EditCommand::ClipboardCopy),
                    data: None,
                });
            }))
            .entry(MenuItem::new("Paste").action(move || {
                sparrow_command.send(SparrowCommand {
                    kind: CommandKind::Edit(EditCommand::ClipboardPaste),
                    data: None,
                });
            }))
            .entry(MenuItem::new("Find...").action(move || {
                sparrow_command.send(SparrowCommand {
                    kind: CommandKind::Focus(FocusCommand::Search),
                    data: None,
                });
            }))
            .entry(MenuItem::new("Find in Files…").action(move || workbench_command.send(FindInFiles)))
            .entry(MenuItem::new("Replace in Files…").action(move || workbench_command.send(ReplaceInFiles)))
            .entry(MenuItem::new("Find Usages").action(move || {
                workbench_command.send(FindReferences);
            }))
            .separator()
            .entry(MenuItem::new("Select All").action(move || {
                sparrow_command.send(SparrowCommand {
                    kind: CommandKind::MultiSelection(
                        MultiSelectionCommand::SelectAllCurrent,
                    ),
                    data: None,
                });
            }))
            .entry(MenuItem::new("Duplicate Line").action(move || {
                sparrow_command.send(SparrowCommand {
                    kind: CommandKind::Edit(EditCommand::DuplicateLineDown),
                    data: None,
                });
            }))
            .entry(MenuItem::new("Join Lines").action(move || {
                sparrow_command.send(SparrowCommand {
                    kind: CommandKind::Edit(EditCommand::JoinLines),
                    data: None,
                });
            }))
            .separator()
            .entry(MenuItem::new("Indent Selection").action(move || {
                sparrow_command.send(SparrowCommand {
                    kind: CommandKind::Edit(EditCommand::IndentLine),
                    data: None,
                });
            }))
            .entry(MenuItem::new("Unindent Line or Selection").action(move || {
                sparrow_command.send(SparrowCommand {
                    kind: CommandKind::Edit(EditCommand::OutdentLine),
                    data: None,
                });
            })),
    );

    // ── VIEW ────────────────────────────────────────────────────────
    menu = menu.entry(
        Menu::new("View")
            .entry(
                {
                    use strum::IntoEnumIterator;
                    let mut tools=Menu::new("Tool Windows");
                    for kind in crate::panel::kind::PanelKind::iter().filter(|k|*k!=crate::panel::kind::PanelKind::DocumentSymbol){tools=tools.entry(MenuItem::new(kind.title()).action(move ||sparrow_command.send(SparrowCommand{kind:CommandKind::Workbench(ShowPanel),data:serde_json::to_value(kind).ok()})));}tools
                }
            )
            .entry(
                Menu::new("Appearance")
                    .entry(MenuItem::new("Change Color Theme").action(move || {
                        workbench_command.send(ChangeColorTheme);
                    }))
                    .entry(MenuItem::new("Change Icon Theme").action(move || {
                        workbench_command.send(ChangeIconTheme);
                    }))
                    .entry(MenuItem::new("Zoom In").action(move || {
                        workbench_command.send(ZoomIn);
                    }))
                    .entry(MenuItem::new("Zoom Out").action(move || {
                        workbench_command.send(ZoomOut);
                    }))
                    .entry(MenuItem::new("Reset Zoom").action(move || {
                        workbench_command.send(ZoomReset);
                    })),
            )
            .separator()
            .entry(MenuItem::new("Recent Files").action(move || {
                workbench_command.send(PaletteHelpAndFile);
            }))
            .entry(MenuItem::new("Active Editor").enabled(false)),
    );

    // ── NAVIGATE ────────────────────────────────────────────────────
    menu = menu.entry(
        Menu::new("Navigate")
            .entry(MenuItem::new("Back").action(move || {
                workbench_command.send(JumpLocationBackward);
            }))
            .entry(MenuItem::new("Forward").action(move || {
                workbench_command.send(JumpLocationForward);
            }))
            .separator()
            .entry(MenuItem::new("Search Everywhere").action(move || {
                workbench_command.send(SearchEverywhere);
            }))
            .entry(MenuItem::new("Class...").action(move || {
                workbench_command.send(PaletteWorkspaceSymbol);
            }))
            .entry(MenuItem::new("File...").action(move || {
                workbench_command.send(Palette);
            }))
            .entry(MenuItem::new("Symbol...").action(move || {
                workbench_command.send(PaletteSymbol);
            }))
            .entry(MenuItem::new("Line:Column...").action(move || {
                workbench_command.send(PaletteLine);
            }))
            .separator()
            .entry(MenuItem::new("Next Highlighted Error").action(move || {
                workbench_command.send(NextError);
            }))
            .entry(MenuItem::new("Previous Highlighted Error").action(move || {
                workbench_command.send(PreviousError);
            }))
            .separator()
            .entry(MenuItem::new("Declaration or Usages").action(move || {
                sparrow_command.send(SparrowCommand {
                    kind: CommandKind::Focus(FocusCommand::GotoDefinition),
                    data: None,
                });
            }))
            .entry(MenuItem::new("Type Declaration").action(move || {
                sparrow_command.send(SparrowCommand {
                    kind: CommandKind::Focus(FocusCommand::GotoTypeDefinition),
                    data: None,
                });
            }))
            .entry(MenuItem::new("Super Method").enabled(false))
            .entry(MenuItem::new("Test").enabled(false))
            .entry(MenuItem::new("File Structure").action(move || {
                workbench_command.send(PaletteSymbol);
            }))
            .entry(MenuItem::new("Call Hierarchy").action(move || {
                workbench_command.send(ShowCallHierarchy);
            }))
            .entry(MenuItem::new("Type Hierarchy").enabled(false))
            .separator()
            .entry(MenuItem::new("Implementation(s)").action(move || {
                workbench_command.send(GoToImplementation);
            }))
            .entry(MenuItem::new("References").action(move || {
                workbench_command.send(FindReferences);
            })),
    );

    // ── CODE ────────────────────────────────────────────────────────
    menu = menu.entry(
        Menu::new("Code")
            .entry(MenuItem::new("Override Methods...").enabled(false))
            .entry(MenuItem::new("Implement Methods...").enabled(false))
            .entry(MenuItem::new("Generate...").enabled(false))
            .separator()
            .entry(MenuItem::new("Code Completion").action(move || {
                sparrow_command.send(SparrowCommand {
                    kind: CommandKind::Focus(FocusCommand::GetCompletion),
                    data: None,
                });
            }))
            .entry(MenuItem::new("Inspect Code…").action(move || {
                workbench_command.send(InspectCode);
            }))
            .separator()
            .entry(MenuItem::new("Comment with Line Comment").action(move || {
                sparrow_command.send(SparrowCommand {
                    kind: CommandKind::Edit(EditCommand::ToggleLineComment),
                    data: None,
                });
            }))
            .entry(MenuItem::new("Reformat Code").action(move || {
                sparrow_command.send(SparrowCommand {
                    kind: CommandKind::Focus(FocusCommand::FormatDocument),
                    data: None,
                });
            }))
            .entry(MenuItem::new("Reformat Code Options…").action(move ||workbench_command.send(ReformatCodeOptions)))
            .entry(
                MenuItem::new("Optimize Imports")
                    .enabled(context.dart_editor)
                    .action(move || workbench_command.send(OptimizeImports)),
            )
            .separator()
            .entry(MenuItem::new("Move Statement Down").action(move || {
                sparrow_command.send(SparrowCommand {
                    kind: CommandKind::Edit(EditCommand::MoveLineDown),
                    data: None,
                });
            }))
            .entry(MenuItem::new("Move Statement Up").action(move || {
                sparrow_command.send(SparrowCommand {
                    kind: CommandKind::Edit(EditCommand::MoveLineUp),
                    data: None,
                });
            }))
            .entry(MenuItem::new("Move Element Left").enabled(false))
            .entry(MenuItem::new("Move Element Right").enabled(false)),
    );

    // ── REFACTOR ────────────────────────────────────────────────────
    menu = menu.entry(
        Menu::new("Refactor")
            .entry(MenuItem::new("Refactor This...").action(move || {
                sparrow_command.send(SparrowCommand {
                    kind: CommandKind::Focus(FocusCommand::ShowCodeActions),
                    data: None,
                });
            }))
            .entry(
                MenuItem::new("Rename File...")
                    .action(move || workbench_command.send(RenameActiveFile)),
            )
            .separator()
            .entry(MenuItem::new("Extract/Introduce...").enabled(context.dart_editor).action(move ||workbench_command.send(RefactorExtract)))
            .entry(MenuItem::new("Inline...").enabled(context.dart_editor).action(move ||workbench_command.send(RefactorInline)))
            .entry(MenuItem::new("Move File...").enabled(context.dart_editor).action(move ||workbench_command.send(MoveActiveFile)))
            .entry(MenuItem::new("Copy File...").action(move ||workbench_command.send(CopyActiveFile)))
            .entry(MenuItem::new("Safe Delete...").enabled(context.dart_editor).action(move ||workbench_command.send(SafeDeleteFile))),
    );

    // ── BUILD ───────────────────────────────────────────────────────
    menu = menu.entry(
        Menu::new("Build")
            .entry(MenuItem::new("Build Project").action(move ||sparrow_command.send(SparrowCommand{kind:CommandKind::Workbench(FlutterProjectTask),data:Some(serde_json::json!(["build"]))})))
            .entry(MenuItem::new("Rebuild Project").action(move ||sparrow_command.send(SparrowCommand{kind:CommandKind::Workbench(FlutterProjectTask),data:Some(serde_json::json!(["clean"]))})))
            .entry(MenuItem::new("Generate App Bundles or APKs").action(move ||sparrow_command.send(SparrowCommand{kind:CommandKind::Workbench(FlutterProjectTask),data:Some(serde_json::json!(["build", "apk"]))})))
            .entry(MenuItem::new("Analyze APK...").enabled(false)),
    );

    // ── RUN ─────────────────────────────────────────────────────────
    menu = menu.entry(
        Menu::new("Run")
            .entry(
                MenuItem::new(context.run_label)
                    .enabled(context.can_run)
                    .action(move || {
                        workbench_command.send(RunSelectedConfiguration);
                    }),
            )
            .entry(
                MenuItem::new(context.debug_label)
                    .enabled(context.can_run)
                    .action(move || {
                        workbench_command.send(DebugSelectedConfiguration);
                    }),
            )
            .entry(MenuItem::new("Edit Configurations...").action(move || {
                workbench_command.send(EditRunConfigurations);
            }))
            .separator()
            .entry(MenuItem::new("Stop").enabled(context.can_stop).action(
                move || {
                    workbench_command.send(RunAndDebugStop);
                },
            ))
            .separator()
            .entry(
                MenuItem::new("Flutter Hot Reload")
                    .enabled(context.can_reload)
                    .action(move || {
                        workbench_command.send(FlutterHotReload);
                    }),
            )
            .entry(
                MenuItem::new("Flutter Hot Restart")
                    .enabled(context.can_reload)
                    .action(move || {
                        workbench_command.send(FlutterHotRestart);
                    }),
            )
            .separator()
            .entry(
                MenuItem::new("Toggle Breakpoint")
                    .enabled(context.dart_editor)
                    .action(move || workbench_command.send(ToggleBreakpoint)),
            )
            .entry(MenuItem::new("View Breakpoints...").action(move || {
                sparrow_command.send(SparrowCommand {
                    kind: CommandKind::Workbench(ShowPanel),
                    data: Some(
                        serde_json::to_value(crate::panel::kind::PanelKind::Debug)
                            .expect("panel kind serializes"),
                    ),
                });
            })),
    );

    // ── TOOLS ───────────────────────────────────────────────────────
    menu = menu.entry(
        Menu::new("Tools")
            .entry(MenuItem::new("Device Manager").action(move || {
                sparrow_command.send(SparrowCommand {
                    kind: CommandKind::Workbench(ShowPanel),
                    data: Some(
                        serde_json::to_value(
                            crate::panel::kind::PanelKind::DeviceManager,
                        )
                        .expect("panel kind serializes"),
                    ),
                });
            }))
            .entry(MenuItem::new("SDK Manager").enabled(false))
            .entry(MenuItem::new("Resource Manager").enabled(false))
            .separator()
            .entry(MenuItem::new("Layout Inspector").enabled(false))
            .entry(MenuItem::new("Create Command Line Launcher...").enabled(false))
            .separator()
            .entry(
                Menu::new("Flutter")
                    .entry(MenuItem::new("Open Widget Tree").action(move || {
                        sparrow_command.send(SparrowCommand {
                            kind: CommandKind::Workbench(ShowPanel),
                            data: Some(
                                serde_json::to_value(
                                    crate::panel::kind::PanelKind::WidgetTree,
                                )
                                .expect("panel kind serializes"),
                            ),
                        });
                    }))
                    .entry(MenuItem::new("Open Outline").action(move || {
                        sparrow_command.send(SparrowCommand {
                            kind: CommandKind::Workbench(ShowPanel),
                            data: Some(
                                serde_json::to_value(
                                    crate::panel::kind::PanelKind::DartOutline,
                                )
                                .expect("panel kind serializes"),
                            ),
                        });
                    })),
            ),
    );

    // ── GIT ─────────────────────────────────────────────────────────
    menu = menu.entry(
        Menu::new("Git")
            .entry(MenuItem::new("Commit...").action(move || {
                sparrow_command.send(SparrowCommand {
                    kind: CommandKind::Workbench(ShowPanel),
                    data: serde_json::to_value(
                        crate::panel::kind::PanelKind::SourceControl,
                    )
                    .ok(),
                });
            }))
            .entry(MenuItem::new("Push...").action(move ||sparrow_command.send(SparrowCommand{kind:CommandKind::Workbench(GitCommand),data:Some(serde_json::json!(["push"]))})))
            .entry(MenuItem::new("Update Project...").action(move ||sparrow_command.send(SparrowCommand{kind:CommandKind::Workbench(GitCommand),data:Some(serde_json::json!(["pull", "--ff-only"]))})))
            .entry(MenuItem::new("Pull...").action(move ||sparrow_command.send(SparrowCommand{kind:CommandKind::Workbench(GitCommand),data:Some(serde_json::json!(["pull", "--ff-only"]))})))
            .entry(MenuItem::new("Fetch...").action(move ||sparrow_command.send(SparrowCommand{kind:CommandKind::Workbench(GitCommand),data:Some(serde_json::json!(["fetch", "--all", "--prune"]))})))
            .separator()
            .entry(MenuItem::new("Merge...").action(move ||sparrow_command.send(SparrowCommand{kind:CommandKind::Workbench(GitCommand),data:Some(serde_json::json!(["merge", "--no-edit", "HEAD"]))})))
            .entry(MenuItem::new("Rebase...").action(move ||sparrow_command.send(SparrowCommand{kind:CommandKind::Workbench(GitCommand),data:Some(serde_json::json!(["rebase", "HEAD"]))})))
            .entry(MenuItem::new("Branches...").action(move || {
                workbench_command.send(PaletteSCMReferences);
            }))
            .entry(MenuItem::new("Reset HEAD...").action(move ||sparrow_command.send(SparrowCommand{kind:CommandKind::Workbench(GitCommand),data:Some(serde_json::json!(["reset", "--soft", "HEAD"]))})))
            .separator()
            .entry(MenuItem::new("Show Git Log").action(move || {
                sparrow_command.send(SparrowCommand {
                    kind: CommandKind::Workbench(ShowPanel),
                    data: serde_json::to_value(
                        crate::panel::kind::PanelKind::GitHistory,
                    )
                    .ok(),
                });
            }))
            .entry(MenuItem::new("Manage Remotes...").action(move ||sparrow_command.send(SparrowCommand{kind:CommandKind::Workbench(GitCommand),data:Some(serde_json::json!(["remote", "-v"]))})))
            .entry(MenuItem::new("Clone...").action(move ||sparrow_command.send(SparrowCommand{kind:CommandKind::Workbench(GitCommand),data:Some(serde_json::json!(["clone"]))}))),
    );

    // ── WINDOW ──────────────────────────────────────────────────────
    menu = menu.entry(
        Menu::new("Window")
            .entry(
                Menu::new("Layouts")
                    .entry(MenuItem::new("Toggle Left Panel").action(move || {
                        workbench_command.send(TogglePanelLeftVisual);
                    }))
                    .entry(MenuItem::new("Toggle Right Panel").action(move || {
                        workbench_command.send(TogglePanelRightVisual);
                    }))
                    .entry(MenuItem::new("Toggle Bottom Panel").action(move || {
                        workbench_command.send(TogglePanelBottomVisual);
                    })),
            )
            .entry(MenuItem::new("Next Editor Tab").action(move || {
                workbench_command.send(NextEditorTab);
            }))
            .entry(MenuItem::new("Previous Editor Tab").action(move || {
                workbench_command.send(PreviousEditorTab);
            }))
            .separator()
            .entry(MenuItem::new("Next Window Tab").action(move || {
                workbench_command.send(NextWindowTab);
            }))
            .entry(MenuItem::new("Previous Window Tab").action(move || {
                workbench_command.send(PreviousWindowTab);
            }))
            .separator()
            .entry(MenuItem::new("New Window").action(move || {
                workbench_command.send(NewWindow);
            })),
    );

    if let Some(actions) = crate::menus::quick_actions_menu(&context.quick_actions, workbench_command) {
        menu = menu.entry(actions);
    }

    // ── HELP ────────────────────────────────────────────────────────
    menu = menu.entry(
        Menu::new("Help")
            .entry(MenuItem::new("Find Action...").action(move || {
                workbench_command.send(PaletteCommand);
            }))
            .separator()
            .entry(MenuItem::new("Keyboard Shortcuts").action(move || {
                workbench_command.send(OpenKeyboardShortcuts);
            }))
            .entry(MenuItem::new("Show IDE Logs").action(move || {
                workbench_command.send(OpenLogFile);
            }))
            .entry(MenuItem::new("Open Logs Directory").action(move || {
                workbench_command.send(OpenLogsDirectory);
            }))
            .entry(MenuItem::new("Check for Updates...").action(move || {
                workbench_command.send(RestartToUpdate);
            }))
            .separator()
            .entry(MenuItem::new("About Sparrow").action(move || {
                workbench_command.send(ShowAbout);
            })),
    );

    menu
}
