use std::{path::PathBuf, rc::Rc, sync::Arc};

use floem::{
    ViewId,
    action::TimerToken,
    peniko::kurbo::{Point, Size},
    reactive::{
        Memo, ReadSignal, RwSignal, Scope, SignalGet, SignalUpdate, SignalWith,
        use_context,
    },
    window::WindowId,
};
use serde::{Deserialize, Serialize};

use crate::{
    app::AppCommand,
    command::{InternalCommand, WindowCommand},
    config::SparrowConfig,
    db::SparrowDb,
    keypress::EventRef,
    listener::Listener,
    update::ReleaseInfo,
    window_tab::WindowTabData,
    workspace::SparrowWorkspace,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabsInfo {
    pub active_tab: usize,
    pub workspaces: Vec<SparrowWorkspace>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowInfo {
    pub size: Size,
    pub pos: Point,
    pub maximised: bool,
    pub tabs: TabsInfo,
}

#[derive(Clone)]
pub struct WindowCommonData {
    pub window_command: Listener<WindowCommand>,
    pub window_scale: RwSignal<f64>,
    pub size: RwSignal<Size>,
    pub num_window_tabs: Memo<usize>,
    pub window_maximized: RwSignal<bool>,
    pub window_tab_header_height: RwSignal<f64>,
    pub latest_release: ReadSignal<Arc<Option<ReleaseInfo>>>,
    pub ime_allowed: RwSignal<bool>,
    pub cursor_blink_timer: RwSignal<TimerToken>,
    // the value to be update by cursor blinking
    pub hide_cursor: RwSignal<bool>,
    pub app_view_id: RwSignal<ViewId>,
    pub extra_plugin_paths: Arc<Vec<PathBuf>>,
}

/// `WindowData` is the application model for a top-level window.
///
/// A top-level window can be independently moved around and
/// resized using your window manager. Normally Sparrow has only one
/// top-level window, but new ones can be created using the "New Window"
/// command.
///
/// Each window has its own collection of "window tabs" (again, there is
/// normally only one window tab), size, position etc.
#[derive(Clone)]
pub struct WindowData {
    pub window_id: WindowId,
    pub scope: Scope,
    /// The set of tabs within the window. These tabs are high-level
    /// constructs for workspaces, in particular they are not **editor tabs**.
    pub window_tabs: RwSignal<im::Vector<(RwSignal<usize>, Rc<WindowTabData>)>>,
    pub num_window_tabs: Memo<usize>,
    /// The index of the active window tab.
    pub active: RwSignal<usize>,
    pub app_command: Listener<AppCommand>,
    pub position: RwSignal<Point>,
    pub root_view_id: RwSignal<ViewId>,
    pub window_scale: RwSignal<f64>,
    pub config: RwSignal<Arc<SparrowConfig>>,
    pub ime_enabled: RwSignal<bool>,
    pub common: Rc<WindowCommonData>,
}

impl WindowData {
    pub fn new(
        window_id: WindowId,
        app_view_id: RwSignal<ViewId>,
        info: WindowInfo,
        window_scale: RwSignal<f64>,
        latest_release: ReadSignal<Arc<Option<ReleaseInfo>>>,
        extra_plugin_paths: Arc<Vec<PathBuf>>,
        app_command: Listener<AppCommand>,
    ) -> Self {
        let cx = Scope::new();
        let config = SparrowConfig::load(
            &SparrowWorkspace::default(),
            &[],
            &extra_plugin_paths,
        );
        let config = cx.create_rw_signal(Arc::new(config));
        let root_view_id = cx.create_rw_signal(ViewId::new());

        let window_tabs = cx.create_rw_signal(im::Vector::new());
        let num_window_tabs =
            cx.create_memo(move |_| window_tabs.with(|tabs| tabs.len()));

        let window_command = Listener::new_empty(cx);
        let ime_allowed = cx.create_rw_signal(false);
        let window_maximized = cx.create_rw_signal(false);
        let size = cx.create_rw_signal(Size::ZERO);
        let window_tab_header_height = cx.create_rw_signal(0.0);
        let cursor_blink_timer = cx.create_rw_signal(TimerToken::INVALID);
        let hide_cursor = cx.create_rw_signal(false);

        let common = Rc::new(WindowCommonData {
            window_command,
            window_scale,
            size,
            num_window_tabs,
            window_maximized,
            window_tab_header_height,
            latest_release,
            ime_allowed,
            cursor_blink_timer,
            hide_cursor,
            app_view_id,
            extra_plugin_paths,
        });

        let workspace = info
            .tabs
            .workspaces
            .into_iter()
            .nth(info.tabs.active_tab)
            .unwrap_or_default();
        crate::widgets::file_picker::set_project(
            workspace
                .path
                .clone()
                .filter(|_| !workspace.kind.is_remote()),
        );
        if let Some(db) = use_context::<Arc<SparrowDb>>() {
            let _ = db.update_recent_workspace(&workspace);
        }
        let window_tab =
            Rc::new(WindowTabData::new(cx, Arc::new(workspace), common.clone()));
        window_tabs
            .update(|tabs| tabs.push_back((cx.create_rw_signal(0), window_tab)));

        let active = cx.create_rw_signal(0);
        let position = cx.create_rw_signal(info.pos);

        let window_data = Self {
            window_id,
            scope: cx,
            window_tabs,
            num_window_tabs,
            active,
            position,
            root_view_id,
            window_scale,
            app_command,
            config,
            ime_enabled: cx.create_rw_signal(false),
            common,
        };

        {
            let window_data = window_data.clone();
            window_data.common.window_command.listen(move |cmd| {
                window_data.run_window_command(cmd);
            });
        }

        {
            cx.create_effect(move |_| {
                let active = active.get();
                let tab = window_tabs
                    .with(|tabs| tabs.get(active).map(|(_, tab)| tab.clone()));
                if let Some(tab) = tab {
                    tab.common
                        .internal_command
                        .send(InternalCommand::ResetBlinkCursor);
                }
            })
        }

        window_data
    }

    pub fn reload_config(&self) {
        let config = SparrowConfig::load(
            &SparrowWorkspace::default(),
            &[],
            &self.common.extra_plugin_paths,
        );
        self.config.set(Arc::new(config));
        let window_tabs = self.window_tabs.get_untracked();
        for (_, window_tab) in window_tabs {
            window_tab.reload_config();
        }
    }

    pub(crate) fn request_workspace(
        &self,
        workspace: SparrowWorkspace,
        files: Vec<sparrow_rpc::file::PathObject>,
    ) {
        let Some(current) = self.active_window_tab() else {
            return;
        };
        if current.workspace.kind == workspace.kind
            && current.workspace.path == workspace.path
        {
            current.open_paths(&files);
            return;
        }
        let opening = workspace.path.is_some() || workspace.kind.is_remote();
        let project_name = workspace.display().unwrap_or_default();
        let switch = {
            let window = self.clone();
            move || {
                window.replace_workspace(workspace.clone());
                if let Some(tab) = window.active_window_tab() {
                    tab.open_paths(&files);
                }
            }
        };
        let has_project =
            current.workspace.path.is_some() || current.workspace.kind.is_remote();
        let dirty = current
            .main_split
            .editors
            .with_editors_untracked(|editors| {
                editors.values().any(|editor| !editor.doc().is_pristine())
            });
        if !has_project && !dirty {
            switch();
            return;
        }
        let title = if opening {
            "Switch Project"
        } else {
            "Close Project"
        };
        let message = if opening {
            format!("Open {project_name} and close the current workspace?")
        } else {
            "Close the current project?".into()
        };
        let message = if dirty {
            format!("{message} Unsaved changes will be discarded.")
        } else {
            message
        };
        let text = match (opening, dirty) {
            (true, true) => "Discard Changes and Open",
            (false, true) => "Discard Changes and Close",
            (true, false) => "Open Project",
            (false, false) => "Close Project",
        };
        let active = current.alert_data.active;
        current.alert_data.title.set(title.into());
        current.alert_data.msg.set(message);
        current
            .alert_data
            .buttons
            .set(vec![crate::widgets::alert::AlertButton {
                text: text.into(),
                action: Rc::new(move || {
                    active.set(false);
                    switch();
                }),
            }]);
        active.set(true);
    }

    fn replace_workspace(&self, workspace: SparrowWorkspace) {
        crate::widgets::file_picker::close_active();
        crate::widgets::file_picker::set_project(
            workspace
                .path
                .clone()
                .filter(|_| !workspace.kind.is_remote()),
        );
        let db: Arc<SparrowDb> = use_context().unwrap();
        if let Some(old) = self.active_window_tab() {
            if let Err(error) = db.insert_window_tab(old.clone()) {
                tracing::error!(%error, "Could not save workspace state");
            }
            old.proxy.shutdown();
        }
        let _ = db.update_recent_workspace(&workspace);
        let tab = Rc::new(WindowTabData::new(
            self.scope,
            Arc::new(workspace),
            self.common.clone(),
        ));
        self.window_tabs
            .set(im::vector![(self.scope.create_rw_signal(0), tab)]);
        self.active.set(0);
        self.app_command.send(AppCommand::SaveApp);
    }

    pub fn run_window_command(&self, cmd: WindowCommand) {
        match cmd {
            WindowCommand::OpenPaths(paths) => {
                if let Some(folder) = paths.iter().find(|path| path.is_dir) {
                    let path = folder.path.clone();
                    let kind = self
                        .active_window_tab()
                        .map(|tab| tab.workspace.kind.clone())
                        .unwrap_or(crate::workspace::SparrowWorkspaceType::Local);
                    self.request_workspace(
                        SparrowWorkspace {
                            kind,
                            path: Some(path),
                            last_open: 0,
                        },
                        paths.into_iter().filter(|path| !path.is_dir).collect(),
                    );
                } else if let Some(tab) = self.active_window_tab() {
                    tab.open_paths(&paths);
                }
            }
            WindowCommand::SetWorkspace { workspace }
            | WindowCommand::OpenProjectTab { workspace }
            | WindowCommand::NewWorkspaceTab { workspace, .. } => {
                self.request_workspace(workspace, Vec::new());
            }
            WindowCommand::CloseWorkspaceTab { .. } => {
                self.request_workspace(SparrowWorkspace::default(), Vec::new());
            }
            WindowCommand::NextWorkspaceTab => {
                let active = self.active.get_untracked();
                let tabs_len = self.window_tabs.with_untracked(|tabs| tabs.len());
                if tabs_len > 1 {
                    let active = if active >= tabs_len - 1 {
                        0
                    } else {
                        active + 1
                    };
                    self.active.set(active);
                }
            }
            WindowCommand::PreviousWorkspaceTab => {
                let active = self.active.get_untracked();
                let tabs_len = self.window_tabs.with_untracked(|tabs| tabs.len());
                if tabs_len > 1 {
                    let active = if active == 0 {
                        tabs_len - 1
                    } else {
                        active - 1
                    };
                    self.active.set(active);
                }
            }
            WindowCommand::CloseWindow => {
                self.app_command
                    .send(AppCommand::CloseWindow(self.window_id));
            }
        }
        self.app_command.send(AppCommand::SaveApp);
    }

    pub fn key_down<'a>(&self, event: impl Into<EventRef<'a>> + Copy) -> bool {
        let active = self.active.get_untracked();
        let window_tab = self.window_tabs.with_untracked(|window_tabs| {
            window_tabs
                .get(active)
                .or_else(|| window_tabs.last())
                .cloned()
        });
        if let Some((_, window_tab)) = window_tab {
            window_tab.key_down(event)
        } else {
            false
        }
    }

    pub fn info(&self) -> WindowInfo {
        let workspaces: Vec<SparrowWorkspace> = self
            .window_tabs
            .get_untracked()
            .iter()
            .map(|(_, t)| (*t.workspace).clone())
            .collect();
        WindowInfo {
            size: self.common.size.get_untracked(),
            pos: self.position.get_untracked(),
            maximised: false,
            tabs: TabsInfo {
                active_tab: self.active.get_untracked(),
                workspaces,
            },
        }
    }

    pub fn active_window_tab(&self) -> Option<Rc<WindowTabData>> {
        let window_tabs = self.window_tabs.get_untracked();
        let active = self
            .active
            .get_untracked()
            .min(window_tabs.len().saturating_sub(1));
        window_tabs.get(active).map(|(_, tab)| tab.clone())
    }

    pub fn move_tab(&self, from_index: usize, to_index: usize) {
        if from_index == to_index {
            return;
        }

        let to_index = if from_index < to_index {
            to_index - 1
        } else {
            to_index
        };
        self.window_tabs.update(|tabs| {
            let tab = tabs.remove(from_index);
            tabs.insert(to_index, tab);
        });
        self.active.set(to_index);
    }
}
