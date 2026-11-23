use std::{
    cell::Cell,
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    rc::Rc,
    time::Instant,
};

use floem::{
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

use crate::{
    command::InternalCommand,
    editor::location::{EditorLocation, EditorPosition},
    window_tab::CommonData,
};

mod floating_toolbar;

pub use floating_toolbar::debug_floating_toolbar;

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

/// Bounded output buffer. Adapter events may contain partial lines.
#[derive(Clone, Default)]
pub struct DebugConsole {
    pub lines: im::Vector<String>,
    partial: String,
}

impl DebugConsole {
    pub fn append(&mut self, output: &str) {
        for segment in output.split_inclusive('\n') {
            self.partial.push_str(segment);
            if self.partial.ends_with('\n') {
                self.lines.push_back(std::mem::take(&mut self.partial)
                    .trim_end_matches(['\r', '\n']).to_owned());
            }
        }
        // Bound both completed lines and output without line breaks.
        if self.partial.len() > 64 * 1024 {
            self.lines.push_back(std::mem::take(&mut self.partial));
        }
        while self.lines.len() > 5000 { self.lines.pop_front(); }
    }

    pub fn rows(&self) -> im::Vector<String> {
        let mut rows = self.lines.clone();
        if !self.partial.is_empty() { rows.push_back(self.partial.clone()); }
        rows
    }

    pub fn text(&self) -> String {
        let mut lines = self.lines.iter().cloned().collect::<Vec<_>>();
        if !self.partial.is_empty() { lines.push(self.partial.clone()); }
        lines.join("\n")
    }
}

#[derive(Clone)]
pub struct RunDebugData {
    /// Console output is isolated by adapter session, including completed sessions.
    pub console_output: RwSignal<im::HashMap<DapId, DebugConsole>>,
    pub muted: RwSignal<bool>,
    pub active_term: RwSignal<Option<TermId>>,
    /// Stable UI session IDs for adapters that launch without a PTY.
    pub adapter_sessions: RwSignal<HashMap<DapId, (TermId, RunDebugProcess)>>,
    /// Adapter-owned sessions (Flutter tests) do not allocate a terminal.
    pub active_dap: RwSignal<Option<DapId>>,
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
            muted: cx.create_rw_signal(false),
            console_output: cx.create_rw_signal(im::HashMap::new()),
            adapter_sessions: cx.create_rw_signal(HashMap::new()),
            active_dap: cx.create_rw_signal(None),
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
                            if b.active && !self.muted.get_untracked() {
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

/// Invalidates outstanding variable reads when the selected frame or session changes.
#[derive(Clone, Default)]
pub(crate) struct VariableRequests(Rc<Cell<u64>>);

impl VariableRequests {
    pub(crate) fn invalidate(&self) {
        self.0.set(self.0.get().wrapping_add(1));
    }

    pub(crate) fn capture(&self) -> impl Fn() -> bool + use<> {
        let epoch = self.0.clone();
        let expected = epoch.get();
        move || epoch.get() == expected
    }
}

#[derive(Clone)]
pub struct DapData {
    pub term_id: TermId,
    pub dap_id: DapId,
    pub stopped: RwSignal<bool>,
    pub thread_id: RwSignal<Option<ThreadId>>,
    pub selected_frame: RwSignal<Option<StackFrame>>,
    pub pause_generation: RwSignal<u64>,
    pub stack_traces: RwSignal<BTreeMap<ThreadId, StackTraceData>>,
    pub evaluation: crate::debug_evaluate::Evaluations,
    pub variables_id: RwSignal<usize>,
    pub(crate) variable_requests: VariableRequests,
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
        let selected_frame: RwSignal<Option<StackFrame>> = cx.create_rw_signal(None);
        let stack_traces: RwSignal<BTreeMap<ThreadId, StackTraceData>> =
            cx.create_rw_signal(BTreeMap::new());
        let breakline = cx.create_memo(move |_| {
            if let Some(frame) = selected_frame.get() {
                return frame.source.and_then(|source| source.path)
                    .map(|path| (frame.line.saturating_sub(1), path));
            }
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
            selected_frame,
            pause_generation: cx.create_rw_signal(0),
            stack_traces,
            evaluation: crate::debug_evaluate::Evaluations::new(cx),
            variables_id: cx.create_rw_signal(0),
            variable_requests: VariableRequests::default(),
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
        self.variable_requests.invalidate();
        self.selected_frame.set(None);
        self.pause_generation.update(|value| *value = value.wrapping_add(1));
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
                            InternalCommand::DebugJumpToLocation {
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
        self.refresh_watches();
    }

    /// Retire suspended state and all pending reads without changing other sessions.
    pub(crate) fn clear_pause(&self) {
        self.variable_requests.invalidate();
        self.evaluation.clear_results();
        self.stopped.set(false);
        self.thread_id.set(None);
        self.selected_frame.set(None);
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
        let is_current = self.variable_requests.capture();

        let send = create_ext_action(self.common.scope, move |result| {
            if !is_current() {
                return;
            }
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

    use super::{DapVariable, ScopeOrVar, VariableRequests};

    #[test]
    fn variable_reads_reject_out_of_order_frames_and_retired_pauses() {
        let requests = VariableRequests::default();
        let first_frame = requests.capture();
        let first_child = requests.capture();
        requests.invalidate(); // Selecting a second stack frame.
        let second_frame = requests.capture();
        assert!(!first_frame());
        assert!(!first_child());
        assert!(second_frame());
        requests.invalidate(); // Resume, stop, close, or rerun.
        assert!(!second_frame());
        let next_pause = requests.capture();
        assert!(next_pause());
        assert!(!first_frame());
    }

    #[test]
    fn variable_reads_are_independent_between_sessions() {
        let first = VariableRequests::default();
        let second = VariableRequests::default();
        let first_reply = first.capture();
        let second_reply = second.capture();
        first.invalidate();
        assert!(!first_reply());
        assert!(second_reply());
    }


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

pub fn run_config_modal(window: Rc<crate::window_tab::WindowTabData>) -> impl floem::View {
    crate::dart::run_config_view::view(window)
}

#[cfg(test)]
mod console_tests {
    use super::DebugConsole;

    #[test]
    fn joins_partial_output_and_preserves_blank_lines() {
        let mut console = DebugConsole::default();
        console.append("Build");
        console.append("ing...\r\n\nLaun");
        console.append("ched\n");
        assert_eq!(console.text(), "Building...\n\nLaunched");
    }

    #[test]
    fn output_is_bounded_and_sessions_are_independent() {
        let mut first = DebugConsole::default();
        let mut second = DebugConsole::default();
        for i in 0..5010 { first.append(&format!("{i}\n")); }
        second.append("other session\n");
        assert_eq!(first.lines.len(), 5000);
        assert_eq!(first.lines.front().map(String::as_str), Some("10"));
        assert_eq!(second.text(), "other session");
        first = DebugConsole::default();
        assert!(first.text().is_empty());
        assert_eq!(second.text(), "other session");
    }
}
