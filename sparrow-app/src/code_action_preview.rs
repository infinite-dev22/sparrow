//! Resolve and preview analyzer edits against live buffers without changing them.
use crate::{
    code_action::CodeActionStatus, config::color::SparrowColor,
    window_tab::WindowTabData,
};
use floem::{
    View,
    event::EventListener,
    ext_event::create_ext_action,
    reactive::{
        RwSignal, Scope, SignalGet, SignalUpdate, SignalWith, create_effect,
    },
    views::{Decorators, dyn_stack, empty, label, scroll, stack},
};
use lapce_xi_rope::Rope;
use lsp_types::{
    CodeActionOrCommand, DocumentChanges, OneOf, Position, TextEdit, WorkspaceEdit,
};
use sparrow_core::buffer::diff::{DiffLines, rope_diff};
use sparrow_rpc::plugin::PluginId;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, atomic::AtomicU64},
};

#[derive(Clone, Debug)]
pub struct PreviewFile {
    pub path: PathBuf,
    pub revision: u64,
    pub before: String,
    pub after: String,
}
#[derive(Clone, Debug)]
pub struct Prepared {
    pub action: CodeActionOrCommand,
    pub plugin_id: PluginId,
    pub origin: Option<(PathBuf, u64)>,
    pub files: Vec<PreviewFile>,
}
#[derive(Clone, Debug, Default)]
pub enum Preview {
    #[default]
    Empty,
    Loading,
    Blocked(String),
    Ready(Prepared),
}
#[derive(Clone)]
pub(crate) struct EditFile {
    pub(crate) path: PathBuf,
    pub(crate) edits: Vec<TextEdit>,
    version: Option<i32>,
}
pub(crate) fn edit_files(edit: &WorkspaceEdit) -> Result<Vec<EditFile>, String> {
    let mut files = BTreeMap::<PathBuf, EditFile>::new();
    let path = |uri: &lsp_types::Url| {
        uri.to_file_path()
            .map_err(|_| "This fix targets an unsupported document URI.".to_string())
    };
    if let Some(changes) = &edit.document_changes {
        let DocumentChanges::Edits(changes) = changes else {
            return Err("This fix includes file operations that cannot be previewed or applied here.".into());
        };
        for change in changes {
            let p = path(&change.text_document.uri)?;
            if files.contains_key(&p) {
                return Err(
                    "This fix contains multiple versions of the same document."
                        .into(),
                );
            }
            files.insert(
                p.clone(),
                EditFile {
                    path: p,
                    version: change.text_document.version,
                    edits: change
                        .edits
                        .iter()
                        .map(|e| match e {
                            OneOf::Left(e) => e.clone(),
                            OneOf::Right(e) => e.text_edit.clone(),
                        })
                        .collect(),
                },
            );
        }
    } else if let Some(changes) = &edit.changes {
        for (uri, edits) in changes {
            let p = path(uri)?;
            files.insert(
                p.clone(),
                EditFile {
                    path: p,
                    edits: edits.clone(),
                    version: None,
                },
            );
        }
    }
    Ok(files.into_values().collect())
}
/// Strict LSP UTF-16 conversion: stale or malformed edits must not be clipped.
fn byte_offset(source: &str, position: Position) -> Result<usize, String> {
    if position.line as usize > source.bytes().filter(|b| *b == b'\n').count() {
        return Err("The fix refers to a line outside the current document.".into());
    }
    let mut offset = 0;
    let mut lines = source.split_inclusive('\n');
    let line = lines.nth(position.line as usize);
    for text in source.split_inclusive('\n').take(position.line as usize) {
        offset += text.len();
    }
    let line = match line {
        Some(line) => line,
        None if offset == source.len()
            && (position.line == 0 || source.ends_with('\n')) =>
        {
            ""
        }
        _ => {
            return Err(
                "The fix refers to a line outside the current document.".into()
            );
        }
    };
    let text = line.trim_end_matches(['\r', '\n']);
    let mut units = 0;
    for (i, ch) in text.char_indices() {
        if units == position.character {
            return Ok(offset + i);
        };
        units += ch.len_utf16() as u32;
        if units > position.character {
            return Err("The fix splits a UTF-16 character.".into());
        }
    }
    if units == position.character {
        Ok(offset + text.len())
    } else {
        Err("The fix refers to a column outside the current document.".into())
    }
}
pub(crate) fn edited(source: &str, edits: &[TextEdit]) -> Result<String, String> {
    let mut ranges = edits
        .iter()
        .map(|e| {
            Ok((
                byte_offset(source, e.range.start)?,
                byte_offset(source, e.range.end)?,
                e.new_text.clone(),
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    ranges.sort_by_key(|e| (e.0, e.1));
    let mut end = 0;
    for (start, next, _) in &ranges {
        if start > next || *start < end {
            return Err("The fix contains overlapping or invalid edits.".into());
        }
        end = *next;
    }
    let mut result = source.to_string();
    for (start, end, text) in ranges.into_iter().rev() {
        result.replace_range(start..end, &text)
    }
    Ok(result)
}
pub(crate) fn prepare(
    window: Rc<WindowTabData>,
    scope: Scope,
    preview: RwSignal<Preview>,
    plugin_id: PluginId,
    action: CodeActionOrCommand,
    origin: Option<(PathBuf, u64)>,
) {
    if matches!(&action,CodeActionOrCommand::CodeAction(a) if a.edit.is_none() && a.command.is_none())
    {
        preview.set(Preview::Blocked(
            "This action did not provide an edit or command.".into(),
        ));
        return;
    }
    let edit = match &action {
        CodeActionOrCommand::CodeAction(a) => a.edit.clone(),
        _ => None,
    };
    let files = match edit.as_ref().map(edit_files).transpose() {
        Ok(files) => files.unwrap_or_default(),
        Err(error) => {
            preview.set(Preview::Blocked(error));
            return;
        }
    };
    let docs: Vec<_> = files
        .iter()
        .map(|file| window.main_split.get_doc(file.path.clone(), None).0)
        .collect();
    let initial = Rc::new(RefCell::new(None::<Vec<u64>>));
    scope.create_effect(move |_| {
        if docs.iter().any(|doc| !doc.loaded.get()) {
            return;
        }
        let revisions: Vec<_> = docs
            .iter()
            .map(|doc| doc.buffer.with(|b| b.rev()))
            .collect();
        if initial
            .borrow()
            .as_ref()
            .is_some_and(|old| old != &revisions)
        {
            preview.set(Preview::Blocked(
                "A document changed. Reopen quick fixes to get a current preview."
                    .into(),
            ));
            return;
        }
        if let Some((path, rev)) = &origin {
            if window.main_split.docs.with_untracked(|docs| {
                docs.get(path).is_none_or(|doc| doc.rev() != *rev)
            }) {
                preview.set(Preview::Blocked(
                    "The source changed. Reopen quick fixes.".into(),
                ));
                return;
            }
        }
        let mut snapshots = Vec::new();
        for ((file, doc), revision) in files.iter().zip(&docs).zip(&revisions) {
            if doc.content.with(|c| c.read_only()) {
                preview.set(Preview::Blocked(format!(
                    "{} is read-only.",
                    file.path.display()
                )));
                return;
            }
            if file
                .version
                .is_some_and(|version| version as i64 != *revision as i64)
            {
                preview.set(Preview::Blocked(
                    "This fix was computed for an older document version.".into(),
                ));
                return;
            }
            let before = doc.buffer.with_untracked(|b| b.to_string());
            let after = match edited(&before, &file.edits) {
                Ok(text) => text,
                Err(error) => {
                    preview.set(Preview::Blocked(error));
                    return;
                }
            };
            snapshots.push(PreviewFile {
                path: file.path.clone(),
                revision: *revision,
                before,
                after,
            });
        }
        *initial.borrow_mut() = Some(revisions);
        preview.set(Preview::Ready(Prepared {
            action: action.clone(),
            plugin_id,
            origin: origin.clone(),
            files: snapshots,
        }));
    });
}
fn install(window: Rc<WindowTabData>, request_scope: Rc<RefCell<Option<Scope>>>) {
    let data = window.code_action;
    create_effect(move |last: Option<Option<(usize, usize, usize)>>| {
        let current = data.get();
        let key = (current.status.get() == CodeActionStatus::Active).then(|| {
            (
                current.request_id,
                current.active.get(),
                current.items_version,
            )
        });
        if last == Some(key) {
            return key;
        }
        if let Some(scope) = request_scope.borrow_mut().take() {
            scope.dispose();
        }
        current.preview.set(Preview::Empty);
        let Some((_, index, _)) = key else { return key };
        let Some(item) = current.filtered_items.get(index).cloned() else {
            return key;
        };
        let scope = window.scope.create_child();
        *request_scope.borrow_mut() = Some(scope);
        let preview = current.preview;
        preview.set(Preview::Loading);
        let origin = current.origin.clone();
        if let CodeActionOrCommand::CodeAction(action) = &item.item {
            if let Some(disabled) = &action.disabled {
                preview.set(Preview::Blocked(disabled.reason.clone()));
                return key;
            }
            if action.edit.is_none() && action.data.is_some() {
                let w = window.clone();
                let send = create_ext_action(
                    scope,
                    move |result: Result<lsp_types::CodeAction, String>| match result
                    {
                        Ok(action) => prepare(
                            w,
                            scope,
                            preview,
                            item.plugin_id,
                            CodeActionOrCommand::CodeAction(action),
                            origin,
                        ),
                        Err(error) => preview.set(Preview::Blocked(error)),
                    },
                );
                if item.plugin_id == PluginId(0)
                    && window.common.dart_lsp.is_available()
                {
                    window
                        .common
                        .dart_lsp
                        .resolve_code_action_full(action.clone(), send);
                } else {
                    window.common.proxy.code_action_resolve(action.clone(),item.plugin_id,move |result|send(match result {Ok(sparrow_rpc::proxy::ProxyResponse::CodeActionResolveResponse{item})=>Ok(*item),_=>Err("Could not resolve this fix.".into())}));
                }
                return key;
            }
        }
        prepare(
            window.clone(),
            scope,
            preview,
            item.plugin_id,
            item.item,
            origin,
        );
        key
    });
}
pub fn apply(window: &WindowTabData, prepared: Prepared) -> Result<(), String> {
    if let Some((path, rev)) = &prepared.origin {
        if window.main_split.docs.with_untracked(|docs| {
            docs.get(path).is_none_or(|doc| doc.rev() != *rev)
        }) {
            return Err(
                "The source changed. No changes were applied; reopen quick fixes."
                    .into(),
            );
        }
    }
    for file in &prepared.files {
        let valid = window.main_split.docs.with_untracked(|docs| {
            docs.get(&file.path).is_some_and(|doc| {
                doc.loaded()
                    && !doc.content.with_untracked(|c| c.read_only())
                    && doc.rev() == file.revision
                    && doc.buffer.with_untracked(|b| b.to_string()) == file.before
            })
        });
        if !valid {
            return Err("A previewed document changed or became read-only. No changes were applied.".into());
        }
    }
    let linked=matches!(&prepared.action,CodeActionOrCommand::CodeAction(action) if action.edit.is_some() && action.command.is_none());
    let snapshots=if linked {prepared.files.iter().filter_map(|file|window.main_split.docs.with_untracked(|docs|docs.get(&file.path).cloned())).map(crate::workspace_undo::snapshot).collect()}else{Vec::new()};
    let label=match &prepared.action {CodeActionOrCommand::CodeAction(action)=>action.title.clone(),CodeActionOrCommand::Command(action)=>action.title.clone()};
    window.main_split.run_code_action(prepared.plugin_id, prepared.action);
    if linked {crate::workspace_undo::record(&window.common.workspace_undo,label,snapshots);}
    Ok(())
}
#[derive(Clone)]
struct DiffRow {
    text: String,
    kind: u8,
}
fn diff_rows(file: &PreviewFile) -> Vec<DiffRow> {
    let before: Vec<_> = file.before.lines().collect();
    let after: Vec<_> = file.after.lines().collect();
    let mut rows = Vec::new();
    for diff in rope_diff(
        Rope::from(file.before.as_str()),
        Rope::from(file.after.as_str()),
        0,
        Arc::new(AtomicU64::new(0)),
        Some(3),
    )
    .unwrap_or_default()
    {
        match diff {
            DiffLines::Left(range) => {
                for i in range {
                    rows.push(DiffRow {
                        text: format!(
                            "{:>4}    − {}",
                            i + 1,
                            before.get(i).unwrap_or(&"")
                        ),
                        kind: 1,
                    });
                }
            }
            DiffLines::Right(range) => {
                for i in range {
                    rows.push(DiffRow {
                        text: format!(
                            "     {:>4} + {}",
                            i + 1,
                            after.get(i).unwrap_or(&"")
                        ),
                        kind: 2,
                    });
                }
            }
            DiffLines::Both(info) => {
                for (n, (left, right)) in info.left.zip(info.right).enumerate() {
                    if let Some(skip) = &info.skip {
                        if skip.contains(&n) {
                            if n == skip.start {
                                rows.push(DiffRow {
                                    text: "          …".into(),
                                    kind: 0,
                                });
                            }
                            continue;
                        }
                    }
                    rows.push(DiffRow {
                        text: format!(
                            "{:>4} {:>4}   {}",
                            left + 1,
                            right + 1,
                            before.get(left).unwrap_or(&"")
                        ),
                        kind: 0,
                    });
                }
            }
        }
    }
    rows
}
pub fn view(window: Rc<WindowTabData>) -> impl View {
    let config = window.common.config;
    let data = window.code_action;
    let initial = data.get_untracked();
    let status = initial.status;
    let active = initial.active;
    let preview = initial.preview;
    let show = initial.show_preview;
    let scope = Rc::new(RefCell::new(None));
    install(window.clone(), scope.clone());
    let preview_view = stack((
        label(|| "Preview").style(|s| {
            s.height(28.0)
                .padding_horiz(10.0)
                .items_center()
                .font_bold()
        }),
        label(move || match preview.get() {
            Preview::Empty => String::new(),
            Preview::Loading => "Loading preview…".into(),
            Preview::Blocked(error) => error,
            Preview::Ready(p) if p.files.is_empty() => {
                "This action does not provide a source preview.".into()
            }
            _ => String::new(),
        })
        .style(move |s| {
            s.padding(12.0)
                .max_width(600.0)
                .apply_if(matches!(preview.get(),Preview::Empty) || matches!(preview.get(),Preview::Ready(p) if !p.files.is_empty()),|s|s.hide())
                .color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
        }),
        scroll(
            dyn_stack(
                move || match preview.get() {
                    Preview::Ready(p) => p.files,
                    _ => vec![],
                },
                |f| (f.path.clone(), f.revision, f.after.clone()),
                move |file| {
                    let name = file.path.display().to_string();
                    let rows = diff_rows(&file);
                    stack((
                        label(move || name.clone()).style(move |s| {
                            s.padding_horiz(10.0)
                                .height(26.0)
                                .items_center()
                                .color(config.get().color(SparrowColor::EDITOR_LINK))
                        }),
                        dyn_stack(
                            move || rows.clone().into_iter().enumerate(),
                            |(i, _)| *i,
                            move |(_, row)| {
                                label(move || row.text.clone()).style(move |s| {
                                    s.min_width_full()
                                        .height(20.0)
                                        .padding_horiz(8.0)
                                        .items_center()
                                        .font_family(
                                            config.get().editor.font_family.clone(),
                                        )
                                        .font_size(12.0)
                                        .apply_if(row.kind == 1, |s| {
                                            s.background(
                                                floem::peniko::Color::from_rgba8(
                                                    200, 60, 60, 35,
                                                ),
                                            )
                                        })
                                        .apply_if(row.kind == 2, |s| {
                                            s.background(
                                                floem::peniko::Color::from_rgba8(
                                                    70, 160, 80, 35,
                                                ),
                                            )
                                        })
                                })
                            },
                        )
                        .style(|s| s.flex_col().min_width_full()),
                    ))
                    .style(|s| s.flex_col().min_width_full())
                },
            )
            .style(|s| s.flex_col().min_width_full()),
        )
        .style(|s| s.flex_grow(1.0_f32).min_height(0.0).width_full()),
    ))
    .style(move |s| {
        s.flex_col()
            .width(600.0)
            .min_width(0.0)
            .height_full()
            .border_left(1.0)
            .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
            .apply_if(!show.get(), |s| s.hide())
    });
    let apply_data = data;
    let close_data = data;
    let retry_window = window.clone();
    let list=scroll(stack((label(move ||data.with(|d|match &d.request_state {crate::code_action::ActionRequestState::Loading=>"Finding quick fixes…".to_string(),crate::code_action::ActionRequestState::Failed(error)=>error.clone(),crate::code_action::ActionRequestState::Ready=>"No quick fixes available".to_string()})).style(move |s|s.padding(10.0).apply_if(!data.with(|d|d.filtered_items.is_empty()),|s|s.hide())),dyn_stack(move ||data.with(|d|d.filtered_items.clone().into_iter().enumerate().collect::<Vec<_>>()),move |(i,_)|(data.with_untracked(|d|d.request_id),*i),move |(i,item)|{
        let title=item.title().replace('\n'," ");let disabled=matches!(&item.item,CodeActionOrCommand::CodeAction(a) if a.disabled.is_some());
        label(move ||title.clone()).on_click_stop(move |_|{let d=data.get_untracked();d.active.set(i);})
            .on_double_click_stop(move |_|data.get_untracked().select())
            .on_event_stop(EventListener::PointerEnter,move |_|active.set(i))
            .style(move |s|s.height(26.0).width_full().padding_horiz(10.0).items_center().text_ellipsis().border_radius(4.0).apply_if(disabled,|s|s.color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))).apply_if(active.get()==i,|s|s.background(config.get().color(SparrowColor::COMPLETION_CURRENT))))
    }).style(|s|s.flex_col().width_full()))).style(|s|s.flex_col().width_full())).ensure_visible(move ||floem::peniko::kurbo::Rect::new(0.0,active.get() as f64*26.0,1.0,(active.get()+1) as f64*26.0)).style(|s|s.width(360.0).height_full());
    stack((
        stack((list, preview_view))
            .style(|s| s.height(360.0).min_height(0.0).width_full()),
        stack((
            label(move || {
                if show.get() {
                    "☑ Preview"
                } else {
                    "☐ Preview"
                }
            })
            .on_click_stop(move |_| show.update(|show| *show = !*show)),
            empty().style(|s| s.flex_grow(1.0_f32)),
            label(|| "Retry")
                .on_click_stop(move |_| {
                    data.get_untracked().cancel();
                    if let Some(editor) =
                        retry_window.main_split.active_editor.get_untracked()
                    {
                        editor.show_code_actions(false);
                    }
                })
                .style(move |s| {
                    s.padding_horiz(12.0)
                        .color(config.get().color(SparrowColor::EDITOR_LINK))
                        .apply_if(
                            data.with(|d| {
                                matches!(
                                    d.request_state,
                                    crate::code_action::ActionRequestState::Loading
                                ) || !d.filtered_items.is_empty()
                            }),
                            |s| s.hide(),
                        )
                }),
            label(|| "Cancel")
                .on_click_stop(move |_| close_data.get_untracked().cancel())
                .style(|s| s.padding_horiz(12.0)),
            label(|| "Apply  ↵")
                .on_click_stop(move |_| apply_data.get_untracked().select())
                .style(move |s| {
                    s.padding_horiz(12.0).color(config.get().color(
                        if matches!(preview.get(), Preview::Ready(_)) {
                            SparrowColor::EDITOR_LINK
                        } else {
                            SparrowColor::PANEL_FOREGROUND_DIM
                        },
                    ))
                }),
        ))
        .style(|s| s.height(32.0).items_center().padding_horiz(10.0)),
    ))
    .on_resize(move |rect| data.update(|d| d.layout_rect = rect))
    .on_event_stop(EventListener::PointerDown, |_| {})
    .on_event_stop(EventListener::PointerMove, |_| {})
    .on_cleanup(move || {
        if let Some(scope) = scope.borrow_mut().take() {
            scope.dispose();
        }
    })
    .style(move |s| {
        let origin = window.code_action_origin();
        s.absolute()
            .margin_left(origin.x as f32)
            .margin_top(origin.y as f32)
            .width(if show.get() { 960.0 } else { 360.0 })
            .max_width(
                (window.common.workbench_size.get().width - 24.0).max(360.0) as f32
            )
            .flex_col()
            .border(1.0)
            .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
            .border_radius(6.0)
            .background(config.get().color(SparrowColor::COMPLETION_BACKGROUND))
            .font_size(13.0)
            .apply_if(status.get() == CodeActionStatus::Inactive, |s| s.hide())
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_edits_utf16_without_mutating_and_rejects_overlaps() {
        let source = "final name = '🙂';\n";
        let edit = TextEdit {
            range: lsp_types::Range::new(Position::new(0, 6), Position::new(0, 10)),
            new_text: "value".into(),
        };
        assert_eq!(
            edited(source, &[edit.clone()]).unwrap(),
            "final value = '🙂';\n"
        );
        assert_eq!(source, "final name = '🙂';\n");
        assert!(edited(source, &[edit.clone(), edit]).is_err());
        assert!(byte_offset(source, Position::new(0, 15)).is_err());
        assert!(byte_offset(source, Position::new(99, 0)).is_err());
    }
    #[test]
    fn multi_file_edits_keep_versions_and_reject_file_operations() {
        let uri = lsp_types::Url::from_file_path("/tmp/preview.dart").unwrap();
        let document = lsp_types::TextDocumentEdit {
            text_document: lsp_types::OptionalVersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version: Some(7),
            },
            edits: vec![],
        };
        let edit = WorkspaceEdit {
            document_changes: Some(DocumentChanges::Edits(vec![document.clone()])),
            ..Default::default()
        };
        assert_eq!(edit_files(&edit).unwrap()[0].version, Some(7));
        let duplicate = WorkspaceEdit {
            document_changes: Some(DocumentChanges::Edits(vec![
                document.clone(),
                document,
            ])),
            ..Default::default()
        };
        assert!(edit_files(&duplicate).is_err());
        let operation = WorkspaceEdit {
            document_changes: Some(DocumentChanges::Operations(vec![
                lsp_types::DocumentChangeOperation::Op(
                    lsp_types::ResourceOp::Create(lsp_types::CreateFile {
                        uri,
                        options: None,
                        annotation_id: None,
                    }),
                ),
            ])),
            ..Default::default()
        };
        assert!(edit_files(&operation).is_err());
    }
}
