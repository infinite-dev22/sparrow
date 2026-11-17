//! Manage template data; all mutations are previewed transactions.
use super::{catalog, engine};
use crate::{
    command::InternalCommand, config::color::SparrowColor, window_tab::WindowTabData,
};
use floem::{
    View,
    event::{Event, EventListener, EventPropagation},
    keyboard::{Key, NamedKey},
    reactive::{RwSignal, Scope, SignalGet, SignalUpdate},
    views::{Decorators, container, dyn_stack, label, scroll, stack},
};
use std::{
    fs,
    path::{Path, PathBuf},
    rc::Rc,
};

#[derive(Clone)]
pub(super) struct State {
    root: RwSignal<PathBuf>,
    entries: RwSignal<Vec<catalog::Entry>>,
    selected: RwSignal<Option<usize>>,
    query: RwSignal<String>,
    id: RwSignal<String>,
    global: RwSignal<bool>,
}
impl State {
    pub fn new(cx: Scope) -> Self {
        Self {
            root: cx.create_rw_signal(PathBuf::new()),
            entries: cx.create_rw_signal(vec![]),
            selected: cx.create_rw_signal(None),
            query: cx.create_rw_signal(String::new()),
            id: cx.create_rw_signal("my.custom.template".into()),
            global: cx.create_rw_signal(false),
        }
    }
    fn refresh(&self) {
        let previous = self.entry().map(|e| (e.id, e.provenance));
        let entries = catalog::manager_entries(&self.root.get_untracked());
        let index = previous
            .and_then(|key| {
                entries
                    .iter()
                    .position(|e| (e.id.clone(), e.provenance.clone()) == key)
            })
            .or_else(|| (!entries.is_empty()).then_some(0));
        let query = self.query.get_untracked().to_lowercase();
        let index = index
            .filter(|i| matches_query(&entries[*i], &query))
            .or_else(|| entries.iter().position(|e| matches_query(e, &query)));
        self.selected.set(index);
        self.entries.set(entries);
    }
    fn entry(&self) -> Option<catalog::Entry> {
        self.selected
            .get_untracked()
            .and_then(|i| self.entries.get_untracked().get(i).cloned())
            .filter(|e| matches_query(e, &self.query.get_untracked().to_lowercase()))
    }
    fn destination(&self) -> Result<PathBuf, String> {
        if self.global.get_untracked() {
            catalog::user_root().ok_or_else(|| {
                "Cannot locate the user configuration directory".into()
            })
        } else {
            Ok(self.root.get_untracked().join(".sparrow/scaffolds"))
        }
    }
}
impl super::State {
    pub fn open_manager(&self, window: &WindowTabData) {
        let result = (|| {
            let root = window
                .workspace
                .path
                .as_deref()
                .ok_or("Open a project first")?;
            let base = window
                .main_split
                .active_editor
                .get_untracked()
                .and_then(|e| {
                    use floem::reactive::SignalWith;
                    e.doc().content.with_untracked(|c| {
                        c.path().and_then(|p| p.parent()).map(Path::to_path_buf)
                    })
                })
                .unwrap_or_else(|| root.to_path_buf());
            engine::context(root, &base)
        })();
        self.generation.update(|g| *g += 1);
        self.busy.set(false);
        self.plan.set(None);
        self.error.set(String::new());
        self.undoing.set(false);
        self.manager_mode.set(true);
        match result {
            Ok(ctx) => {
                self.manager.root.set(ctx.root);
                self.manager.refresh();
            }
            Err(e) => {
                self.manager.entries.set(vec![]);
                self.manager.root.set(PathBuf::new());
                self.error.set(e);
            }
        }
        self.active.set(true);
    }
}

