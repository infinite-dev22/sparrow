use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    rc::Rc,
    time::Instant,
};

use floem::reactive::{create_effect, create_memo, create_rw_signal};
use floem::style::{CursorStyle, Display};
use floem::views::{Decorators, container, empty, label, scroll, stack, svg};
use floem::{
    IntoView,
    ext_event::create_ext_action,
    reactive::{Memo, RwSignal, Scope, SignalGet, SignalUpdate, SignalWith},
    views::VirtualVector,
};
use serde::{Deserialize, Serialize};
use sparrow_rpc::{
    dap_types::{
        self, DapId, RunDebugConfig, SourceBreakpoint, StackFrame, Stopped,
        ThreadId, Variable,
    },
    proxy::ProxyResponse,
    terminal::TermId,
};

use crate::app::clickable_icon;
use crate::config::color::SparrowColor;
use crate::config::icon::SparrowIcons;
use crate::editor::EditorData;
use crate::text_input::TextInputBuilder;
use crate::{
    command::InternalCommand,
    editor::location::{EditorLocation, EditorPosition},
    window_tab::CommonData,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RunDebugMode {
    Run,
    Debug,
}

impl std::fmt::Display for RunDebugMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            RunDebugMode::Run => "Run",
            RunDebugMode::Debug => "Debug",
        };
        f.write_str(s)
    }
}

#[derive(Clone)]
pub struct RunDebugProcess {
    pub mode: RunDebugMode,
    pub config: RunDebugConfig,
    pub stopped: bool,
    pub created: Instant,
    pub is_prelaunch: bool,
}

#[derive(Deserialize, Serialize)]
pub struct RunDebugConfigs {
    pub configs: Vec<RunDebugConfig>,
}

#[derive(Clone)]
pub struct RunDebugData {
    pub active_term: RwSignal<Option<TermId>>,
    pub daps: RwSignal<im::HashMap<DapId, DapData>>,
    pub breakpoints: RwSignal<BTreeMap<PathBuf, BTreeMap<usize, SparrowBreakpoint>>>,
}

impl RunDebugData {
    pub fn new(
        cx: Scope,
        breakpoints: RwSignal<BTreeMap<PathBuf, BTreeMap<usize, SparrowBreakpoint>>>,
    ) -> Self {
        let active_term: RwSignal<Option<TermId>> = cx.create_rw_signal(None);
        let daps: RwSignal<im::HashMap<DapId, DapData>> =
            cx.create_rw_signal(im::HashMap::new());

        Self {
            active_term,
            daps,
            breakpoints,
        }
    }

    pub fn source_breakpoints(&self) -> HashMap<PathBuf, Vec<SourceBreakpoint>> {
        self.breakpoints
            .get_untracked()
            .iter()
            .map(|(path, breakpoints)| {
                (
                    path.to_path_buf(),
                    breakpoints
                        .iter()
                        .filter_map(|(_, b)| {
                            if b.active {
                                Some(SourceBreakpoint {
                                    line: b.line + 1,
                                    column: None,
                                    condition: None,
                                    hit_condition: None,
                                    log_message: None,
                                })
                            } else {
                                None
                            }
                        })
                        .collect(),
                )
            })
            .collect()
    }
}

#[derive(Clone, PartialEq)]
pub struct StackTraceData {
    pub expanded: RwSignal<bool>,
    pub frames: RwSignal<im::Vector<StackFrame>>,
    pub frames_shown: usize,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SparrowBreakpoint {
    pub id: Option<usize>,
    pub verified: bool,
    pub message: Option<String>,
    pub line: usize,
    pub offset: usize,
    pub dap_line: Option<usize>,
    pub active: bool,
}

#[derive(Clone, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)]
pub enum ScopeOrVar {
    Scope(dap_types::Scope),
    Var(Variable),
}

impl Default for ScopeOrVar {
    fn default() -> Self {
        ScopeOrVar::Scope(dap_types::Scope::default())
    }
}

impl ScopeOrVar {
    pub fn name(&self) -> &str {
        match self {
            ScopeOrVar::Scope(scope) => &scope.name,
            ScopeOrVar::Var(var) => &var.name,
        }
    }

    pub fn value(&self) -> Option<&str> {
        match self {
            ScopeOrVar::Scope(_) => None,
            ScopeOrVar::Var(var) => Some(&var.value),
        }
    }

    pub fn ty(&self) -> Option<&str> {
        match self {
            ScopeOrVar::Scope(_) => None,
            ScopeOrVar::Var(var) => var.ty.as_deref(),
        }
    }

