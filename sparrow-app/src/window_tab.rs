use sparrow_core::rope_text_pos::RopeTextPosition;
use std::{
    collections::{BTreeMap, HashSet},
    env,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc,
        mpsc::{Sender, channel},
    },
    time::Instant,
};

use alacritty_terminal::vte::ansi::Handler;
use floem::{
    ViewId,
    action::{TimerToken, open_file, remove_overlay},
    ext_event::{create_ext_action, create_signal_from_channel},
    file::FileDialogOptions,
    keyboard::Modifiers,
    kurbo::Size,
    peniko::kurbo::{Point, Rect, Vec2},
    prelude::SignalTrack,
    reactive::{
        Memo, ReadSignal, RwSignal, Scope, SignalGet, SignalUpdate, SignalWith,
        WriteSignal, use_context,
    },
    text::{Attrs, AttrsList, FamilyOwned, LineHeightValue, TextLayout},
    views::editor::core::buffer::rope_text::RopeText,
};
use im::HashMap;
use indexmap::IndexMap;
use itertools::Itertools;
use lsp_types::{
    CodeActionOrCommand, CodeLens, Diagnostic, ProgressParams, ProgressToken,
    ShowMessageParams,
};
use serde_json::Value;
use sparrow_core::{
    command::{EditCommand, FocusCommand},
    cursor::CursorAffinity,
    directory::Directory,
    meta,
    mode::Mode,
    register::Register,
};
use sparrow_rpc::{
    RpcError,
    core::CoreNotification,
    dap_types::{ConfigSource, RunDebugConfig},
    file::{Naming, PathObject},
    plugin::PluginId,
    proxy::{ProxyResponse, ProxyRpcHandler, ProxyStatus},
    source_control::FileDiff,
    terminal::TermId,
};
use tracing::{Level, debug, error, event};

use crate::dart::{
    DartLspService, FlutterHotReloadData, FlutterInspectorData, SignatureHelpData,
};
use crate::outline::OutlineData;
use crate::widget_tree::WidgetTreeData;
use crate::{
    about::AboutData,
    alert::{AlertBoxData, AlertButton},
    code_action::{CodeActionData, CodeActionStatus},
    command::{
        CommandExecuted, CommandKind, InternalCommand, SparrowCommand,
        SparrowWorkbenchCommand, WindowCommand,
    },
    completion::{CompletionData, CompletionStatus},
    config::SparrowConfig,
    db::SparrowDb,
    debug::{DapData, RunDebugMode, RunDebugProcess, SparrowBreakpoint},
    doc::DocContent,
    editor::{
        InlineFindDirection,
        location::{EditorLocation, EditorPosition},
    },
    editor_tab::EditorTabChild,
    file_explorer::data::FileExplorerData,
    find::Find,
    global_search::GlobalSearchData,
    hover::HoverData,
    id::WindowTabId,
    inline_completion::InlineCompletionData,
    keypress::{EventRef, KeyPressData, KeyPressFocus, condition::Condition},
    listener::Listener,
    lsp::path_from_url,
    main_split::{MainSplitData, SplitData, SplitDirection, SplitMoveDirection},
    palette::{DEFAULT_RUN_TOML, PaletteData, PaletteStatus, kind::PaletteKind},
    panel::{
        call_hierarchy_view::{CallHierarchyData, CallHierarchyItemData},
        data::{PanelData, PanelSection, default_panel_order, normalize_panel_layout},
        kind::PanelKind,
        position::PanelContainerPosition,
    },
    plugin::PluginData,
    proxy::{ProxyData, new_proxy},
    rename::RenameData,
    source_control::SourceControlData,
    terminal::{
        event::{TermEvent, TermNotification, terminal_update_process},
        panel::TerminalPanelData,
    },
    tracing::*,
    window::WindowCommonData,
    workspace::{SparrowWorkspace, SparrowWorkspaceType, WorkspaceInfo},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Focus {
    Workbench,
    Modal,
    Palette,
    CodeAction,
    Rename,
    AboutPopup,
    Panel(PanelKind),
}

#[derive(Clone)]
pub enum DragContent {
    Panel(PanelKind),
    EditorTab(EditorTabChild),
}

impl DragContent {
    pub fn is_panel(&self) -> bool {
        matches!(self, DragContent::Panel(_))
    }
}

#[derive(Clone)]
pub struct WorkProgress {
    pub token: ProgressToken,
    pub title: String,
    pub message: Option<String>,
    pub percentage: Option<u32>,
}

#[derive(Clone)]
pub struct CommonData {
    pub workspace_undo: crate::workspace_undo::SharedHistory,
    pub workspace: Arc<SparrowWorkspace>,
    pub scope: Scope,
    pub focus: RwSignal<Focus>,
    pub keypress: RwSignal<KeyPressData>,
    pub completion: RwSignal<CompletionData>,
    pub inline_completion: RwSignal<InlineCompletionData>,
    pub hover: HoverData,
    pub register: RwSignal<Register>,
    pub find: Find,
    pub dart_lsp: DartLspService,
    pub signature_help: RwSignal<SignatureHelpData>,
    pub workbench_size: RwSignal<Size>,
    pub window_origin: RwSignal<Point>,
    pub internal_command: Listener<InternalCommand>,
    pub sparrow_command: Listener<SparrowCommand>,
    pub workbench_command: Listener<SparrowWorkbenchCommand>,
    pub term_tx: Sender<(TermId, TermEvent)>,
    pub term_notification_tx: Sender<TermNotification>,
    pub proxy: ProxyRpcHandler,
    pub view_id: RwSignal<ViewId>,
    pub ui_line_height: Memo<f64>,
    pub dragging: RwSignal<Option<DragContent>>,
    pub config: ReadSignal<Arc<SparrowConfig>>,
    pub proxy_status: RwSignal<Option<ProxyStatus>>,
    pub mouse_hover_timer: RwSignal<TimerToken>,
    pub breakpoints: RwSignal<BTreeMap<PathBuf, BTreeMap<usize, SparrowBreakpoint>>>,
    // the current focused view which will receive keyboard events
    pub keyboard_focus: RwSignal<Option<ViewId>>,
    pub window_common: Rc<WindowCommonData>,
}

impl std::fmt::Debug for CommonData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommonData")
            .field("workspace", &self.workspace)
            .finish()
    }
}

#[derive(Clone)]
pub struct WindowTabData {
    pub scope: Scope,
    pub window_tab_id: WindowTabId,
    pub workspace: Arc<SparrowWorkspace>,
    pub palette: PaletteData,
    pub main_split: MainSplitData,
    pub file_explorer: FileExplorerData,
    pub panel: PanelData,
    pub terminal: TerminalPanelData,
    pub plugin: PluginData,
    pub code_action: RwSignal<CodeActionData>,
    pub code_lens: RwSignal<Option<ViewId>>,
    pub dart_lsp: DartLspService,
    pub flutter_hot_reload: FlutterHotReloadData,
    pub flutter_inspector: FlutterInspectorData,
    pub devtools: crate::dart::devtools::DevToolsData,
    pub flutter_project: crate::dart::project_wizard::ProjectWizard,
    pub flutter_test: crate::dart::test_service::FlutterTestData,
    pub source_control: SourceControlData,
    pub rename: RenameData,
    pub global_search: GlobalSearchData,
    pub call_hierarchy_data: CallHierarchyData,
    pub about_data: AboutData,
    pub alert_data: AlertBoxData,
    pub settings_modal_visible: RwSignal<bool>,
    pub layout_rect: RwSignal<Rect>,
    pub title_height: RwSignal<f64>,
    pub status_height: RwSignal<f64>,
    pub proxy: ProxyData,
    pub set_config: WriteSignal<Arc<SparrowConfig>>,
    pub update_in_progress: RwSignal<bool>,
    pub progresses: RwSignal<IndexMap<ProgressToken, WorkProgress>>,
    pub messages: RwSignal<Vec<(String, ShowMessageParams)>>,
    pub notifications: RwSignal<Vec<crate::panel::workflow_tools::Notice>>,
    pub common: Rc<CommonData>,
    pub widget_tree: WidgetTreeData,
    pub outline: OutlineData,
    pub project_menu_visible: RwSignal<bool>,
    pub branch_menu_visible: RwSignal<bool>,
    pub device_menu_visible: RwSignal<bool>,
    pub run_configs: RwSignal<Vec<RunDebugConfig>>,
    pub active_run_config_idx: RwSignal<usize>,
    pub run_config_modal_visible: RwSignal<bool>,
}

impl std::fmt::Debug for WindowTabData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowTabData")
            .field("window_tab_id", &self.window_tab_id)
            .finish()
    }
}

impl KeyPressFocus for WindowTabData {
    fn get_mode(&self) -> Mode {
        Mode::Normal
    }

    fn check_condition(&self, condition: Condition) -> bool {
        match condition {
            Condition::PanelFocus => {
                matches!(self.common.focus.get_untracked(), Focus::Panel(_))
            }
            Condition::SourceControlFocus => {
                self.common.focus.get_untracked()
                    == Focus::Panel(PanelKind::SourceControl)
            }
            Condition::FileExplorerFocus => {
                self.common.focus.get_untracked()
                    == Focus::Panel(PanelKind::FileExplorer)
            }
            _ => false,
        }
    }

    fn run_command(
        &self,
        command: &SparrowCommand,
        _count: Option<usize>,
        _mods: Modifiers,
    ) -> CommandExecuted {
        match &command.kind {
            CommandKind::Workbench(cmd) => {
                self.run_workbench_command(cmd.clone(), None);
            }
            CommandKind::Focus(cmd) => {
                if self.common.focus.get_untracked() == Focus::Workbench {
                    match cmd {
                        FocusCommand::SplitClose => {
                            self.main_split.editor_tab_child_close_active();
                        }
                        FocusCommand::SplitVertical => {
                            self.main_split.split_active(SplitDirection::Vertical);
                        }
                        FocusCommand::SplitHorizontal => {
                            self.main_split.split_active(SplitDirection::Horizontal);
                        }
                        FocusCommand::SplitRight => {
                            self.main_split
                                .split_move_active(SplitMoveDirection::Right);
                        }
                        FocusCommand::SplitLeft => {
                            self.main_split
                                .split_move_active(SplitMoveDirection::Left);
                        }
                        FocusCommand::SplitUp => {
                            self.main_split
                                .split_move_active(SplitMoveDirection::Up);
                        }
                        FocusCommand::SplitDown => {
                            self.main_split
                                .split_move_active(SplitMoveDirection::Down);
                        }
                        FocusCommand::SplitExchange => {
                            self.main_split.split_exchange_active();
                        }
                        _ => {
                            return CommandExecuted::No;
                        }
                    }
                }
            }
            _ => {
                return CommandExecuted::No;
            }
        }

        CommandExecuted::Yes
    }

    fn receive_char(&self, _c: &str) {}
}

