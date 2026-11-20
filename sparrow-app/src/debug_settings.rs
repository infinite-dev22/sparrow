//! Project debugger preferences and the behavior controlled by their menu items.
use crate::{
    popup_menu::{Menu, MenuItem},
    terminal::panel::TerminalPanelData,
    window_tab::WindowTabData,
};
use floem::reactive::{SignalGet, SignalUpdate, SignalWith};
use sparrow_rpc::dap_types::{DapId, DebugAction, DebuggerSettings};
use std::{
    path::{Path, PathBuf},
    rc::Rc,
};

#[derive(Clone)]
pub struct InlineValues {
    pub path: PathBuf,
    pub line: usize,
    pub text: String,
}

pub fn load(root: Option<&Path>) -> DebuggerSettings {
    root.and_then(|root| {
        std::fs::read_to_string(root.join(".sparrow/debugger.toml")).ok()
    })
    .and_then(|text| toml::from_str(&text).ok())
    .unwrap_or_default()
}

pub fn refresh(terminal: &TerminalPanelData) {
    let settings = terminal.common.debugger_settings.get_untracked();
    let values = terminal
        .get_active_dap(false)
        .filter(|dap| {
            dap.stopped.get_untracked()
                && terminal
                    .run_debug_process(false)
                    .iter()
                    .any(|(_, p)| p.config.dap_id == dap.dap_id && !p.stopped)
        })
        .and_then(|dap| {
            let (path_line, path) = dap.breakline.get_untracked()?;
            let text = dap.variables.with_untracked(|root| {
                root.children
                    .iter()
                    .flat_map(|scope| &scope.children)
                    .filter_map(|v| {
                        v.item.value().map(|value| {
                            format!(
                                "{} = {}",
                                v.item.name(),
                                value.chars().take(100).collect::<String>()
                            )
                        })
                    })
                    .take(8)
                    .collect::<Vec<_>>()
                    .join(", ")
            });
            (!text.is_empty() && settings.inline_values).then_some(InlineValues {
                path,
                line: path_line,
                text,
            })
        });
    terminal.common.debug_inline_values.set(values);
    terminal.main_split.docs.with_untracked(|docs| {
        for doc in docs.values() {
            doc.clear_text_cache();
        }
    });
}

pub fn menu(window: Rc<WindowTabData>) -> Menu {
    let preferences = window.common.debugger_settings.get_untracked();
    let mut menu = Menu::new("Debugger Settings");
    for (index, title, checked) in [
        (0, "Open Files in Preview Tab", preferences.preview_files),
        (
            1,
            "Show Variable Values in Editor",
            preferences.inline_values,
        ),
        (
            2,
            "Sort Variables Alphabetically",
            preferences.sort_variables,
        ),
        (
            3,
            "Unmute Breakpoints on Session Finish",
            preferences.unmute_on_finish,
        ),
        (
            4,
            "Enable Value Renderers (next launch)",
            preferences.value_renderers,
        ),
        (5, "Step into SDK Libraries", preferences.debug_sdk),
        (6, "Step into External Packages", preferences.debug_external),
    ] {
        let window = window.clone();
        menu = menu.entry(
            MenuItem::new(format!("{}{}", if checked { "✓ " } else { "" }, title))
                .action(move || {
                    let mut settings =
                        window.common.debugger_settings.get_untracked();
                    let flag = match index {
                        0 => &mut settings.preview_files,
                        1 => &mut settings.inline_values,
                        2 => &mut settings.sort_variables,
                        3 => &mut settings.unmute_on_finish,
                        4 => &mut settings.value_renderers,
                        5 => &mut settings.debug_sdk,
                        _ => &mut settings.debug_external,
                    };
                    *flag = !*flag;
                    if let Err(error) =
                        save(window.workspace.path.as_deref(), &settings)
                    {
                        window.show_alert(
                            "Cannot save debugger settings".into(),
                            error,
                            Vec::new(),
                        );
                        return;
                    }
                    window.common.debugger_settings.set(settings.clone());
                    for (_, process) in window
                        .terminal
                        .run_debug_process(false)
                        .into_iter()
                        .filter(|(_, p)| !p.stopped)
                    {
                        window.common.proxy.dap_advanced(
                            process.config.dap_id,
                            DebugAction::UpdateOptions {
                                debug_sdk: settings.debug_sdk,
                                debug_external: settings.debug_external,
                            },
                        );
                    }
                    refresh(&window.terminal);
                }),
        );
    }
    menu
}

