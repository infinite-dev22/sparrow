//! Per-session expression evaluation. Responses are bound to the current frame epoch.
use crate::{debug::DapData, window_tab::WindowTabData};
use floem::reactive::{RwSignal, Scope, SignalGet, SignalUpdate};
use sparrow_rpc::dap_types::{DapId, DebugAction};
use std::{cell::Cell, rc::Rc};

#[derive(Clone)]
pub struct Evaluation {
    pub expression: String,
    pub watch: bool,
    pub request_id: u64,
    pub result: Option<Result<String, String>>,
    current: Rc<dyn Fn() -> bool>,
}

#[derive(Clone)]
pub struct Evaluations {
    pub entries: RwSignal<Vec<Evaluation>>,
    sequence: Rc<Cell<u64>>,
}
impl Evaluations {
    pub fn new(scope: Scope) -> Self {
        Self {
            entries: scope.create_rw_signal(Vec::new()),
            sequence: Rc::new(Cell::new(0)),
        }
    }
    fn register(
        &self,
        expression: &str,
        watch: bool,
        current: Rc<dyn Fn() -> bool>,
    ) -> u64 {
        let request_id = self.sequence.get().wrapping_add(1);
        self.sequence.set(request_id);
        let entry = Evaluation {
            expression: expression.into(),
            watch,
            request_id,
            result: None,
            current,
        };
        self.entries.update(|entries| {
            entries.retain(|entry| {
                if watch {
                    !entry.watch || entry.expression != expression
                } else {
                    entry.watch
                }
            });
            entries.push(entry);
        });
        request_id
    }
    fn accept_response(&self, event: &serde_json::Value) {
        let request_id = event["requestId"].as_u64();
        self.entries.update(|entries| {
            if let Some(entry) = entries.iter_mut().find(|entry| {
                Some(entry.request_id) == request_id && (entry.current)()
            }) {
                entry.result = Some(match event["error"].as_str() {
                    Some(error) => Err(error.into()),
                    None => event["body"]["result"]
                        .as_str()
                        .map(|v| Ok(v.into()))
                        .unwrap_or_else(|| {
                            Err("The adapter returned no evaluation result".into())
                        }),
                });
            }
        });
    }
    pub fn clear_results(&self) {
        self.entries.update(|entries| {
            entries.retain(|entry| entry.watch);
            for entry in entries {
                entry.result = None;
            }
        });
    }
}
impl DapData {
    pub fn evaluation_frame(&self) -> Option<usize> {
        if !self.stopped.get_untracked() {
            return None;
        }
        self.selected_frame
            .get_untracked()
            .map(|frame| frame.id)
            .or_else(|| {
                let thread = self.thread_id.get_untracked()?;
                self.stack_traces
                    .get_untracked()
                    .get(&thread)?
                    .frames
                    .get_untracked()
                    .front()
                    .map(|frame| frame.id)
            })
    }
    pub fn evaluate(&self, expression: &str, watch: bool) {
        let expression = expression.trim();
        if expression.is_empty() {
            return;
        }
        let frame = self.evaluation_frame();
        let request_id = self.evaluation.register(
            expression,
            watch,
            Rc::new(self.variable_requests.capture()),
        );
        let Some(frame_id) = frame else {
            if !watch {
                self.evaluation.accept_response(&serde_json::json!({"requestId":request_id,"error":"Pause execution to evaluate expressions"}));
            }
            return;
        };
        self.common.proxy.dap_advanced(
            self.dap_id,
            DebugAction::Evaluate {
                expression: expression.into(),
                frame_id,
                request_id,
                watch,
            },
        );
    }
    pub fn refresh_watches(&self) {
        let expressions = self
            .evaluation
            .entries
            .get_untracked()
            .into_iter()
            .filter(|entry| entry.watch)
            .map(|entry| entry.expression)
            .collect::<Vec<_>>();
        self.evaluation.clear_results();
        for expression in expressions {
            self.evaluate(&expression, true);
        }
    }
}
impl WindowTabData {
    pub(crate) fn handle_debug_evaluation_event(
        &self,
        id: DapId,
        event: &serde_json::Value,
    ) -> bool {
        if event["type"] != "sparrow.evaluation" {
            return false;
        }
        let Some(dap) = self.terminal.debug.daps.get_untracked().get(&id).cloned()
        else {
            return true;
        };
        if dap.stopped.get_untracked() {
            dap.evaluation.accept_response(event);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debug::VariableRequests;

    #[test]
    fn watch_results_reject_old_frames_removed_entries_and_replaced_requests() {
        let scope = Scope::new();
        let model = Evaluations::new(scope);
        let epoch = VariableRequests::default();
        let first = model.register("counter", true, Rc::new(epoch.capture()));
        epoch.invalidate();
        model.accept_response(
            &serde_json::json!({"requestId":first,"body":{"result":"old"}}),
        );
        assert!(model.entries.get_untracked()[0].result.is_none());
        let second = model.register("counter", true, Rc::new(epoch.capture()));
        model.accept_response(
            &serde_json::json!({"requestId":first,"body":{"result":"old"}}),
        );
        model.accept_response(
            &serde_json::json!({"requestId":second,"body":{"result":"42"}}),
        );
        let entries = model.entries.get_untracked();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].result, Some(Ok("42".into())));
        model.entries.set(Vec::new());
        model.accept_response(
            &serde_json::json!({"requestId":second,"body":{"result":"late"}}),
        );
        assert!(model.entries.get_untracked().is_empty());
        scope.dispose();
    }

    #[test]
    fn transient_results_errors_and_watch_cleanup_are_session_local() {
        let scope = Scope::new();
        let first = Evaluations::new(scope);
        let second = Evaluations::new(scope);
        first.register("saved", true, Rc::new(|| true));
        first.register("old input", false, Rc::new(|| true));
        let id = first.register("bad input", false, Rc::new(|| true));
        second.register("other session", true, Rc::new(|| true));
        first.accept_response(
            &serde_json::json!({"requestId":id,"error":"Undefined name"}),
        );
        assert_eq!(first.entries.get_untracked().len(), 2);
        assert_eq!(
            first.entries.get_untracked()[1].result,
            Some(Err("Undefined name".into()))
        );
        first.clear_results();
        assert_eq!(first.entries.get_untracked().len(), 1);
        assert_eq!(
            second.entries.get_untracked()[0].expression,
            "other session"
        );
        scope.dispose();
    }
}
