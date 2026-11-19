//! In-memory pack editing with the generator's strict renderer and transactional saves.
mod analyzer;
mod language;
use super::{
    boundary, catalog, copy_files, editable, engine, pack_from_files, stage,
    valid_id,
};
use crate::{config::color::SparrowColor, window_tab::WindowTabData};
use floem::{
    View,
    event::{Event, EventListener, EventPropagation},
    keyboard::{Key, Modifiers, NamedKey},
    reactive::{RwSignal, Scope, SignalGet, SignalUpdate},
    views::{
        Decorators, container, dyn_container, dyn_stack,
        editor::{
            Editor,
            command::CommandExecuted,
            core::{cursor::CursorAffinity, editor::EditType, selection::Selection},
            keypress::{default_key_handler, key::KeyInput},
            text::Document,
            text_document::TextDocument,
        },
        label, scroll, stack, text_editor_keys,
    },
};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
    rc::Rc,
};

type Rendered = Result<Vec<(PathBuf, String)>, String>;

#[derive(Clone, Debug)]
pub(super) struct Draft {
    files: BTreeMap<PathBuf, String>,
    original: BTreeMap<PathBuf, String>,
    directory: PathBuf,
    create_only: bool,
    ctx: engine::Context,
    source: String,
}
impl Draft {
    fn load(
        entry: &catalog::Entry,
        root: &Path,
        destination: &Path,
    ) -> Result<Self, String> {
        let ctx = engine::context(root, root)?;
        let (files, directory, create_only) = match entry.manifest() {
            Some(manifest) if manifest.file_name().is_some_and(|n|n=="pack.yaml") => {
                let manifest = editable(entry,root)?;
                (copy_files(entry)?,manifest.parent().unwrap().to_path_buf(),false)
            }
            Some(_) => return Err("Duplicate this legacy template into a pack first, then open Template Editor.".into()),
            None => {
                valid_id(&entry.id)?;
                (copy_files(entry)?,destination.join(&entry.id),true)
            }
        };
        let files = files.into_iter().collect::<BTreeMap<_, _>>();
        Ok(Self {
            original: files.clone(),
            files,
            directory,
            create_only,
            ctx,
            source: entry.provenance.clone(),
        })
    }
    pub(super) fn dirty(&self) -> bool {
        self.files != self.original
    }
    fn validate(&self, sample: &str) -> Rendered {
        let files = self
            .files
            .iter()
            .map(|(p, s)| (p.clone(), s.clone()))
            .collect::<Vec<_>>();
        let pack = pack_from_files(&files)?;
        valid_id(&pack.id)?;
        // Disabled packs can still be inspected and edited without enabling them.
        engine::render_pack(&self.ctx, &pack, sample)
    }
    fn save_plan(&self, sample: &str) -> Result<engine::Plan, String> {
        self.validate(sample)?;
        if !self.dirty() && !self.create_only {
            return Err("No draft changes to save".into());
        }
        let paths = self
            .original
            .keys()
            .chain(self.files.keys())
            .collect::<std::collections::BTreeSet<_>>();
        let changes = paths
            .into_iter()
            .map(|p| (self.directory.join(p), self.files.get(p).cloned()))
            .collect();
        let inputs = if self.create_only {
            vec![]
        } else {
            self.original
                .keys()
                .chain(self.files.keys())
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .map(|p| (self.directory.join(p), self.original.get(p).cloned()))
                .collect()
        };
        engine::management_plan(
            &boundary(&self.directory)?,
            "Save template edits",
            changes,
            self.create_only,
        )?
        .with_management_inputs(inputs)
    }
    fn add_file(&mut self, path: &str) -> Result<PathBuf, String> {
        let path = PathBuf::from(path);
        if path.as_os_str().is_empty()
            || path
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err("Use a relative file path without traversal".into());
        }
        if self.files.contains_key(&path) {
            return Err("This draft file already exists".into());
        }
        if self.files.len() >= 256 {
            return Err("A pack may contain at most 256 files".into());
        }
        self.files.insert(path.clone(), String::new());
        Ok(path)
    }
    fn remove_file(&mut self, path: &Path) -> Result<(), String> {
        if path == Path::new("pack.yaml") {
            return Err("The pack manifest cannot be removed".into());
        }
        self.files.remove(path).ok_or("Select a draft file")?;
        Ok(())
    }
    fn mark_saved(&mut self) {
        self.original = self.files.clone();
        self.create_only = false;
        self.source = format!("Editable: {}", self.directory.display());
    }
}