    pub fn reference(&self) -> usize {
        match self {
            ScopeOrVar::Scope(scope) => scope.variables_reference,
            ScopeOrVar::Var(var) => var.variables_reference,
        }
    }
}

#[derive(Clone, Default)]
pub struct DapVariable {
    pub item: ScopeOrVar,
    pub parent: Vec<usize>,
    pub expanded: bool,
    pub read: bool,
    pub children: Vec<DapVariable>,
    pub children_expanded_count: usize,
}

#[derive(Clone)]
pub struct DapData {
    pub term_id: TermId,
    pub dap_id: DapId,
    pub stopped: RwSignal<bool>,
    pub thread_id: RwSignal<Option<ThreadId>>,
    pub stack_traces: RwSignal<BTreeMap<ThreadId, StackTraceData>>,
    pub variables_id: RwSignal<usize>,
    pub variables: RwSignal<DapVariable>,
    pub breakline: Memo<Option<(usize, PathBuf)>>,
    pub common: Rc<CommonData>,
}

impl DapData {
    pub fn new(
        cx: Scope,
        dap_id: DapId,
        term_id: TermId,
        common: Rc<CommonData>,
    ) -> Self {
        let stopped = cx.create_rw_signal(false);
        let thread_id = cx.create_rw_signal(None);
        let stack_traces: RwSignal<BTreeMap<ThreadId, StackTraceData>> =
            cx.create_rw_signal(BTreeMap::new());
        let breakline = cx.create_memo(move |_| {
            let thread_id = thread_id.get();
            if let Some(thread_id) = thread_id {
                let trace = stack_traces
                    .with(|stack_traces| stack_traces.get(&thread_id).cloned());

                if let Some(trace) = trace {
                    let breakline = trace.frames.with(|f| {
                        f.get(0)
                            .and_then(|f| {
                                f.source
                                    .as_ref()
                                    .map(|s| (f.line.saturating_sub(1), s))
                            })
                            .and_then(|(line, s)| s.path.clone().map(|p| (line, p)))
                    });
                    return breakline;
                }
                None
            } else {
                None
            }
        });
        Self {
            term_id,
            dap_id,
            stopped,
            thread_id,
            stack_traces,
            variables_id: cx.create_rw_signal(0),
            variables: cx.create_rw_signal(DapVariable {
                item: ScopeOrVar::Scope(dap_types::Scope::default()),
                parent: Vec::new(),
                expanded: true,
                read: true,
                children: Vec::new(),
                children_expanded_count: 0,
            }),
            breakline,
            common,
        }
    }

    pub fn stopped(
        &self,
        cx: Scope,
        stopped: &Stopped,
        stack_traces: &HashMap<ThreadId, Vec<StackFrame>>,
        variables: &[(dap_types::Scope, Vec<Variable>)],
    ) {
        self.stopped.set(true);
        self.thread_id.update(|thread_id| {
            *thread_id = Some(stopped.thread_id.unwrap_or_default());
        });

        let main_thread_id = self.thread_id.get_untracked();
        let mut current_stack_traces = self.stack_traces.get_untracked();
        current_stack_traces.retain(|t, _| stack_traces.contains_key(t));
        for (thread_id, frames) in stack_traces {
            let is_main_thread = main_thread_id.as_ref() == Some(thread_id);
            if is_main_thread {
                if let Some(frame) = frames.first() {
                    if let Some(path) =
                        frame.source.as_ref().and_then(|source| source.path.clone())
                    {
                        self.common.internal_command.send(
                            InternalCommand::JumpToLocation {
                                location: EditorLocation {
                                    path,
                                    position: Some(EditorPosition::Line(
                                        frame.line.saturating_sub(1),
                                    )),
                                    scroll_offset: None,
                                    ignore_unconfirmed: false,
                                    same_editor_tab: false,
                                },
                            },
                        );
                    }
                }
            }
            if let Some(current) = current_stack_traces.get_mut(thread_id) {
                current.frames.set(frames.into());
                if is_main_thread {
                    current.expanded.set(true);
                }
            } else {
                current_stack_traces.insert(
                    *thread_id,
                    StackTraceData {
                        expanded: cx.create_rw_signal(is_main_thread),
                        frames: cx.create_rw_signal(frames.into()),
                        frames_shown: 20,
                    },
                );
            }
        }
        self.stack_traces.set(current_stack_traces);
        self.variables.update(|dap_var| {
            dap_var.children = variables
                .iter()
                .enumerate()
                .map(|(i, (scope, vars))| DapVariable {
                    item: ScopeOrVar::Scope(scope.to_owned()),
                    parent: Vec::new(),
                    expanded: i == 0,
                    read: true,
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
                    children_expanded_count: if i == 0 { vars.len() } else { 0 },
                })
                .collect();
            dap_var.children_expanded_count = dap_var
                .children
                .iter()
                .map(|v| v.children_expanded_count + 1)
                .sum::<usize>();
        });
    }

    pub fn toggle_expand(&self, parent: Vec<usize>, reference: usize) {
        self.variables_id.update(|id| {
            *id += 1;
        });
        self.variables.update(|variables| {
            if let Some(var) = variables.get_var_mut(&parent, reference) {
                if var.expanded {
                    var.expanded = false;
                    variables.update_count_recursive(&parent, reference);
                } else {
                    var.expanded = true;
                    if !var.read {
                        var.read = true;
                        self.read_var_children(&parent, reference);
                    } else {
                        variables.update_count_recursive(&parent, reference);
                    }
                }
            }
        });
    }