impl WindowTabData {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cx: Scope,
        workspace: Arc<SparrowWorkspace>,
        window_common: Rc<WindowCommonData>,
    ) -> Self {
        let cx = cx.create_child();
        let db: Arc<SparrowDb> = use_context().unwrap();

        let disabled_volts = db.get_disabled_volts().unwrap_or_default();
        let workspace_disabled_volts = db
            .get_workspace_disabled_volts(&workspace)
            .unwrap_or_default();
        let mut all_disabled_volts = disabled_volts.clone();
        all_disabled_volts.extend(workspace_disabled_volts.clone());

        let workspace_info = if workspace.path.is_some() {
            db.get_workspace_info(&workspace).ok()
        } else {
            let mut info = db.get_workspace_info(&workspace).ok();
            if let Some(info) = info.as_mut() {
                info.split.children.clear();
            }
            info
        };

        let config = SparrowConfig::load(
            &workspace,
            &all_disabled_volts,
            &window_common.extra_plugin_paths,
        );
        let sparrow_command = Listener::new_empty(cx);
        let workbench_command = Listener::new_empty(cx);
        let internal_command = Listener::new_empty(cx);
        let keypress = cx.create_rw_signal(KeyPressData::new(cx, &config));
        let proxy_status = cx.create_rw_signal(None);

        let (term_tx, term_rx) = channel();
        let (term_notification_tx, term_notification_rx) = channel();
        {
            let term_notification_tx = term_notification_tx.clone();
            std::thread::Builder::new()
                .name("terminal update process".to_owned())
                .spawn(move || {
                    terminal_update_process(term_rx, term_notification_tx);
                })
                .unwrap();
        }

        let proxy = new_proxy(
            workspace.clone(),
            all_disabled_volts,
            window_common.extra_plugin_paths.as_ref().clone(),
            config.plugins.clone(),
            term_tx.clone(),
        );
        let (config, set_config) = cx.create_signal(Arc::new(config));

        let focus = cx.create_rw_signal(Focus::Workbench);
        let completion = cx.create_rw_signal(CompletionData::new(cx, config));
        let inline_completion = cx.create_rw_signal(InlineCompletionData::new(cx));
        let hover = HoverData::new(cx);

        let register = cx.create_rw_signal(Register::default());
        let view_id = cx.create_rw_signal(ViewId::new());
        let find = Find::new(cx);

        let ui_line_height = cx.create_memo(move |_| {
            let config = config.get();
            let mut text_layout = TextLayout::new();

            let family: Vec<FamilyOwned> =
                FamilyOwned::parse_list(&config.ui.font_family).collect();
            let attrs = Attrs::new()
                .family(&family)
                .font_size(config.ui.font_size() as f32)
                .line_height(LineHeightValue::Normal(1.0));
            let attrs_list = AttrsList::new(attrs);
            text_layout.set_text("W", attrs_list, None);
            text_layout
                .size()
                .height
                .max(config.ui.list_line_height() as f64)
        });

        let (dart_lsp, dart_lsp_rx) = match &workspace.kind {
            SparrowWorkspaceType::Local => match workspace.path.clone() {
                Some(root) => DartLspService::spawn(root),
                None => DartLspService::spawn_unavailable(),
            },
            _ => DartLspService::spawn_unavailable(),
        };

        if let Some(root) = &workspace.path {
            if workspace.kind == SparrowWorkspaceType::Local {
                crate::dart::assets::start(root.clone());
            }
        }
        let signature_help =
            cx.create_rw_signal(crate::dart::SignatureHelpData::new(cx));

        let common = Rc::new(CommonData {
            workspace_undo: Default::default(),
            workspace: workspace.clone(),
            scope: cx,
            keypress,
            focus,
            completion,
            inline_completion,
            hover,
            dart_lsp: dart_lsp.clone(),
            signature_help,
            register,
            find,
            internal_command,
            sparrow_command,
            workbench_command,
            term_tx,
            term_notification_tx,
            proxy: proxy.proxy_rpc.clone(),
            view_id,
            ui_line_height,
            dragging: cx.create_rw_signal(None),
            workbench_size: cx.create_rw_signal(Size::ZERO),
            config,
            proxy_status,
            mouse_hover_timer: cx.create_rw_signal(TimerToken::INVALID),
            window_origin: cx.create_rw_signal(Point::ZERO),
            breakpoints: cx.create_rw_signal(BTreeMap::new()),
            keyboard_focus: cx.create_rw_signal(None),
            window_common: window_common.clone(),
        });

        let flutter_inspector = FlutterInspectorData::new(cx, common.clone());
        let main_split = MainSplitData::new(cx, common.clone());
        let code_action =
            cx.create_rw_signal(CodeActionData::new(cx, common.clone()));
        let source_control =
            SourceControlData::new(cx, main_split.editors, common.clone());
        let widget_tree = WidgetTreeData::new(cx, common.clone());
        let outline = OutlineData::new(cx, common.clone());
        let file_explorer =
            FileExplorerData::new(cx, main_split.editors, common.clone());

        if let Some(info) = workspace_info.as_ref() {
            let root_split = main_split.root_split;
            info.split.to_data(main_split.clone(), None, root_split);
        } else {
            let root_split = main_split.root_split;
            let root_split_data = {
                let cx = cx.create_child();
                let root_split_data = SplitData {
                    scope: cx,
                    parent_split: None,
                    split_id: root_split,
                    children: Vec::new(),
                    direction: SplitDirection::Horizontal,
                    window_origin: Point::ZERO,
                    layout_rect: Rect::ZERO,
                };
                cx.create_rw_signal(root_split_data)
            };
            main_split.splits.update(|splits| {
                splits.insert(root_split, root_split_data);
            });
        }

        let palette = PaletteData::new(
            cx,
            workspace.clone(),
            main_split.clone(),
            keypress.read_only(),
            source_control.clone(),
            common.clone(),
        );

        let title_height = cx.create_rw_signal(0.0);
        let status_height = cx.create_rw_signal(0.0);
        let panel_available_size = cx.create_memo(move |_| {
            let title_height = title_height.get();
            let status_height = status_height.get();
            let num_window_tabs = window_common.num_window_tabs.get();
            let window_size = window_common.size.get();
            Size::new(
                window_size.width,
                window_size.height
                    - title_height
                    - status_height
                    - if num_window_tabs > 1 {
                        window_common.window_tab_header_height.get()
                    } else {
                        0.0
                    },
            )
        });
        let panel = workspace_info
            .as_ref()
            .map(|i| {
                // Active indices belong to this workspace's saved order, not
                // the last order saved by a different workspace.
                let panel_order = i.panel.panels.clone();
                let (panel_order, styles) = normalize_panel_layout(panel_order, i.panel.styles.clone());
                PanelData {
                    panels: cx.create_rw_signal(panel_order),
                    styles: cx.create_rw_signal(styles),
                    size: cx.create_rw_signal(i.panel.size.clone()),
                    hidden_styles: cx.create_rw_signal(None),
                    last_opened: cx.create_rw_signal(Some(PanelKind::FileExplorer)),
                    available_size: panel_available_size,
                    sections: cx.create_rw_signal(
                        i.panel
                            .sections
                            .iter()
                            .map(|(key, value)| (*key, cx.create_rw_signal(*value)))
                            .collect(),
                    ),
                    common: common.clone(),
                }
            })
            .unwrap_or_else(|| {
                let panel_order = db
                    .get_panel_orders()
                    .unwrap_or_else(|_| default_panel_order());
                PanelData::new(
                    cx,
                    panel_order,
                    panel_available_size,
                    HashMap::new(),
                    common.clone(),
                )
            });

        let terminal = TerminalPanelData::new(
            workspace.clone(),
            common.config.get_untracked().terminal.get_default_profile(),
            common.clone(),
            main_split.clone(),
        );
        if let Some(workspace_info) = workspace_info.as_ref() {
            terminal.debug.breakpoints.set(
                workspace_info
                    .breakpoints
                    .clone()
                    .into_iter()
                    .map(|(path, breakpoints)| {
                        (
                            path,
                            breakpoints
                                .into_iter()
                                .map(|b| (b.line, b))
                                .collect::<BTreeMap<usize, SparrowBreakpoint>>(),
                        )
                    })
                    .collect(),
            );
        }

        let flutter_hot_reload =
            FlutterHotReloadData::new(cx, common.clone(), terminal.clone());

        let rename = RenameData::new(cx, main_split.editors, common.clone());
        let global_search = GlobalSearchData::new(cx, main_split.clone());

        let plugin = PluginData::new(
            cx,
            HashSet::from_iter(disabled_volts),
            HashSet::from_iter(workspace_disabled_volts),
            main_split.editors,
            common.clone(),
            proxy.core_rpc.clone(),
        );

        {
            let notification = create_signal_from_channel(term_notification_rx);
            let terminal = terminal.clone();
            cx.create_effect(move |_| {
                notification.with(|notification| {
                    if let Some(notification) = notification.as_ref() {
                        match notification {
                            TermNotification::SetTitle { term_id, title } => {
                                terminal.set_title(term_id, title);
                            }
                            TermNotification::RequestPaint => {
                                view_id.get_untracked().request_paint();
                            }
                            TermNotification::VmServiceUrl { term_id, url } => {
                                // Only connect for the terminal that flutter_hot_reload tracks,
                                // and only if the tab is actually in the Running state.
                                let tracked =
                                    flutter_hot_reload.active_term.get_untracked();
                                let running =
                                    flutter_hot_reload.state.get_untracked()
                                        == crate::dart::FlutterRunState::Running;
                                if running && tracked == Some(*term_id) {
                                    flutter_hot_reload.ws_uri.set(Some(url.to_string()));
                                    // Inspector connection is explicit; discovering a VM is not an inspection request.

                                }
                            }
                        }
                    }
                });
            });
        }

        let about_data = AboutData::new(cx, common.focus);
        let alert_data = AlertBoxData::new(cx, common.clone());

        let run_configs = cx.create_rw_signal(Vec::new());
        let active_run_config_idx = cx.create_rw_signal(0);
        let run_config_modal_visible = cx.create_rw_signal(false);

        let loaded_configs = workspace.path.as_deref().map(|root| {
            crate::dart::run_config::load(root).unwrap_or_else(|| crate::dart::run_config::discover_defaults(root))
        }).unwrap_or_default();
        run_configs.set(loaded_configs);

        let window_tab_data = Self {
            scope: cx,
            window_tab_id: WindowTabId::next(),
            workspace,
            palette,
            main_split: main_split.clone(),
            terminal,
            flutter_hot_reload: flutter_hot_reload.clone(),
            flutter_inspector: flutter_inspector.clone(),
            devtools: crate::dart::devtools::DevToolsData::new(cx),
            flutter_project: crate::dart::project_wizard::ProjectWizard::new(
                cx,
                common
                    .workspace
                    .path
                    .as_ref()
                    .and_then(|p| p.parent())
                    .unwrap_or(std::env::temp_dir().as_path())
                    .display()
                    .to_string(),
            ),
            flutter_test: crate::dart::test_service::FlutterTestData::new(
                cx,
                common.proxy.clone(),
            ),
            panel,
            file_explorer,
            code_action,
            code_lens: cx.create_rw_signal(None),
            source_control,
            dart_lsp: dart_lsp.clone(),
            widget_tree: widget_tree.clone(),
            outline: outline.clone(),
            plugin,
            rename,
            global_search,
            call_hierarchy_data: CallHierarchyData {
                root: cx.create_rw_signal(None),
                common: common.clone(),
                scroll_to_line: cx.create_rw_signal(None),
            },
            about_data,
            alert_data,
            settings_modal_visible: cx.create_rw_signal(false),
            layout_rect: cx.create_rw_signal(Rect::ZERO),
            title_height,
            status_height,
            proxy,
            set_config,
            update_in_progress: cx.create_rw_signal(false),
            progresses: cx.create_rw_signal(IndexMap::new()),
            messages: cx.create_rw_signal(Vec::new()),
            notifications: cx.create_rw_signal(Vec::new()),
            common,
            project_menu_visible: cx.create_rw_signal(false),
            branch_menu_visible: cx.create_rw_signal(false),
            device_menu_visible: cx.create_rw_signal(false),
            run_configs,
            active_run_config_idx,
            run_config_modal_visible,
        };

        {
            let settings = window_tab_data.settings_modal_visible;
            let run = window_tab_data.run_config_modal_visible;
            let wizard = window_tab_data.flutter_project.active;
            let focus = window_tab_data.common.focus;
            let root = window_tab_data.common.view_id;
            let previous_focus = cx.create_rw_signal(Focus::Workbench);
            cx.create_effect(move |_| {
                if settings.get() || run.get() || wizard.get() {
                    let current = focus.get_untracked();
                    if current != Focus::Modal {
                        // Command palettes and other transient overlays close
                        // when they open a dialog; do not restore their stale focus.
                        previous_focus.set(match current {
                            Focus::Workbench | Focus::Panel(_) => current,
                            _ => Focus::Workbench,
                        });
                    }
                    focus.set(Focus::Modal);
                } else if focus.get_untracked() == Focus::Modal {
                    focus.set(previous_focus.get_untracked());
                    root.get_untracked().request_focus();
                }
            });
        }

        // Terminal data owns the Flutter run session, independent of its view.
        {
            let terminal = window_tab_data.terminal.clone();
            let flutter = window_tab_data.flutter_hot_reload.clone();
            cx.create_effect(move |_| {
                let processes = terminal.run_debug_process(true);
                let session = processes.iter().rev().find(|(_, process)| {
                    !process.stopped
                        && (!process.is_prelaunch || process.config.prelaunch.is_none())
                        && crate::dart::run_config::is_flutter_target(&process.config)
                });
                if let Some((id, _)) = session {
                    flutter.active_term.set(Some(*id));
                    flutter.state.set(crate::dart::FlutterRunState::Running);
                } else if flutter.active_term.get_untracked().is_some() {
                    flutter.state.set(crate::dart::FlutterRunState::Idle);
                    flutter.ws_uri.set(None);
                }
            });
        }

        // Only a run-state transition opens the console. Device discovery and
        // manual panel toggles must not steal editor space in an idle workspace.
        {
            let state = flutter_hot_reload.state;
            let panel = window_tab_data.panel.clone();
            cx.create_effect(move |previous: Option<bool>| {
                let running = state.get() == crate::dart::FlutterRunState::Running;
                floem::reactive::untrack(|| {
                    if previous.is_none() && !running {
                        if let Some((PanelKind::FlutterRun, true)) = panel
                            .active_panel_at_position(
                                &crate::panel::position::PanelPosition::BottomLeft,
                                false,
                            )
                        {
                            panel.hide_panel(&PanelKind::FlutterRun);
                        }
                    }
                });
                running
            });
        }

        {
            let focus = window_tab_data.common.focus;
            let active_editor = window_tab_data.main_split.active_editor;
            let rename_active = window_tab_data.rename.active;
            let internal_command = window_tab_data.common.internal_command;
            cx.create_effect(move |_| {
                let focus = focus.get();
                active_editor.track();
                internal_command.send(InternalCommand::ResetBlinkCursor);

                if focus != Focus::Rename && rename_active.get_untracked() {
                    rename_active.set(false);
                }
            });
        }

        {
            let window_tab_data = window_tab_data.clone();
            window_tab_data.common.sparrow_command.listen(move |cmd| {
                window_tab_data.run_sparrow_command(cmd);
            });
        }

        {
            let window_tab_data = window_tab_data.clone();
            window_tab_data.common.workbench_command.listen(move |cmd| {
                window_tab_data.run_workbench_command(cmd, None);
            });
        }

        {
            let window_tab_data = window_tab_data.clone();
            let internal_command = window_tab_data.common.internal_command;
            internal_command.listen(move |cmd| {
                window_tab_data.run_internal_command(cmd);
            });
        }

        {
            let window_tab_data = window_tab_data.clone();
            let notification = window_tab_data.proxy.notification;
            cx.create_effect(move |_| {
                notification.with(|rpc| {
                    if let Some(rpc) = rpc.as_ref() {
                        window_tab_data.handle_core_notification(rpc);
                    }
                });
            });
        }

        // ── Widget tree: debounced refresh on editor/rev change ─────
        {
            let widget_tree = widget_tree.clone();
            let main_split_wt = main_split.clone();
            let dart_lsp_wt = dart_lsp.clone();
            let timer: RwSignal<Option<floem::action::TimerToken>> =
                cx.create_rw_signal(None);

            cx.create_effect(move |_| {
                let Some(editor) = main_split_wt.active_editor.get() else {
                    widget_tree.clear();
                    return;
                };
                let doc = editor.doc_signal().get();
                let path = doc.content.with_untracked(|c| c.path().cloned());
                let Some(path) = path else {
                    widget_tree.clear();
                    return;
                };
                if !crate::dart::is_dart_path(&path) {
                    widget_tree.clear();
                    return;
                }
                // Track loaded + rev so both async-load and edits trigger.
                let _ = doc.loaded.get();
                let _ = doc.buffer.with(|b| b.rev());
                widget_tree.schedule_refresh(
                    timer,
                    main_split_wt.clone(),
                    dart_lsp_wt.clone(),
                );
            });
        }

        {
            let widget_tree = widget_tree.clone();
            cx.create_effect(move |_| {
                widget_tree.updates.with(|notif| {
                    let Some((path, widgets)) = notif.as_ref() else {
                        return;
                    };
                    widget_tree.apply_update(path.clone(), widgets.clone());
                });
            });
        }

        // ── Dart LSP: forward buffer changes ─────────────────────────────
        {
            let dart_lsp = dart_lsp.clone();
            let main_split_effect = main_split.clone();
            cx.create_effect(move |_| {
                let Some(editor) = main_split_effect.active_editor.get() else {
                    return;
                };
                let doc = editor.doc_signal().get();
                let path = doc.content.with_untracked(|c| c.path().cloned());
                let Some(path) = path else { return };
                if !crate::dart::is_dart_path(&path) {
                    return;
                }
                let loaded = doc.loaded.get();
                if !loaded {
                    dart_lsp.sync_document(path, None);
                    return;
                }
                let version = doc.buffer.with(|b| b.rev() as i64);
                let content = doc.buffer.with(|b| b.to_string());
                let closing =
                    crate::dart::extract_closing_labels_from_source(&content);
                doc.closing_labels.set(Some(closing));
                dart_lsp.sync_document(path, Some((content, version)));
            });
        }

        // ── Dart LSP: apply diagnostics ──────────────────────────────────
        {
            use crate::dart::DartNotification;

            // Per-file highest applied LSP version, so out-of-order publishes are
            // dropped instead of flickering the editor back to stale diagnostics.
            let last_applied: Arc<
                parking_lot::Mutex<std::collections::HashMap<PathBuf, i64>>,
            > = Default::default();

            let notices = window_tab_data.notifications;
            let main_split_apply = main_split.clone();
            let notification = create_signal_from_channel(dart_lsp_rx);

            cx.create_effect(move |_| {
                notification.with(|notif| {
                    let Some(notif) = notif.as_ref() else { return };
                    match notif {
                        DartNotification::Diagnostics {
                            path,
                            version,
                            diagnostics,
                        } => {
                            // Version discipline: skip anything older than what we
                            // already applied. `-1` means "server didn't tell us",
                            // so we apply unconditionally in that case.
                            if *version >= 0 {
                                let mut guard = last_applied.lock();
                                if guard.get(path).copied().unwrap_or(-1) >= *version
                                {
                                    return;
                                }
                                guard.insert(path.clone(), *version);
                            }
                            let Some(doc) = main_split_apply
                                .docs
                                .with_untracked(|docs| docs.get(path).cloned())
                            else {
                                return;
                            };
                            let data = main_split_apply.get_diagnostic_data(path);
                            data.diagnostics.set(
                                diagnostics
                                    .iter()
                                    .cloned()
                                    .collect::<im::Vector<_>>(),
                            );
                            doc.init_diagnostics();
                        }
                        DartNotification::ClosingLabels { path, labels } => {
                            if let Some(doc) = main_split_apply
                                .docs
                                .with_untracked(|docs| docs.get(path).cloned())
                            {
                                doc.closing_labels.set(Some(labels.clone()));
                                doc.clear_text_cache();
                            }
                        }
                        DartNotification::Log(msg) => {
                            debug!("dart lsp: {msg}");
                        }
                        DartNotification::Error(e) => {
                            tracing::warn!("dart lsp: {e}");
                            crate::panel::workflow_tools::record(
                                notices,
                                "Dart analysis",
                                e,
                            );
                        }
                        DartNotification::ApplyEdit { id, edit } => {
                            let applied = edit.as_ref().is_some_and(|edit| main_split_apply.apply_dart_server_edit(edit));
                            main_split_apply.common.dart_lsp.respond_to_edit(id.clone(), applied);
                        }
                        DartNotification::Ready => {
                            crate::panel::workflow_tools::record(
                                notices,
                                "Dart analysis",
                                "Analysis server is ready.",
                            );
                            // LSP handshake done — request semantic tokens for every open Dart doc.
                            main_split_apply.docs.with_untracked(|docs| {
                                for (path, doc) in docs.iter() {
                                    if crate::dart::is_dart_path(path) {
                                        doc.get_semantic_styles();
                                    }
                                }
                            });
                        }
                        _ => {}
                    }
                });
            });
        }

        // ── Dart outline: debounced refresh on editor/rev change ──────
        {
            let outline = outline.clone();
            let main_split_outline = main_split.clone();
            let dart_lsp_outline = dart_lsp.clone();
            let timer: RwSignal<Option<floem::action::TimerToken>> =
                cx.create_rw_signal(None);

            cx.create_effect(move |_| {
                let Some(editor) = main_split_outline.active_editor.get() else {
                    outline.clear();
                    return;
                };
                let doc = editor.doc_signal().get();
                let path = doc.content.with_untracked(|c| c.path().cloned());
                let Some(path) = path else {
                    outline.clear();
                    return;
                };
                if !crate::dart::is_dart_path(&path) {
                    outline.clear();
                    return;
                }
                // Track loaded + rev so both async-load and edits trigger.
                let _ = doc.loaded.get();
                let _ = doc.buffer.with(|b| b.rev());
                outline.schedule_refresh(
                    timer,
                    main_split_outline.clone(),
                    dart_lsp_outline.clone(),
                );
            });
        }

        // ── Dart outline: apply completed analyses ───────────────────
        {
            let outline = outline.clone();
            cx.create_effect(move |_| {
                outline.updates.with(|notif| {
                    let Some((path, symbols)) = notif.as_ref() else {
                        return;
                    };
                    outline.apply_update(path.clone(), symbols.clone());
                });
            });
        }

        // ── Flutter hot reload: keep state in sync with the terminal ──
        {
            let hr = flutter_hot_reload.clone();
            cx.create_effect(move |_| {
                // Track tab_info so this fires when tabs are added / removed /
                // re-indexed.
                let _ = hr.terminal.tab_info.get();

                let Some(term_id) = hr.active_term.get() else {
                    return;
                };

                // Adapter-owned Flutter processes deliberately have no PTY.
                // The adapter session model owns their lifetime and console.
                if hr.terminal.debug.adapter_sessions.with(|sessions| {
                    sessions.values().any(|(id, _)| *id == term_id)
                }) { return; }

                match hr.terminal.get_terminal(&term_id) {
                    None => {
                        // Terminal was closed entirely — back to idle.
                        hr.active_term.set(None);
                        hr.state.set(crate::dart::FlutterRunState::Idle);
                    }
                    Some(terminal) => {
                        // `.with` (not `_untracked`) so we re-fire when the
                        // run_debug.stopped flag flips.
                        let stopped = terminal.run_debug.with(|r| {
                            r.as_ref().map(|p| p.stopped).unwrap_or(false)
                        });
                        if stopped
                            && hr.state.get_untracked()
                                != crate::dart::FlutterRunState::Error
                        {
                            hr.state.set(crate::dart::FlutterRunState::Error);
                        }
                    }
                }
            });
        }

        // ── Flutter inspector: apply VM service notifications ───────
        {
            let inspector = flutter_inspector.clone();
            cx.create_effect(move |_| {
                inspector.updates.with(|notif| {
                    let Some(notif) = notif.as_ref() else { return };
                    inspector.apply(notif);
                });
            });
        }

        // ── Flutter inspector: disconnect when the run stops ───────
        {
            let inspector = flutter_inspector.clone();
            let hot_reload = flutter_hot_reload.clone();
            cx.create_effect(move |_| {
                if hot_reload.state.get() == crate::dart::FlutterRunState::Idle
                    && inspector.state.get_untracked()
                        != crate::dart::InspectorState::Disconnected
                {
                    inspector.disconnect();
                }
            });
        }

        // ── Flutter inspector: connect when VM Service URL is received from daemon ──
        {
            let inspector = flutter_inspector.clone();
            let inspector_panel = window_tab_data.panel.clone();
            let hot_reload = flutter_hot_reload.clone();
            cx.create_effect(move |_| {
                if let Some(ws_uri) = hot_reload.ws_uri.get() {
                    let running = hot_reload.state.get()
                        == crate::dart::FlutterRunState::Running;
                    let _ = inspector_panel.styles.get();
                    if running && !ws_uri.is_empty() && inspector_panel.is_panel_visible(&PanelKind::FlutterInspector) {
                        let term_id = hot_reload
                            .active_term
                            .get_untracked()
                            .unwrap_or(sparrow_rpc::terminal::TermId(0));
                        inspector.on_vm_url(term_id, ws_uri);
                    }
                }
            });
        }

        window_tab_data
    }

    pub fn config_for_workspace(&self, workspace: &SparrowWorkspace) -> SparrowConfig {
        let db: Arc<SparrowDb> = use_context().unwrap();

        let disabled_volts = db.get_disabled_volts().unwrap_or_default();
        let workspace_disabled_volts = db
            .get_workspace_disabled_volts(workspace)
            .unwrap_or_default();
        let mut all_disabled_volts = disabled_volts;
        all_disabled_volts.extend(workspace_disabled_volts);

        SparrowConfig::load(
            workspace,
            &all_disabled_volts,
            &self.common.window_common.extra_plugin_paths,
        )
    }

    pub fn reload_config(&self) {
        let config = self.config_for_workspace(&self.workspace);
        self.common.keypress.update(|keypress| {
            keypress.update_keymaps(&config);
        });

        self.set_config.set(Arc::new(config));
    }

    pub fn run_sparrow_command(&self, cmd: SparrowCommand) {
        match cmd.kind {
            CommandKind::Workbench(command) => {
                self.run_workbench_command(command, cmd.data);
            }
            CommandKind::Scroll(_)
            | CommandKind::Focus(_)
            | CommandKind::Edit(_)
            | CommandKind::Move(_) => {
                if self.palette.status.get_untracked() != PaletteStatus::Inactive {
                    self.palette.run_command(&cmd, None, Modifiers::empty());
                } else if self.common.focus.get_untracked()
                    == Focus::Panel(PanelKind::FileExplorer)
                    && matches!(
                        cmd.kind,
                        CommandKind::Edit(
                            EditCommand::ClipboardCopy
                                | EditCommand::ClipboardCut
                                | EditCommand::ClipboardPaste
                        )
                    )
                {
                    self.file_explorer
                        .run_command(&cmd, None, Modifiers::empty());
                } else if let Some(editor_data) =
                    self.main_split.active_editor.get_untracked()
                {
                    editor_data.run_command(&cmd, None, Modifiers::empty());
                }
            }
            CommandKind::MotionMode(_) => {}
            CommandKind::MultiSelection(_) => {}
        }
    }

    pub fn run_workbench_command(
        &self,
        cmd: SparrowWorkbenchCommand,
        data: Option<Value>,
    ) {
        use SparrowWorkbenchCommand::*;
        match cmd {
            // ==== Modal ====
            EnableModal => {
                let internal_command = self.common.internal_command;
                internal_command.send(InternalCommand::SetModal { modal: true });
            }
            DisableModal => {
                let internal_command = self.common.internal_command;
                internal_command.send(InternalCommand::SetModal { modal: false });
            }

            // ==== Files / Folders ====
            OpenFolder => {
                if !self.workspace.kind.is_remote() {
                    let window_command = self.common.window_common.window_command;
                    let mut options = FileDialogOptions::new().title("Choose a folder").select_directories();
                    options = if let Some(parent) = self.workspace.path.as_ref().and_then(|x| x.parent()) {
                        options.force_starting_directory(parent)
                    } else {
                        options
                    };
                    open_file(options, move |file| {
                        if let Some(mut file) = file {
                            let workspace = SparrowWorkspace {
                                kind: SparrowWorkspaceType::Local,
                                path: Some(if let Some(path) = file.path.pop() {
                                    path
                                } else {
                                    error!("No path");
                                    return;
                                }),
                                last_open: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap()
                                    .as_secs(),
                            };
                            window_command
                                .send(WindowCommand::SetWorkspace { workspace });
                        }
                    });
                } else {
                    self.palette.run(PaletteKind::FileChooser);
                }
            }
            CloseFolder => {
                if !self.workspace.kind.is_remote() {
                    let window_command = self.common.window_common.window_command;
                    let workspace = SparrowWorkspace {
                        kind: SparrowWorkspaceType::Local,
                        path: None,
                        last_open: 0,
                    };
                    window_command.send(WindowCommand::SetWorkspace { workspace });
                }
            }
            OpenFile => {
                if !self.workspace.kind.is_remote() {
                    let internal_command = self.common.internal_command;
                    let options = FileDialogOptions::new().title("Choose a file");
                    open_file(options, move |file| {
                        if let Some(mut file) = file {
                            internal_command.send(InternalCommand::OpenFile {
                                path: if let Some(path) = file.path.pop() {
                                    path
                                } else {
                                    error!("No path");
                                    return;
                                },
                            })
                        }
                    });
                } else { // TODO: Force only choosing files?
                    self.palette.run(PaletteKind::FileChooser);
                }
            }
            NewFile => {
                self.main_split.new_file();
            }
            RevealActiveFileInFileExplorer => {
                if let Some(editor_data) = self.main_split.active_editor.get() {
                    let doc = editor_data.doc();
                    let path = if let DocContent::File { path, .. } =
                        doc.content.get_untracked()
                    {
                        Some(path)
                    } else {
                        None
                    };
                    let Some(path) = path else { return };
                    let path = path.parent().unwrap_or(&path);

                    open_uri(path);
                }
            }

            SaveAll => {
                let mut paths = HashSet::new();
                self.main_split.editors.with_editors_untracked(|editors| {
                    for (editor_id, editor_data) in editors.iter() {
                        // Defensive safeguard: `floem`'s `Editor::doc()` panics if the reactive scope was
                        // already destroyed. This catches "ghost" editors (like closed Settings or Plugin tabs)
                        // that leaked into the map, preventing "Save All" from crashing the entire app.
                        let doc_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            editor_data.doc()
                        }));

                        let doc = match doc_result {
                            Ok(document) => document,
                            Err(_) => {
                                tracing::warn!(
                                    "Ghost editor caught and bypassed during Save All. View ID: {:?}",
                                    editor_id
                                );
                                continue;
                            }
                        };

                        let should_save = if let DocContent::File { path, .. } =
                            doc.content.get_untracked()
                        {
                            if paths.contains(&path) {
                                false
                            } else {
                                paths.insert(path.clone());
                                true
                            }
                        } else {
                            false
                        };

                        if should_save {
                            editor_data.save(true, || {});
                        }
                    }
                });
                // Refactorings can update documents that have no editor tab.
                let docs=self.main_split.docs.with_untracked(|docs|docs.values().cloned().collect::<Vec<_>>());
                for doc in docs {
                    if doc.loaded() && !doc.is_pristine() && doc.content.with_untracked(|c|c.path().is_some_and(|p|!paths.contains(p))) {
                        doc.save(||{});
                    }
                }
            }

            // ==== Configuration / Info Files and Folders ====
            OpenSettings => {
                self.settings_modal_visible.set(true);
            }
            OpenSettingsFile => {
                if let Some(path) = SparrowConfig::settings_file() {
                    self.main_split.jump_to_location(
                        EditorLocation {
                            path,
                            position: None,
                            scroll_offset: None,
                            ignore_unconfirmed: false,
                            same_editor_tab: false,
                        },
                        None,
                    );
                }
            }
            OpenSettingsDirectory => {
                if let Some(dir) = Directory::config_directory() {
                    open_uri(&dir);
                }
            }
            OpenThemeColorSettings => {
                self.main_split.open_theme_color_settings();
            }
            OpenKeyboardShortcuts => {
                self.main_split.open_keymap();
            }
            OpenKeyboardShortcutsFile => {
                if let Some(path) = SparrowConfig::keymaps_file() {
                    self.main_split.jump_to_location(
                        EditorLocation {
                            path,
                            position: None,
                            scroll_offset: None,
                            ignore_unconfirmed: false,
                            same_editor_tab: false,
                        },
                        None,
                    );
                }
            }
            OpenLogFile => {
                if let Some(dir) = Directory::logs_directory() {
                    self.open_paths(&[PathObject::from_path(
                        dir.join(format!(
                            "sparrow.{}.log",
                            chrono::prelude::Local::now().format("%Y-%m-%d")
                        )),
                        false,
                    )])
                }
            }
            OpenLogsDirectory => {
                if let Some(dir) = Directory::logs_directory() {
                    open_uri(&dir);
                }
            }
            OpenProxyDirectory => {
                if let Some(dir) = Directory::proxy_directory() {
                    open_uri(&dir);
                }
            }
            OpenThemesDirectory => {
                if let Some(dir) = Directory::themes_directory() {
                    open_uri(&dir);
                }
            }
            OpenPluginsDirectory => {
                if let Some(dir) = Directory::plugins_directory() {
                    open_uri(&dir);
                }
            }
            OpenGrammarsDirectory => {
                if let Some(dir) = Directory::grammars_directory() {
                    open_uri(&dir);
                }
            }
            OpenQueriesDirectory => {
                if let Some(dir) = Directory::queries_directory() {
                    open_uri(&dir);
                }
            }

            InstallTheme => {}
            ExportCurrentThemeSettings => {
                self.main_split.export_theme();
            }
            ToggleInlayHints => {
                let enabled = !self.common.config.get_untracked().editor.enable_inlay_hints;
                // Keep an existing project override in its original scope.
                let project = self.workspace.path.as_ref().map(|root| root.join(".sparrow/settings.toml"));
                let target = project.filter(|path| {
                    std::fs::read_to_string(path).ok()
                        .and_then(|text| text.parse::<toml::Value>().ok())
                        .is_some_and(|value| value.get("editor").and_then(|e|e.get("enable-inlay-hints")).is_some())
                }).or_else(SparrowConfig::settings_file);
                if let Some(target) = target {
                    match SparrowConfig::update_scoped_settings(vec![(target, vec![(
                        "editor".into(), "enable-inlay-hints".into(), Some(enabled.into()),
                    )])]) {
                        Ok(()) => self.reload_config(),
                        Err(error) => self.show_alert("Inlay Hints".into(), error, Vec::new()),
                    }
                }
            }

            // ==== Window ====
            ReloadWindow => {
                self.common.window_common.window_command.send(
                    WindowCommand::SetWorkspace {
                        workspace: (*self.workspace).clone(),
                    },
                );
            }
            NewWindow => {
                self.common
                    .window_common
                    .window_command
                    .send(WindowCommand::NewWindow);
            }
            CloseWindow => {
                self.common
                    .window_common
                    .window_command
                    .send(WindowCommand::CloseWindow);
            }
            // ==== Window Tabs ====
            NewWindowTab => {
                self.common.window_common.window_command.send(
                    WindowCommand::NewWorkspaceTab {
                        workspace: SparrowWorkspace::default(),
                        end: false,
                    },
                );
            }
            CloseWindowTab => {
                self.common
                    .window_common
                    .window_command
                    .send(WindowCommand::CloseWorkspaceTab { index: None });
            }
            NextWindowTab => {
                self.common
                    .window_common
                    .window_command
                    .send(WindowCommand::NextWorkspaceTab);
            }
            PreviousWindowTab => {
                self.common
                    .window_common
                    .window_command
                    .send(WindowCommand::PreviousWorkspaceTab);
            }

            // ==== Editor Tabs ====
            NextEditorTab => {
                if let Some(editor_tab_id) =
                    self.main_split.active_editor_tab.get_untracked()
                {
                    self.main_split.editor_tabs.with_untracked(|editor_tabs| {
                        let Some(editor_tab) = editor_tabs.get(&editor_tab_id)
                        else {
                            return;
                        };

                        let new_index = editor_tab.with_untracked(|editor_tab| {
                            if editor_tab.children.is_empty() {
                                None
                            } else if editor_tab.active
                                == editor_tab.children.len() - 1
                            {
                                Some(0)
                            } else {
                                Some(editor_tab.active + 1)
                            }
                        });

                        if let Some(new_index) = new_index {
                            editor_tab.update(|editor_tab| {
                                editor_tab.active = new_index;
                            });
                        }
                    });
                }
            }
            PreviousEditorTab => {
                if let Some(editor_tab_id) =
                    self.main_split.active_editor_tab.get_untracked()
                {
                    self.main_split.editor_tabs.with_untracked(|editor_tabs| {
                        let Some(editor_tab) = editor_tabs.get(&editor_tab_id)
                        else {
                            return;
                        };

                        let new_index = editor_tab.with_untracked(|editor_tab| {
                            if editor_tab.children.is_empty() {
                                None
                            } else if editor_tab.active == 0 {
                                Some(editor_tab.children.len() - 1)
                            } else {
                                Some(editor_tab.active - 1)
                            }
                        });

                        if let Some(new_index) = new_index {
                            editor_tab.update(|editor_tab| {
                                editor_tab.active = new_index;
                            });
                        }
                    });
                }
            }

            // ==== File Explorer ====
            FileExplorerRename => {
                self.file_explorer.rename_selected();
            }
            FileExplorerTrash => {
                self.file_explorer.trash_selected();
            }

            // ==== Terminal ====
            NewTerminalTab => {
                self.terminal.new_tab(
                    self.common
                        .config
                        .get_untracked()
                        .terminal
                        .get_default_profile(),
                );
                if !self.panel.is_panel_visible(&PanelKind::Terminal) {
                    self.panel.show_panel(&PanelKind::Terminal);
                }
                self.common.focus.set(Focus::Panel(PanelKind::Terminal));
            }
            FlutterDebugTestAtCursor => self.run_test_at_cursor(true),
            FlutterRunTestFile => {
                if let Some(editor) = self.main_split.active_editor.get_untracked() {
                    if let Some(path) = editor.doc().content.with_untracked(|c| c.path().cloned()) {
                        self.prepare_flutter_tests(vec![(path, String::new())], false, false);
                    }
                }
            }
            FlutterRerunFailedTests => self.rerun_flutter_tests(true),
            FlutterRunTestAtCursor => {
                self.run_test_at_cursor(false);
            }
            CloseTerminalTab => {
                self.terminal.close_tab(None);
                if self
                    .terminal
                    .tab_info
                    .with_untracked(|info| info.tabs.is_empty())
                {
                    if self.panel.is_panel_visible(&PanelKind::Terminal) {
                        self.panel.hide_panel(&PanelKind::Terminal);
                    }
                    self.common.focus.set(Focus::Workbench);
                } else {
                    if !self.panel.is_panel_visible(&PanelKind::Terminal) {
                        self.panel.show_panel(&PanelKind::Terminal);
                    }
                    self.common.focus.set(Focus::Panel(PanelKind::Terminal));
                }
            }
            NextTerminalTab => {
                self.terminal.next_tab();
                if !self.panel.is_panel_visible(&PanelKind::Terminal) {
                    self.panel.show_panel(&PanelKind::Terminal);
                }
                self.common.focus.set(Focus::Panel(PanelKind::Terminal));
            }

            PreviousTerminalTab => {
                self.terminal.previous_tab();
                if !self.panel.is_panel_visible(&PanelKind::Terminal) {
                    self.panel.show_panel(&PanelKind::Terminal);
                }
                self.common.focus.set(Focus::Panel(PanelKind::Terminal));
            }

            // ==== Remote ====
            ConnectSshHost => {
                self.palette.run(PaletteKind::SshHost);
            }
            #[cfg(windows)]
            ConnectWslHost => {
                self.palette.run(PaletteKind::WslHost);
            }
            DisconnectRemote => {
                self.common.window_common.window_command.send(
                    WindowCommand::SetWorkspace {
                        workspace: SparrowWorkspace {
                            kind: SparrowWorkspaceType::Local,
                            path: None,
                            last_open: 0,
                        },
                    },
                );
            }

            // ==== Palette Commands ====
            PaletteHelp => self.palette.run(PaletteKind::PaletteHelp),
            PaletteHelpAndFile => self.palette.run(PaletteKind::HelpAndFile),
            PaletteLine => {
                self.palette.run(PaletteKind::Line);
            }
            Palette => {
                self.palette.run(PaletteKind::File);
            },
            PaletteSymbol => {
                self.palette.run(PaletteKind::DocumentSymbol);
            }
            PaletteWorkspaceSymbol => {}
            PaletteCommand => {
                self.palette.run(PaletteKind::Command);
            }
            PaletteWorkspace => {
                self.palette.run(PaletteKind::Workspace);
            }
            PaletteRunAndDebug => {
                self.palette.run(PaletteKind::RunAndDebug);
            }
            PaletteSCMReferences => {
                self.palette.run(PaletteKind::SCMReferences);
            }
            ChangeColorTheme => {
                self.palette.run(PaletteKind::ColorTheme);
            }
            ChangeIconTheme => {
                self.palette.run(PaletteKind::IconTheme);
            }
            ChangeFileLanguage => {
                self.palette.run(PaletteKind::Language);
            }
            ChangeFileLineEnding => {
                self.palette.run(PaletteKind::LineEnding);
            }
            DiffFiles => self.palette.run(PaletteKind::DiffFiles),

            // ==== Running / Debugging ====
            RunAndDebugRestart => {
                let active_term = self.terminal.debug.active_term.get_untracked();
                if let Some(is_debug) = active_term
                    .and_then(|term_id| self.terminal.restart_run_debug(term_id))
                {
                    let panel = self.terminal.debug.active_term.get_untracked()
                        .and_then(|id| self.terminal.get_terminal(&id))
                        .and_then(|terminal| terminal.run_debug.get_untracked())
                        .map(|process| crate::dart::run_config::console_panel(process.mode, &process.config))
                        .unwrap_or(PanelKind::Terminal);
                    self.panel.show_panel(&panel);
                    if is_debug {
                        self.panel.show_panel(&PanelKind::Debug);
                    }
                } else {
                    self.palette.run(PaletteKind::RunAndDebug);
                }
            }
            RunAndDebugStop => {
                let active = self.terminal.debug.active_term.get_untracked();
                let processes = self.terminal.run_debug_process(false);
                let selected = self.run_configs.with_untracked(|configs| configs.get(self.active_run_config_idx.get_untracked()).map(|config| config.name.clone()));
                let term_id = active.filter(|id| processes.iter().any(|(term, process)| term == id && !process.stopped))
                    .or_else(|| processes.iter().find(|(_, process)| !process.stopped && Some(&process.config.name) == selected.as_ref()).map(|(term, _)| *term))
                    .or_else(|| processes.iter().find(|(_, process)| !process.stopped).map(|(term, _)| *term));
                if let Some(term_id) = term_id {
                    self.terminal.stop_run_debug(term_id);
                } else if self.flutter_hot_reload.state.get_untracked() == crate::dart::FlutterRunState::Running {
                    self.flutter_hot_reload.stop();
                }
            }

            // ==== UI ====
            ZoomIn => {
                let mut scale =
                    self.common.window_common.window_scale.get_untracked();
                scale += 0.1;
                if scale > 4.0 {
                    scale = 4.0
                }
                self.common.window_common.window_scale.set(scale);

                SparrowConfig::update_file(
                    "ui",
                    "scale",
                    toml_edit::Value::from(scale),
                );
            }
            ZoomOut => {
                let mut scale =
                    self.common.window_common.window_scale.get_untracked();
                scale -= 0.1;
                if scale < 0.1 {
                    scale = 0.1
                }
                self.common.window_common.window_scale.set(scale);

                SparrowConfig::update_file(
                    "ui",
                    "scale",
                    toml_edit::Value::from(scale),
                );
            }
            ZoomReset => {
                self.common.window_common.window_scale.set(1.0);

                SparrowConfig::update_file(
                    "ui",
                    "scale",
                    toml_edit::Value::from(1.0),
                );
            }

            ToggleMaximizedPanel => {
                if let Some(data) = data {
                    if let Ok(kind) = serde_json::from_value::<PanelKind>(data) {
                        self.panel.toggle_maximize(&kind);
                    }
                } else {
                    self.panel.toggle_active_maximize();
                }
            }
            HidePanel => {
                let kind = data
                    .and_then(|data| serde_json::from_value::<PanelKind>(data).ok())
                    .or_else(|| match self.common.focus.get_untracked() {
                        Focus::Panel(kind) => Some(kind),
                        _ => self.panel.last_opened.get_untracked()
                            .filter(|kind| self.panel.is_panel_visible(kind)),
                    });
                if let Some(kind) = kind {
                    self.hide_panel(kind);
                }
            }
            ToggleAllToolWindows => {
                self.panel.toggle_all_tool_windows();
                self.common.focus.set(Focus::Workbench);
                self.common.view_id.get_untracked().request_focus();
            }
            ShowPanel => {
                if let Some(data) = data {
                    if let Ok(kind) = serde_json::from_value::<PanelKind>(data) {
                        self.show_panel(kind);
                    }
                }
            }
            TogglePanelFocus => {
                if let Some(data) = data {
                    if let Ok(kind) = serde_json::from_value::<PanelKind>(data) {
                        self.toggle_panel_focus(kind);
                    }
                }
            }
            TogglePanelVisual => {
                if let Some(data) = data {
                    if let Ok(kind) = serde_json::from_value::<PanelKind>(data) {
                        self.toggle_panel_visual(kind);
                    }
                }
            }
            TogglePanelLeftVisual => {
                self.toggle_container_visual(&PanelContainerPosition::Left);
            }
            TogglePanelRightVisual => {
                self.toggle_container_visual(&PanelContainerPosition::Right);
            }
            TogglePanelBottomVisual => {
                self.toggle_container_visual(&PanelContainerPosition::Bottom);
            }
            ToggleTerminalFocus => {
                self.toggle_panel_focus(PanelKind::Terminal);
            }
            ToggleSourceControlFocus => {
                self.toggle_panel_focus(PanelKind::SourceControl);
            }
            TogglePluginFocus => {
                self.toggle_panel_focus(PanelKind::Plugin);
            }
            ToggleFileExplorerFocus => {
                self.toggle_panel_focus(PanelKind::FileExplorer);
            }
            ToggleProblemFocus => {
                self.toggle_panel_focus(PanelKind::Problem);
            }
            ToggleSearchFocus => {
                self.toggle_panel_focus(PanelKind::Search);
            }
            ActivateRunToolWindow => self.toggle_panel_focus(PanelKind::FlutterRun),
            ActivateDebugToolWindow => self.toggle_panel_focus(PanelKind::Debug),
            ActivateProblemsToolWindow => self.toggle_panel_focus(PanelKind::Problems),
            ActivateStructureToolWindow => self.toggle_panel_focus(PanelKind::DartOutline),
            ActivateGitToolWindow => self.toggle_panel_focus(PanelKind::GitHistory),
            ToggleTerminalVisual => {
                self.toggle_panel_visual(PanelKind::Terminal);
            }
            ToggleSourceControlVisual => {
                self.toggle_panel_visual(PanelKind::SourceControl);
            }
            TogglePluginVisual => {
                self.toggle_panel_visual(PanelKind::Plugin);
            }
            ToggleFileExplorerVisual => {
                self.toggle_panel_visual(PanelKind::FileExplorer);
            }
            ToggleProblemVisual => {
                self.toggle_panel_visual(PanelKind::Problem);
            }
            ToggleDebugVisual => {
                self.toggle_panel_visual(PanelKind::Debug);
            }
            ToggleSearchVisual => {
                self.toggle_panel_visual(PanelKind::Search);
            }
            FocusEditor => {
                self.common.focus.set(Focus::Workbench);
                self.common.view_id.get_untracked().request_focus();
            }
            FocusTerminal => {
                self.common.focus.set(Focus::Panel(PanelKind::Terminal));
            }
            OpenUIInspector => {
                self.common.view_id.get_untracked().inspect();
            }
            ShowEnvironment => {
                self.main_split.show_env();
            }

            // ==== Source Control ====
            SourceControlInit => {
                self.proxy.proxy_rpc.git_init();
            }
            CheckoutReference => match data {
                Some(reference) => {
                    if let Some(reference) = reference.as_str() {
                        self.proxy.proxy_rpc.git_checkout(reference.to_string());
                    }
                }
                None => error!("No ref provided"),
            },
            SourceControlCommit => {
                self.source_control.commit();
            }
            SourceControlCopyActiveFileRemoteUrl => {}
            SourceControlDiscardActiveFileChanges => {}
            SourceControlDiscardTargetFileChanges => {
                if let Some(diff) = data
                    .and_then(|data| serde_json::from_value::<FileDiff>(data).ok())
                {
                    match diff {
                        FileDiff::Added(path) => {
                            self.common.proxy.trash_path(path, Box::new(|_| {}));
                        }
                        FileDiff::Modified(path) | FileDiff::Deleted(path) => {
                            self.common.proxy.git_discard_files_changes(vec![path]);
                        }
                        FileDiff::Renamed(old_path, new_path) => {
                            self.common
                                .proxy
                                .git_discard_files_changes(vec![old_path]);
                            self.common.proxy.trash_path(new_path, Box::new(|_| {}));
                        }
                    }
                }
            }
            SourceControlDiscardWorkspaceChanges => {}

            // ==== UI ====
            ShowAbout => {
                self.about_data.open();
            }

            // ==== Updating ====
            RestartToUpdate => {
                if let Some(release) = self
                    .common
                    .window_common
                    .latest_release
                    .get_untracked()
                    .as_ref()
                {
                    let release = release.clone();
                    let update_in_progress = self.update_in_progress;
                    if release.version != *meta::VERSION {
                        if let Ok(process_path) = env::current_exe() {
                            update_in_progress.set(true);
                            let send = create_ext_action(
                                self.common.scope,
                                move |_started| {
                                    update_in_progress.set(false);
                                },
                            );
                            std::thread::Builder::new().name("RestartToUpdate".to_owned()).spawn(move || {
                                let do_update = || -> anyhow::Result<()> {
                                    let src =
                                        crate::update::download_release(&release)?;

                                    let path =
                                        crate::update::extract(&src, &process_path)?;

                                    crate::update::restart(&path)?;

                                    Ok(())
                                };

                                if let Err(err) = do_update() {
                                    error!("Failed to update: {err}");
                                }

                                send(false);
                            }).unwrap();
                        }
                    }
                }
            }

            // ==== Movement ====
            #[cfg(target_os = "macos")]
            InstallToPATH => {
                self.common.internal_command.send(
                    InternalCommand::ExecuteProcess {
                        program: String::from("osascript"),
                        arguments: vec![String::from("-e"), format!(r#"do shell script "ln -sf '{}' /usr/local/bin/sparrow" with administrator privileges"#, std::env::args().next().unwrap())],
                    }
                )
            }
            #[cfg(target_os = "macos")]
            UninstallFromPATH => {
                self.common.internal_command.send(
                    InternalCommand::ExecuteProcess {
                        program: String::from("osascript"),
                        arguments: vec![String::from("-e"), String::from(r#"do shell script "rm /usr/local/bin/sparrow" with administrator privileges"#)],
                    }
                )
            }
            JumpLocationForward => {
                self.main_split.jump_location_forward(false);
            }
            JumpLocationBackward => {
                self.main_split.jump_location_backward(false);
            }
            JumpLocationForwardLocal => {
                self.main_split.jump_location_forward(true);
            }
            JumpLocationBackwardLocal => {
                self.main_split.jump_location_backward(true);
            }
            NextError => {
                self.main_split.next_error();
            }
            PreviousError => self.main_split.previous_error(),
            Quit => {
                floem::quit_app();
            }
            RevealInPanel => {
                if let Some(editor_data) =
                    self.main_split.active_editor.get_untracked()
                {
                    self.show_panel(PanelKind::FileExplorer);
                    self.panel
                        .section_open(PanelSection::FileExplorer).update(|x| {
                        *x = true;
                    });
                    if let DocContent::File {path, ..} = editor_data.doc().content.get_untracked() {
                        self.file_explorer.reveal_in_file_tree(path);
                    }
                }
            }
            SourceControlOpenActiveFileRemoteUrl => {
                if let Some(editor_data) =
                    self.main_split.active_editor.get_untracked()
                {
                    if let DocContent::File {path, ..} = editor_data.doc().content.get_untracked() {
                        let offset = editor_data.cursor().with_untracked(|c| c.offset());
                        let line = editor_data.doc()
                            .buffer
                            .with_untracked(|buffer| buffer.line_of_offset(offset));
                        self.common.proxy.git_get_remote_file_url(
                            path,
                            create_ext_action(self.scope, move |result| {
                                if let Ok(ProxyResponse::GitGetRemoteFileUrl {
                                              file_url
                                          }) = result
                                {
                                    if let Err(err) = open::that(format!("{}#L{}", file_url, line)) {
                                        error!("Failed to open file in github: {}",  err);
                                    }
                                }
                            }),
                        );

                    }
                }
            }
            RevealInFileExplorer => {
                if let Some(editor_data) =
                    self.main_split.active_editor.get_untracked()
                {
                    if let DocContent::File {path, ..} = editor_data.doc().content.get_untracked() {
                        let path = path.parent().unwrap_or(&path);
                        if !path.exists() {
                            return;
                        }
                        if let Err(err) = open::that(path) {
                            error!(
                            "Failed to reveal file in system file explorer: {}",
                            err
                        );
                        }
                    }
                }
            }
            ShowCallHierarchy => {
                if let Some(editor_data) =
                    self.main_split.active_editor.get_untracked()
                {
                    editor_data.call_hierarchy(self.clone());
                }
            }
            FindReferences => {
                if let Some(editor_data) =
                    self.main_split.active_editor.get_untracked()
                {
                    editor_data.find_reference(self.clone());
                }
            }
            GoToImplementation => {
                if let Some(editor_data) =
                    self.main_split.active_editor.get_untracked()
                {
                    editor_data.go_to_implementation(self.clone());
                }
            }
            RunInTerminal => {
                if let Some(editor_data) =
                    self.main_split.active_editor.get_untracked()
                {
                    let name = editor_data.word_at_cursor();
                    if !name.is_empty() {
                        let mut args_str = name.split(" ");
                        let program = args_str.next().map(|x| x.to_string()).unwrap();
                        let args: Vec<String> = args_str.map(|x| x.to_string()).collect();
                        let args = if args.is_empty() {
                            None
                        } else {
                            Some(args)
                        };

                        let config = RunDebugConfig {
                            ty: None,
                            name,
                            program,
                            args,
                            cwd: None,
                            env: None,
                            prelaunch: None,
                before_launch: Vec::new(),
                store_in_project: false,
                            flutter_options: Default::default(),
                            debug_adapter: None,
                            debug_command: None,
                            dap_id: Default::default(),
                            tracing_output: false,
                            config_source: ConfigSource::RunInTerminal,
                        };
                        self.common
                            .internal_command
                            .send(InternalCommand::RunAndDebug { mode: RunDebugMode::Run, config });
                    }
                }
            }
            GoToLocation => {
                if let Some(editor_data) =
                    self.main_split.active_editor.get_untracked()
                {
                    let doc = editor_data.doc();
                    let path = match if doc.loaded() {
                        doc.content.with_untracked(|c| c.path().cloned())
                    } else {
                        None
                    } {
                        Some(path) => path,
                        None => return,
                    };
                    let offset = editor_data.cursor().with_untracked(|c| c.offset());
                    let internal_command = self.common.internal_command;

                    internal_command.send(InternalCommand::MakeConfirmed);
                    internal_command.send(InternalCommand::GoToLocation { location: EditorLocation {
                        path,
                        position: Some(EditorPosition::Offset(offset)),
                        scroll_offset: None,
                        ignore_unconfirmed: false,
                        same_editor_tab: false,
                    } });
                }
            }
            AddRunDebugConfig => {
                if let Some(editor_data) =
                    self.main_split.active_editor.get_untracked()
                {
                    editor_data.receive_char(DEFAULT_RUN_TOML);
                }
            }

            JumpHighlightNext => {
                if let Some(editor_data) =
                    self.main_split.active_editor.get_untracked()
                {
                    editor_data.jump_highlight(InlineFindDirection::Right);
                }
            }
            JumpHighlightPrev => {
                if let Some(editor_data) =
                    self.main_split.active_editor.get_untracked()
                {
                    editor_data.jump_highlight(InlineFindDirection::Left);
                }
            }
            RunCurrentFile | DebugCurrentFile => {
                let path = data.clone().and_then(|value| serde_json::from_value::<PathBuf>(value).ok())
                    .or_else(|| self.main_split.active_editor.get_untracked()
                        .and_then(|editor| editor.doc().content.with_untracked(|content| content.path().cloned())));
                if let Some(path) = path {
                    if crate::dart::test_runner::is_test_file(&path)
                        && crate::dart::test_runner::project_root(&path).is_some_and(|root| crate::dart::is_flutter_project(&root)) {
                        self.prepare_flutter_tests(vec![(path, String::new())], cmd == DebugCurrentFile, false);
                        return;
                    }
                    match crate::dart::run_config::configuration_for_file(&path, self.workspace.path.as_deref()) {
                        Ok(config) => self.common.internal_command.send(InternalCommand::RunAndDebug {
                            mode: if cmd == DebugCurrentFile { RunDebugMode::Debug } else { RunDebugMode::Run }, config,
                        }),
                        Err(message) => self.show_alert("Run configuration".into(), message, Vec::new()),
                    }
                }
            }
            EditRunConfigurations => self.run_config_modal_visible.set(true),
            OptimizeImports => {
                if let Some(editor) = self.main_split.active_editor.get_untracked() {
                    let doc = editor.doc();
                    let path = doc.content.with_untracked(|content| content.path().cloned());
                    if let Some(path) = path.filter(|path| crate::dart::is_dart_path(path)) {
                        if !self.common.dart_lsp.is_available() {
                            self.show_alert("Optimize Imports".into(), "The Dart analysis server is not ready yet.".into(), Vec::new());
                            return;
                        }
                        let rev = doc.rev();
                        let commands = self.common.internal_command;
                        let apply = create_ext_action(self.scope, move |actions: Vec<CodeActionOrCommand>| {
                            if doc.rev() != rev { return; }
                            if let Some(action) = actions.into_iter().find(|action| match action {
                                CodeActionOrCommand::CodeAction(action) => action.disabled.is_none(),
                                CodeActionOrCommand::Command(_) => true,
                            }) {
                                commands.send(InternalCommand::RunCodeAction { plugin_id: PluginId(0), action });
                            }
                        });
                        self.common.dart_lsp.code_actions_with_kind(&path, 0, 0, &[], Some("source.organizeImports"), move |actions| apply(actions));
                    }
                }
            }
            RenameActiveFile => {
                let path = data.and_then(|value| serde_json::from_value::<PathBuf>(value).ok())
                    .or_else(|| self.main_split.active_editor.get_untracked().and_then(|editor| editor.doc().content.with_untracked(|content| content.path().cloned())));
                if let Some(path) = path {
                    self.show_panel(PanelKind::FileExplorer);
                    self.common.focus.set(Focus::Panel(PanelKind::FileExplorer));
                    self.file_explorer.reveal_in_file_tree(path.clone());
                    self.file_explorer.rename_path(path);
                }
            }
            ToggleBreakpoint => {
                if let Some(editor) = self.main_split.active_editor.get_untracked() {
                    let doc = editor.doc();
                    let offset = editor.cursor().with_untracked(|cursor| cursor.offset());
                    let line = doc.buffer.with_untracked(|buffer| buffer.line_of_offset(offset));
                    crate::editor::view::toggle_gutter_breakpoint(line, editor.doc_signal(),
                        self.terminal.debug.daps, self.terminal.debug.breakpoints, self.common.clone());
                }
            }
            RunSelectedConfiguration | DebugSelectedConfiguration => {
                let mode = if cmd == RunSelectedConfiguration { RunDebugMode::Run } else { RunDebugMode::Debug };
                self.launch_selected_configuration(mode);
            }
            FlutterRun => self.launch_selected_configuration(RunDebugMode::Run),
            DartPubGet => {
                if let Some(root)=self.workspace.path.as_deref() {
                    let notifications=self.notifications;
                    crate::panel::workflow_tools::record(notifications,"Pub Get","Resolving package dependencies…");
                    let send=create_ext_action(self.scope,move |result:Result<(),String>|{
                        let message=match result {Ok(())=>"Package dependencies resolved.".into(),Err(error)=>error};
                        crate::panel::workflow_tools::record(notifications,"Pub Get",&message);
                    });
                    crate::dart::run_pub_get(root,Some(Box::new(move |result|send(result))));
                }
            }
            FlutterHotReload => {
                self.reload_flutter_after_saving(false);
            }
            FlutterHotRestart => {
                self.reload_flutter_after_saving(true);
            }
            FlutterStop => {
                self.flutter_hot_reload.stop();
            }
            FlutterNewProject => self.flutter_project.active.set(true),
            FlutterOpenDevTools | FlutterDevToolsMemory | FlutterDevToolsNetwork => {
                let page = match cmd {
                    FlutterDevToolsMemory => crate::dart::devtools::DevToolsPage::Memory,
                    FlutterDevToolsNetwork => crate::dart::devtools::DevToolsPage::Network,
                    _ => crate::dart::devtools::DevToolsPage::Home,
                };
                self.open_devtools(page);
            }
            AndroidStudioReferenceLayout => self.apply_android_studio_layout(),
            PubAdd => {
                self.palette.run(PaletteKind::PubPackage);
            }
        }
    }

    pub fn run_internal_command(&self, cmd: InternalCommand) {
        let cx = self.scope;
        match cmd {
            InternalCommand::ReloadConfig => {
                self.reload_config();
            }
            InternalCommand::UpdateLogLevel { level } => {
                // TODO: implement logging panel, runtime log level change
                debug!("{level}");
            }
            InternalCommand::MakeConfirmed => {
                if let Some(editor) = self.main_split.active_editor.get_untracked() {
                    editor.confirmed.set(true);
                }
            }
            InternalCommand::OpenFile { path } => {
                self.main_split.jump_to_location(
                    EditorLocation {
                        path,
                        position: None,
                        scroll_offset: None,
                        ignore_unconfirmed: false,
                        same_editor_tab: false,
                    },
                    None,
                );
            }
            InternalCommand::OpenLibraryFile { path, read_only } => {
                if read_only {
                    let (doc, _) = self.main_split.get_doc(path.clone(), None);
                    doc.content.update(|content| {
                        if let DocContent::File {read_only, ..} = content { *read_only = true; }
                    });
                }
                self.run_internal_command(InternalCommand::OpenAndConfirmedFile { path });
            }
            InternalCommand::OpenAndConfirmedFile { path } => {
                self.main_split.jump_to_location(
                    EditorLocation {
                        path,
                        position: None,
                        scroll_offset: None,
                        ignore_unconfirmed: false,
                        same_editor_tab: false,
                    },
                    None,
                );
                if let Some(editor) = self.main_split.active_editor.get_untracked() {
                    editor.confirmed.set(true);
                }
            }
            InternalCommand::OpenFileInNewTab { path } => {
                self.main_split.jump_to_location(
                    EditorLocation {
                        path,
                        position: None,
                        scroll_offset: None,
                        ignore_unconfirmed: true,
                        same_editor_tab: false,
                    },
                    None,
                );
            }
            InternalCommand::OpenFileChanges { path } => {
                self.main_split.open_file_changes(path);
            }
            InternalCommand::ReloadFileExplorer => {
                self.file_explorer.reload();
            }
            InternalCommand::TestPathCreation { new_path } => {
                let naming = self.file_explorer.naming;

                let send = create_ext_action(
                    self.scope,
                    move |response: Result<ProxyResponse, RpcError>| match response {
                        Ok(_) => {
                            naming.update(Naming::set_ok);
                        }
                        Err(err) => {
                            naming.update(|naming| naming.set_err(err.message));
                        }
                    },
                );

                self.common.proxy.test_create_at_path(new_path, send);
            }
            InternalCommand::FinishRenamePath {
                current_path,
                new_path,
            } => {
                let send_current_path = current_path.clone();
                let send_new_path = new_path.clone();
                let file_explorer = self.file_explorer.clone();
                let editors = self.main_split.editors;
                let docs = self.main_split.docs;
                let diagnostics = self.main_split.diagnostics;
                let dart_lsp = self.common.dart_lsp.clone();


                let send = create_ext_action(
                    self.scope,
                    move |response: Result<ProxyResponse, RpcError>| match response {
                        Ok(response) => {
                            // Get the canonicalized new path from the proxy.
                            let new_path =
                                if let ProxyResponse::CreatePathResponse { path } =
                                    response
                                {
                                    path
                                } else {
                                    send_new_path
                                };

                            // If the renamed item is a file, update any editors the file is open
                            // in to use the new path.
                            // If the renamed item is a directory, update any editors in which a
                            // file the renamed directory is an ancestor of is open to use the
                            // file's new path.
                            let renamed_editors_content: Vec<_> = editors
                                .with_editors_untracked(|editors| {
                                    editors
                                        .values()
                                        .map(|editor| editor.doc().content)
                                        .filter(|content| {
                                            content.with_untracked(|content| {
                                                match content {
                                                    DocContent::File {
                                                        path,
                                                        ..
                                                    } => path.starts_with(
                                                        &send_current_path,
                                                    ),
                                                    _ => false,
                                                }
                                            })
                                        })
                                        .collect()
                                });

                            for content in renamed_editors_content {
                                content.update(|content| {
                                    if let DocContent::File { path, .. } = content {
                                        if let Ok(suffix) =
                                            path.strip_prefix(&send_current_path)
                                        {
                                            *path = new_path.join(suffix);
                                        }
                                    }
                                });
                            }

                            // Rekey caches as well as editor content. Otherwise reopening the
                            // old path can return the moved document under its stale key.
                            docs.update(|docs| {
                                let paths = docs.keys().filter(|path| path.starts_with(&send_current_path)).cloned().collect::<Vec<_>>();
                                for old in paths {
                                    if let Some(doc) = docs.remove(&old) {
                                        let target = new_path.join(old.strip_prefix(&send_current_path).expect("matching prefix"));
                                        dart_lsp.send_close(old);
                                        docs.insert(target, doc);
                                    }
                                }
                            });
                            diagnostics.update(|diagnostics| {
                                let paths = diagnostics.keys().filter(|path| path.starts_with(&send_current_path)).cloned().collect::<Vec<_>>();
                                for old in paths {
                                    if let Some(data) = diagnostics.remove(&old) {
                                        diagnostics.insert(new_path.join(old.strip_prefix(&send_current_path).expect("matching prefix")), data);
                                    }
                                }
                            });

                            file_explorer.finish_file_move(&send_current_path);
                            file_explorer.reload();
                            file_explorer.naming.set(Naming::None);
                        }
                        Err(err) => {
                            file_explorer
                                .naming
                                .update(|naming| naming.set_err(err.message));
                        }
                    },
                );

                self.file_explorer.naming.update(Naming::set_pending);
                self.common
                    .proxy
                    .rename_path(current_path.clone(), new_path, send);
            }
            InternalCommand::RunProcessInTerminal {
                program,
                args,
                name,
            } => {
                use crate::debug::RunDebugMode;
                use sparrow_rpc::dap_types::{ConfigSource, RunDebugConfig};
                use std::time::Instant;

                let cwd = self
                    .common
                    .workspace
                    .path
                    .as_ref()
                    .and_then(|p| p.to_str().map(|s| s.to_string()));
                let process = crate::debug::RunDebugProcess {
                    mode: RunDebugMode::Run,
                    config: RunDebugConfig {
                        ty: None,
                        name,
                        program,
                        args: Some(args),
                        cwd,
                        env: None,
                        prelaunch: None,
                before_launch: Vec::new(),
                store_in_project: false,
                        flutter_options: Default::default(),
                        debug_adapter: None,
                        debug_command: None,
                        dap_id: Default::default(),
                        tracing_output: false,
                        config_source: ConfigSource::Palette,
                    },
                    stopped: false,
                    created: Instant::now(),
                    is_prelaunch: false,
                };
                let tab = self.terminal.new_tab_run_debug(Some(process), None);
                if let Some(term) = tab.active_terminal(false) {
                    self.terminal.debug.active_term.set(Some(term.term_id));
                }
                self.show_panel(crate::panel::kind::PanelKind::Terminal);
            }
            InternalCommand::FinishNewNode { is_dir, path } => {
                let file_explorer = self.file_explorer.clone();
                let internal_command = self.common.internal_command;

                let send = create_ext_action(
                    self.scope,
                    move |response: Result<ProxyResponse, RpcError>| {
                        match response {
                            Ok(response) => {
                                file_explorer.reload();
                                file_explorer.naming.set(Naming::None);

                                // Open a new file in the editor
                                if let ProxyResponse::CreatePathResponse { path } =
                                    response
                                {
                                    if !is_dir {
                                        internal_command.send(
                                            InternalCommand::OpenFile { path },
                                        );
                                    }
                                }
                            }
                            Err(err) => {
                                file_explorer
                                    .naming
                                    .update(|naming| naming.set_err(err.message));
                            }
                        }
                    },
                );

                self.file_explorer.naming.update(Naming::set_pending);
                if is_dir {
                    self.common.proxy.create_directory(path, send);
                } else {
                    self.common.proxy.create_file(path, send);
                }
            }
            InternalCommand::FinishDuplicate { source, path } => {
                let file_explorer = self.file_explorer.clone();

                let send = create_ext_action(
                    self.scope,
                    move |response: Result<_, RpcError>| {
                        if let Err(err) = response {
                            file_explorer
                                .naming
                                .update(|naming| naming.set_err(err.message));
                        } else {
                            file_explorer.reload();
                            file_explorer.naming.set(Naming::None);
                        }
                    },
                );

                self.file_explorer.naming.update(Naming::set_pending);
                self.common.proxy.duplicate_path(source, path, send);
            }
            InternalCommand::GoToLocation { location } => {
                self.main_split.go_to_location(location, None);
            }
            InternalCommand::JumpToLocation { location } => {
                self.main_split.jump_to_location(location, None);
            }
            InternalCommand::PaletteReferences { references } => {
                self.palette.references.set(references);
                self.palette.run(PaletteKind::Reference);
            }
            InternalCommand::Split {
                direction,
                editor_tab_id,
            } => {
                self.main_split.split(direction, editor_tab_id);
            }
            InternalCommand::SplitAndMove {
                editor_tab_id,
                index,
                direction,
            } => {
                self.main_split.move_editor_tab_child_to_new_split(
                    editor_tab_id,
                    index,
                    editor_tab_id,
                    direction,
                );
            }
            InternalCommand::SplitMove {
                direction,
                editor_tab_id,
            } => {
                self.main_split.split_move(direction, editor_tab_id);
            }
            InternalCommand::SplitExchange { editor_tab_id } => {
                self.main_split.split_exchange(editor_tab_id);
            }
            InternalCommand::EditorTabClose { editor_tab_id } => {
                self.main_split.editor_tab_close(editor_tab_id);
            }
            InternalCommand::EditorTabChildClose {
                editor_tab_id,
                child,
            } => {
                self.main_split
                    .editor_tab_child_close(editor_tab_id, child, false);
            }
            InternalCommand::EditorTabCloseByKind {
                editor_tab_id,
                child,
                kind,
            } => {
                self.main_split.editor_tab_child_close_by_kind(
                    editor_tab_id,
                    child,
                    kind,
                );
            }
            InternalCommand::RequestCodeActions {
                offset,
                mouse_click,
            } => {
                let Some(editor) = self.main_split.active_editor.get_untracked()
                else {
                    return;
                };
                let doc = editor.doc();
                let Some(path) = doc.content.with_untracked(|c| c.path().cloned())
                else {
                    return;
                };
                if !doc.loaded()
                    || editor.cursor().with_untracked(|c| c.offset()) != offset
                {
                    return;
                }
                let revision = doc.rev();
                self.common.hover.active.set(false);
                let mut data = self.code_action.get_untracked();
                data.show(PluginId(0), im::Vector::new(), offset, mouse_click);
                data.origin = Some((path.clone(), revision));
                data.request_state = crate::code_action::ActionRequestState::Loading;
                let request_id = data.request_id;
                self.code_action.set(data);
                let actions = self.code_action;
                let scope = self.scope.create_child();
                let active_editor = self.main_split.active_editor;
                let current_editor = editor.clone();
                let source = doc.clone();
                scope.create_effect(move |_| {
                    let data = actions.get();
                    if data.request_id != request_id
                        || data.status.get()
                            == crate::code_action::CodeActionStatus::Inactive
                    {
                        scope.dispose();
                        return;
                    }
                    let current = active_editor.get();
                    if current.is_none_or(|e| e.id() != current_editor.id())
                        || !Rc::ptr_eq(&current_editor.doc_signal().get(), &source)
                        || current_editor.cursor().with(|c| c.offset()) != offset
                        || source.buffer.with(|b| b.rev()) != revision
                    {
                        data.cancel();
                        scope.dispose();
                    }
                });
                let send =
                    create_ext_action(
                        scope,
                        move |result: Result<
                            (PluginId, Vec<lsp_types::CodeActionOrCommand>),
                            String,
                        >| {
                            let mut data = actions.get_untracked();
                            if data.request_id != request_id
                                || data.status.get_untracked()
                                    != crate::code_action::CodeActionStatus::Active
                            {
                                return;
                            }
                            match result {
                                Ok((plugin_id, items)) => {
                                    doc.code_actions().update(|cache| {
                                        cache.insert(
                                            offset,
                                            (
                                                plugin_id,
                                                items.clone().into_iter().collect(),
                                            ),
                                        );
                                    });
                                    data.replace_items(
                                        plugin_id,
                                        items.into_iter().collect(),
                                    );
                                }
                                Err(error) => data.request_state =
                                    crate::code_action::ActionRequestState::Failed(
                                        error,
                                    ),
                            }
                            actions.set(data);
                        },
                    );
                editor.request_code_actions(move |result| send(result));
            }

            InternalCommand::ShowCodeActions {
                offset,
                mouse_click,
                plugin_id,
                code_actions,
            } => {
                self.common.hover.active.set(false);
                let mut code_action = self.code_action.get_untracked();
                code_action.show(plugin_id, code_actions, offset, mouse_click);
                code_action.origin = self.main_split.active_editor.get_untracked().and_then(|editor| {
                    let doc=editor.doc();doc.content.with_untracked(|c|c.path().cloned()).map(|path|(path,doc.rev()))
                });
                self.code_action.set(code_action);
            }
            InternalCommand::QuickFixAt { location } => {
                self.main_split.jump_to_location(location, None);
                if let Some(editor) = self.main_split.active_editor.get_untracked() {
                    let scope = self.scope.create_child();
                    let doc = editor.doc();
                    scope.create_effect(move |_| {
                        if doc.loaded.get() { editor.show_code_actions(false); scope.dispose(); }
                    });
                }
            }
            InternalCommand::ApplyCodeActionPreview { prepared } => {
                if let Err(error) = crate::code_action_preview::apply(self, prepared) {
                    self.show_alert("Quick Fix".into(), error, Vec::new());
                }
            }
            InternalCommand::RunCodeAction { plugin_id, action } => {
                self.main_split.run_code_action(plugin_id, action);
            }
            InternalCommand::ApplyWorkspaceEdit { edit } => {
                self.main_split.apply_workspace_edit(&edit);
            }
            InternalCommand::SaveJumpLocation {
                path,
                offset,
                scroll_offset,
            } => {
                self.main_split
                    .save_jump_location(path, offset, scroll_offset);
            }
            InternalCommand::NewTerminal { profile } => {
                self.terminal.new_tab(profile);
            }
            InternalCommand::SplitTerminal { term_id } => {
                self.terminal.split(term_id);
            }
            InternalCommand::SplitTerminalNext { term_id } => {
                self.terminal.split_next(term_id);
            }
            InternalCommand::SplitTerminalPrevious { term_id } => {
                self.terminal.split_previous(term_id);
            }
            InternalCommand::SplitTerminalExchange { term_id } => {
                self.terminal.split_exchange(term_id);
            }
            InternalCommand::RunAndDebug { mode, config } => {
                let dirty = self.main_split.docs.with_untracked(|docs| {
                    docs.values()
                        .filter(|doc| {
                            doc.content
                                .with_untracked(|content| content.path().is_some())
                                && !doc
                                    .buffer
                                    .with_untracked(|buffer| buffer.is_pristine())
                        })
                        .cloned()
                        .collect::<Vec<_>>()
                });
                let commands = self.common.internal_command;
                if dirty.is_empty() {
                    commands
                        .send(InternalCommand::RunAndDebugSaved { mode, config });
                } else {
                    let remaining = Rc::new(std::cell::Cell::new(dirty.len()));
                    for doc in dirty {
                        let remaining = remaining.clone();
                        let config = config.clone();
                        doc.save(move || {
                            remaining.set(remaining.get() - 1);
                            if remaining.get() == 0 {
                                commands.send(InternalCommand::RunAndDebugSaved {
                                    mode,
                                    config,
                                });
                            }
                        });
                    }
                }
            }
            InternalCommand::RunAndDebugSaved { mode, mut config } => {
                // A configuration can launch repeatedly or switch Run/Debug.
                // Keep each launch distinct from stopped adapter sessions.
                config.dap_id = Default::default();
                self.run_and_debug(cx, &mode, &config);
            }
            InternalCommand::RequestSymbolRename { token, preview } => {
                crate::rename::request(Rc::new(self.clone()), token, preview);
            }
            InternalCommand::PreparedRename {
                path,
                revision,
                offset,
                response,
            } => {
                let Some(editor) = self.main_split.active_editor.get_untracked()
                else {
                    return;
                };
                let doc = editor.doc();
                if self.common.focus.get_untracked() != Focus::Workbench
                    || doc.rev() != revision
                    || doc
                        .content
                        .with_untracked(|c| c.path() != Some(&path) || c.read_only())
                    || editor.cursor().with_untracked(|c| c.offset()) != offset
                {
                    return;
                }
                let response = match response {
                    Ok(Some(response)) => response,
                    other => {
                        self.show_alert(
                            "Rename".into(),
                            other.err().unwrap_or_else(|| {
                                "This symbol cannot be renamed.".into()
                            }),
                            Vec::new(),
                        );
                        return;
                    }
                };
                let (start,position,placeholder)=doc.buffer.with_untracked(|buffer|{
                    let (range,placeholder)=match response {
                        lsp_types::PrepareRenameResponse::Range(range)=>(Some(range),None),
                        lsp_types::PrepareRenameResponse::RangeWithPlaceholder{range,placeholder}=>(Some(range),Some(placeholder)),
                        lsp_types::PrepareRenameResponse::DefaultBehavior{..}=>(None,None),
                    };
                    let (start,end)=range.map(|r|(buffer.offset_of_position(&r.start),buffer.offset_of_position(&r.end))).unwrap_or_else(||buffer.select_word(offset));
                    (start,buffer.offset_to_position(start),placeholder.unwrap_or_else(||buffer.slice_to_cow(start..end).to_string()))
                });
                self.common.hover.active.set(false);
                self.rename
                    .start(path, placeholder, start, position, editor);
            }

            InternalCommand::Search { pattern } => {
                self.main_split.set_find_pattern(pattern);
            }
            InternalCommand::FindEditorReceiveChar { s } => {
                self.main_split.find_editor.receive_char(&s);
            }
            InternalCommand::ReplaceEditorReceiveChar { s } => {
                self.main_split.replace_editor.receive_char(&s);
            }
            InternalCommand::FindEditorCommand {
                command,
                count,
                mods,
            } => {
                self.main_split
                    .find_editor
                    .run_command(&command, count, mods);
            }
            InternalCommand::ReplaceEditorCommand {
                command,
                count,
                mods,
            } => {
                self.main_split
                    .replace_editor
                    .run_command(&command, count, mods);
            }
            InternalCommand::FocusEditorTab { editor_tab_id } => {
                self.main_split.active_editor_tab.set(Some(editor_tab_id));
            }
            InternalCommand::SetColorTheme { name, save } => {
                if save {
                    // The config file is watched
                    SparrowConfig::update_file(
                        "core",
                        "color-theme",
                        toml_edit::Value::from(name),
                    );
                } else {
                    let mut new_config = self.common.config.get_untracked();
                    Arc::make_mut(&mut new_config)
                        .set_color_theme(&self.workspace, &name);
                    self.set_config.set(new_config);
                }
            }
            InternalCommand::SetIconTheme { name, save } => {
                if save {
                    // The config file is watched
                    SparrowConfig::update_file(
                        "core",
                        "icon-theme",
                        toml_edit::Value::from(name),
                    );
                } else {
                    let mut new_config = self.common.config.get_untracked();
                    Arc::make_mut(&mut new_config)
                        .set_icon_theme(&self.workspace, &name);
                    self.set_config.set(new_config);
                }
            }
            InternalCommand::SetModal { modal } => {
                SparrowConfig::update_file(
                    "core",
                    "modal",
                    toml_edit::Value::from(modal),
                );
            }
            InternalCommand::OpenWebUri { uri } => {
                if !uri.is_empty() {
                    match open::that(&uri) {
                        Ok(_) => {
                            trace!(TraceLevel::TRACE, "opened web uri: {uri:?}");
                        }
                        Err(e) => {
                            trace!(
                                TraceLevel::ERROR,
                                "failed to open web uri: {uri:?}, error: {e}"
                            );
                        }
                    }
                }
            }
            InternalCommand::ShowAlert {
                title,
                msg,
                buttons,
            } => {
                self.show_alert(title, msg, buttons);
            }
            InternalCommand::HideAlert => {
                self.alert_data.active.set(false);
            }
            InternalCommand::SaveScratchDoc { doc } => {
                self.main_split.save_scratch_doc(doc);
            }
            InternalCommand::SaveScratchDoc2 { doc } => {
                self.main_split.save_scratch_doc2(doc);
            }
            InternalCommand::UpdateProxyStatus { status } => {
                self.common.proxy_status.set(Some(status));
            }
            InternalCommand::DapFrameScopes { dap_id, frame_id } => {
                self.terminal.dap_frame_scopes(dap_id, frame_id);
            }
            InternalCommand::OpenVoltView { volt_id } => {
                self.main_split.open_volt_view(volt_id);
            }
            InternalCommand::ResetBlinkCursor => {
                // All the editors share the blinking information and logic, so we can just reset
                // one of them.
                if let Some(e_data) = self.main_split.active_editor.get_untracked() {
                    e_data.editor.cursor_info.reset();
                }
            }
            InternalCommand::OpenDiffFiles {
                left_path,
                right_path,
            } => self.main_split.open_diff_files(left_path, right_path),
            InternalCommand::ExecuteProcess { program, arguments } => {
                let mut cmd = match std::process::Command::new(program)
                    .args(arguments)
                    .spawn()
                {
                    Ok(v) => v,
                    Err(e) => {
                        return event!(Level::ERROR, "Failed to spawn process: {e}");
                    }
                };

                match cmd.wait() {
                    Ok(v) => event!(Level::TRACE, "Process exited with status {v}"),
                    Err(e) => {
                        event!(Level::ERROR, "Process exited with an error: {e}")
                    }
                };
            }
            InternalCommand::ClearTerminalBuffer {
                view_id,
                tab_index,
                terminal_index,
            } => {
                let Some(tab) = self.terminal.tab_info.with_untracked(|x| {
                    x.tabs.iter().find_map(|(index, data)| {
                        if index.get_untracked() == tab_index {
                            Some(data.clone())
                        } else {
                            None
                        }
                    })
                }) else {
                    error!("could not find terminal tab data: index={tab_index}");
                    return;
                };
                let Some(raw) = tab.terminals.with_untracked(|x| {
                    x.iter().find_map(|(index, data)| {
                        if index.get_untracked() == terminal_index {
                            Some(data.raw.get_untracked())
                        } else {
                            None
                        }
                    })
                }) else {
                    error!("could not find terminal data: index={terminal_index}");
                    return;
                };
                raw.write().term.reset_state();
                view_id.request_paint();
            }
            InternalCommand::StopTerminal { term_id } => {
                self.terminal.stop_run_debug(term_id);
            }
            InternalCommand::RestartTerminal { term_id } => {
                if let Some(is_debug) = self.terminal.restart_run_debug(term_id) {
                    self.panel.show_panel(&PanelKind::Terminal);
                    if is_debug {
                        self.panel.show_panel(&PanelKind::Debug);
                    }
                } else {
                    self.palette.run(PaletteKind::RunAndDebug);
                }
            }
            InternalCommand::CallHierarchyIncoming { item_id } => {
                self.call_hierarchy_incoming(item_id);
            }
            InternalCommand::FlutterRunTest { path, name, debug } => {
                self.prepare_flutter_tests(vec![(path, name)], debug, false);
            }
            InternalCommand::FlutterStartTests {
                targets,
                debug,
                exact,
            } => self.start_flutter_tests(targets, debug, exact),
            InternalCommand::FlutterRerunTests { failed_only } => {
                self.rerun_flutter_tests(failed_only)
            }
            InternalCommand::FlutterRunTestAtCursor { term_id: _ } => {
                self.run_test_at_cursor(false);
            }
        }
    }

    fn handle_core_notification(&self, rpc: &CoreNotification) {
        let cx = self.scope;
        match rpc {
            CoreNotification::DapSessionEvent { dap_id, event } => {
                if self.terminal.debug.adapter_sessions.with_untracked(|sessions| sessions.contains_key(&dap_id)) {
                    if let Some(output) = event.get("output").and_then(|v| v.as_str()) {
                        self.flutter_hot_reload.run_logs.update(|logs| {
                            for line in output.lines() { logs.push_back(line.to_owned()); }
                            while logs.len() > 5000 { logs.pop_front(); }
                        });
                    }
                    if event.get("type").and_then(|v| v.as_str()) == Some("terminated") {
                        self.terminal.debug.adapter_sessions.update(|sessions| {
                            if let Some((_, process)) = sessions.get_mut(&dap_id) { process.stopped = true; }
                        });
                        if self.terminal.debug.active_dap.get_untracked() == Some(*dap_id) {
                            self.terminal.debug.active_dap.set(None);
                        }
                    }
                }
            }
            CoreNotification::FlutterTestEvent { dap_id, event } => {
                self.flutter_test.debug_event(*dap_id, event);
                if self.terminal.debug.active_dap.get_untracked() == Some(*dap_id)
                    && self.flutter_test.debug_dap.get_untracked().is_none()
                {
                    self.terminal.debug.active_dap.set(None);
                    self.terminal.debug.daps.update(|daps| {
                        daps.remove(dap_id);
                    });
                }
            }
            CoreNotification::ProxyStatus { status } => {
                self.common.proxy_status.set(Some(status.to_owned()));
            }
            CoreNotification::DiffInfo { diff } => {
                self.source_control.branch.set(diff.head.clone());
                self.source_control
                    .branches
                    .set(diff.branches.iter().cloned().collect());
                self.source_control
                    .tags
                    .set(diff.tags.iter().cloned().collect());
                self.source_control.file_diffs.update(|file_diffs| {
                    *file_diffs = diff
                        .diffs
                        .iter()
                        .cloned()
                        .map(|diff| {
                            let checked =
                                file_diffs.get(diff.path()).is_none_or(|(_, c)| *c);
                            (diff.path().clone(), (diff, checked))
                        })
                        .collect();
                });

                let docs = self.main_split.docs.get_untracked();
                for (_, doc) in docs {
                    doc.retrieve_head();
                }
            }
            CoreNotification::CompletionResponse {
                request_id,
                input,
                resp,
                plugin_id,
            } => {
                self.common.completion.update(|completion| {
                    completion.receive(*request_id, input, resp, *plugin_id);
                });

                let completion = self.common.completion.get_untracked();
                let editor_data = completion
                    .latest_editor_id
                    .and_then(|id| self.main_split.editors.editor_untracked(id));
                if let Some(editor_data) = editor_data {
                    let cursor_offset =
                        editor_data.cursor().with_untracked(|c| c.offset());
                    completion
                        .update_document_completion(&editor_data, cursor_offset);
                }
            }
            CoreNotification::PublishDiagnostics { diagnostics } => {
                let path = path_from_url(&diagnostics.uri);
                let diagnostics: im::Vector<Diagnostic> = diagnostics
                    .diagnostics
                    .clone()
                    .into_iter()
                    .sorted_by_key(|d| d.range.start)
                    .collect();

                self.main_split
                    .get_diagnostic_data(&path)
                    .diagnostics
                    .set(diagnostics);

                // inform the document about the diagnostics
                if let Some(doc) = self
                    .main_split
                    .docs
                    .with_untracked(|docs| docs.get(&path).cloned())
                {
                    doc.init_diagnostics();
                }
            }
            CoreNotification::ServerStatus { params } => {
                if params.is_ok() {
                    // todo filter by language
                    self.main_split.docs.with_untracked(|x| {
                        for doc in x.values() {
                            doc.get_code_lens();
                            doc.get_semantic_styles();
                            doc.get_folding_range();
                            doc.get_inlay_hints();
                        }
                    });
                }
            }
            CoreNotification::TerminalProcessStopped { term_id, exit_code } => {
                debug!("TerminalProcessStopped {:?}, {:?}", term_id, exit_code);
                if let Err(err) = self
                    .common
                    .term_tx
                    .send((*term_id, TermEvent::CloseTerminal))
                {
                    error!("{:?}", err);
                }
                self.terminal.terminal_stopped(term_id, *exit_code);
                if self
                    .terminal
                    .tab_info
                    .with_untracked(|info| info.tabs.is_empty())
                {
                    if self.panel.is_panel_visible(&PanelKind::Terminal) {
                        self.panel.hide_panel(&PanelKind::Terminal);
                    }
                    self.common.focus.set(Focus::Workbench);
                }
            }
            CoreNotification::TerminalLaunchFailed { term_id, error } => {
                self.terminal.launch_failed(term_id, error);
            }
            CoreNotification::RunInTerminal { config } => {
                self.run_in_terminal(cx, &RunDebugMode::Debug, config, true);
            }
            CoreNotification::TerminalProcessId {
                term_id,
                process_id,
            } => {
                self.terminal.set_process_id(term_id, *process_id);
            }
            CoreNotification::DapStopped {
                dap_id,
                stopped,
                stack_frames,
                variables,
            } => {
                self.show_panel(PanelKind::Debug);
                self.terminal
                    .dap_stopped(dap_id, stopped, stack_frames, variables);
            }
            CoreNotification::OpenPaths { paths } => {
                self.open_paths(paths);
            }
            CoreNotification::DapContinued { dap_id } => {
                self.terminal.dap_continued(dap_id);
            }
            CoreNotification::DapBreakpointsResp {
                path, breakpoints, ..
            } => {
                self.terminal.debug.breakpoints.update(|all_breakpoints| {
                    if let Some(current_breakpoints) = all_breakpoints.get_mut(path)
                    {
                        let mut line_changed = HashSet::new();
                        let mut i = 0;
                        for (_, current_breakpoint) in current_breakpoints.iter_mut()
                        {
                            if !current_breakpoint.active {
                                continue;
                            }
                            if let Some(breakpoint) = breakpoints.get(i) {
                                current_breakpoint.id = breakpoint.id;
                                current_breakpoint.verified = breakpoint.verified;
                                current_breakpoint
                                    .message
                                    .clone_from(&breakpoint.message);
                                if let Some(new_line) = breakpoint.line {
                                    if current_breakpoint.line + 1 != new_line {
                                        line_changed.insert(current_breakpoint.line);
                                        current_breakpoint.line =
                                            new_line.saturating_sub(1);
                                    }
                                }
                            }
                            i += 1;
                        }
                        for line in line_changed {
                            if let Some(changed) = current_breakpoints.remove(&line)
                            {
                                current_breakpoints.insert(changed.line, changed);
                            }
                        }
                    }
                });
            }
            CoreNotification::OpenFileChanged { path, content } => {
                self.main_split.open_file_changed(path, content);
            }
            CoreNotification::VoltInstalled { volt, icon } => {
                self.plugin.volt_installed(volt, icon);
            }
            CoreNotification::VoltRemoved { volt, .. } => {
                self.plugin.volt_removed(volt);
            }
            CoreNotification::WorkDoneProgress { progress } => {
                self.update_progress(progress);
            }
            CoreNotification::ShowMessage { title, message } => {
                self.show_message(title, message);
            }
            CoreNotification::Log {
                level,
                message,
                target,
            } => {
                use sparrow_rpc::core::LogLevel;
                use tracing_log::log::{Level, log};

                let target = target.clone().unwrap_or(String::from("unknown"));

                match level {
                    LogLevel::Trace => {
                        log!(target: &target, Level::Trace, "{}", message);
                    }
                    LogLevel::Debug => {
                        log!(target: &target, Level::Debug, "{}", message);
                    }
                    LogLevel::Info => {
                        log!(target: &target, Level::Info, "{}", message);
                    }
                    LogLevel::Warn => {
                        log!(target: &target, Level::Warn, "{}", message);
                    }
                    LogLevel::Error => {
                        log!(target: &target, Level::Error, "{}", message);
                    }
                }
            }
            CoreNotification::LogMessage { message, target } => {
                use lsp_types::MessageType;
                use tracing_log::log::{Level, log};
                match message.typ {
                    MessageType::ERROR => {
                        log!(target: target, Level::Error, "{}", message.message)
                    }
                    MessageType::WARNING => {
                        log!(target: target, Level::Warn, "{}", message.message)
                    }
                    MessageType::INFO => {
                        log!(target: target, Level::Info, "{}", message.message)
                    }
                    MessageType::DEBUG => {
                        log!(target: target, Level::Debug, "{}", message.message)
                    }
                    MessageType::LOG => {
                        log!(target: target, Level::Debug, "{}", message.message)
                    }
                    _ => {}
                }
            }
            CoreNotification::WorkspaceFileChange => {
                self.file_explorer.reload();
            }
            _ => {}
        }
    }

    pub fn key_down<'a>(&self, event: impl Into<EventRef<'a>> + Copy) -> bool {
        if self.settings_modal_visible.get_untracked()
            || self.run_config_modal_visible.get_untracked()
            || self.flutter_project.active.get_untracked()
        {
            return false;
        }
        if self.alert_data.active.get_untracked() {
            if let EventRef::Keyboard(key)=event.into() {
                if key.modifiers.is_empty() && key.key.logical_key==floem::keyboard::Key::Named(floem::keyboard::NamedKey::Escape) {
                    self.alert_data.active.set(false);
                    self.common.window_common.app_view_id.get_untracked().request_focus();
                    return true;
                }
            }
            return false;
        }
        let focus = self.common.focus.get_untracked();
        let keypress = self.common.keypress.get_untracked();
        let handle = match focus {
            Focus::Workbench => self.main_split.key_down(event, &keypress),
            Focus::Palette => Some(keypress.key_down(event, &self.palette)),
            Focus::CodeAction => {
                let code_action = self.code_action.get_untracked();
                Some(keypress.key_down(event, &code_action))
            }
            Focus::Rename => Some(keypress.key_down(event, &self.rename)),
            Focus::AboutPopup => Some(keypress.key_down(event, &self.about_data)),
            Focus::Panel(PanelKind::Terminal) => self.terminal.key_down(event, &keypress),
            Focus::Panel(PanelKind::FlutterRun) => {
                // Terminal may have selected a shell since Run was last visible.
                // Never send Run console keys to that unrelated terminal.
                if let Some(id) = self.flutter_hot_reload.active_term.get_untracked()
                    .filter(|id| self.terminal.get_terminal(id).is_some()) {
                    self.terminal.focus_terminal_in_panel(id, PanelKind::FlutterRun);
                    self.terminal.key_down(event, &keypress)
                } else { None }
            }
            Focus::Panel(PanelKind::FileExplorer) => {
                if let EventRef::Keyboard(key) = event.into() {
                    if self.file_explorer.navigate_key(key) {
                        return true;
                    }
                }
                Some(keypress.key_down(event, &self.file_explorer))
            }
            Focus::Panel(PanelKind::Search) => {
                Some(keypress.key_down(event, &self.global_search))
            }
            Focus::Panel(PanelKind::Plugin) => {
                Some(keypress.key_down(event, &self.plugin))
            }
            Focus::Panel(PanelKind::SourceControl) => {
                Some(keypress.key_down(event, &self.source_control))
            }
            _ => None,
        };

        if let Some(handle) = &handle {
            if handle.handled {
                true
            } else {
                keypress
                    .handle_keymatch(
                        self,
                        handle.keymatch.clone(),
                        handle.keypress.clone(),
                    )
                    .handled
            }
        } else {
            keypress.key_down(event, self).handled
        }
    }

    pub fn workspace_info(&self) -> WorkspaceInfo {
        let main_split_data = self
            .main_split
            .splits
            .get_untracked()
            .get(&self.main_split.root_split)
            .cloned()
            .unwrap();
        WorkspaceInfo {
            split: main_split_data.get_untracked().split_info(self),
            panel: self.panel.panel_info(),
            breakpoints: self
                .terminal
                .debug
                .breakpoints
                .get_untracked()
                .into_iter()
                .map(|(path, breakpoints)| {
                    (path, breakpoints.into_values().collect::<Vec<_>>())
                })
                .collect(),
        }
    }

    pub fn hover_origin(&self) -> Option<Point> {
        if !self.common.hover.active.get_untracked() {
            return None;
        }

        let editor_id = self.common.hover.editor_id.get_untracked();
        let editor_data = self.main_split.editors.editor(editor_id)?;

        let (window_origin, viewport, editor) = (
            editor_data.window_origin(),
            editor_data.viewport(),
            &editor_data.editor,
        );

        // TODO(minor): affinity should be gotten from where the hover was started at.
        let (point_above, point_below) = editor.points_of_offset(
            self.common.hover.offset.get_untracked(),
            CursorAffinity::Forward,
        );

        let window_origin =
            window_origin.get() - self.common.window_origin.get().to_vec2();
        let viewport = viewport.get();
        let hover_size = self.common.hover.layout_rect.get().size();
        let tab_size = self.layout_rect.get().size();

        let mut origin = window_origin
            + Vec2::new(
                point_below.x - viewport.x0,
                (point_above.y - viewport.y0) - hover_size.height,
            );
        if origin.y < 0.0 {
            origin.y = window_origin.y + point_below.y - viewport.y0;
        }
        if origin.x + hover_size.width + 1.0 > tab_size.width {
            origin.x = tab_size.width - hover_size.width - 1.0;
        }
        if origin.x <= 0.0 {
            origin.x = 0.0;
        }

        Some(origin)
    }

    /// Screen position for the signature-help popup: just below the
    /// cursor's current line.
    pub fn signature_help_origin(&self) -> Option<floem::peniko::kurbo::Point> {
        use floem::peniko::kurbo::Vec2;
        use sparrow_core::cursor::CursorAffinity;

        let sh = self.common.signature_help.get_untracked();
        if !sh.is_active() {
            return None;
        }
        let editor_data = self.main_split.active_editor.get_untracked()?;
        if Some(editor_data.id()) != sh.editor_id {
            return None;
        }
        let (window_origin, viewport, editor) = (
            editor_data.window_origin(),
            editor_data.viewport(),
            &editor_data.editor,
        );
        let offset = editor_data.cursor().with_untracked(|c| c.offset());
        let (_, point_below) =
            editor.points_of_offset(offset, CursorAffinity::Forward);
        let window_origin =
            window_origin.get() - self.common.window_origin.get().to_vec2();
        let viewport = viewport.get();
        let tab_size = self.layout_rect.get().size();
        let sh_size = sh.layout_rect.size();

        let mut origin = window_origin
            + Vec2::new(
                point_below.x - viewport.x0,
                point_below.y - viewport.y0 + 4.0,
            );

        if origin.x + sh_size.width + 8.0 > tab_size.width {
            origin.x = (tab_size.width - sh_size.width - 8.0).max(0.0);
        }
        if origin.x < 0.0 {
            origin.x = 0.0;
        }

        Some(origin)
    }

    pub fn completion_origin(&self) -> Point {
        let completion = self.common.completion.get();
        if completion.status == CompletionStatus::Inactive {
            return Point::ZERO;
        }
        let config = self.common.config.get();
        let editor_data =
            if let Some(editor) = self.main_split.active_editor.get_untracked() {
                editor
            } else {
                return Point::ZERO;
            };

        let (window_origin, viewport, editor) = (
            editor_data.window_origin(),
            editor_data.viewport(),
            &editor_data.editor,
        );

        // TODO(minor): What affinity should we use for this? Probably just use the cursor's
        // original affinity..
        let (point_above, point_below) =
            editor.points_of_offset(completion.offset, CursorAffinity::Forward);

        let window_origin =
            window_origin.get() - self.common.window_origin.get().to_vec2();
        let viewport = viewport.get();
        let completion_size = completion.layout_rect.size();
        let tab_size = self.layout_rect.get().size();

        let mut origin = window_origin
            + Vec2::new(
                point_below.x
                    - viewport.x0
                    - config.editor.line_height() as f64
                    - 5.0,
                point_below.y - viewport.y0,
            );
        if origin.y + completion_size.height > tab_size.height {
            origin.y = window_origin.y + (point_above.y - viewport.y0)
                - completion_size.height;
        }
        if origin.x + completion_size.width + 1.0 > tab_size.width {
            origin.x = tab_size.width - completion_size.width - 1.0;
        }
        if origin.x <= 0.0 {
            origin.x = 0.0;
        }

        origin
    }

    pub fn code_action_origin(&self) -> Point {
        let code_action = self.code_action.get();
        let config = self.common.config.get();
        if code_action.status.get_untracked() == CodeActionStatus::Inactive {
            return Point::ZERO;
        }

        let tab_size = self.layout_rect.get().size();
        let code_action_size = code_action.layout_rect.size();

        let editor_data =
            if let Some(editor) = self.main_split.active_editor.get_untracked() {
                editor
            } else {
                return Point::ZERO;
            };

        let (window_origin, viewport, editor) = (
            editor_data.window_origin(),
            editor_data.viewport(),
            &editor_data.editor,
        );

        // TODO(minor): What affinity should we use for this?
        let (_point_above, point_below) =
            editor.points_of_offset(code_action.offset, CursorAffinity::Forward);

        let window_origin =
            window_origin.get() - self.common.window_origin.get().to_vec2();
        let viewport = viewport.get();

        let mut origin = window_origin
            + Vec2::new(
                if code_action.mouse_click {
                    0.0
                } else {
                    point_below.x - viewport.x0
                },
                point_below.y - viewport.y0,
            );

        if origin.y + code_action_size.height > tab_size.height {
            origin.y = origin.y
                - config.editor.line_height() as f64
                - code_action_size.height;
        }
        if origin.x + code_action_size.width + 1.0 > tab_size.width {
            origin.x = tab_size.width - code_action_size.width - 1.0;
        }
        if origin.x <= 0.0 {
            origin.x = 0.0;
        }

        origin
    }

    pub fn rename_origin(&self) -> Point {
        let config = self.common.config.get();
        if !self.rename.active.get() {
            return Point::ZERO;
        }

        let tab_size = self.layout_rect.get().size();
        let rename_size = self.rename.layout_rect.get().size();

        let editor_data =
            if let Some(editor) = self.main_split.active_editor.get_untracked() {
                editor
            } else {
                return Point::ZERO;
            };

        let (window_origin, viewport, editor) = (
            editor_data.window_origin(),
            editor_data.viewport(),
            &editor_data.editor,
        );

        // TODO(minor): What affinity should we use for this?
        let (_point_above, point_below) = editor.points_of_offset(
            self.rename.start.get_untracked(),
            CursorAffinity::Forward,
        );

        let window_origin =
            window_origin.get() - self.common.window_origin.get().to_vec2();
        let viewport = viewport.get();

        let mut origin = window_origin
            + Vec2::new(point_below.x - viewport.x0, point_below.y - viewport.y0);

        if origin.y + rename_size.height > tab_size.height {
            origin.y =
                origin.y - config.editor.line_height() as f64 - rename_size.height;
        }
        if origin.x + rename_size.width + 1.0 > tab_size.width {
            origin.x = tab_size.width - rename_size.width - 1.0;
        }
        if origin.x <= 0.0 {
            origin.x = 0.0;
        }

        origin
    }

    /// Get the mode for the current editor or terminal
    pub fn mode(&self) -> Mode {
        if self.common.config.get().core.modal {
            let mode = if self.common.focus.get() == Focus::Workbench {
                self.main_split
                    .active_editor
                    .get()
                    .map(|editor| editor.cursor().with(|c| c.get_mode()))
            } else {
                None
            };

            mode.unwrap_or(Mode::Normal)
        } else {
            Mode::Insert
        }
    }

    pub fn toggle_panel_visual(&self, kind: PanelKind) {
        if self.panel.is_panel_visible(&kind) {
            self.hide_panel(kind);
        } else {
            self.show_panel(kind);
        }
    }

    /// Toggle a specific kind of panel.
    fn toggle_panel_focus(&self, kind: PanelKind) {
        let should_hide = match kind {
            PanelKind::Plugin
            | PanelKind::Problem
            | PanelKind::CallHierarchy
            | PanelKind::References
            | PanelKind::Implementation => {
                // Some panels don't accept focus (yet). Fall back to visibility check
                // in those cases.
                self.panel.is_panel_visible(&kind)
            }
            PanelKind::Terminal
            | PanelKind::Debug
            | PanelKind::SourceControl
            | PanelKind::Search
            | PanelKind::WidgetTree
            | PanelKind::DartOutline
            | PanelKind::DocumentSymbol
            | PanelKind::FlutterInspector
            | PanelKind::FlutterRun
            | PanelKind::FlutterTest
            | PanelKind::DeviceManager
            | PanelKind::FlutterPerformance
            | PanelKind::Problems
            | PanelKind::Logcat
            | PanelKind::GitHistory
            | PanelKind::Notifications
            | PanelKind::PullRequests
            | PanelKind::RunningDevices
            | PanelKind::FileExplorer => self.is_panel_focused(kind),
        };
        if should_hide {
            self.hide_panel(kind);
        } else {
            self.show_panel(kind);
        }
    }

    /// Toggle a panel on one of the sides.
    fn toggle_container_visual(&self, position: &PanelContainerPosition) {
        let shown = !self.panel.is_container_shown(position, false);
        self.panel.set_shown(&position.first(), shown);
        self.panel.set_shown(&position.second(), shown);

        if shown {
            if let Some((kind, _)) = self
                .panel
                .active_panel_at_position(&position.second(), false)
            {
                self.show_panel(kind);
            }

            if let Some((kind, _)) = self
                .panel
                .active_panel_at_position(&position.first(), false)
            {
                self.show_panel(kind);
            }
        } else {
            if let Some((kind, _)) = self
                .panel
                .active_panel_at_position(&position.second(), false)
            {
                self.hide_panel(kind);
            }

            if let Some((kind, _)) = self
                .panel
                .active_panel_at_position(&position.first(), false)
            {
                self.hide_panel(kind);
            }
        }
    }

    fn is_panel_focused(&self, kind: PanelKind) -> bool {
        // Moving between e.g. Search and Problems doesn't affect focus, so we need to also check
        // visibility.
        self.common.focus.get_untracked() == Focus::Panel(kind)
            && self.panel.is_panel_visible(&kind)
    }

    fn hide_panel(&self, kind: PanelKind) {
        self.panel.hide_panel(&kind);
        self.common.focus.set(Focus::Workbench);
        self.common.view_id.get_untracked().request_focus();
    }

    pub fn show_panel(&self, kind: PanelKind) {
        let kind = kind.canonical();
        if kind == PanelKind::FlutterInspector {
            if let (Some(term), Some(uri)) = (self.flutter_hot_reload.active_term.get_untracked(), self.flutter_hot_reload.ws_uri.get_untracked()) {
                self.flutter_inspector.on_vm_url(term, uri);
            }
        }
        if kind == PanelKind::Terminal
            && self
                .terminal
                .tab_info
                .with_untracked(|info| info.tabs.is_empty())
        {
            self.terminal.new_tab(
                self.common
                    .config
                    .get_untracked()
                    .terminal
                    .get_default_profile(),
            );
        }
        self.panel.show_panel(&kind);
        if kind == PanelKind::FileExplorer
            && self.file_explorer.select.get_untracked().is_none()
        {
            if let Some(path) = self.workspace.path.clone() {
                self.file_explorer
                    .select
                    .set(Some(sparrow_rpc::file::FileNodeViewKind::Path(path)));
            }
        }
        self.common.view_id.get_untracked().request_focus();
        if kind == PanelKind::FlutterRun {
            if let Some(id) = self.flutter_hot_reload.active_term.get_untracked() {
                self.terminal.focus_terminal_in_panel(id, PanelKind::FlutterRun);
            }
        }
        if kind == PanelKind::Search
            && self.common.focus.get_untracked() == Focus::Workbench
        {
            let active_editor = self.main_split.active_editor.get_untracked();
            let word = active_editor.map(|editor| editor.word_at_cursor());
            if let Some(word) = word {
                if !word.is_empty() {
                    self.global_search.set_pattern(word);
                }
            }
        }
        self.common.focus.set(Focus::Panel(kind));
    }

    fn reload_flutter_after_saving(&self, restart: bool) {
        let mut paths = HashSet::new();
        let mut dirty = Vec::new();
        self.main_split.editors.with_editors_untracked(|editors| {
            for editor in editors.values() {
                let doc = editor.doc();
                if !doc.is_pristine() {
                    if let Some(path) = doc.content.with_untracked(|content| content.path().cloned()) {
                        if paths.insert(path) { dirty.push(editor.clone()); }
                    }
                }
            }
        });
        let flutter = self.flutter_hot_reload.clone();
        if dirty.is_empty() {
            if restart { flutter.hot_restart(); } else { flutter.hot_reload(); }
            return;
        }
        let remaining = Rc::new(std::cell::Cell::new(dirty.len()));
        for editor in dirty {
            let remaining = remaining.clone();
            let flutter = flutter.clone();
            editor.save(false, move || {
                remaining.set(remaining.get() - 1);
                if remaining.get() == 0 {
                    if restart { flutter.hot_restart(); } else { flutter.hot_reload(); }
                }
            });
        }
    }

    pub fn launch_selected_configuration(&self, mode: RunDebugMode) {
        let selected = self.active_run_config_idx.get_untracked();
        if let Some(config) = self.run_configs.with_untracked(|configs| configs.get(selected).cloned()) {
            if let Some((term_id, _)) = self.terminal.run_debug_process(false).into_iter().find(|(_, process)| !process.stopped && process.mode == mode && process.config.name == config.name) {
                let panel = crate::dart::run_config::console_panel(mode, &config);
                self.show_panel(panel);
                self.terminal.focus_terminal_in_panel(term_id, panel);
                return;
            }
            self.common.internal_command.send(InternalCommand::RunAndDebug { mode, config });
        } else {
            self.run_config_modal_visible.set(true);
        }
    }

    fn run_and_debug(
        &self,
        cx: Scope,
        mode: &RunDebugMode,
        config: &RunDebugConfig,
    ) {
        if !config.is_dart_flutter()
            && !(config.config_source == ConfigSource::RunInTerminal
                && *mode == RunDebugMode::Run)
        {
            self.show_alert("Unsupported run configuration".into(),"Sparrow runs and debugs Dart and Flutter SDK targets only. Use the Terminal for auxiliary build commands.".into(),Vec::new());
            return;
        }
        let device = self.flutter_hot_reload.selected_device.get_untracked();
        let mut prepared = crate::dart::run_config::with_selected_device(config.clone(), device.as_ref().map(|device| device.id.as_str()));
        if let Some(workspace) = self.workspace.path.as_deref() {
            prepared.cwd = Some(crate::dart::run_config::launch_directory(workspace, prepared.cwd.as_deref()).to_string_lossy().into_owned());
        }
        let root = prepared.cwd.as_deref().map(std::path::Path::new).or(self.workspace.path.as_deref());
        let binary = match prepared.program.as_str() {
            "dart" => crate::dart::sdk::find_dart_binary(root),
            "flutter" => crate::dart::sdk::find_flutter_binary(root),
            _ => None,
        };
        if let Some(binary) = binary { prepared.program = binary.to_string_lossy().into_owned(); }
        prepared.begin_before_launch();
        let config = &prepared;
        debug!("Starting Dart/Flutter target: {}", config.name);
        match mode {
            RunDebugMode::Run => {
                self.run_in_terminal(cx, mode, config, false);
            }
            RunDebugMode::Debug => {
                if config.prelaunch.is_some() {
                    self.run_in_terminal(cx, mode, config, false);
                } else {
                    let id = sparrow_rpc::terminal::TermId::next();
                    self.terminal.debug.adapter_sessions.update(|sessions| {
                        sessions.insert(config.dap_id, (id, RunDebugProcess {
                            mode: RunDebugMode::Debug, config: config.clone(), stopped: false,
                            created: Instant::now(), is_prelaunch: false,
                        }));
                    });
                    self.terminal.debug.active_dap.set(Some(config.dap_id));
                    self.terminal.debug.active_term.set(Some(id));
                    self.terminal.debug.daps.update(|daps| {
                        daps.insert(config.dap_id, DapData::new(cx, config.dap_id, id, self.common.clone()));
                    });
                    self.common.proxy.dap_start(
                        config.clone(), self.terminal.debug.source_breakpoints(),
                    )
                };
                if config.flutter_options.activate_tool_window || config.flutter_options.focus_tool_window {
                    self.panel.show_panel(&PanelKind::Debug);
                }
                if config.flutter_options.focus_tool_window {
                    self.common.focus.set(Focus::Panel(PanelKind::Debug));
                    self.common.view_id.get_untracked().request_focus();
                }
            }
        }
    }

    fn run_in_terminal(
        &self,
        cx: Scope,
        mode: &RunDebugMode,
        config: &RunDebugConfig,
        from_dap: bool,
    ) {
        // if not from dap, then run prelaunch first
        let is_prelaunch = !from_dap;
        let term_id = if let Some(terminal) =
            self.terminal.get_stopped_run_debug_terminal(mode, config)
        {
            terminal.new_process(Some(RunDebugProcess {
                mode: *mode,
                config: config.clone(),
                stopped: false,
                created: Instant::now(),
                is_prelaunch,
            }));

            terminal.term_id
        } else {
            let new_terminal_tab = self.terminal.new_tab_run_debug(
                Some(RunDebugProcess {
                    mode: *mode,
                    config: config.clone(),
                    stopped: false,
                    created: Instant::now(),
                    is_prelaunch,
                }),
                None,
            );
            new_terminal_tab.active_terminal(false).unwrap().term_id
        };
        if *mode == RunDebugMode::Run && crate::dart::run_config::is_flutter_run(config) {
            self.flutter_hot_reload.active_term.set(Some(term_id));
        }
        let console_panel = crate::dart::run_config::console_panel(*mode, config);
        if config.flutter_options.focus_tool_window {
            self.terminal.focus_terminal_in_panel(term_id, console_panel);
        }

        self.terminal.debug.active_term.set(Some(term_id));
        self.terminal.debug.daps.update(|daps| {
            daps.insert(
                config.dap_id,
                DapData::new(cx, config.dap_id, term_id, self.common.clone()),
            );
        });

        if config.flutter_options.activate_tool_window || config.flutter_options.focus_tool_window {
            self.panel.show_panel(&console_panel);
        }

    }

    pub fn open_paths(&self, paths: &[PathObject]) {
        let (folders, files): (Vec<&PathObject>, Vec<&PathObject>) =
            paths.iter().partition(|p| p.is_dir);

        for folder in folders {
            self.common.window_common.window_command.send(
                WindowCommand::NewWorkspaceTab {
                    workspace: SparrowWorkspace {
                        kind: self.workspace.kind.clone(),
                        path: Some(folder.path.clone()),
                        last_open: 0,
                    },
                    end: false,
                },
            );
        }

        for file in files {
            let position = file.linecol.map(|pos| {
                EditorPosition::Position(lsp_types::Position {
                    line: pos.line.saturating_sub(1) as u32,
                    character: pos.column.saturating_sub(1) as u32,
                })
            });

            self.common
                .internal_command
                .send(InternalCommand::GoToLocation {
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

    pub fn show_alert(&self, title: String, msg: String, buttons: Vec<AlertButton>) {
        crate::panel::workflow_tools::record(self.notifications, &title, &msg);
        self.alert_data.title.set(title);
        self.alert_data.msg.set(msg);
        self.alert_data.buttons.set(buttons);
        self.alert_data.active.set(true);
    }

    fn update_progress(&self, progress: &ProgressParams) {
        let token = progress.token.clone();
        match &progress.value {
            lsp_types::ProgressParamsValue::WorkDone(progress) => match progress {
                lsp_types::WorkDoneProgress::Begin(progress) => {
                    let progress = WorkProgress {
                        token: token.clone(),
                        title: progress.title.clone(),
                        message: progress.message.clone(),
                        percentage: progress.percentage,
                    };
                    self.progresses.update(|p| {
                        p.insert(token, progress);
                    });
                }
                lsp_types::WorkDoneProgress::Report(report) => {
                    self.progresses.update(|p| {
                        if let Some(progress) = p.get_mut(&token) {
                            progress.message.clone_from(&report.message);
                            progress.percentage = report.percentage;
                        }
                    })
                }
                lsp_types::WorkDoneProgress::End(_) => {
                    self.progresses.update(|p| {
                        p.swap_remove(&token);
                    });
                }
            },
        }
    }

    fn show_message(&self, title: &str, message: &ShowMessageParams) {
        crate::panel::workflow_tools::record(
            self.notifications,
            title,
            &message.message,
        );
        self.messages.update(|messages| {
            messages.push((title.to_string(), message.clone()));
        });
    }

    pub fn update_code_lens_id(&self, view_id: Option<ViewId>) {
        if let Some(Some(old_id)) = self.code_lens.try_update(|x| {
            let old = x.take();
            if let Some(id) = view_id {
                let _ = x.insert(id);
            }
            old
        }) {
            remove_overlay(old_id);
        }
    }

    pub fn show_code_lens(
        &self,
        mouse_click: bool,
        plugin_id: PluginId,
        offset: usize,
        lens: im::Vector<CodeLens>,
    ) {
        self.common
            .internal_command
            .send(InternalCommand::ShowCodeActions {
                offset,
                mouse_click,
                plugin_id,
                code_actions: lens
                    .into_iter()
                    .filter_map(|lens| {
                        Some(CodeActionOrCommand::Command(lens.command?))
                    })
                    .collect(),
            });
    }

    pub fn call_hierarchy_incoming(&self, item_id: ViewId) {
        let Some(root) = self.call_hierarchy_data.root.get_untracked() else {
            return;
        };
        let Some(item) = CallHierarchyItemData::find_by_id(root, item_id) else {
            return;
        };
        let root_item = item;
        let path: PathBuf = item.get_untracked().item.uri.to_file_path().unwrap();
        let scope = self.scope;
        let send =
            create_ext_action(scope, move |_rs: Result<ProxyResponse, RpcError>| {
                match _rs {
                    Ok(ProxyResponse::CallHierarchyIncomingResponse { items }) => {
                        if let Some(items) = items {
                            let mut item_children = Vec::new();
                            for x in items {
                                let item = Rc::new(x.from);
                                for range in x.from_ranges {
                                    item_children.push(scope.create_rw_signal(
                                        CallHierarchyItemData {
                                            view_id: ViewId::new(),
                                            item: item.clone(),
                                            from_range: range,
                                            init: false,
                                            open: scope.create_rw_signal(false),
                                            children:
                                                scope.create_rw_signal(Vec::new()),
                                        },
                                    ))
                                }
                            }
                            root_item.update(|x| {
                                x.init = true;
                                x.children.update(|children| {
                                    *children = item_children;
                                })
                            });
                        }
                    }
                    Err(err) => {
                        error!("{:?}", err);
                    }
                    Ok(_) => {}
                }
            });
        self.common.proxy.call_hierarchy_incoming(
            path,
            item.get_untracked().item.as_ref().clone(),
            send,
        );
    }

    fn apply_android_studio_layout(&self) {
        let values: Vec<(&str, &str, toml_edit::Value)> = vec![
            ("core", "color-theme", "Islands Dark".into()),
            ("core", "icon-theme", "sparrow Modern".into()),
            ("ui", "font-family", "Inter, Noto Sans, sans-serif".into()),
            ("ui", "font-size", 13.into()),
            ("ui", "icon-size", 16.into()),
            ("ui", "header-height", 32.into()),
            ("ui", "status-height", 24.into()),
            ("ui", "tool-window-stripe-width", 35.into()),
            ("ui", "list-line-height", 22.into()),
            ("ui", "scroll-width", 8.into()),
            ("ui", "open-editors-visible", false.into()),
            ("ui", "rounded-corners", true.into()),
            (
                "editor",
                "font-family",
                "JetBrains Mono, Noto Sans Mono, monospace".into(),
            ),
            ("editor", "font-size", 14.into()),
            ("editor", "line-height", 1.5.into()),
            ("terminal", "font-family", "JetBrains Mono, Noto Sans Mono, monospace".into()),
            ("terminal", "font-size", 13.into()),
            ("terminal", "line-height", 1.4.into()),
        ];
        for (section, key, value) in values {
            if SparrowConfig::update_file(section, key, value).is_none() {
                self.show_alert(
                    "Appearance".into(),
                    "Could not save appearance settings.".into(),
                    Vec::new(),
                );
                return;
            }
        }
        self.reload_config();
        let available = self.common.workbench_size.get_untracked();
        self.panel.size.update(|size| {
            size.left = 400.0_f64.min((available.width * 0.4).max(150.0));
            size.right =
                (self.layout_rect.get_untracked().width() * 0.243).max(250.0);
            size.bottom = (self.layout_rect.get_untracked().height() * 342.0
                / 1048.0)
                .max(150.0);
        });
        self.panel
            .panels
            .set(crate::panel::data::default_panel_order());
        self.panel.styles.update(|styles| {
            for (_, style) in styles.iter_mut() {
                style.shown = false;
                style.maximized = false;
            }
        });
        self.panel.show_panel(&PanelKind::FileExplorer);
        if self.flutter_hot_reload.state.get_untracked()
            == crate::dart::FlutterRunState::Running
        {
            self.panel.show_panel(&PanelKind::FlutterRun);
        }
    }

    pub fn open_devtools(&self, page: crate::dart::devtools::DevToolsPage) {
        if self.workspace.kind != SparrowWorkspaceType::Local {
            self.show_alert(
                "DevTools".into(),
                "DevTools currently requires a local workspace.".into(),
                Vec::new(),
            );
            return;
        }
        let vm = self.flutter_hot_reload.ws_uri.get_untracked().or_else(|| {
            self.flutter_inspector
                .vm_url
                .get_untracked()
                .map(|(_, url)| url)
        });
        self.devtools.open(self.workspace.path.clone(), vm, page);
        self.show_panel(PanelKind::FlutterPerformance);
    }

    fn run_test_at_cursor(&self, debug: bool) {
        use crate::dart::test_runner;

        let Some(editor) = self.main_split.active_editor.get_untracked() else {
            return;
        };
        let doc = editor.doc();
        let Some(path) = doc.content.with_untracked(|c| c.path().cloned()) else {
            return;
        };
        if !test_runner::is_test_file(&path) {
            return;
        }
        let source = doc.buffer.with_untracked(|b| b.to_string());
        let offset = editor.cursor().with_untracked(|c| c.offset());

        let Some(test) = test_runner::find_test_at_offset(&source, offset) else {
            self.show_alert(
                "No test found".to_string(),
                "Place the cursor inside a `test(...)` or `testWidgets(...)` block."
                    .to_string(),
                Vec::new(),
            );
            return;
        };

        self.prepare_flutter_tests(vec![(path, test.name)], debug, false);
    }

    fn rerun_flutter_tests(&self, failed_only: bool) {
        let targets = if failed_only {
            self.flutter_test.run.with_untracked(|r| r.failed_targets())
        } else {
            self.flutter_test.last_targets.get_untracked()
        };
        self.prepare_flutter_tests(
            targets,
            false,
            failed_only || self.flutter_test.last_exact.get_untracked(),
        );
    }

    fn prepare_flutter_tests(
        &self,
        targets: Vec<(PathBuf, String)>,
        debug: bool,
        exact: bool,
    ) {
        if self.flutter_test.run.with_untracked(|r| r.running) || targets.is_empty()
        {
            return;
        }
        if self.workspace.kind != SparrowWorkspaceType::Local {
            self.show_alert(
                "Flutter Test".into(),
                "Flutter tests currently require a local workspace.".into(),
                Vec::new(),
            );
            return;
        }
        if targets
            .iter()
            .any(|(path, _)| !crate::dart::test_runner::is_test_file(path))
        {
            return;
        }
        // Save all edited files before starting, including implementation files
        // used by the tests. A failed save never launches stale code.
        let dirty = self.main_split.docs.with_untracked(|docs| {
            docs.values()
                .filter(|doc| !doc.buffer.with_untracked(|b| b.is_pristine()))
                .cloned()
                .collect::<Vec<_>>()
        });
        let internal = self.common.internal_command;
        let remaining = Rc::new(std::cell::Cell::new(dirty.len()));
        if dirty.is_empty() {
            internal.send(InternalCommand::FlutterStartTests {
                targets,
                debug,
                exact,
            });
        } else {
            for doc in dirty {
                let remaining = remaining.clone();
                let targets = targets.clone();
                doc.save(move || {
                    remaining.set(remaining.get() - 1);
                    if remaining.get() == 0 {
                        internal.send(InternalCommand::FlutterStartTests {
                            targets,
                            debug,
                            exact,
                        });
                    }
                });
            }
        }
    }

    fn start_flutter_tests(
        &self,
        targets: Vec<(PathBuf, String)>,
        debug: bool,
        exact: bool,
    ) {
        if self.flutter_test.run.with_untracked(|r| r.running) {
            return;
        }
        if debug {
            let Some((path, name)) = targets.first() else {
                return;
            };
            let root = crate::dart::test_runner::project_root(path);
            let Some(flutter) =
                crate::dart::sdk::find_flutter_binary(root.as_deref())
            else {
                self.show_alert(
                    "Flutter SDK not found".into(),
                    "Configure a Flutter SDK before debugging tests.".into(),
                    Vec::new(),
                );
                return;
            };
            let config = RunDebugConfig {
                ty: Some("flutter-test".into()),
                name: format!("Debug Test: {name}"),
                program: path.display().to_string(),
                args: Some(if name.is_empty() {
                    vec![]
                } else {
                    vec!["--plain-name".into(), name.clone()]
                }),
                cwd: root.map(|p| p.display().to_string()),
                env: None,
                prelaunch: None,
                before_launch: Vec::new(),
                store_in_project: false,
                flutter_options: Default::default(),
                debug_adapter: Some(sparrow_rpc::dap_types::RunDebugProgram {
                    program: flutter.display().to_string(),
                    args: Some(vec!["debug-adapter".into(), "--test".into()]),
                }),
                debug_command: None,
                dap_id: Default::default(),
                tracing_output: false,
                config_source: Default::default(),
            };
            self.terminal.debug.active_dap.set(Some(config.dap_id));
            self.terminal.debug.daps.update(|daps| {
                daps.insert(
                    config.dap_id,
                    DapData::new(
                        self.scope,
                        config.dap_id,
                        TermId::next(),
                        self.common.clone(),
                    ),
                );
            });
            self.flutter_test.begin_debug(config.dap_id, targets);
            self.show_panel(PanelKind::FlutterTest);
            self.run_and_debug(self.scope, &RunDebugMode::Debug, &config);
        } else {
            self.flutter_test.start(targets, exact);
            self.show_panel(PanelKind::FlutterTest);
        }
    }
}

/// Open path with the default application without blocking.
fn open_uri(path: &Path) {
    match open::that(path) {
        Ok(_) => {
            debug!("opened active file: {path:?}");
        }
        Err(e) => {
            error!("failed to open active file: {path:?}, error: {e}");
        }
    }
}