fn matches_query(entry: &catalog::Entry, query: &str) -> bool {
    format!("{} {} {}", entry.title, entry.id, entry.provenance)
        .to_lowercase()
        .contains(query)
}
/// Existing ancestor is the transaction boundary when the user root is new.
fn boundary(path: &Path) -> Result<PathBuf, String> {
    let mut ancestor = path;
    loop {
        if ancestor.exists() {
            return ancestor.canonicalize().map_err(|e| e.to_string());
        }
        ancestor = ancestor
            .parent()
            .ok_or("No existing template parent directory")?;
    }
}
fn valid_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > 120
        || id == "."
        || id == ".."
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return Err(
            "Use a pack ID of 1–120 letters, digits, dots, underscores or hyphens"
                .into(),
        );
    }
    Ok(())
}
fn editable(entry: &catalog::Entry, root: &Path) -> Result<PathBuf, String> {
    let path = entry
        .manifest()
        .ok_or("Export the bundled pack to an editable override first")?;
    let allowed = path.starts_with(root.join(".sparrow/scaffolds"))
        || path.starts_with(root.join(".sparrow/templates"))
        || catalog::user_root().is_some_and(|r| path.starts_with(r));
    if !allowed {
        return Err("Pack is outside the template discovery roots".into());
    }
    Ok(path.to_path_buf())
}
fn tree(dir: &Path) -> Result<Vec<(PathBuf, String)>, String> {
    fn visit(
        base: &Path,
        dir: &Path,
        out: &mut Vec<(PathBuf, String)>,
    ) -> Result<(), String> {
        if dir
            .strip_prefix(base)
            .map_err(|e| e.to_string())?
            .components()
            .count()
            > 16
        {
            return Err("Template folder nesting exceeds 16 levels".into());
        }
        if dir.is_symlink() {
            return Err("Template folders cannot be symlinks".into());
        }
        let mut paths = fs::read_dir(dir)
            .map_err(|e| e.to_string())?
            .map(|e| e.map(|e| e.path()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        paths.sort();
        for path in paths {
            if path.is_symlink() {
                return Err("Template files cannot be symlinks".into());
            }
            if path.is_dir() {
                visit(base, &path, out)?;
            } else {
                if out.len() >= 256 {
                    return Err(
                        "A managed template may contain at most 256 files".into()
                    );
                }
                out.push((
                    path.strip_prefix(base)
                        .map_err(|e| e.to_string())?
                        .to_path_buf(),
                    engine::read(&path)?.ok_or("Template file disappeared")?,
                ));
            }
        }
        Ok(())
    }
    let mut files = vec![];
    visit(dir, dir, &mut files)?;
    Ok(files)
}
fn copy_files(entry: &catalog::Entry) -> Result<Vec<(PathBuf, String)>, String> {
    if let Some(manifest) = entry.manifest() {
        if manifest.file_name().is_none_or(|n| n != "pack.yaml") {
            return Err("Legacy templates can be edited or removed; create a pack to export or disable them".into());
        }
        tree(manifest.parent().ok_or("Missing pack directory")?)
    } else {
        Ok(catalog::export_files(&entry.id)?
            .into_iter()
            .map(|(p, s)| (p.strip_prefix(&entry.id).unwrap().to_path_buf(), s))
            .collect())
    }
}
fn create(destination: &Path, id: &str) -> Result<engine::Plan, String> {
    valid_id(id)?;
    let manifest = format!(
        "schema: 2\nid: {id}\nversion: 1.0.0\ntitle: My Template\ncategory: other\ntarget: sibling\nfiles:\n  - path: '{{{{snake}}}}.dart'\n    source: templates/template.dart.tmpl\n"
    );
    engine::management_plan(
        &boundary(destination)?,
        "Create template pack",
        vec![
            (destination.join(id).join("pack.yaml"), Some(manifest)),
            (
                destination.join(id).join("templates/template.dart.tmpl"),
                Some("class {{name}} {\n  const {{name}}();\n}\n".into()),
            ),
        ],
        true,
    )
}
fn export(
    entry: &catalog::Entry,
    destination: &Path,
) -> Result<engine::Plan, String> {
    valid_id(&entry.id)?;
    let contents = copy_files(entry)?;
    let inputs = entry
        .manifest()
        .map(|manifest| {
            contents
                .iter()
                .map(|(p, s)| (manifest.parent().unwrap().join(p), Some(s.clone())))
                .collect()
        })
        .unwrap_or_default();
    let files = contents
        .into_iter()
        .map(|(p, s)| (destination.join(&entry.id).join(p), Some(s)))
        .collect();
    engine::management_plan(
        &boundary(destination)?,
        "Export template pack",
        files,
        true,
    )?
    .with_management_inputs(inputs)
}
fn toggle(
    entry: &catalog::Entry,
    root: &Path,
    destination: &Path,
) -> Result<engine::Plan, String> {
    if entry.manifest().is_none() {
        let mut plan = export(entry, destination)?;
        for change in &mut plan.changes {
            if change.path.file_name().is_some_and(|n| n == "pack.yaml") {
                change.after =
                    Some(disabled_manifest(change.after.as_deref().unwrap(), true)?);
            }
        }
        plan.title = "Disable bundled template with an override".into();
        return Ok(plan);
    }
    let path = editable(entry, root)?;
    if path.file_name().is_none_or(|n| n != "pack.yaml") {
        return Err("Legacy templates do not have a disabled flag; edit or remove the template instead".into());
    }
    let body = engine::read(&path)?.ok_or("Manifest disappeared")?;
    let after = disabled_manifest(&body, !entry.is_disabled())?;
    engine::management_plan(
        &boundary(path.parent().unwrap())?,
        "Change template enabled state",
        vec![(path.clone(), Some(after))],
        false,
    )?
    .with_management_inputs(vec![(path, Some(body))])
}
fn disabled_manifest(body: &str, disabled: bool) -> Result<String, String> {
    let mut value: serde_yaml::Value =
        serde_yaml::from_str(body).map_err(|e| e.to_string())?;
    let map = value
        .as_mapping_mut()
        .ok_or("Manifest must be a YAML mapping")?;
    map.insert(
        serde_yaml::Value::String("disabled".into()),
        serde_yaml::Value::Bool(disabled),
    );
    serde_yaml::to_string(&value).map_err(|e| e.to_string())
}
fn remove(entry: &catalog::Entry, root: &Path) -> Result<engine::Plan, String> {
    let manifest = editable(entry, root)?;
    let dir = manifest.parent().ok_or("Missing template directory")?;
    let snapshots = if manifest.file_name().is_some_and(|n| n == "pack.yaml") {
        tree(dir)?
            .into_iter()
            .map(|(p, s)| (dir.join(p), Some(s)))
            .collect::<Vec<_>>()
    } else {
        vec![(manifest.clone(), engine::read(&manifest)?)]
    };
    let files = snapshots.iter().map(|(p, _)| (p.clone(), None)).collect();
    let mut plan = engine::management_plan(
        &boundary(dir)?,
        "Remove template override",
        files,
        false,
    )?
    .with_management_inputs(snapshots)?;

    plan.notes.push("Removing an override reveals any lower-priority pack with the same ID. Bundled defaults remain installed.".into());
    Ok(plan)
}
fn stage(window: &WindowTabData, result: Result<engine::Plan, String>) {
    let state = &window.scaffold;
    state.plan.set(None);
    state.selected.set(0);
    match result {
        Ok(plan) => {
            state.planned_name.set(state.name.get_untracked());
            state.error.set(String::new());
            state.plan.set(Some(plan));
        }
        Err(e) => state.error.set(e),
    }
}

pub(super) fn view(window: Rc<WindowTabData>) -> impl View {
    let state = window.scaffold.clone();
    let manager = state.manager.clone();
    let config = window.common.config;
    let active = state.active;
    crate::modal_focus::restore_workbench_on_close(active, window.common.clone());
    let entries = manager.entries;
    let selected = manager.selected;
    let query = manager.query;
    let new_id = manager.id;
    let global = manager.global;
    let root = manager.root;
    let plan = state.plan;
    let error = state.error;
    let undoing = state.undoing;
    let focus = crate::modal_focus::ModalFocus::default();
    // Never apply actions to an entry hidden by the filter.
    floem::reactive::create_effect(move |_| {
        let q = query.get().to_lowercase();
        let all = entries.get();
        let matches = |e: &catalog::Entry| {
            format!("{} {} {}", e.title, e.id, e.provenance)
                .to_lowercase()
                .contains(&q)
        };
        let current = selected.get();
        if !current.and_then(|i| all.get(i)).is_some_and(matches) {
            let next = all.iter().position(matches);
            if next != current {
                selected.set(next);
                plan.set(None);
            }
        }
    });
    let list_focus = focus.clone();
    let list =
        scroll(
            dyn_stack(
                move || {
                    let q = query.get().to_lowercase();
                    entries
                        .get()
                        .into_iter()
                        .enumerate()
                        .filter(|(_, e)| {
                            format!("{} {} {}", e.title, e.id, e.provenance)
                                .to_lowercase()
                                .contains(&q)
                        })
                        .collect::<Vec<_>>()
                },
                |(i, e)| (*i, e.id.clone(), e.provenance.clone()),
                move |(i, e)| {
                    let status = if e.problem().is_some() {
                        "Invalid"
                    } else if e.is_disabled() {
                        "Disabled"
                    } else {
                        "Enabled"
                    };
                    list_focus.control(
                        label(move || {
                            format!(
                                "{} · {}\n{}\n{}",
                                e.title, status, e.id, e.provenance
                            )
                        })
                        .on_click_stop(move |_| {
                            selected.set(Some(i));
                            plan.set(None);
                            error.set(String::new());
                            undoing.set(false);
                        })
                        .style(move |s| {
                            s.width_full()
                                .padding(8.0)
                                .border_bottom(1.0)
                                .border_color(
                                    config.get().color(SparrowColor::SPARROW_BORDER),
                                )
                                .apply_if(selected.get() == Some(i), |s| {
                                    s.background(config.get().color(
                                        SparrowColor::PANEL_CURRENT_BACKGROUND,
                                    ))
                                })
                        }),
                    )
                },
            )
            .style(|s| s.flex_col()),
        )
        .style(|s| s.width(360.0).height_full());
    let detail = label(move || {
        let Some(e) = selected.get().and_then(|i| entries.get().get(i).cloned()) else { return "Select a template pack.".into(); };
        let mut text = format!("{}\n{}\n{}",e.title,e.id,e.provenance);
        if let Some(problem) = e.problem() { text.push_str(&format!("\nError: {problem}")); }
        else if let Ok(pack) = e.load() {
            text.push_str(&format!("\n{}\n{} output file(s) · {:?} · v{}",
                pack.description,pack.files.len(),pack.target,pack.version));
        }
        if e.is_disabled() { text.push_str("\nDisabled: hidden from New From Pack."); }
        text.push_str("\nPriority: Project > User > Bundled. Higher-priority packs with the same ID override this entry.");
        text
    }).style(|s|s.width_full().padding(8.0));
    let preview = scroll(dyn_stack(
        move || {
            let text = plan.get().map(|p| {
                let mut text = format!("{}\n{}\n",p.title,p.notes.join("\n"));
                for i in 0..p.changes.len() { text.push_str(&p.diff(i)); }
                if p.changes.is_empty() { text.push_str("No changes."); }
                text
            }).unwrap_or_else(|| "Choose an action to preview its changes. Apply writes the preview; Undo restores the last applied transaction.\nEdit opens pack files in the editor; save them, then Refresh to validate.".into());
            text.lines().enumerate().map(|(i,l)|(i,l.to_string())).collect::<Vec<_>>()
        }, |(i,l)|(*i,l.clone()),
        move |(_,line)| {
            let added = line.starts_with('+');
            let removed = line.starts_with('-');
            label(move ||line.clone()).style(move |s|s.height(20.0).padding_horiz(8.0)
                .font_family(config.get().editor.font_family.clone()).font_size(12.0)
                .apply_if(added,|s|s.background(floem::peniko::Color::from_rgba8(70,160,80,35)))
                .apply_if(removed,|s|s.background(floem::peniko::Color::from_rgba8(200,60,60,35))))
        }
    ).style(|s|s.flex_col())).style(|s|s.flex_grow(1.0_f32).min_width(0.0).width_full());
    let edit_window = window.clone();
    let export_window = window.clone();
    let toggle_window = window.clone();
    let remove_window = window.clone();
    let create_window = window.clone();
    let apply_window = window.clone();
    let undo_window = window.clone();
    let refresh_window = window.clone();
    let source_window = window.clone();
    let input = focus.input(query).style(|s| s.width_full().height(28.0));
    let input_id = input.id();
    floem::reactive::create_effect(move |_| {
        if active.get() {
            floem::action::exec_after(
                std::time::Duration::from_millis(30),
                move |_| input_id.request_focus(),
            );
        }
    });
    let body = stack((
        label(||"Scaffolding Template Manager").style(|s|s.font_bold()),
        label(move ||root.get().display().to_string()),
        label(||"Filter templates"),
        input,
        stack((
            focus.control(label(move || if global.get() {"Destination: User ▾"} else {"Destination: Project ▾"})
                .on_click_stop(move |_| { global.update(|g|*g=!*g); plan.set(None); })),
            label(||"New pack ID:"),
            focus.input(new_id).style(|s|s.width(260.0).height(28.0)),
            focus.control(label(||"Create…").on_click_stop(move |_| {
                let m = &create_window.scaffold.manager;
                undoing.set(false);
                stage(&create_window,m.destination().and_then(|d|create(&d,&new_id.get_untracked())));
            })),
            focus.control(label(||"Refresh").on_click_stop(move |_| {
                refresh_window.scaffold.manager.refresh(); plan.set(None);
            })),
        )).style(|s|s.items_center().gap(14.0)),
        stack((list,stack((detail,preview)).style(|s|s.flex_col().flex_grow(1.0_f32).min_width(0.0).height_full())))
            .style(|s|s.height(420.0).width_full().gap(12.0)),
        stack((
            focus.control(label(||"Edit Manifest").on_click_stop(move |_| {
                let m = &edit_window.scaffold.manager;
                let result = m.entry().ok_or("Select a template".to_string())
                    .and_then(|e|editable(&e,&root.get_untracked()));
                match result {
                    Ok(path) => { active.set(false); edit_window.common.internal_command.send(InternalCommand::OpenFile {path}); }
                    Err(e) => error.set(e),
                }
            })),
            focus.control(label(||"Edit Sources").on_click_stop(move |_| {
                let m = &source_window.scaffold.manager;
                let result = (|| {
                    let e = m.entry().ok_or("Select a template")?;
                    let manifest = editable(&e,&root.get_untracked())?;
                    let pack = e.load()?;
                    let sources = pack.files.iter().filter_map(|f|f.source.as_ref())
                        .map(|p| manifest.parent().unwrap().join(p)).collect::<Vec<_>>();
                    Ok::<_,String>((manifest,sources))
                })();
                match result {
                    Ok((manifest,sources)) => {
                        active.set(false);
                        for path in if sources.is_empty() {vec![manifest]} else {sources} {
                            source_window.common.internal_command.send(InternalCommand::OpenFile {path});
                        }
                    }
                    Err(e) => error.set(e),
                }
            })),
            focus.control(label(||"Enable / Disable…").on_click_stop(move |_| {
                let m = &toggle_window.scaffold.manager;
                undoing.set(false);
                stage(&toggle_window,(||toggle(&m.entry().ok_or("Select a template")?,&root.get_untracked(),&m.destination()?))());
            })),
            focus.control(label(||"Remove…").on_click_stop(move |_| {
                let m = &remove_window.scaffold.manager;
                undoing.set(false);
                stage(&remove_window,m.entry().ok_or("Select a template".to_string()).and_then(|e|remove(&e,&root.get_untracked())));
            })),
            focus.control(label(||"Export…").on_click_stop(move |_| {
                let m = &export_window.scaffold.manager;
                undoing.set(false);
                stage(&export_window,(||export(&m.entry().ok_or("Select a template")?,&m.destination()?))());
            })),
        )).style(|s|s.gap(18.0)),
        label(move ||error.get()).style(|s|s.width_full()),
        stack((
            focus.control(label(move ||if undoing.get() {"Apply Undo"} else {"Apply Preview"})
                .disabled(move ||plan.get().is_none())
                .on_click_stop(move |_| {
                    apply_window.scaffold.apply(&apply_window);
                    apply_window.scaffold.manager.refresh();
                    if plan.get_untracked().is_none() {
                        undoing.set(false);
                        error.set("Template changes applied. Undo is available in this manager and the command palette.".into());
                    }
                })),
            focus.control(label(||"Undo Last Change…").on_click_stop(move |_| {
                let s = &undo_window.scaffold;
                match s.history.get_untracked().last() {
                    Some(applied) => { undoing.set(true); plan.set(Some(applied.undo_plan())); error.set(String::new()); }
                    None => error.set("No scaffold or template change to undo in this window session.".into()),
                }
            })),
            focus.control(label(||"Close").on_click_stop(move |_|active.set(false))),
        )).style(|s|s.gap(18.0)),
    )).style(move |s|s.width(1120.0).max_width_pct(95.0).max_height_pct(95.0)
        .flex_col().gap(10.0).padding(18.0).border_radius(6.0).border(1.0)
        .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
        .background(config.get().color(SparrowColor::PANEL_BACKGROUND))
        .color(config.get().color(SparrowColor::EDITOR_FOREGROUND)));
    let dialog = container(body).style(move |s|s.absolute().size_full().items_center().justify_center().z_index(100)
        .apply_if(!active.get(),|s|s.hide()))
        .on_event_stop(EventListener::PointerDown, |_|{})
        .on_event(EventListener::KeyDown,move |event| {
            if matches!(event,Event::KeyDown(k) if k.key.logical_key==Key::Named(NamedKey::Escape)) {
                active.set(false); EventPropagation::Stop
            } else {EventPropagation::Continue}
        });
    focus.bind_root(dialog, active, config)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("pubspec.yaml"), "name: test_app\n").unwrap();
        dir
    }
    fn project_entry(root: &Path, id: &str) -> catalog::Entry {
        catalog::manager_entries(root)
            .into_iter()
            .find(|e| e.id == id && e.provenance.starts_with("Project:"))
            .unwrap()
    }
    #[test]
    fn refresh_keeps_selection_inside_the_active_filter() {
        let root = project();
        create(&root.path().join(".sparrow/scaffolds"), "my.template")
            .unwrap()
            .apply()
            .unwrap();
        let cx = Scope::new();
        let manager = State::new(cx);
        manager.root.set(root.path().to_path_buf());
        manager.refresh();
        assert!(manager.entry().is_some());
        manager.query.set("my.template".into());
        // Actions refuse a stale invisible selection even before refresh.
        assert!(manager.entry().is_none());
        manager.refresh();
        assert_eq!(manager.entry().unwrap().id, "my.template");
        manager.query.set("nothing matches this".into());
        manager.refresh();
        assert!(manager.selected.get_untracked().is_none());
        manager.query.set(String::new());
        manager.refresh();
        assert!(manager.entry().is_some());
        cx.dispose();
    }
    #[test]
    fn create_edit_disable_enable_remove_and_undo() {
        let root = project();
        let destination = root.path().join(".sparrow/scaffolds");
        let created = create(&destination, "my.template")
            .unwrap()
            .apply()
            .unwrap();
        let entry = project_entry(root.path(), "my.template");
        let manifest = editable(&entry, root.path()).unwrap();
        let source = manifest
            .parent()
            .unwrap()
            .join("templates/template.dart.tmpl");
        fs::write(&source, "// edited\nclass {{name}} {}\n").unwrap();
        assert!(
            entry.load().unwrap().files[0]
                .content
                .starts_with("// edited")
        );
        let disabled = toggle(&entry, root.path(), &destination)
            .unwrap()
            .apply()
            .unwrap();
        assert!(project_entry(root.path(), "my.template").is_disabled());
        assert!(
            !catalog::discover_at(None, root.path())
                .iter()
                .any(|e| e.id == "my.template")
        );
        let disabled_entry = project_entry(root.path(), "my.template");
        toggle(&disabled_entry, root.path(), &destination)
            .unwrap()
            .apply()
            .unwrap();
        assert!(!project_entry(root.path(), "my.template").is_disabled());
        // Undo guards refuse to overwrite edits made after the original create.
        assert!(created.undo().is_err());
        assert!(disabled.undo().is_err());
        let removed =
            remove(&project_entry(root.path(), "my.template"), root.path())
                .unwrap()
                .apply()
                .unwrap();
        assert!(!source.exists());
        assert!(!manifest.exists());
        removed.undo().unwrap();
        assert!(source.exists());
        assert!(
            engine::load_pack(&manifest).unwrap().files[0]
                .content
                .starts_with("// edited")
        );
    }
    #[test]
    fn bundled_disable_creates_override_and_undo_reveals_default() {
        let root = project();
        let entry = catalog::bundled_entries()
            .into_iter()
            .find(|e| e.id == "sparrow.state.cubit")
            .unwrap();
        let destination = root.path().join(".sparrow/scaffolds");
        let plan = toggle(&entry, root.path(), &destination).unwrap();
        assert!(plan.changes.len() > 1);
        assert!(!destination.exists());
        let applied = plan.apply().unwrap();
        assert!(project_entry(root.path(), &entry.id).is_disabled());
        assert!(
            !catalog::discover_at(None, root.path())
                .iter()
                .any(|e| e.id == entry.id)
        );
        applied.undo().unwrap();
        assert!(
            catalog::discover_at(None, root.path())
                .iter()
                .any(|e| e.id == entry.id)
        );
        assert!(remove(&entry, root.path()).is_err());
    }
    #[test]
    fn export_copies_sources_without_clobber_and_can_be_undone() {
        let root = project();
        let destination = root.path().join(".sparrow/scaffolds");
        let entry = catalog::bundled_entries().remove(0);
        let applied = export(&entry, &destination).unwrap().apply().unwrap();
        let copied = project_entry(root.path(), &entry.id);
        assert_eq!(
            copied.load().unwrap().files.len(),
            entry.load().unwrap().files.len()
        );
        assert!(export(&entry, &destination).is_err());
        applied.undo().unwrap();
        assert!(!destination.join(&entry.id).exists());
    }
    #[test]
    fn exports_are_invalidated_by_source_edits() {
        let root = project();
        let local = root.path().join(".sparrow/scaffolds");
        create(&local, "my.template").unwrap().apply().unwrap();
        let entry = project_entry(root.path(), "my.template");
        let elsewhere = root.path().join("personal/scaffolds");
        let plan = export(&entry, &elsewhere).unwrap();
        fs::write(
            local.join("my.template/templates/template.dart.tmpl"),
            "// changed",
        )
        .unwrap();
        assert!(plan.apply().is_err());
        assert!(!elsewhere.exists());
    }
    #[test]
    fn removal_preview_rejects_later_edits_and_can_remove_invalid_pack() {
        let root = project();
        let local = root.path().join(".sparrow/scaffolds");
        create(&local, "my.template").unwrap().apply().unwrap();
        let entry = project_entry(root.path(), "my.template");
        let plan = remove(&entry, root.path()).unwrap();
        let manifest = local.join("my.template/pack.yaml");
        fs::write(&manifest, "not valid yaml: [").unwrap();
        assert!(plan.apply().is_err());
        let invalid = project_entry(root.path(), "my.template");
        assert!(invalid.problem().is_some());
        let removed = remove(&invalid, root.path()).unwrap().apply().unwrap();
        assert!(!manifest.exists());
        removed.undo().unwrap();
        assert_eq!(fs::read_to_string(manifest).unwrap(), "not valid yaml: [");
    }
    #[test]
    fn create_rejects_invalid_ids_and_stale_previews() {
        let root = project();
        let local = root.path().join(".sparrow/scaffolds");
        for id in ["", "..", "../escape", "a/b", "a\nb", "x: yaml"] {
            assert!(create(&local, id).is_err());
        }
        let plan = create(&local, "safe").unwrap();
        fs::create_dir_all(local.join("safe")).unwrap();
        fs::write(local.join("safe/pack.yaml"), "external change").unwrap();
        assert!(plan.apply().is_err());
        assert!(!local.join("safe/templates/template.dart.tmpl").exists());
    }
    #[test]
    #[cfg(unix)]
    fn symlink_sources_cannot_be_removed_or_exported() {
        use std::os::unix::fs::symlink;
        let root = project();
        let local = root.path().join(".sparrow/scaffolds");
        create(&local, "safe").unwrap().apply().unwrap();
        let entry = project_entry(root.path(), "safe");
        let source = local.join("safe/templates/template.dart.tmpl");
        fs::remove_file(&source).unwrap();
        symlink(root.path().join("pubspec.yaml"), source).unwrap();
        assert!(remove(&entry, root.path()).is_err());
        assert!(export(&entry, &root.path().join("other")).is_err());
        assert!(root.path().join("pubspec.yaml").exists());
    }
    #[test]
    fn manager_includes_disabled_and_overridden_layers() {
        let root = project();
        let destination = root.path().join(".sparrow/scaffolds");
        let entry = catalog::bundled_entries().remove(0);
        toggle(&entry, root.path(), &destination)
            .unwrap()
            .apply()
            .unwrap();
        let all = catalog::manager_entries(root.path());
        assert!(
            all.iter()
                .any(|e| e.id == entry.id && e.manifest().is_none())
        );
        assert!(all.iter().any(|e| e.id == entry.id && e.is_disabled()));
    }
}