    fn read_var_children(&self, parent: &[usize], reference: usize) {
        let root = self.variables;
        let parent = parent.to_vec();
        let variables_id = self.variables_id;

        let send = create_ext_action(self.common.scope, move |result| {
            if let Ok(ProxyResponse::DapVariableResponse { variables }) = result {
                variables_id.update(|id| {
                    *id += 1;
                });
                root.update(|root| {
                    if let Some(var) = root.get_var_mut(&parent, reference) {
                        let mut new_parent = parent.clone();
                        new_parent.push(reference);
                        var.read = true;
                        var.children = variables
                            .into_iter()
                            .map(|v| DapVariable {
                                item: ScopeOrVar::Var(v),
                                parent: new_parent.clone(),
                                expanded: false,
                                read: false,
                                children: Vec::new(),
                                children_expanded_count: 0,
                            })
                            .collect();
                        root.update_count_recursive(&parent, reference);
                    }
                });
            }
        });
        self.common
            .proxy
            .dap_variable(self.dap_id, reference, move |result| {
                send(result);
            });
    }
}

pub struct DapVariableViewdata {
    pub item: ScopeOrVar,
    pub parent: Vec<usize>,
    pub expanded: bool,
    pub level: usize,
}

impl VirtualVector<DapVariableViewdata> for DapVariable {
    fn total_len(&self) -> usize {
        self.children_expanded_count
    }

    fn slice(
        &mut self,
        range: std::ops::Range<usize>,
    ) -> impl Iterator<Item = DapVariableViewdata> {
        let min = range.start;
        let max = range.end;
        let mut i = 0;
        let mut view_items = Vec::new();
        for item in self.children.iter() {
            i = item.append_view_slice(&mut view_items, min, max, i + 1, 0);
            if i > max {
                return view_items.into_iter();
            }
        }

        view_items.into_iter()
    }
}

impl DapVariable {
    pub fn append_view_slice(
        &self,
        view_items: &mut Vec<DapVariableViewdata>,
        min: usize,
        max: usize,
        current: usize,
        level: usize,
    ) -> usize {
        if current > max {
            return current;
        }
        if current + self.children_expanded_count < min {
            return current + self.children_expanded_count;
        }

        let mut i = current;
        if current >= min {
            view_items.push(DapVariableViewdata {
                item: self.item.clone(),
                parent: self.parent.clone(),
                expanded: self.expanded,
                level,
            });
        }

        if self.expanded {
            for item in self.children.iter() {
                i = item.append_view_slice(view_items, min, max, i + 1, level + 1);
                if i > max {
                    return i;
                }
            }
        }
        i
    }

    pub fn get_var_mut(
        &mut self,
        parent: &[usize],
        reference: usize,
    ) -> Option<&mut DapVariable> {
        let parent = if parent.is_empty() {
            self
        } else {
            parent.iter().try_fold(self, |item, parent| {
                item.children
                    .iter_mut()
                    .find(|c| c.item.reference() == *parent)
            })?
        };
        parent
            .children
            .iter_mut()
            .find(|c| c.item.reference() == reference)
    }

    pub fn update_count_recursive(&mut self, parent: &[usize], reference: usize) {
        let mut parent = parent.to_vec();
        self.update_count(&parent, reference);
        while let Some(reference) = parent.pop() {
            self.update_count(&parent, reference);
        }
        self.children_expanded_count = self
            .children
            .iter()
            .map(|item| item.children_expanded_count + 1)
            .sum::<usize>();
    }