#[derive(Clone)]
pub(super) struct State {
    pub open: RwSignal<bool>,
    pub draft: RwSignal<Option<Draft>>,
    file: RwSignal<PathBuf>,
    sample: RwSignal<String>,
    new_file: RwSignal<String>,
    rendered: RwSignal<Rendered>,
    epoch: RwSignal<u64>,
    revision: RwSignal<u64>,
    diagnostics: RwSignal<Vec<language::Diagnostic>>,
}
impl State {
    pub fn new(cx: Scope) -> Self {
        Self {
            open: cx.create_rw_signal(false),
            draft: cx.create_rw_signal(None),
            file: cx.create_rw_signal("pack.yaml".into()),
            sample: cx.create_rw_signal("Example".into()),
            new_file: cx.create_rw_signal("templates/new.dart.tmpl".into()),
            rendered: cx.create_rw_signal(Err("Validating…".into())),
            epoch: cx.create_rw_signal(0),
            revision: cx.create_rw_signal(0),
            diagnostics: cx.create_rw_signal(vec![]),
        }
    }
}
pub(super) fn open(window: &WindowTabData) -> Result<(), String> {
    let manager = &window.scaffold.manager;
    let editor = &manager.editor;
    if !editor.draft.get_untracked().is_some_and(|d| d.dirty()) {
        let entry = manager.entry().ok_or("Select a template pack")?;
        let draft = Draft::load(
            &entry,
            &manager.root.get_untracked(),
            &manager.destination()?,
        )?;
        editor.draft.set(Some(draft));
        editor.file.set("pack.yaml".into());
        editor.epoch.update(|v| *v += 1);
    }
    window.scaffold.plan.set(None);
    window.scaffold.error.set(String::new());
    editor.open.set(true);
    Ok(())
}

fn accept_completion(
    ed: &Editor,
    choice: &language::Completion,
    draft: RwSignal<Option<Draft>>,
    plan: RwSignal<Option<engine::Plan>>,
    error: RwSignal<String>,
    selected: RwSignal<PathBuf>,
) {
    let body = ed.doc().text().to_string();
    if choice.range.end > body.len()
        || !body.is_char_boundary(choice.range.start)
        || !body.is_char_boundary(choice.range.end)
    {
        return;
    }
    ed.doc().edit_single(
        Selection::region(choice.range.start, choice.range.end),
        &choice.insert,
        EditType::InsertChars,
    );
    ed.cursor.update(|c| {
        c.set_offset(choice.range.start + choice.insert.len(), false, false)
    });
    let text = ed.doc().text().to_string();
    // Programmatic edits have no OnUpdate.editor; explicitly sync the draft.
    // The selected path is supplied by the live editor's completion caller.
    plan.set(None);
    draft.update(|d| {
        if let Some(d) = d {
            d.files.insert(selected.get_untracked(), text);
        }
    });
    error.set(String::new());
    if let Some(id) = ed.editor_view_id.get_untracked() {
        id.request_focus();
    }
}
fn action_controls(
    session: Rc<analyzer::Session>, draft: RwSignal<Option<Draft>>, selected: RwSignal<PathBuf>,
    documents: Rc<RefCell<BTreeMap<PathBuf, Rc<TextDocument>>>>, handle: Rc<RefCell<Option<Editor>>>,
    plan: RwSignal<Option<engine::Plan>>, error: RwSignal<String>, focus: &crate::modal_focus::ModalFocus,
) -> impl View + use<> {
    let actions=session.actions.clone();
    let list=actions.list;
    let preview=actions.preview;
    let undo=actions.undo;
    let message=actions.message;
    let request_session=session.clone();
    let request_handle=handle.clone();
    let apply_session=session.clone();
    let apply_documents=documents.clone();
    let apply_handle=handle.clone();
    let undo_session=session.clone();
    let cancel=actions.clone();
    let row_focus=focus.clone();
    stack((
        stack((
            focus.control(label(||"Dart Actions…  Alt+Enter").on_click_stop(move |_| {
                plan.set(None);
                if let Some(ed)=request_handle.borrow().as_ref() { request_session.request_actions(ed,selected); }
                else {message.set("Open a Dart template first.".into());}
            })),
            focus.control_if(label(||"Apply Action").disabled(move ||preview.get().is_none()).on_click_stop(move |_| {
                match apply_session.apply_action(draft,&apply_documents.borrow(),plan,false) {
                    Ok(())=>{
                        error.set(String::new());
                        if let Some(ed)=apply_handle.borrow().as_ref() {
                            let text=ed.doc().text().to_string();
                            ed.cursor.update(|c| {
                                let mut offset=c.offset().min(text.len());
                                while !text.is_char_boundary(offset) {offset-=1;}
                                c.set_offset(offset,false,false);
                            });
                            if let Some(id)=ed.editor_view_id.get_untracked() {id.request_focus();}
                            ed.editor_view_focused.notify();
                        }
                    }
                    Err(e)=>error.set(e),
                }
            }),move ||preview.get_untracked().is_some()),
            focus.control_if(label(||"Undo Action").disabled(move ||undo.get().is_none()).on_click_stop(move |_| {
                match undo_session.apply_action(draft,&documents.borrow(),plan,true) {
                    Ok(())=>{
                        error.set(String::new());
                        if let Some(ed)=handle.borrow().as_ref() {
                            let text=ed.doc().text().to_string();
                            ed.cursor.update(|c| {
                                let mut offset=c.offset().min(text.len());
                                while !text.is_char_boundary(offset) {offset-=1;}
                                c.set_offset(offset,false,false);
                            });
                            if let Some(id)=ed.editor_view_id.get_untracked() {id.request_focus();}
                            ed.editor_view_focused.notify();
                        }
                    }
                    Err(e)=>error.set(e),
                }
            }),move ||undo.get_untracked().is_some()),
            focus.control(label(||"Cancel Action").on_click_stop(move |_|cancel.clear())),
        )).style(|s|s.gap(18.0).height(24.0).items_center()),
        label(move ||message.get()).style(|s|s.font_size(12.0)),
        scroll(dyn_stack(move ||list.get().into_iter().enumerate().collect::<Vec<_>>(),
            |(index,action)|(*index,action.title.clone()),move |(_,choice)| {
                let session=session.clone();
                let enabled=choice.disabled.is_none();
                let title=if let Some(reason)=&choice.disabled{format!("{} · {reason}",choice.title)}else{format!("{} · Preview…",choice.title)};
                row_focus.control_if(label(move ||title.clone()).disabled(move ||!enabled)
                    .on_click_stop(move |_|session.preview_action(&choice,draft))
                    .style(|s|s.padding(3.0).width_full()),move ||enabled)
            }).style(|s|s.flex_col().width_full()))
            .style(move |s|s.width_full().max_height(86.0).apply_if(list.get().is_empty(),|s|s.hide())),
    )).style(|s|s.flex_col().width_full().gap(3.0))
}

