use std::{collections::HashMap, path::PathBuf, rc::Rc, sync::Arc};

use floem::{
    ext_event::create_ext_action,
    reactive::{Memo, RwSignal, Scope, SignalGet, SignalUpdate, SignalWith},
};
use sparrow_core::mode::Mode;
use sparrow_rpc::{
    dap_types::{
        self, DapId, RunDebugConfig, StackFrame, Stopped, ThreadId, Variable,
    },
    proxy::ProxyResponse,
    terminal::{TermId, TerminalProfile},
};

use super::{data::TerminalData, tab::TerminalTabData};
use crate::{
    debug::{
        DapData, DapVariable, RunDebugData, RunDebugMode,
        RunDebugProcess, ScopeOrVar,
    },
    id::TerminalTabId,
    keypress::{EventRef, KeyPressData, KeyPressFocus, KeyPressHandle},
    main_split::MainSplitData,
    panel::kind::PanelKind,
    window_tab::{CommonData, Focus},
    workspace::SparrowWorkspace,
};

#[derive(Clone)]
pub struct TerminalTabInfo {
    pub active: usize,
    pub tabs: im::Vector<(RwSignal<usize>, TerminalTabData)>,
}

#[derive(Clone)]
pub struct TerminalPanelData {
    pub cx: Scope,
    pub workspace: Arc<SparrowWorkspace>,
    pub tab_info: RwSignal<TerminalTabInfo>,
    pub debug: RunDebugData,
    pub breakline: Memo<Option<(usize, PathBuf)>>,
    pub common: Rc<CommonData>,
    pub main_split: MainSplitData,
}

impl TerminalPanelData {
    pub fn new(
        workspace: Arc<SparrowWorkspace>,
        profile: Option<TerminalProfile>,
        common: Rc<CommonData>,
        main_split: MainSplitData,
    ) -> Self {
        let terminal_tab =
            TerminalTabData::new(workspace.clone(), profile, common.clone());

        let cx = common.scope;

        let tabs =
            im::vector![(terminal_tab.scope.create_rw_signal(0), terminal_tab)];
        let tab_info = TerminalTabInfo { active: 0, tabs };
        let tab_info = cx.create_rw_signal(tab_info);

        let debug = RunDebugData::new(cx, common.breakpoints);

        let breakline = {
            let adapter_dap = debug.active_dap;
            let active_term = debug.active_term;
            let daps = debug.daps;
            cx.create_memo(move |_| {
                if let Some(id) = adapter_dap.get() {
                    return daps
                        .with(|daps| daps.get(&id).cloned())
                        .filter(|dap| dap.stopped.get())
                        .and_then(|dap| dap.breakline.get());
                }
                let active_term = active_term.get();
                let active_term = match active_term {
                    Some(active_term) => active_term,
                    None => return None,
                };

                let term = tab_info.with_untracked(|info| {
                    for (_, tab) in &info.tabs {
                        let terminal = tab.terminals.with_untracked(|terminals| {
                            terminals
                                .iter()
                                .find(|(_, t)| t.term_id == active_term)
                                .cloned()
                        });
                        if let Some(terminal) = terminal {
                            return Some(terminal.1);
                        }
                    }
                    None
                });
                let term = match term {
                    Some(term) => term,
                    None => return None,
                };
                let stopped = term
                    .run_debug
                    .with(|run_debug| run_debug.as_ref().map(|r| r.stopped))
                    .unwrap_or(true);
                if stopped {
                    return None;
                }

                let daps = daps.get();
                let dap = daps.values().find(|d| d.term_id == active_term);
                dap.and_then(|dap| dap.breakline.get())
            })
        };

        let panel = Self { cx, workspace, tab_info, debug, breakline, common, main_split };
        let monitored = panel.clone();
        floem::reactive::create_effect(move |previous: Option<(HashMap<std::path::PathBuf, Vec<dap_types::SourceBreakpoint>>, Vec<DapId>)>| {
            monitored.debug.breakpoints.with(|_| ());
            monitored.debug.muted.get();
            let current = monitored.debug.source_breakpoints();
            let mut adapters = monitored.run_debug_process(true).into_iter()
                .filter(|(_, p)| p.mode == RunDebugMode::Debug && !p.stopped).map(|(_, p)|p.config.dap_id).collect::<Vec<_>>();
            adapters.sort_by_key(|id|id.0);
            if previous.as_ref() != Some(&(current.clone(), adapters.clone())) {
                let mut paths = current.keys().cloned().collect::<std::collections::BTreeSet<_>>();
                if let Some((old, _)) = &previous { paths.extend(old.keys().cloned()); }
                for dap_id in &adapters {
                    for path in &paths { monitored.common.proxy.dap_set_breakpoints(*dap_id, path.clone(), current.get(path).cloned().unwrap_or_default()); }
                }
            }
            (current, adapters)
        });
        panel
    }

