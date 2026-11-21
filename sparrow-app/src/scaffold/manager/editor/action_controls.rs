//! Template analyzer quick fixes and code action controls.

use crate::scaffold::manager::engine;
use floem::{
    View,
    reactive::{RwSignal, SignalGet, SignalUpdate},
    views::{
        Decorators, dyn_stack,
        editor::{Editor, text_document::TextDocument},
        label, scroll, stack,
    },
};
use std::{cell::RefCell, collections::BTreeMap, path::PathBuf, rc::Rc};

use super::{Draft, analyzer};

pub(super) fn action_controls(
    session: Rc<analyzer::Session>, draft: RwSignal<Option<Draft>>, selected: RwSignal<PathBuf>,
    documents: Rc<RefCell<BTreeMap<PathBuf, Rc<TextDocument>>>>, handle: Rc<RefCell<Option<Editor>>>,
    plan: RwSignal<Option<engine::Plan>>, error: RwSignal<String>, focus: &crate::widgets::modal_focus::ModalFocus,
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