    pub fn update_count(
        &mut self,
        parent: &[usize],
        reference: usize,
    ) -> Option<()> {
        let var = self.get_var_mut(parent, reference)?;
        var.children_expanded_count = if var.expanded {
            var.children
                .iter()
                .map(|item| item.children_expanded_count + 1)
                .sum::<usize>()
        } else {
            0
        };
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use sparrow_rpc::dap_types::{Scope, Variable};

    use super::{DapVariable, ScopeOrVar};

    #[test]
    fn test_update_count() {
        let variables = [
            (
                Scope {
                    variables_reference: 0,
                    ..Default::default()
                },
                vec![
                    Variable {
                        variables_reference: 3,
                        ..Default::default()
                    },
                    Variable {
                        variables_reference: 4,
                        ..Default::default()
                    },
                ],
            ),
            (
                Scope {
                    variables_reference: 1,
                    ..Default::default()
                },
                vec![
                    Variable {
                        variables_reference: 5,
                        ..Default::default()
                    },
                    Variable {
                        variables_reference: 6,
                        ..Default::default()
                    },
                ],
            ),
            (
                Scope {
                    variables_reference: 2,
                    ..Default::default()
                },
                vec![
                    Variable {
                        variables_reference: 7,
                        ..Default::default()
                    },
                    Variable {
                        variables_reference: 8,
                        ..Default::default()
                    },
                ],
            ),
        ];

        let mut root = DapVariable {
            item: ScopeOrVar::Scope(Scope::default()),
            parent: Vec::new(),
            expanded: true,
            read: true,
            children: variables
                .iter()
                .map(|(scope, vars)| DapVariable {
                    item: ScopeOrVar::Scope(scope.to_owned()),
                    parent: Vec::new(),
                    expanded: true,
                    read: true,
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
                    children_expanded_count: vars.len(),
                })
                .collect(),
            children_expanded_count: 0,
        };
        root.children_expanded_count = root
            .children
            .iter()
            .map(|v| v.children_expanded_count + 1)
            .sum::<usize>();
        assert_eq!(root.children_expanded_count, 9);

        let var = root.get_var_mut(&[0], 3).unwrap();
        var.expanded = true;
        var.read = true;
        var.children = [
            Variable {
                variables_reference: 9,
                ..Default::default()
            },
            Variable {
                variables_reference: 10,
                ..Default::default()
            },
        ]
        .iter()
        .map(|var| DapVariable {
            item: ScopeOrVar::Var(var.to_owned()),
            parent: vec![0, 3],
            expanded: false,
            read: false,
            children: Vec::new(),
            children_expanded_count: 0,
        })
        .collect();
        root.update_count_recursive(&[0], 3);
        let var = root.get_var_mut(&[0], 3).unwrap();
        assert_eq!(var.children_expanded_count, 2);
        let var = root.get_var_mut(&[], 0).unwrap();
        assert_eq!(var.children_expanded_count, 4);
        assert_eq!(root.children_expanded_count, 11);
    }
}

pub fn run_config_modal(
    window_tab_data: Rc<crate::window_tab::WindowTabData>,
) -> impl floem::View {
    let visible = window_tab_data.run_config_modal_visible;
    let config = window_tab_data.common.config;
    let active_idx = window_tab_data.active_run_config_idx;
    let configs = window_tab_data.run_configs;

    let cx = Scope::current();

    // 1. Create editors for the form fields
    let name_input = window_tab_data
        .main_split
        .editors
        .make_local(cx, window_tab_data.common.clone());
    let program_input = window_tab_data
        .main_split
        .editors
        .make_local(cx, window_tab_data.common.clone());
    let args_input = window_tab_data
        .main_split
        .editors
        .make_local(cx, window_tab_data.common.clone());
    let attach_args_input = window_tab_data
        .main_split
        .editors
        .make_local(cx, window_tab_data.common.clone());
    let build_flavor_input = window_tab_data
        .main_split
        .editors
        .make_local(cx, window_tab_data.common.clone());
    let env_vars_input = window_tab_data
        .main_split
        .editors
        .make_local(cx, window_tab_data.common.clone());

    // 2. Sync editor contents when the modal opens or the active item changes
    create_effect({
        let name_input = name_input.clone();
        let program_input = program_input.clone();
        let args_input = args_input.clone();
        let attach_args_input = attach_args_input.clone();
        let build_flavor_input = build_flavor_input.clone();
        let env_vars_input = env_vars_input.clone();
        move |_| {
            if visible.get() {
                let idx = active_idx.get();
                if let Some(c) = configs.with(|cfgs| cfgs.get(idx).cloned()) {
                    name_input
                        .doc()
                        .reload(lapce_xi_rope::Rope::from(c.name), true);
                    program_input
                        .doc()
                        .reload(lapce_xi_rope::Rope::from(c.program), true);

                    let args = c.args.unwrap_or_default().join(" ");
                    args_input
                        .doc()
                        .reload(lapce_xi_rope::Rope::from(args), true);

                    attach_args_input
                        .doc()
                        .reload(lapce_xi_rope::Rope::from(""), true);
                    build_flavor_input
                        .doc()
                        .reload(lapce_xi_rope::Rope::from(""), true);

                    let env_str = c
                        .env
                        .map(|e| {
                            e.into_iter()
                                .map(|(k, v)| format!("{}={}", k, v))
                                .collect::<Vec<_>>()
                                .join(";")
                        })
                        .unwrap_or_default();
                    env_vars_input
                        .doc()
                        .reload(lapce_xi_rope::Rope::from(env_str), true);
                }
            }
        }
    });

    // 3. Actions
    let add_action = {
        let wt = window_tab_data.clone();
        move || {
            wt.run_configs.update(|cfgs| {
                cfgs.push(RunDebugConfig {
                    ty: Some("flutter".to_string()),
                    name: "New Config".to_string(),
                    program: "flutter".to_string(),
                    args: Some(vec!["run".to_string()]),
                    cwd: wt
                        .workspace
                        .path
                        .as_ref()
                        .map(|p| p.to_string_lossy().to_string()),
                    env: None,
                    prelaunch: None,
                    debug_command: None,
                    dap_id: Default::default(),
                    tracing_output: true,
                    config_source: Default::default(),
                });
            });
            let new_len = wt.run_configs.with(|c| c.len());
            wt.active_run_config_idx.set(new_len - 1);
        }
    };

    let remove_action = {
        let wt = window_tab_data.clone();
        move || {
            if wt.run_configs.with(|c| c.len() > 1) {
                let idx = wt.active_run_config_idx.get_untracked();
                wt.run_configs.update(|cfgs| {
                    cfgs.remove(idx);
                });
                let new_idx = if idx > 0 { idx - 1 } else { 0 };
                wt.active_run_config_idx.set(new_idx);
            }
        }
    };

    let save_action = {
        let wt = window_tab_data.clone();
        let name_input = name_input.clone();
        let program_input = program_input.clone();
        let args_input = args_input.clone();
        let env_vars_input = env_vars_input.clone();

        move || {
            let idx = active_idx.get_untracked();
            let name = name_input.doc().buffer.with_untracked(|b| b.to_string());
            let prog = program_input.doc().buffer.with_untracked(|b| b.to_string());
            let args_str = args_input.doc().buffer.with_untracked(|b| b.to_string());
            let env_str = env_vars_input
                .doc()
                .buffer
                .with_untracked(|b| b.to_string());

            let args: Vec<String> =
                args_str.split_whitespace().map(|s| s.to_string()).collect();
            let env = if env_str.is_empty() {
                None
            } else {
                let mut map = HashMap::new();
                for pair in env_str.split(';') {
                    if let Some((k, v)) = pair.split_once('=') {
                        map.insert(k.trim().to_string(), v.trim().to_string());
                    }
                }
                Some(map)
            };

            configs.update(|cfgs| {
                if let Some(c) = cfgs.get_mut(idx) {
                    c.name = name;
                    c.program = prog;
                    c.args = if args.is_empty() { None } else { Some(args) };
                    c.env = env;
                }
            });

            let cfgs = configs.get_untracked();
            if let Some(workspace) = &wt.workspace.path {
                let run_toml = workspace.join(".sparrow").join("run.toml");
                if let Ok(toml_str) =
                    toml::to_string_pretty(&RunDebugConfigs { configs: cfgs })
                {
                    let _ = std::fs::create_dir_all(workspace.join(".sparrow"));
                    let _ = std::fs::write(run_toml, toml_str);
                }
            }
            wt.run_config_modal_visible.set(false);
        }
    };

    let run_action = {
        let wt = window_tab_data.clone();
        let save_action = save_action.clone();
        move || {
            save_action();
            let idx = wt.active_run_config_idx.get_untracked();
            if let Some(c) =
                wt.run_configs.with_untracked(|cfgs| cfgs.get(idx).cloned())
            {
                wt.common
                    .internal_command
                    .send(InternalCommand::RunAndDebug {
                        mode: RunDebugMode::Run,
                        config: c,
                    });
            }
        }
    };

    // 4. UI Helpers
    let form_row = move |label_text: &'static str,
                         editor: EditorData,
                         help_text: Option<&'static str>,
                         has_folder_icon: bool,
                         has_env_icon: bool| {
        stack((
            stack((
                label(move || label_text.to_string()).style(|s| {
                    s.width(150.0)
                        .selectable(false)
                        .justify_end()
                        .padding_right(10.0)
                }),
                container(
                    TextInputBuilder::new()
                        .is_focused(move || visible.get())
                        .build_editor(editor)
                        .style(move |s| {
                            s.width_full()
                                .min_height(26.0)
                                .padding_horiz(6.0)
                                .border(1.0)
                                .border_radius(4.0)
                                .border_color(
                                    config.get().color(SparrowColor::SPARROW_BORDER),
                                )
                                .background(
                                    config
                                        .get()
                                        .color(SparrowColor::EDITOR_BACKGROUND),
                                )
                        }),
                )
                .style(|s| s.flex_grow(1.0).flex_basis(0.0)),
                // FIX: Use .into_any() on clickable_icon to match empty().into_any()
                if has_folder_icon {
                    clickable_icon(
                        || SparrowIcons::DIRECTORY_CLOSED,
                        || {},
                        || false,
                        || false,
                        || "Browse",
                        config,
                    )
                    .style(|s| s.margin_left(4.0))
                    .into_any()
                } else if has_env_icon {
                    clickable_icon(
                        || SparrowIcons::SETTINGS,
                        || {},
                        || false,
                        || false,
                        || "Env Vars",
                        config,
                    )
                    .style(|s| s.margin_left(4.0))
                    .into_any()
                } else {
                    empty().into_any()
                },
            ))
            .style(|s| s.flex_row().items_center().width_full()),
            if let Some(help) = help_text {
                stack((
                    empty().style(|s| s.width(150.0)),
                    label(move || help.to_string()).style(move |s| {
                        s.font_size(11.0)
                            .color(config.get().color(SparrowColor::EDITOR_DIM))
                            .selectable(false)
                    }),
                ))
                .style(|s| s.flex_row().margin_top(2.0).width_full())
                .into_any()
            } else {
                empty().into_any()
            },
        ))
        .style(|s| s.flex_col().width_full().margin_bottom(8.0))
    };

    let store_as_project = create_rw_signal(true);
    let show_this_page = create_rw_signal(false);
    let activate_tool_window = create_rw_signal(true);
    let focus_tool_window = create_rw_signal(false);

    let custom_checkbox = move |sig: RwSignal<bool>| {
        container(
            label(move || if sig.get() { "✓" } else { "" }.to_string())
                .style(|s| s.font_size(10.0).color(floem::peniko::Color::WHITE)),
        )
        .on_click_stop(move |_| sig.update(|v| *v = !*v))
        .style(move |s| {
            s.size(14.0, 14.0)
                .items_center()
                .justify_center()
                .border(1.0)
                .border_radius(2.0)
                .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
                .apply_if(sig.get(), |s| {
                    s.background(
                        config
                            .get()
                            .color(SparrowColor::SPARROW_BUTTON_PRIMARY_BACKGROUND),
                    )
                })
                .hover(|s| s.cursor(CursorStyle::Pointer))
        })
    };

    let view_id = floem::ViewId::new();
    create_effect(move |_| {
        if visible.get() {
            view_id.request_focus();
        }
    });

    // 5. Main Layout
    container(
        container(
            stack((
                // Header
                container(
                    label(|| "Run/Debug Configurations".to_string())
                        .style(move |s| s.font_bold().font_size(14.0).color(config.get().color(SparrowColor::EDITOR_FOREGROUND)))
                ).style(move |s| s.width_full().padding(10.0).border_bottom(1.0).border_color(config.get().color(SparrowColor::SPARROW_BORDER))),

                // Body
                stack((
                    // Sidebar
                    container(
                        stack((
                            stack((
                                svg(move || config.get().ui_svg(SparrowIcons::ITEM_OPENED)).style(|s| s.size(12.0, 12.0).margin_right(4.0)),
                                label(|| "Flutter".to_string()).style(|s| s.font_bold())
                            )).style(|s| s.items_center().padding(10.0).padding_bottom(4.0)),

                            scroll(
                                floem::views::dyn_stack(
                                    move || configs.get().into_iter().enumerate().collect::<Vec<_>>(),
                                    |(i, _)| *i,
                                    move |(i, c)| {
                                        let is_active = active_idx.get() == i;
                                        stack((
                                            svg(move || config.get().ui_svg(SparrowIcons::FLUTTER)).style(|s| s.size(14.0, 14.0).margin_right(6.0)),
                                            label(move || c.name.clone())
                                        ))
                                            .on_click_stop(move |_| active_idx.set(i))
                                            .style(move |s| {
                                                s.width_full()
                                                    .padding_horiz(24.0).padding_vert(4.0)
                                                    .items_center()
                                                    .apply_if(is_active, |s| {
                                                        s.background(config.get().color(SparrowColor::SPARROW_TAB_ACTIVE_BACKGROUND))
                                                            .color(config.get().color(SparrowColor::EDITOR_FOREGROUND))
                                                    })
                                                    .apply_if(!is_active, |s| {
                                                        s.color(config.get().color(SparrowColor::EDITOR_DIM))
                                                    })
                                                    .hover(|s| s.cursor(CursorStyle::Pointer))
                                            })
                                    }
                                ).style(|s| s.flex_col().width_full())
                            ).style(|s| s.flex_grow(1.0_f32))
                        )).style(|s| s.flex_col().size_full())
                    ).style(move |s| s.width(240.0).height_full().border_right(1.0).border_color(config.get().color(SparrowColor::SPARROW_BORDER)).background(config.get().color(SparrowColor::PANEL_BACKGROUND))),

                    // Form
                    container(
                        scroll(
                            stack((
                                // Name + Checkbox
                                stack((
                                    form_row("Name:", name_input, None, false, false),
                                    stack((
                                        custom_checkbox(store_as_project).style(|s| s.margin_right(4.0)),
                                        label(|| "Store as project file".to_string())
                                    )).style(|s| s.items_center().margin_left(10.0))
                                )).style(|s| s.flex_row().width_full().items_center()),

                                form_row("Dart entrypoint:", program_input, Some("The entry-point for the application (e.g. lib/main.dart)."), true, false),
                                form_row("Additional run args:", args_input, Some("Additional arguments to 'flutter run'."), false, false),
                                form_row("Additional attach args:", attach_args_input, Some("Additional arguments to 'flutter attach'."), false, false),
                                form_row("Build flavor:", build_flavor_input, Some("An optional build flavor; either a Gradle product flavor or an Xcode custom scheme."), false, false),
                                form_row("Environment variables:", env_vars_input, Some("Additional environment variables."), false, true),

                                // Before launch
                                stack((
                                    stack((
                                        svg(move || config.get().ui_svg(SparrowIcons::DROPDOWN_ARROW)).style(|s| s.size(12.0, 12.0).margin_right(4.0).rotate(270.0)),
                                        label(|| "Before launch".to_string())
                                    )).style(|s| s.items_center().margin_top(10.0).margin_bottom(6.0).margin_left(150.0)),

                                    stack((
                                        empty().style(|s| s.width(150.0)),
                                        container(
                                            stack((
                                                stack((
                                                    clickable_icon(|| SparrowIcons::ADD, || {}, || false, || false, || "Add", config),
                                                    clickable_icon(|| SparrowIcons::REMOVE, || {}, || false, || false, || "Remove", config),
                                                    clickable_icon(|| SparrowIcons::SETTINGS, || {}, || false, || false, || "Edit", config),
                                                    clickable_icon(|| SparrowIcons::FOLD_UP, || {}, || false, || false, || "Move Up", config),
                                                    clickable_icon(|| SparrowIcons::FOLD_DOWN, || {}, || false, || false, || "Move Down", config),
                                                )).style(|s| s.items_center().padding(4.0)),
                                                empty().style(|s| s.flex_grow(1.0_f32)),
                                                label(|| "There are no tasks to run before launch".to_string()).style(move |s| s.color(config.get().color(SparrowColor::EDITOR_DIM))),
                                                empty().style(|s| s.flex_grow(1.0_f32)),
                                            )).style(|s| s.flex_col().size_full())
                                        ).style(move |s| s.flex_grow(1.0_f32).flex_basis(0.0).height(100.0).border(1.0).border_color(config.get().color(SparrowColor::SPARROW_BORDER)))
                                    )).style(|s| s.flex_row().width_full()),

                                    // Checkboxes
                                    stack((
                                        empty().style(|s| s.width(150.0)),
                                        stack((
                                            custom_checkbox(show_this_page).style(|s| s.margin_right(4.0)),
                                            label(|| "Show this page".to_string()).style(|s| s.margin_right(20.0)),
                                            custom_checkbox(activate_tool_window).style(|s| s.margin_right(4.0)),
                                            label(|| "Activate tool window".to_string()).style(|s| s.margin_right(20.0)),
                                            custom_checkbox(focus_tool_window).style(|s| s.margin_right(4.0)),
                                            label(|| "Focus tool window".to_string()),
                                        )).style(|s| s.items_center())
                                    )).style(|s| s.flex_row().margin_top(15.0).items_center().width_full())
                                )).style(|s| s.flex_col().width_full())
                            )).style(|s| s.flex_col().padding(20.0).width_full())
                        ).style(|s| s.absolute().size_full())
                    ).style(|s| s.flex_grow(1.0_f32).flex_basis(0.0).height_full()),
                )).style(|s| s.flex_row().flex_grow(1.0_f32).flex_basis(0.0).width_full()),

                // Footer
                stack((
                    label(|| "Edit configuration templates...".to_string()).style(move |s| s.color(config.get().color(SparrowColor::EDITOR_LINK)).hover(|s| s.cursor(CursorStyle::Pointer))),
                    empty().style(|s| s.flex_grow(1.0_f32)),
                    label(|| "?".to_string()).style(move |s| s.padding_horiz(10.0).color(config.get().color(SparrowColor::EDITOR_DIM))),

                    // Run button with dropdown
                    stack((
                        label(|| "Run".to_string()),
                        svg(move || config.get().ui_svg(SparrowIcons::DROPDOWN_ARROW)).style(|s| s.size(10.0, 10.0).margin_left(4.0))
                    )).on_click_stop(move |_| run_action()).style(move |s| {
                        s.padding_horiz(12.0).padding_vert(6.0).margin_right(10.0).border_radius(4.0)
                            .color(config.get().color(SparrowColor::EDITOR_FOREGROUND))
                            .border(1.0).border_color(config.get().color(SparrowColor::SPARROW_BORDER))
                            .items_center()
                            .hover(|s| s.cursor(CursorStyle::Pointer).background(config.get().color(SparrowColor::PANEL_HOVERED_BACKGROUND)))
                    }),

                    label(|| "OK".to_string()).on_click_stop(move |_| save_action()).style(move |s| {
                        s.padding_horiz(20.0).padding_vert(6.0).margin_right(6.0).border_radius(4.0)
                            .color(floem::peniko::Color::WHITE)
                            .background(config.get().color(SparrowColor::SPARROW_BUTTON_PRIMARY_BACKGROUND))
                            .hover(|s| s.cursor(CursorStyle::Pointer).background(config.get().color(SparrowColor::SPARROW_BUTTON_PRIMARY_BACKGROUND).multiply_alpha(0.8)))
                    }),
                    label(|| "Cancel".to_string()).on_click_stop(move |_| visible.set(false)).style(move |s| {
                        s.padding_horiz(20.0).padding_vert(6.0).margin_right(6.0).border_radius(4.0)
                            .border(1.0).border_color(config.get().color(SparrowColor::SPARROW_BORDER))
                            .hover(|s| s.cursor(CursorStyle::Pointer).background(config.get().color(SparrowColor::PANEL_HOVERED_BACKGROUND)))
                    }),
                    label(|| "Apply".to_string()).style(move |s| {
                        s.padding_horiz(20.0).padding_vert(6.0).border_radius(4.0)
                            .color(config.get().color(SparrowColor::EDITOR_DIM))
                            .border(1.0).border_color(config.get().color(SparrowColor::SPARROW_BORDER))
                    }),
                )).style(move |s| s.width_full().padding(10.0).items_center().border_top(1.0).border_color(config.get().color(SparrowColor::SPARROW_BORDER)))
            ))
                .style(move |s| {
                    s.flex_col().width(850.0).height(650.0)
                        .background(config.get().color(SparrowColor::PANEL_BACKGROUND))
                        .border(1.0).border_radius(8.0)
                        .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
                        .box_shadow_blur(15.0)
                        .box_shadow_color(config.get().color(SparrowColor::SPARROW_DROPDOWN_SHADOW))
                })
        )
            .on_click_stop(|_| {})
    )
        .style(move |s| {
            s.absolute()
                .size_full()
                .items_center()
                .justify_center()
                .background(floem::peniko::Color::from_rgba8(0, 0, 0, 180))
                .display(if visible.get() {
                    Display::Flex
                } else {
                    Display::None
                })
        })
        .on_click_stop(move |_| visible.set(false))
        .keyboard_navigable()
        .request_focus(move || {visible.get();})
}

pub fn debug_floating_toolbar(
    window_tab_data: Rc<crate::window_tab::WindowTabData>,
) -> impl floem::View {
    let config = window_tab_data.common.config;
    let terminal = window_tab_data.terminal.clone();

    let is_active = create_memo(move |_| {
        terminal
            .debug
            .daps
            .with(|daps| daps.values().any(|dap| dap.stopped.get()))
    });

    let icon_btn =
        |icon: &'static str, action: Box<dyn Fn()>, tooltip: &'static str| {
            clickable_icon(
                move || icon,
                move || action(),
                || false,
                || false,
                move || tooltip,
                config,
            )
        };

    container(
        stack((
            icon_btn(
                SparrowIcons::DEBUG_CONTINUE,
                Box::new({
                    let term = terminal.clone();
                    move || {
                        if let Some(id) = term.debug.active_term.get_untracked() {
                            term.dap_continue(id);
                        }
                    }
                }),
                "Continue",
            ),
            icon_btn(
                SparrowIcons::DEBUG_PAUSE,
                Box::new({
                    let term = terminal.clone();
                    move || {
                        if let Some(id) = term.debug.active_term.get_untracked() {
                            term.dap_pause(id);
                        }
                    }
                }),
                "Pause",
            ),
            icon_btn(
                SparrowIcons::DEBUG_STEP_OVER,
                Box::new({
                    let term = terminal.clone();
                    move || {
                        if let Some(id) = term.debug.active_term.get_untracked() {
                            term.dap_step_over(id);
                        }
                    }
                }),
                "Step Over",
            ),
            icon_btn(
                SparrowIcons::DEBUG_STEP_INTO,
                Box::new({
                    let term = terminal.clone();
                    move || {
                        if let Some(id) = term.debug.active_term.get_untracked() {
                            term.dap_step_into(id);
                        }
                    }
                }),
                "Step Into",
            ),
            icon_btn(
                SparrowIcons::DEBUG_STEP_OUT,
                Box::new({
                    let term = terminal.clone();
                    move || {
                        if let Some(id) = term.debug.active_term.get_untracked() {
                            term.dap_step_out(id);
                        }
                    }
                }),
                "Step Out",
            ),
            empty().style(move |s| {
                s.width(1.0)
                    .height(16.0)
                    .background(config.get().color(SparrowColor::SPARROW_BORDER))
            }),
            icon_btn(
                SparrowIcons::DEBUG_RESTART,
                Box::new({
                    let term = terminal.clone();
                    move || {
                        if let Some(id) = term.debug.active_term.get_untracked() {
                            term.restart_run_debug(id);
                        }
                    }
                }),
                "Restart",
            ),
            icon_btn(
                SparrowIcons::DEBUG_STOP,
                Box::new({
                    let term = terminal.clone();
                    move || {
                        if let Some(id) = term.debug.active_term.get_untracked() {
                            term.stop_run_debug(id);
                        }
                    }
                }),
                "Stop",
            ),
        ))
        .style(move |s| {
            let c = config.get();
            s.items_center()
                .padding_horiz(8.0)
                .padding_vert(4.0)
                .gap(4.0)
                .background(c.color(SparrowColor::PANEL_BACKGROUND))
                .border(1.0)
                .border_radius(6.0)
                .border_color(c.color(SparrowColor::SPARROW_BORDER))
                .box_shadow_blur(5.0)
                .box_shadow_color(c.color(SparrowColor::SPARROW_DROPDOWN_SHADOW))
                .pointer_events_auto() // <--- CRITICAL: Enables clicks on the pill
        }),
    )
    .style(move |s| {
        s.absolute()
            .inset_top(60.0)
            .justify_center()
            .width_full()
            .display(if is_active.get() {
                Display::Flex
            } else {
                Display::None
            })
            .pointer_events_none() // Let clicks pass through the empty space around the pill
    })
}