    pub fn active_tab(&self, tracked: bool) -> Option<TerminalTabData> {
        if tracked {
            self.tab_info.with(|info| {
                info.tabs
                    .get(info.active)
                    .or_else(|| info.tabs.last())
                    .cloned()
                    .map(|(_, tab)| tab)
            })
        } else {
            self.tab_info.with_untracked(|info| {
                info.tabs
                    .get(info.active)
                    .or_else(|| info.tabs.last())
                    .cloned()
                    .map(|(_, tab)| tab)
            })
        }
    }

    pub fn key_down<'a>(
        &self,
        event: impl Into<EventRef<'a>> + Copy,
        keypress: &KeyPressData,
    ) -> Option<KeyPressHandle> {
        if self.tab_info.with_untracked(|info| info.tabs.is_empty()) {
            self.new_tab(None);
        }

        let tab = self.active_tab(false);
        let terminal = tab.and_then(|tab| tab.active_terminal(false));
        if let Some(terminal) = terminal {
            let handle = keypress.key_down(event, &terminal);
            let mode = terminal.get_mode();

            if !handle.handled && mode == Mode::Terminal {
                if let EventRef::Keyboard(key_event) = event.into() {
                    if terminal.send_keypress(key_event) {
                        return Some(KeyPressHandle {
                            handled: true,
                            keymatch: handle.keymatch,
                            keypress: handle.keypress,
                        });
                    }
                }
            }
            Some(handle)
        } else {
            None
        }
    }

    pub fn new_tab(&self, profile: Option<TerminalProfile>) {
        self.new_tab_run_debug(None, profile);
    }

    /// Create a new terminal tab with the given run debug process.  
    /// Errors if expanding out the run debug process failed.
    pub fn new_tab_run_debug(
        &self,
        run_debug: Option<RunDebugProcess>,
        profile: Option<TerminalProfile>,
    ) -> TerminalTabData {
        let terminal_tab = TerminalTabData::new_run_debug(
            self.workspace.clone(),
            run_debug,
            profile,
            self.common.clone(),
        );

        self.tab_info.update(|info| {
            info.tabs.insert(
                if info.tabs.is_empty() {
                    0
                } else {
                    (info.active + 1).min(info.tabs.len())
                },
                (terminal_tab.scope.create_rw_signal(0), terminal_tab.clone()),
            );
            let new_active = (info.active + 1).min(info.tabs.len() - 1);
            info.active = new_active;
        });

        terminal_tab
    }

    pub fn next_tab(&self) {
        self.tab_info.update(|info| {
            if info.active >= info.tabs.len().saturating_sub(1) {
                info.active = 0;
            } else {
                info.active += 1;
            }
        });
        self.update_debug_active_term();
    }

    pub fn previous_tab(&self) {
        self.tab_info.update(|info| {
            if info.active == 0 {
                info.active = info.tabs.len().saturating_sub(1);
            } else {
                info.active -= 1;
            }
        });
        self.update_debug_active_term();
    }

    pub fn close_tab(&self, terminal_tab_id: Option<TerminalTabId>) {
        if let Some(close_tab) = self
            .tab_info
            .try_update(|info| {
                let mut close_tab = None;
                if let Some(terminal_tab_id) = terminal_tab_id {
                    if let Some(index) =
                        info.tabs.iter().enumerate().find_map(|(index, (_, t))| {
                            if t.terminal_tab_id == terminal_tab_id {
                                Some(index)
                            } else {
                                None
                            }
                        })
                    {
                        close_tab = Some(
                            info.tabs.remove(index).1.terminals.get_untracked(),
                        );
                    }
                } else {
                    let active = info.active.min(info.tabs.len().saturating_sub(1));
                    if !info.tabs.is_empty() {
                        close_tab = Some(info.tabs.remove(active).1.terminals.get_untracked());
                    }
                }
                let new_active = info.active.min(info.tabs.len().saturating_sub(1));
                info.active = new_active;
                close_tab
            })
            .flatten()
        {
            for (_, data) in close_tab {
                data.stop();
            }
        }
        self.update_debug_active_term();
    }

    pub fn set_title(&self, term_id: &TermId, title: &str) {
        if let Some(t) = self.get_terminal(term_id) {
            t.title.set(title.to_string());
        }
    }

    pub fn get_terminal(&self, term_id: &TermId) -> Option<TerminalData> {
        self.tab_info.with_untracked(|info| {
            for (_, tab) in &info.tabs {
                let terminal = tab.terminals.with_untracked(|terminals| {
                    terminals
                        .iter()
                        .find(|(_, t)| &t.term_id == term_id)
                        .cloned()
                });
                if let Some(terminal) = terminal {
                    return Some(terminal.1);
                }
            }
            None
        })
    }

    fn get_terminal_in_tab(
        &self,
        term_id: &TermId,
    ) -> Option<(usize, TerminalTabData, usize, TerminalData)> {
        self.tab_info.with_untracked(|info| {
            for (tab_index, (_, tab)) in info.tabs.iter().enumerate() {
                let result = tab.terminals.with_untracked(|terminals| {
                    terminals
                        .iter()
                        .enumerate()
                        .find(|(_, (_, t))| &t.term_id == term_id)
                        .map(|(i, (_, terminal))| (i, terminal.clone()))
                });
                if let Some((index, terminal)) = result {
                    return Some((tab_index, tab.clone(), index, terminal));
                }
            }
            None
        })
    }

    pub fn split(&self, term_id: TermId) {
        if let Some((_, tab, index, _)) = self.get_terminal_in_tab(&term_id) {
            let terminal_data = TerminalData::new(
                tab.scope,
                self.workspace.clone(),
                None,
                self.common.clone(),
            );
            let i = terminal_data.scope.create_rw_signal(0);
            tab.terminals.update(|terminals| {
                terminals.insert(index + 1, (i, terminal_data));
            });
        }
    }

    pub fn split_next(&self, term_id: TermId) {
        if let Some((_, tab, index, _)) = self.get_terminal_in_tab(&term_id) {
            let max = tab.terminals.with_untracked(|t| t.len() - 1);
            let new_index = (index + 1).min(max);
            if new_index != index {
                tab.active.set(new_index);
                self.update_debug_active_term();
            }
        }
    }

    pub fn split_previous(&self, term_id: TermId) {
        if let Some((_, tab, index, _)) = self.get_terminal_in_tab(&term_id) {
            let new_index = index.saturating_sub(1);
            if new_index != index {
                tab.active.set(new_index);
                self.update_debug_active_term();
            }
        }
    }

    pub fn split_exchange(&self, term_id: TermId) {
        if let Some((_, tab, index, _)) = self.get_terminal_in_tab(&term_id) {
            let max = tab.terminals.with_untracked(|t| t.len() - 1);
            if index < max {
                tab.terminals.update(|terminals| {
                    terminals.swap(index, index + 1);
                });
                self.update_debug_active_term();
            }
        }
    }

    pub fn close_terminal(&self, term_id: &TermId) {
        if let Some(terminal) = self.get_terminal(term_id) { terminal.activity.set(None); }
        if let Some((_, tab, index, terminal)) = self.get_terminal_in_tab(term_id) {
            terminal.stop();
            let active = tab.active.get_untracked();
            let len = tab
                .terminals
                .try_update(|terminals| {
                    terminals.remove(index);
                    terminals.len()
                })
                .unwrap();
            if len == 0 {
                self.close_tab(Some(tab.terminal_tab_id));
            } else {
                let new_active = active.min(len.saturating_sub(1));
                if new_active != active {
                    tab.active.set(new_active);
                    self.update_debug_active_term();
                }
            }
        }
    }

    pub fn launch_failed(&self, term_id: &TermId, error: &str) {
        if let Some(terminal) = self.get_terminal(term_id) {
            terminal.activity.set(None);
            terminal.launch_error.set(Some(error.to_string()));
        }
    }

    pub fn terminal_stopped(&self, term_id: &TermId, exit_code: Option<i32>) {
        if let Some(terminal) = self.get_terminal(term_id) {
            terminal.activity.set(None);
            if terminal.run_debug.with_untracked(|r| r.is_some()) {
                let was_prelaunch = terminal
                    .run_debug
                    .try_update(|run_debug| {
                        if let Some(run_debug) = run_debug.as_mut() {
                            let prelaunch = run_debug.is_prelaunch && run_debug.config.prelaunch.is_some();
                            run_debug.is_prelaunch = false;
                            run_debug.stopped = true;
                            Some(prelaunch)
                        } else {
                            None
                        }
                    })
                    .unwrap();
                let exit_code = exit_code.unwrap_or(-1);
                if was_prelaunch == Some(true) && exit_code == 0 {
                    let run_debug = terminal.run_debug.get_untracked();
                    if let Some(mut run_debug) = run_debug {
                        run_debug.stopped = false;
                        if run_debug.config.advance_before_launch() {
                            run_debug.is_prelaunch = true;
                            terminal.new_process(Some(run_debug));
                        } else if run_debug.mode == RunDebugMode::Debug {
                            // A completed task's PTY is not the debugger session.
                            // Keep one adapter-owned process alive even before
                            // the SDK requests an optional runInTerminal console.
                            self.debug.adapter_sessions.update(|sessions| {
                                sessions.insert(run_debug.config.dap_id, (*term_id, run_debug.clone()));
                            });
                            self.debug.active_term.set(Some(*term_id));
                            self.debug.active_dap.set(Some(run_debug.config.dap_id));
                            crate::background_tasks::debug_launch(&self.common.background_tasks.reporter(), run_debug.config.dap_id, &run_debug.config.name);
                            self.common.proxy.dap_start(
                                run_debug.config,
                                self.debug.source_breakpoints(),
                            )
                        } else {
                            terminal.new_process(Some(run_debug));
                        }
                    }
                }
            } else {
                self.close_terminal(term_id);
            }
        }
    }

    pub fn get_stopped_run_debug_terminal(
        &self,
        mode: &RunDebugMode,
        config: &RunDebugConfig,
    ) -> Option<TerminalData> {
        self.tab_info.with_untracked(|info| {
            for (_, tab) in &info.tabs {
                let terminal = tab.terminals.with_untracked(|terminals| {
                    for (_, terminal) in terminals {
                        if let Some(run_debug) =
                            terminal.run_debug.get_untracked().as_ref()
                        {
                            if run_debug.stopped && &run_debug.mode == mode {
                                match run_debug.mode {
                                    RunDebugMode::Run => {
                                        if run_debug.config.name == config.name {
                                            return Some(terminal.clone());
                                        }
                                    }
                                    RunDebugMode::Debug => {
                                        if run_debug.config.dap_id == config.dap_id {
                                            return Some(terminal.clone());
                                        }
                                    }
                                }
                            }
                        }
                    }
                    None
                });
                if let Some(terminal) = terminal {
                    return Some(terminal);
                }
            }
            None
        })
    }

    /// Return whether it is in debug mode.
    pub fn restart_run_debug(&self, term_id: TermId) -> Option<bool> {
        let process = self.run_debug_process(false).into_iter().find(|(id,_)| *id==term_id)?.1;
        let old_dap = process.config.dap_id;
        let adapter_owned = self.debug.adapter_sessions.with_untracked(|sessions|sessions.contains_key(&old_dap));
        let mut config = self.workspace.path.as_deref().and_then(crate::dart::run_config::load)
            .and_then(|configs|configs.into_iter().find(|config|config.name==process.config.name)).unwrap_or(process.config);
        config.dap_id = DapId::next();
        self.stop_run_debug(term_id);
        if adapter_owned {
            if let Some(dap) = self.debug.daps.with_untracked(|daps| daps.get(&old_dap).cloned()) {
                dap.clear_pause();
            }
            self.debug.adapter_sessions.update(|sessions| { sessions.remove(&old_dap); });
            self.debug.daps.update(|daps| { daps.remove(&old_dap); });
            self.debug.console_output.update(|logs| { logs.remove(&old_dap); });
            if self.debug.active_term.get_untracked() == Some(term_id) {
                self.debug.active_term.set(None);
                self.debug.active_dap.set(None);
            }
            if self.common.debugger_settings.get_untracked().unmute_on_finish { self.debug.muted.set(false); }
            crate::debug_settings::refresh(self);
        }
        self.common.internal_command.send(crate::command::InternalCommand::RunAndDebug {mode:process.mode,config});
        Some(process.mode==RunDebugMode::Debug)
    }

    pub fn focus_terminal(&self, term_id: TermId) {
        self.focus_terminal_in_panel(term_id, PanelKind::Terminal);
    }

    pub fn focus_terminal_in_panel(&self, term_id: TermId, panel: PanelKind) {
        if let Some(dap_id) = self.debug.adapter_sessions.with_untracked(|sessions| sessions.iter()
            .find(|(_, (id, _))| *id == term_id).map(|(dap_id, _)| *dap_id)) {
            self.debug.active_term.set(Some(term_id));
            self.debug.active_dap.set(Some(dap_id));
            self.common.focus.set(Focus::Panel(panel));
            return;
        }
        self.debug.active_dap.set(None);
        if let Some((tab_index, terminal_tab, index, _terminal)) =
            self.get_terminal_in_tab(&term_id)
        {
            self.tab_info.update(|info| {
                info.active = tab_index;
            });
            terminal_tab.active.set(index);
            self.common.focus.set(Focus::Panel(panel));

            self.update_debug_active_term();
        }
    }

    pub fn update_debug_active_term(&self) {
        let tab = self.active_tab(false);
        let terminal = tab.and_then(|tab| tab.active_terminal(false));
        if let Some(terminal) = terminal {
            let term_id = terminal.term_id;
            let is_run_debug =
                terminal.run_debug.with_untracked(|run| run.is_some());
            let current_active = self.debug.active_term.get_untracked();
            if is_run_debug {
                if current_active != Some(term_id) {
                    self.debug.active_term.set(Some(term_id));
                }
            } else if let Some(active) = current_active {
                if self.get_terminal(&active).is_none() {
                    self.debug.active_term.set(None);
                }
            }
        } else {
            self.debug.active_term.set(None);
        }
    }

    pub fn stop_run_debug(&self, term_id: TermId) -> Option<()> {
        if let Some(id) = self.debug.adapter_sessions.with_untracked(|sessions| sessions.iter()
            .find(|(_, (id, _))| *id == term_id).map(|(dap, _)| *dap)) {
            self.common.proxy.dap_stop(id);
            return Some(());
        }
        let terminal = self.get_terminal(&term_id)?;
        let run_debug = terminal.run_debug.get_untracked()?;

        if run_debug.is_prelaunch && run_debug.config.prelaunch.is_some() {
            // Cancel the task before closing its PTY, so its exit notification
            // cannot start the application after the user pressed Stop.
            terminal.run_debug.update(|process| {
                if let Some(process) = process { process.is_prelaunch = false; process.stopped = true; }
            });
            self.common.proxy.terminal_close(term_id);
            return Some(());
        }

        match run_debug.mode {
            RunDebugMode::Run => {
                self.common.proxy.terminal_close(term_id);
            }
            RunDebugMode::Debug => {
                let dap_id = run_debug.config.dap_id;
                let daps = self.debug.daps.get_untracked();
                let dap = daps.get(&dap_id)?;
                self.common.proxy.dap_stop(dap.dap_id);
            }
        }

        self.focus_terminal(term_id);
        Some(())
    }

    pub fn run_debug_process(
        &self,
        tracked: bool,
    ) -> Vec<(TermId, RunDebugProcess)> {
        let mut processes = Vec::new();
        if tracked {
            self.tab_info.with(|info| {
                for (_, tab) in &info.tabs {
                    tab.terminals.with(|terminals| {
                        for (_, terminal) in terminals {
                            if let Some(run_debug) = terminal.run_debug.get() {
                                processes.push((terminal.term_id, run_debug));
                            }
                        }
                    })
                }
            });
        } else {
            self.tab_info.with_untracked(|info| {
                for (_, tab) in &info.tabs {
                    tab.terminals.with_untracked(|terminals| {
                        for (_, terminal) in terminals {
                            if let Some(run_debug) = terminal.run_debug.get_untracked() {
                                processes.push((terminal.term_id, run_debug));
                            }
                        }
                    })
                }
            });
        }
        let adapters = if tracked { self.debug.adapter_sessions.get() } else { self.debug.adapter_sessions.get_untracked() };
        merge_processes(processes, adapters)
    }

    pub fn set_process_id(&self, term_id: &TermId, process_id: Option<u32>) {
        if let Some(terminal) = self.get_terminal(term_id) {
            terminal.run_debug.with_untracked(|run_debug| {
                if let Some(run_debug) = run_debug.as_ref() {
                    if run_debug.config.debug_command.is_some() {
                        let dap_id = run_debug.config.dap_id;
                        self.common
                            .proxy
                            .dap_process_id(dap_id, process_id, *term_id);
                    }
                }
            });
        }
    }

    pub fn dap_continued(&self, dap_id: &DapId) {
        let dap = self
            .debug
            .daps
            .with_untracked(|daps| daps.get(dap_id).cloned());
        if let Some(dap) = dap {
            dap.clear_pause();
        }
        crate::debug_settings::refresh(self);
    }

    pub fn dap_stopped(
        &self,
        dap_id: &DapId,
        stopped: &Stopped,
        stack_frames: &HashMap<ThreadId, Vec<StackFrame>>,
        variables: &[(dap_types::Scope, Vec<Variable>)],
    ) {
        let dap = self
            .debug
            .daps
            .with_untracked(|daps| daps.get(dap_id).cloned());
        if let Some(dap) = dap {
            self.debug.active_term.set(Some(dap.term_id));
            self.debug.active_dap.set(Some(dap.dap_id));
            dap.stopped(self.cx, stopped, stack_frames, variables);
        }
        crate::debug_settings::refresh(self);
        floem::action::focus_window();
    }

    pub fn dap_continue(&self, term_id: TermId) -> Option<()> {
        let dap_id = self.get_dap(term_id, false)?.dap_id;
        let thread_id = self.debug.daps.with_untracked(|daps| {
            daps.get(&dap_id)
                .and_then(|dap| dap.thread_id.get_untracked())
        });
        let thread_id = thread_id.unwrap_or_default();
        self.common.proxy.dap_continue(dap_id, thread_id);
        Some(())
    }

    pub fn dap_pause(&self, term_id: TermId) -> Option<()> {
        let dap_id = self.get_dap(term_id, false)?.dap_id;
        let thread_id = self.debug.daps.with_untracked(|daps| {
            daps.get(&dap_id)
                .and_then(|dap| dap.thread_id.get_untracked())
        });
        let thread_id = thread_id.unwrap_or_default();
        self.common.proxy.dap_pause(dap_id, thread_id);
        Some(())
    }

    pub fn dap_step_over(&self, term_id: TermId) -> Option<()> {
        let dap_id = self.get_dap(term_id, false)?.dap_id;
        let thread_id = self.debug.daps.with_untracked(|daps| {
            daps.get(&dap_id)
                .and_then(|dap| dap.thread_id.get_untracked())
        });
        let thread_id = thread_id.unwrap_or_default();
        self.common.proxy.dap_step_over(dap_id, thread_id);
        Some(())
    }

    pub fn dap_step_into(&self, term_id: TermId) -> Option<()> {
        let dap_id = self.get_dap(term_id, false)?.dap_id;
        let thread_id = self.debug.daps.with_untracked(|daps| {
            daps.get(&dap_id)
                .and_then(|dap| dap.thread_id.get_untracked())
        });
        let thread_id = thread_id.unwrap_or_default();
        self.common.proxy.dap_step_into(dap_id, thread_id);
        Some(())
    }

    pub fn dap_step_out(&self, term_id: TermId) -> Option<()> {
        let dap_id = self.get_dap(term_id, false)?.dap_id;
        let thread_id = self.debug.daps.with_untracked(|daps| {
            daps.get(&dap_id)
                .and_then(|dap| dap.thread_id.get_untracked())
        });
        let thread_id = thread_id.unwrap_or_default();
        self.common.proxy.dap_step_out(dap_id, thread_id);
        Some(())
    }

    pub fn get_active_dap(&self, tracked: bool) -> Option<DapData> {
        let adapter = if tracked {
            self.debug.active_dap.get()
        } else {
            self.debug.active_dap.get_untracked()
        };
        if let Some(id) = adapter {
            return if tracked {
                self.debug.daps.with(|daps| daps.get(&id).cloned())
            } else {
                self.debug
                    .daps
                    .with_untracked(|daps| daps.get(&id).cloned())
            };
        }
        let active_term = if tracked {
            self.debug.active_term.get()?
        } else {
            self.debug.active_term.get_untracked()?
        };
        self.get_dap(active_term, tracked)
    }

    pub fn get_dap(&self, term_id: TermId, tracked: bool) -> Option<DapData> {
        let adapters = if tracked { self.debug.adapter_sessions.get() } else { self.debug.adapter_sessions.get_untracked() };
        if let Some((id, _)) = adapters.iter().find(|(_, (id, _))| *id == term_id) {
            return self.debug.daps.with(|daps| daps.get(id).cloned());
        }

        let terminal = self.get_terminal(&term_id)?;
        let dap_id = if tracked {
            terminal
                .run_debug
                .with(|r| r.as_ref().map(|r| r.config.dap_id))?
        } else {
            terminal
                .run_debug
                .with_untracked(|r| r.as_ref().map(|r| r.config.dap_id))?
        };

        if tracked {
            self.debug.daps.with(|daps| daps.get(&dap_id).cloned())
        } else {
            self.debug
                .daps
                .with_untracked(|daps| daps.get(&dap_id).cloned())
        }
    }

    pub fn dap_frame_scopes(&self, dap_id: DapId, frame_id: usize) {
        if let Some(dap) = self.debug.daps.get_untracked().get(&dap_id) {
            if !dap.stopped.get_untracked() {
                return;
            }
            let frame = dap.stack_traces.with_untracked(|traces| {
                traces.values().find_map(|trace| {
                    trace.frames.with_untracked(|frames| {
                        frames.iter().find(|frame| frame.id == frame_id).cloned()
                    })
                })
            });
            let Some(frame) = frame else { return };
            dap.variable_requests.invalidate();
            dap.selected_frame.set(Some(frame));
            let is_current = dap.variable_requests.capture();
            let variables = dap.variables;
            // Clear the previous frame immediately while its replacement loads.
            variables.set(DapVariable::default());
            dap.refresh_watches();
            let terminal = self.clone();
            crate::debug_settings::refresh(self);
            let send = create_ext_action(self.common.scope, move |result| {
                if !is_current() {
                    return;
                }
                if let Ok(ProxyResponse::DapGetScopesResponse { scopes }) = result {
                    variables.update(|dap_var| {
                        dap_var.children = scopes
                            .iter()
                            .enumerate()
                            .map(|(i, (scope, vars))| DapVariable {
                                item: ScopeOrVar::Scope(scope.to_owned()),
                                parent: Vec::new(),
                                expanded: i == 0,
                                read: i == 0,
                                children: vars
                                    .iter()
                                    .map(|var| DapVariable {
                                        item: ScopeOrVar::Var(var.to_owned()),
                                        parent: vec![scope.variables_reference],
                                        expanded: false,
                                        read: false,
                                        children: Vec::new(),
                                        children_expanded_count: 0,
                                    })
                                    .collect(),
                                children_expanded_count: if i == 0 {
                                    vars.len()
                                } else {
                                    0
                                },
                            })
                            .collect();
                        dap_var.children_expanded_count = dap_var
                            .children
                            .iter()
                            .map(|v| v.children_expanded_count + 1)
                            .sum::<usize>();
                    });
                    crate::debug_settings::refresh(&terminal);
                }
            });

            self.common
                .proxy
                .dap_get_scopes(dap_id, frame_id, move |result| {
                    send(result);
                });
        }
    }
}