fn save(root: Option<&Path>, settings: &DebuggerSettings) -> Result<(), String> {
    let root = root
        .ok_or("Open a project to save debugger settings")?
        .join(".sparrow");
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let temporary = root.join(format!("debugger-{}.tmp", std::process::id()));
    std::fs::write(
        &temporary,
        toml::to_string_pretty(settings).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    std::fs::rename(temporary, root.join("debugger.toml")).map_err(|e| e.to_string())
}

pub fn sort_variables(root: &mut crate::debug::DapVariable) {
    root.children.sort_by(|a, b| {
        a.item
            .name()
            .to_lowercase()
            .cmp(&b.item.name().to_lowercase())
    });
    for child in &mut root.children {
        sort_variables(child);
    }
}

pub fn set_muted(window: &WindowTabData, muted: bool) {
    window.terminal.debug.muted.set(muted);
}

impl WindowTabData {
    pub(crate) fn handle_debug_action_event(
        &self,
        dap_id: DapId,
        event: &serde_json::Value,
    ) {
        if self.handle_debug_evaluation_event(dap_id, event) {
            return;
        }
        let Some(dap) = self
            .terminal
            .debug
            .daps
            .with_untracked(|daps| daps.get(&dap_id).cloned())
        else {
            return;
        };
        if let Some(error) = event.get("error").and_then(|e| e.as_str()) {
            self.show_alert(
                "Debugger action failed".into(),
                error.into(),
                Vec::new(),
            );
            return;
        }
        if event["type"] != "sparrow.stepTargets"
            || !dap.stopped.get_untracked()
            || self
                .terminal
                .get_active_dap(false)
                .is_none_or(|active| active.dap_id != dap_id)
        {
            return;
        }
        let generation = dap.pause_generation.get_untracked();
        if event["body"]["generation"].as_u64() != Some(generation) {
            return;
        }
        let daps = self.terminal.debug.daps;
        let still_paused = move || {
            daps.with_untracked(|daps| {
                daps.get(&dap_id).is_some_and(|dap| {
                    dap.stopped.get_untracked()
                        && dap.pause_generation.get_untracked() == generation
                })
            })
        };
        let current_frame = dap.thread_id.get_untracked().and_then(|thread| {
            dap.stack_traces.with_untracked(|traces| {
                traces.get(&thread).and_then(|trace| {
                    trace.frames.with_untracked(|f| f.front().map(|f| f.id))
                })
            })
        });
        if event["body"]["frameId"].as_u64() != current_frame.map(|id| id as u64) {
            return;
        }
        let thread_id = dap.thread_id.get_untracked().unwrap_or_default();
        let mut menu = Menu::new("Smart Step Into");
        let targets = event["body"]["targets"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        for target in targets.iter().take(100) {
            let (Some(id), Some(label)) =
                (target["id"].as_u64(), target["label"].as_str())
            else {
                continue;
            };
            let proxy = self.common.proxy.clone();
            menu = menu.entry(MenuItem::new(label).action(move || {
                if !still_paused() { return; }
                proxy.dap_advanced(
                    dap_id,
                    DebugAction::StepIntoTarget {
                        thread_id,
                        target_id: id as usize,
                    },
                )
            }));
        }
        if targets.is_empty() {
            let proxy = self.common.proxy.clone();
            menu = menu.entry(
                MenuItem::new(if event["body"]["supported"] == false {
                    "Adapter has no call targets — use Step Into"
                } else {
                    "No call targets — use Step Into"
                })
                .action(move || {
                    if still_paused() {
                        proxy.dap_step_into(dap_id, thread_id);
                    }
                }),
            );
        }
        crate::popup_menu::show_context_menu(menu, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_round_trip_and_partial_files_keep_defaults() {
        let root = std::env::temp_dir()
            .join(format!("sparrow-debug-settings-{}", std::process::id()));
        let mut preferences = DebuggerSettings::default();
        preferences.inline_values = false;
        preferences.sort_variables = true;
        save(Some(&root), &preferences).unwrap();
        assert_eq!(load(Some(&root)), preferences);
        std::fs::write(
            root.join(".sparrow/debugger.toml"),
            "sort-variables = true\n",
        )
        .unwrap();
        let partial = load(Some(&root));
        assert!(
            partial.sort_variables
                && partial.inline_values
                && partial.value_renderers
        );
        std::fs::remove_dir_all(root).unwrap();
        assert!(save(None, &preferences).is_err());
    }
    #[test]
    fn alphabetical_sort_preserves_values_and_expansion_state() {
        use crate::debug::{DapVariable, ScopeOrVar};
        let variable = |name: &str| {
            DapVariable { item: ScopeOrVar::Var(serde_json::from_value(
            serde_json::json!({"name":name,"value":"7","variablesReference":0})).unwrap()), expanded:true, ..Default::default() }
        };
        let mut root = DapVariable {
            children: vec![variable("zebra"), variable("Alpha")],
            ..Default::default()
        };
        sort_variables(&mut root);
        assert_eq!(root.children[0].item.name(), "Alpha");
        assert_eq!(root.children[1].item.value(), Some("7"));
        assert!(root.children[0].expanded);
    }
}
