//! Resource rename preview and application dialog.

use crate::panel::{resource_manager::Resource, resource_rename::Plan};
use crate::{
    config::color::SparrowColor, widgets::text_input, window_tab::WindowTabData,
};
use floem::{
    View,
    reactive::{
        RwSignal, Scope, SignalGet, SignalUpdate, SignalWith, create_effect,
        create_rw_signal,
    },
    views::{Decorators, label, stack},
};
use std::rc::Rc;
use crate::widgets::sparrow_scroll;

#[derive(Clone, Copy)]
pub(super) struct RenameForm {
    rename_resource: RwSignal<Option<Resource>>,
    rename_name: RwSignal<String>,
    rename_plan: RwSignal<Option<Plan>>,
    rename_busy: RwSignal<bool>,
    rename_error: RwSignal<String>,
}

impl RenameForm {
    pub(super) fn new() -> Self {
        let form = Self {
            rename_resource: create_rw_signal(None),
            rename_name: create_rw_signal(String::new()),
            rename_plan: create_rw_signal(None),
            rename_busy: create_rw_signal(false),
            rename_error: create_rw_signal(String::new()),
        };
        create_effect(move |_| {
            form.rename_name.get();
            form.rename_resource.get();
            form.rename_plan.set(None);
            form.rename_error.set(String::new());
        });
        form
    }

    pub(super) fn show(self, resource: Resource) {
        self.rename_name.set(resource.name.clone());
        self.rename_resource.set(Some(resource));
    }

    pub(super) fn view(
        self,
        window: Rc<WindowTabData>,
        selected: RwSignal<Option<Resource>>,
        refresh: RwSignal<u64>,
        scope: Scope,
    ) -> impl View {
        let Self {
            rename_resource,
            rename_name,
            rename_plan,
            rename_busy,
            rename_error,
        } = self;
        let config = window.common.config;
        stack((
            label(||"Rename resource and usages").style(|s|s.font_bold()),
            text_input(rename_name, config).disabled(move ||rename_busy.get()).placeholder("New resource name"),
            sparrow_scroll(label(move ||rename_plan.with(|p|p.as_ref().map(|p|p.details()).unwrap_or_default()))).style(|s|s.width_full().max_height(220.0)),
            label(move ||rename_error.get()),
            crate::widgets::action_group((
                crate::widgets::compact_button(|| "Cancel", crate::widgets::ButtonKind::Outlined, config)
                    .disabled(move || rename_busy.get())
                    .on_click_stop(move |_|{if !rename_busy.get_untracked(){rename_resource.set(None);}}),
                crate::widgets::compact_button(move ||if rename_busy.get(){"Working…"}else if rename_plan.with(|p|p.is_some()){"Apply rename"}else{"Preview usages"}, crate::widgets::ButtonKind::Primary, config)
                    .disabled(move || rename_busy.get())
                    .on_click_stop({let window=window.clone();move |_|{
                    if rename_busy.get_untracked(){return;}
                    let Some(root)=window.workspace.path.clone()else{return;};
                    let Some(target)=rename_resource.get_untracked()else{return;};
                    if window.main_split.docs.with_untracked(|docs|docs.iter().any(|(path,doc)|path.starts_with(root.join(&target.module))&&!doc.is_pristine())){rename_error.set("Save modified module files before renaming.".into());return;}
                    rename_busy.set(true);let name=rename_name.get_untracked();let plan=rename_plan.get_untracked();
                    let requested_name=name.clone();let requested_module=target.module.clone();let applied=plan.clone();let completed_window=window.clone();
                    let send=floem::ext_event::create_ext_action(scope,move |result:Result<Option<crate::panel::resource_rename::Plan>,String>|{
                        rename_busy.set(false);
                        if applied.is_none() && (rename_name.get_untracked()!=requested_name || !rename_resource.with_untracked(|r|r.as_ref().is_some_and(|r|r.module==requested_module))) {return;}
                        match result {Ok(Some(plan))=>rename_plan.set(Some(plan)),Ok(None)=>{
                            if let Some(applied)=&applied {
                                for (old,new) in &applied.moves {
                                    let editors=completed_window.main_split.editors.with_editors_untracked(|items|items.values().filter(|e|e.doc().content.with_untracked(|c|c.path()==Some(old))).cloned().collect::<Vec<_>>());
                                    if !editors.is_empty(){let (doc,_)=completed_window.main_split.get_doc(new.clone(),None);for editor in editors{editor.update_doc(doc.clone());}}
                                    completed_window.main_split.docs.update(|docs|{docs.remove(old);});
                                }
                                for (path,_,text) in &applied.edits {let path=applied.moves.iter().find(|(old,_)|old==path).map(|(_,new)|new).unwrap_or(path);completed_window.main_split.docs.with_untracked(|docs|{if let Some(doc)=docs.get(path){if doc.is_pristine(){doc.reload(lapce_xi_rope::Rope::from(text.clone()),true);}}});}
                                completed_window.file_explorer.reload();
                            }
                            rename_resource.set(None);selected.set(None);refresh.update(|n|*n+=1);},Err(e)=>{rename_plan.set(None);rename_error.set(e);}}
                    });
                    std::thread::spawn(move ||send(if let Some(plan)=plan {plan.apply().map(|_|None)}else{crate::panel::resource_rename::plan(&root,&target,&name).map(Some)}));
                }}),
            )),
        )).style(move |s|s.absolute().inset_left(12.0).inset_right(12.0).inset_top(100.0).padding(16.0).gap(12.0).flex_col().border(1.0).border_radius(5.0).background(config.get().color(SparrowColor::PANEL_BACKGROUND)).border_color(config.get().color(SparrowColor::SPARROW_BORDER)).z_index(60).apply_if(rename_resource.with(|r|r.is_none()),|s|s.hide()))
    }
}