fn merge_processes(mut terminals: Vec<(TermId, RunDebugProcess)>, adapters: HashMap<DapId, (TermId, RunDebugProcess)>) -> Vec<(TermId, RunDebugProcess)> {
    // runInTerminal may reuse the task's PTY; the adapter owns that session.
    terminals.retain(|(_, process)| !adapters.contains_key(&process.config.dap_id));
    terminals.extend(adapters.into_values());
    terminals.sort_by_key(|(_, process)| process.created);
    terminals
}

#[cfg(test)]
mod session_tests {
    use super::*;
    #[test]
    fn completed_task_is_replaced_by_live_adapter_without_hiding_other_launches() {
        let make = |mode, stopped| RunDebugProcess {
            mode, stopped, created: std::time::Instant::now(), is_prelaunch: false,
            config: serde_json::from_value(serde_json::json!({"name":"app", "program":"flutter", "args":["run"]})).unwrap(),
        };
        let task_id = TermId::next();
        let task = make(RunDebugMode::Debug, true);
        let mut adapter = task.clone(); adapter.stopped = false;
        let other_id = TermId::next();
        let other = make(RunDebugMode::Run, false);
        let processes = merge_processes(vec![(task_id, task.clone()), (other_id, other)], HashMap::from([(task.config.dap_id, (task_id, adapter))]));
        assert_eq!(processes.len(), 2);
        assert!(processes.iter().all(|(_, process)| !process.stopped));
        assert_eq!(processes.iter().filter(|(id, _)| *id == task_id).count(), 1);
        assert!(processes.iter().any(|(id, _)| *id == other_id));
    }
}