pub(super) fn view(window: Rc<WindowTabData>) -> impl View {
    let parent = window.scaffold.clone();
    let state = parent.manager.editor.clone();
    let open = state.open;
    let active = parent.active;
    let draft = state.draft;
    let selected = state.file;
    let sample = state.sample;
    let new_file = state.new_file;
    let rendered = state.rendered;
    let epoch = state.epoch;
    let revision = state.revision;
    let plan = parent.plan;
    let error = parent.error;
    let config = window.common.config;
    let diagnostics = state.diagnostics;
    let local_diagnostics =
        Scope::current().create_rw_signal(Vec::<language::Diagnostic>::new());
    let analyzer = analyzer::Session::new(
        draft
            .get_untracked()
            .map(|d| d.ctx.root)
            .unwrap_or_default(),
    );
    let dart_status = analyzer.status;
    let merged_analyzer = analyzer.clone();
    floem::reactive::create_effect(move |_| {
        let mut all = local_diagnostics.get();
        all.extend(merged_analyzer.diagnostics.get());
        diagnostics.set(all);
    });
    let validation_analyzer = analyzer.clone();
    let suggestions =
        Scope::current().create_rw_signal(Vec::<language::Completion>::new());
    let suggestion_index = Scope::current().create_rw_signal(0usize);
    let popup_point = Scope::current().create_rw_signal((0.0f64, 0.0f64));
    let active_editor = Rc::new(RefCell::new(None::<Editor>));
    let focus = crate::modal_focus::ModalFocus::default();
    crate::modal_focus::restore_workbench_on_close(active, window.common.clone());
    floem::reactive::create_effect(move |_| {
        let current = draft.get();
        let sample = sample.get();
        let _ = validation_analyzer.ready.get();
        validation_analyzer.invalidate();
        local_diagnostics.set(vec![]);
        let analysis = validation_analyzer.clone();
        revision.update(|r| *r += 1);
        let token = revision.get_untracked();
        rendered.set(Err("Validating…".into()));
        plan.set(None);
        floem::action::exec_after(
            std::time::Duration::from_millis(150),
            move |_| {
                if revision.get_untracked() == token && open.get_untracked() {
                    local_diagnostics.set(
                        current
                            .as_ref()
                            .map(|d| language::diagnostics(d, &sample))
                            .unwrap_or_default(),
                    );
                    if let Some(d) = current.as_ref() {
                        analysis.update(d, &sample, token);
                    }
                    rendered.set(
                        current
                            .ok_or("No draft".to_string())
                            .and_then(|d| d.validate(&sample)),
                    );
                }
            },
        );
    });
    let file_focus = focus.clone();
    let files =
        scroll(
            dyn_stack(
                move || {
                    draft
                        .get()
                        .map(|d| d.files.keys().cloned().collect::<Vec<_>>())
                        .unwrap_or_default()
                },
                |p| p.clone(),
                move |path| {
                    let label_path = path.clone();
                    let click_path = path.clone();
                    let diagnostic_path = path.clone();
                    file_focus.control(
                        label(move || {
                            format!(
                                "{}{}",
                                if diagnostics
                                    .get()
                                    .iter()
                                    .any(|d| d.path == diagnostic_path)
                                {
                                    "● "
                                } else {
                                    ""
                                },
                                label_path.display()
                            )
                        })
                        .on_click_stop(move |_| selected.set(click_path.clone()))
                        .style(move |s| {
                            s.padding(6.0).width_full().apply_if(
                                selected.get() == path,
                                |s| {
                                    s.background(config.get().color(
                                        SparrowColor::PANEL_CURRENT_BACKGROUND,
                                    ))
                                },
                            )
                        }),
                    )
                },
            )
            .style(|s| s.flex_col()),
        )
        .style(|s| s.width(210.0).min_width(140.0).height_full());
    let editor_focus = focus.clone();
    let documents =
        Rc::new(RefCell::new(BTreeMap::<PathBuf, Rc<TextDocument>>::new()));
    let action_bar = action_controls(analyzer.clone(),draft,selected,documents.clone(),active_editor.clone(),plan,error,&focus);
    let editor_scope = Scope::current();
    let text_editor_handle = active_editor.clone();
    let text_analyzer = analyzer.clone();
    let text = dyn_container(
        move || (epoch.get(), selected.get()),
        move |(_, path)| {
            let body = draft
                .get_untracked()
                .and_then(|d| d.files.get(&path).cloned())
                .unwrap_or_default();
            // Keep document undo history when switching file tabs. Documents
            // belong to the editor scope, rather than the disposable file view.
            let cached = documents
                .borrow()
                .get(&path)
                .cloned()
                .filter(|doc| doc.text().to_string() == body);
            let doc = cached.unwrap_or_else(|| {
                let doc = Rc::new(TextDocument::new(editor_scope, body));
                let update_path = path.clone();
                doc.add_on_update(move |event| {
                    let Some(editor) = event.editor else {
                        return;
                    };
                    let text = editor.doc().text().to_string();
                    plan.set(None);
                    draft.update(|d| {
                        if let Some(d) = d {
                            d.files.insert(update_path.clone(), text);
                        }
                    });
                    error.set(String::new());
                });
                documents.borrow_mut().insert(path.clone(), doc.clone());
                doc
            });
            suggestions.set(vec![]);
            let key_path = path.clone();
            let refresh_path = path.clone();
            let style_path = path.clone();
            let text_editor_handle = text_editor_handle.clone();
            let key_analyzer=text_analyzer.clone();
            let refresh_analyzer=text_analyzer.clone();
            let editor = text_editor_keys("",move |editor_signal,keypress,modifiers| {
                let ed = editor_signal.get_untracked();
                let key = match &keypress.key {KeyInput::Keyboard(key,_)=>Some(key),_=>None};
                if matches!(key,Some(Key::Named(NamedKey::Escape))) && (!key_analyzer.actions.list.get_untracked().is_empty() || key_analyzer.actions.preview.get_untracked().is_some()) {
                    key_analyzer.actions.clear();
                    return CommandExecuted::Yes;
                }
                if modifiers.contains(Modifiers::ALT) && matches!(key,Some(Key::Named(NamedKey::Enter))) {
                    plan.set(None);
                    suggestions.set(vec![]);
                    key_analyzer.request_actions(&ed,selected);
                    return CommandExecuted::Yes;
                }
                let explicit = modifiers.contains(Modifiers::CONTROL)
                    && (matches!(key,Some(Key::Named(NamedKey::Space))) || matches!(key,Some(Key::Character(c)) if c.as_str()==" "));
                if explicit {
                    let body=ed.doc().text().to_string();
                    let files=draft.get_untracked().map(|d|d.files).unwrap_or_default();
                    suggestions.set(language::completions(&key_path,&body,ed.cursor.get_untracked().offset(),&files,true));
                    suggestion_index.set(0);
                    key_analyzer.request_completions(&key_path,&ed,selected,suggestions,true);
                    return CommandExecuted::Yes;
                }
                let available=suggestions.get_untracked();
                if !available.is_empty() && !modifiers.intersects(Modifiers::CONTROL|Modifiers::ALT|Modifiers::META) {
                    match key {
                        Some(Key::Named(NamedKey::ArrowDown))=>{
                            suggestion_index.update(|i|*i=(*i+1)%available.len());return CommandExecuted::Yes;
                        }
                        Some(Key::Named(NamedKey::ArrowUp))=>{
                            suggestion_index.update(|i|*i=(*i+available.len()-1)%available.len());return CommandExecuted::Yes;
                        }
                        Some(Key::Named(NamedKey::Enter|NamedKey::Tab))=>{
                            if let Some(choice)=available.get(suggestion_index.get_untracked()) {
                                key_analyzer.accept(&ed,choice,draft,selected,plan,error);
                            }
                            suggestions.set(vec![]);return CommandExecuted::Yes;
                        }
                        Some(Key::Named(NamedKey::Escape))=>{
                            suggestions.set(vec![]);return CommandExecuted::Yes;
                        }
                        _=>{}
                    }
                }
                default_key_handler(editor_signal)(keypress,modifiers)
            }).use_doc(doc).with_editor(move |ed| {
                *text_editor_handle.borrow_mut()=Some(ed.clone());
                let ed=ed.clone();
                let refresh_ed=ed.clone();
                floem::reactive::create_effect(move |_| {
                    let offset=refresh_ed.cursor.get().offset();
                    // Track buffer edits through the draft, even if the cursor did not move.
                    let files=draft.get().map(|d|d.files).unwrap_or_default();
                    let body=refresh_ed.doc().text().to_string();
                    suggestions.set(language::completions(&refresh_path,&body,offset,&files,false));
                    suggestion_index.set(0);
                    refresh_analyzer.request_completions(&refresh_path,&refresh_ed,selected,suggestions,false);
                    let (_,point)=refresh_ed.points_of_offset(offset,CursorAffinity::Forward);
                    let viewport=refresh_ed.viewport.get_untracked();
                    popup_point.set(((point.x-viewport.x0+45.0).clamp(0.0,180.0),(point.y-viewport.y0).clamp(0.0,260.0)));
                });
                let version=Rc::new(std::cell::Cell::new(0u64));
                floem::reactive::create_effect(move |_| {
                    let diags=diagnostics.get();
                    let config=config.get();
                    // Each new document revision and diagnostics refresh invalidates line styles.
                    let files=draft.get();
                    let body=files.and_then(|d|d.files.get(&style_path).cloned()).unwrap_or_default();
                    version.set(version.get()+1);
                    // Styling rebuilds Floem's internal editor effects. Those
                    // implementation signals must not become dependencies of
                    // this diagnostics effect, or the rebuild triggers itself.
                    floem::reactive::untrack(|| {
                        ed.update_styling(Rc::new(language::CodeStyle::new(&style_path,&body,&diags,config,version.get())));
                    });
                });
            })
                .editor_style(move |s| {
                    s.cursor_color(config.get().color(SparrowColor::EDITOR_CARET))
                        .selection_color(
                            config.get().color(SparrowColor::EDITOR_SELECTION),
                        )
                        .current_line_color(
                            config.get().color(SparrowColor::EDITOR_CURRENT_LINE),
                        )
                })
                .style(move |s| {
                    s.size_full()
                        .font_family(config.get().editor.font_family.clone())
                        .font_size(13.0)
                        .background(
                            config.get().color(SparrowColor::EDITOR_BACKGROUND),
                        )
                });
            editor_focus.control(editor)
        },
    )
    .style(|s| s.flex_grow(1.0_f32).min_width(0.0).height_full());
    let popup_handle = active_editor.clone();
    let popup_analyzer = analyzer.clone();
    let popup = scroll(
        dyn_stack(
            move || {
                suggestions
                    .get()
                    .into_iter()
                    .enumerate()
                    .collect::<Vec<_>>()
            },
            |(i, c)| (*i, c.label.clone()),
            move |(i, choice)| {
                let handle = popup_handle.clone();
                let analyzer = popup_analyzer.clone();
                let label_text = choice.label.clone();
                label(move || label_text.clone())
                    .on_click_stop(move |_| {
                        if let Some(ed) = handle.borrow().as_ref() {
                            analyzer
                                .accept(ed, &choice, draft, selected, plan, error);
                        }
                        suggestions.set(vec![]);
                    })
                    .style(move |s| {
                        s.height(24.0).padding_horiz(8.0).width_full().apply_if(
                            suggestion_index.get() == i,
                            |s| {
                                s.background(
                                    config.get().color(
                                        SparrowColor::PANEL_CURRENT_BACKGROUND,
                                    ),
                                )
                            },
                        )
                    })
            },
        )
        .style(|s| s.flex_col()),
    )
    .style(move |s| {
        s.absolute()
            .inset_left(popup_point.get().0)
            .inset_top(popup_point.get().1)
            .width(300.0)
            .max_height(150.0)
            .z_index(200)
            .border(1.0)
            .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
            .background(config.get().color(SparrowColor::PANEL_BACKGROUND))
            .apply_if(suggestions.get().is_empty(), |s| s.hide())
    });
    let diagnostic_handle = active_editor.clone();
    let rows_focus = focus.clone();
    let diagnostic_rows = scroll(
        dyn_stack(
            move || {
                diagnostics
                    .get()
                    .into_iter()
                    .filter(|d| d.path == selected.get())
                    .collect::<Vec<_>>()
            },
            |d| (d.path.clone(), d.range.start, d.message.clone()),
            move |diagnostic| {
                let handle = diagnostic_handle.clone();
                rows_focus.control(
                    label({
                        let diagnostic = diagnostic.clone();
                        move || {
                            let body = draft
                                .get()
                                .and_then(|d| d.files.get(&diagnostic.path).cloned())
                                .unwrap_or_default();
                            let (line, col) =
                                language::position(&body, diagnostic.range.start);
                            format!(
                                "{}:{line}:{col} · {}",
                                diagnostic.path.display(),
                                diagnostic.message
                            )
                        }
                    })
                    .on_click_stop(move |_| {
                        if let Some(ed) = handle.borrow().as_ref() {
                            ed.cursor.update(|c| {
                                c.set_offset(diagnostic.range.start, false, false)
                            });
                            if let Some(id) = ed.editor_view_id.get_untracked() {
                                id.request_focus();
                            }
                            ed.editor_view_focused.notify();
                        }
                    })
                    .style(move |s| {
                        s.padding(4.0)
                            .width_full()
                            .color(config.get().color(SparrowColor::SPARROW_ERROR))
                    }),
                )
            },
        )
        .style(|s| s.flex_col()),
    )
    .style(move |s| {
        s.width_full().max_height(80.0).apply_if(
            !diagnostics.get().iter().any(|d| d.path == selected.get()),
            |s| s.hide(),
        )
    });
    let code_panel = stack((
        stack((text, popup))
            .style(|s| s.flex_grow(1.0_f32).min_height(160.0).width_full()),
        diagnostic_rows,
    ))
    .style(|s| s.flex_col().flex_grow(1.0_f32).min_width(0.0).height_full());
    let action_preview = analyzer.actions.preview;
    let output = scroll(
        dyn_stack(
            move || {
                let mut text = match rendered.get() {
                    Err(e) => e,
                    Ok(files) => files
                        .iter()
                        .map(|(p, s)| format!("── {} ──\n{}\n", p.display(), s))
                        .collect::<Vec<_>>()
                        .join("\n"),
                };
                if let Some(p) = plan.get() {
                    text = format!(
                        "SAVE DIFF · {}\n{}\n",
                        p.title,
                        (0..p.changes.len())
                            .map(|i| p.diff(i))
                            .collect::<Vec<_>>()
                            .join("\n")
                    );
                }
                if let Some(preview) = action_preview.get() { text = preview.diff(); }
                let mut lines = text
                    .lines()
                    .take(2000)
                    .map(str::to_string)
                    .collect::<Vec<_>>();
                if text.lines().count() > 2000 {
                    lines.push("Preview display limited to 2,000 lines.".into());
                }
                lines.into_iter().enumerate().collect::<Vec<_>>()
            },
            |(i, s)| (*i, s.clone()),
            move |(_, text)| {
                let added=text.starts_with('+') && !text.starts_with("+++");
                let removed=text.starts_with('-') && !text.starts_with("---");
                label(move || text.clone()).style(move |s| {
                    s.apply_if(action_preview.get().is_some() && added,|s|s.color(config.get().color(SparrowColor::SOURCE_CONTROL_ADDED)))
                        .apply_if(action_preview.get().is_some() && removed,|s|s.color(config.get().color(SparrowColor::SOURCE_CONTROL_REMOVED)))
                        .height(20.0)
                        .padding_horiz(6.0)
                        .font_family(config.get().editor.font_family.clone())
                        .font_size(12.0)
                })
            },
        )
        .style(|s| s.flex_col()),
    )
    .style(|s| s.width(420.0).min_width(250.0).height_full());
    let add_window = window.clone();
    let remove_window = window.clone();
    let save_window = window.clone();
    let apply_window = window.clone();
    let discard_window = window.clone();
    let save_actions=analyzer.actions.clone();
    let generated_actions=analyzer.actions.clone();
    let body = stack((
        label(move ||format!("Template Editor{}",if draft.get().is_some_and(|d|d.dirty()) {" · Unsaved draft"} else {""}))
            .style(|s|s.font_bold()),
        label(move ||draft.get().map(|d|format!("{} → {}",d.source,d.directory.display())).unwrap_or_default()),
        label(||"Edit metadata in pack.yaml. Ctrl+Space: complete · ↑/↓: choose · Enter/Tab: insert · Esc: dismiss."),
        label(move ||dart_status.get()),
        stack((
            label(||"Sample name:"),focus.input(sample, config).style(|s|s.width(180.0)),
            label(move ||match rendered.get() {Ok(files)=>format!("Pack valid · {} generated files",files.len()),Err(e)=>e})
                .style(move |s|s.flex_grow(1.0_f32).min_width(0.0).apply_if(rendered.get().is_err(),|s|s.color(config.get().color(SparrowColor::SPARROW_ERROR)))) ,
        )).style(|s|s.gap(12.0).items_center()),
        action_bar,
        stack((files,code_panel,output)).style(|s|s.width_full().height(430.0).gap(10.0)),
        stack((
            focus.input(new_file, config).style(|s|s.width(270.0)),
            focus.control(label(||"Add File").on_click_stop(move |_| {
                let mut result = Err("No draft".to_string());
                draft.update(|d| {if let Some(d)=d {result=d.add_file(&new_file.get_untracked());}});
                match result {Ok(path)=>selected.set(path),Err(e)=>add_window.scaffold.error.set(e)}
            })),
            focus.control(label(||"Remove File").on_click_stop(move |_| {
                let mut result = Err("No draft".to_string());
                draft.update(|d| {if let Some(d)=d {result=d.remove_file(&selected.get_untracked());}});
                match result {Ok(())=>selected.set("pack.yaml".into()),Err(e)=>remove_window.scaffold.error.set(e)}
            })),
        )).style(|s|s.gap(16.0).items_center()),
        label(move ||error.get()),
        stack((
            focus.control(label(||"Preview Save").on_click_stop(move |_| {
                save_actions.clear();
                let result = draft.get_untracked().ok_or("No draft".into()).and_then(|d|d.save_plan(&sample.get_untracked()));
                save_window.scaffold.undoing.set(false);
                stage(&save_window,result);
            })),
            focus.control(label(||"Apply Save").disabled(move ||plan.get().is_none()).on_click_stop(move |_| {
                apply_window.scaffold.apply(&apply_window);
                if plan.get_untracked().is_none() {
                    draft.update(|d| {if let Some(d)=d {d.mark_saved();}});
                    apply_window.scaffold.manager.refresh();
                    error.set("Template saved. Undo Last Change is available in the manager.".into());
                }
            })),
            focus.control(label(||"Generated Preview").on_click_stop(move |_| {generated_actions.clear();plan.set(None);} )),
            focus.control(label(||"Back to Manager").on_click_stop(move |_| {plan.set(None);open.set(false);})),
            focus.control(label(||"Discard Draft").on_click_stop(move |_| {
                draft.set(None);plan.set(None);open.set(false);
                discard_window.scaffold.error.set("Draft discarded; saved files are unchanged.".into());
            })),
        )).style(|s|s.gap(18.0)),
    )).style(move |s|s.width(1380.0).max_width_pct(96.0).max_height_pct(96.0).flex_col()
        .gap(10.0).padding(18.0).border(1.0).border_radius(6.0)
        .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
        .background(config.get().color(SparrowColor::PANEL_BACKGROUND))
        .color(config.get().color(SparrowColor::EDITOR_FOREGROUND)));
    let dialog = container(body).style(move |s|s.absolute().size_full().items_center().justify_center().z_index(100)
        .apply_if(!active.get(),|s|s.hide()))
        .on_event_stop(EventListener::PointerDown,|_|{})
        .on_event(EventListener::KeyDown,move |event| {
            if matches!(event,Event::KeyDown(k) if k.key.logical_key==Key::Named(NamedKey::Escape)) {
                plan.set(None);open.set(false);EventPropagation::Stop
            } else {EventPropagation::Continue}
        });
    focus
        .bind_root(dialog, active, config)
        .on_cleanup(move || analyzer.close())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    fn project() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        fs::write(
            d.path().join("pubspec.yaml"),
            "name: editor_test
",
        )
        .unwrap();
        d
    }
    fn local(root: &Path) -> Draft {
        let destination = root.join(".sparrow/scaffolds");
        super::super::create(&destination, "team.example")
            .unwrap()
            .apply()
            .unwrap();
        let entry = catalog::manager_entries(root)
            .into_iter()
            .find(|e| e.id == "team.example")
            .unwrap();
        Draft::load(&entry, root, &destination).unwrap()
    }
    fn body(d: &mut Draft, text: &str) {
        d.files
            .insert("templates/template.dart.tmpl".into(), text.into());
    }
    #[test]
    fn accepted_completion_updates_draft_cursor_and_document_undo() {
        use floem::views::editor::{
            command::Command, core::command::EditCommand, text::SimpleStyling,
        };
        let root = project();
        let mut initial = local(root.path());
        body(&mut initial, "class {{na}} {}");
        let source = PathBuf::from("templates/template.dart.tmpl");
        let text = initial.files[&source].clone();
        let choices =
            language::completions(&source, &text, 10, &initial.files, true);
        let name = choices.iter().find(|c| c.label == "{{name}}").unwrap();
        let cx = Scope::new();
        let draft = cx.create_rw_signal(Some(initial));
        let plan = cx.create_rw_signal(None);
        let error = cx.create_rw_signal(String::new());
        let selected = cx.create_rw_signal(source.clone());
        let doc = Rc::new(TextDocument::new(cx, text.clone()));
        let ed = Editor::new(cx, doc.clone(), Rc::new(SimpleStyling::new()), false);
        ed.cursor.update(|c| c.set_offset(10, false, false));
        accept_completion(&ed, name, draft, plan, error, selected);
        assert_eq!(doc.text().to_string(), "class {{name}} {}");
        assert_eq!(
            draft.get_untracked().unwrap().files[&source],
            "class {{name}} {}"
        );
        assert_eq!(ed.cursor.get_untracked().offset(), 14);
        ed.doc().run_command(
            &ed,
            &Command::Edit(EditCommand::Undo),
            Some(1),
            Modifiers::empty(),
        );
        assert_eq!(doc.text().to_string(), text);
        cx.dispose();
    }
    #[test]
    fn live_validation_checks_yaml_sources_metadata_variables_and_output_paths() {
        let root = project();
        let original = local(root.path());
        for invalid in [
            "not yaml: [",
            "schema: 99
id: bad
version: 1.0.0
title: Bad
files: []",
        ] {
            let mut d = original.clone();
            d.files.insert("pack.yaml".into(), invalid.into());
            assert!(d.validate("Counter").is_err());
            assert!(d.save_plan("Counter").is_err());
        }
        for invalid in ["{{unknown}}", "{{name", "{{ name }} {{missing}}"] {
            let mut d = original.clone();
            body(&mut d, invalid);
            assert!(d.validate("Counter").is_err());
        }
        let mut d = original.clone();
        d.files.remove(Path::new("templates/template.dart.tmpl"));
        assert!(
            d.validate("Counter")
                .unwrap_err()
                .contains("Missing template source")
        );
        let mut d = original.clone();
        let manifest = d.files.get_mut(Path::new("pack.yaml")).unwrap();
        *manifest = manifest.replace("{{snake}}.dart", "../escape.dart");
        assert!(d.validate("Counter").is_err());
        assert!(original.validate("lowercase").is_err());
        assert!(original.validate("Counter").is_ok());
        assert!(!original.dirty());
    }
    #[test]
    fn preview_uses_unsaved_buffers_and_does_not_touch_generated_files() {
        let root = project();
        let mut d = local(root.path());
        body(
            &mut d,
            "class {{ name }} { final value = '{{variable}}'; }
",
        );
        fs::write(root.path().join("counter.dart"), "existing app code").unwrap();
        let preview = d.validate("Counter").unwrap();
        assert_eq!(
            preview[0],
            (
                PathBuf::from("counter.dart"),
                "class Counter { final value = 'counter'; }
"
                .into()
            )
        );
        assert_eq!(
            fs::read_to_string(root.path().join("counter.dart")).unwrap(),
            "existing app code"
        );
        assert!(
            !fs::read_to_string(d.directory.join("templates/template.dart.tmpl"))
                .unwrap()
                .contains("final value")
        );
    }
    #[test]
    fn edited_pack_save_and_undo_use_original_snapshots() {
        let root = project();
        let mut d = local(root.path());
        let before = d.original.clone();
        body(
            &mut d,
            "class {{name}} { int value = 42; }
",
        );
        let manifest = d.files.get_mut(Path::new("pack.yaml")).unwrap();
        *manifest = manifest.replace("My Template", "Edited Template");
        let applied = d.save_plan("Counter").unwrap().apply().unwrap();
        let pack = engine::load_pack(&d.directory.join("pack.yaml")).unwrap();
        assert_eq!(pack.title, "Edited Template");
        assert!(pack.files[0].content.contains("42"));
        assert!(d.dirty());
        d.mark_saved();
        assert!(!d.dirty());
        assert!(d.save_plan("Counter").is_err());
        applied.undo().unwrap();
        for (p, s) in before {
            assert_eq!(fs::read_to_string(d.directory.join(p)).unwrap(), s);
        }
    }
    #[test]
    fn file_add_remove_and_external_conflicts_are_transactional() {
        let root = project();
        let mut d = local(root.path());
        for path in ["", "../escape", "/absolute", "pack.yaml"] {
            assert!(d.add_file(path).is_err());
        }
        assert!(d.remove_file(Path::new("pack.yaml")).is_err());
        let file = d.add_file("templates/extra.dart.tmpl").unwrap();
        d.files.insert(file.clone(), "// helper".into());
        assert!(d.dirty());
        let plan = d.save_plan("Counter").unwrap();
        fs::write(d.directory.join(&file), "external").unwrap();
        assert!(plan.apply().is_err());
        assert!(d.save_plan("Counter").is_err());
        fs::remove_file(d.directory.join(&file)).unwrap();
        let added = d.save_plan("Counter").unwrap().apply().unwrap();
        assert!(d.directory.join(&file).exists());
        d.mark_saved();
        d.remove_file(&file).unwrap();
        let removed = d.save_plan("Counter").unwrap().apply().unwrap();
        assert!(!d.directory.join(&file).exists());
        removed.undo().unwrap();
        assert_eq!(
            fs::read_to_string(d.directory.join(&file)).unwrap(),
            "// helper"
        );
        added.undo().unwrap();
        assert!(!d.directory.join(&file).exists());
    }
    #[test]
    fn external_source_changes_block_saving_even_when_source_not_edited() {
        let root = project();
        let mut d = local(root.path());
        let manifest = d.files.get_mut(Path::new("pack.yaml")).unwrap();
        *manifest = manifest.replace("My Template", "New Title");
        fs::write(
            d.directory.join("templates/template.dart.tmpl"),
            "// external",
        )
        .unwrap();
        assert!(d.save_plan("Counter").is_err());
        assert_eq!(
            engine::load_pack(&d.directory.join("pack.yaml"))
                .unwrap()
                .title,
            "My Template"
        );
    }
    #[test]
    fn bundled_edit_saves_override_without_mutating_defaults_or_clobbering() {
        let root = project();
        let entry = catalog::bundled_entries()
            .into_iter()
            .find(|e| e.id == "sparrow.state.cubit")
            .unwrap();
        let destination = root.path().join(".sparrow/scaffolds");
        let mut d = Draft::load(&entry, root.path(), &destination).unwrap();
        let original = entry.load().unwrap().files[0].content.clone();
        body(
            &mut d,
            "class {{name}} {}
",
        );
        let applied = d.save_plan("Counter").unwrap().apply().unwrap();
        assert_eq!(entry.load().unwrap().files[0].content, original);
        assert_eq!(
            engine::load_pack(&d.directory.join("pack.yaml"))
                .unwrap()
                .files[0]
                .content,
            "class {{name}} {}
"
        );
        assert!(d.save_plan("Counter").is_err());
        applied.undo().unwrap();
        assert!(!d.directory.exists());
    }
    #[test]
    fn invalid_pack_can_be_repaired_and_disabled_pack_can_be_previewed() {
        let root = project();
        let d = local(root.path());
        let manifest = d.directory.join("pack.yaml");
        let valid = d.files.get(Path::new("pack.yaml")).unwrap().clone();
        fs::write(&manifest, "invalid: [").unwrap();
        let entry = catalog::manager_entries(root.path())
            .into_iter()
            .find(|e| e.id == "team.example")
            .unwrap();
        assert!(entry.problem().is_some());
        let mut repaired = Draft::load(
            &entry,
            root.path(),
            &root.path().join(".sparrow/scaffolds"),
        )
        .unwrap();
        assert!(repaired.validate("Counter").is_err());
        repaired.files.insert(
            "pack.yaml".into(),
            format!(
                "{valid}
disabled: true
"
            ),
        );
        assert!(repaired.validate("Counter").is_ok());
        repaired.save_plan("Counter").unwrap().apply().unwrap();
        assert!(engine::load_pack(&manifest).unwrap().disabled);
    }
    #[test]
    fn all_bundled_draft_previews_match_generation_bytes() {
        let root = project();
        let ctx = engine::context(root.path(), root.path()).unwrap();
        for entry in catalog::bundled_entries() {
            let d = Draft::load(
                &entry,
                root.path(),
                &root.path().join(".sparrow/scaffolds"),
            )
            .unwrap();
            let pack = entry.load().unwrap();
            let plan = match pack.target {
                engine::Target::Package => engine::plan_pack(&ctx, &pack, "Counter"),
                engine::Target::Sibling => {
                    engine::plan_sibling(&ctx, root.path(), &pack, "Counter")
                }
            }
            .unwrap();
            let preview = d.validate("Counter").unwrap();
            for (path, body) in preview {
                assert!(
                    plan.changes
                        .iter()
                        .any(|c| c.path == root.path().join(&path)
                            && c.after.as_deref() == Some(body.as_str())),
                    "{}",
                    entry.id
                );
            }
        }
    }
}
