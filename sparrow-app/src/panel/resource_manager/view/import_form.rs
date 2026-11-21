//! Resource imports and new Android resource values.

use crate::panel::resource_manager::{Kind, create_value, import_asset};
use crate::{
    config::color::SparrowColor, widgets::text_input, window_tab::WindowTabData,
};
use floem::{
    View,
    reactive::{
        RwSignal, Scope, SignalGet, SignalUpdate, SignalWith, create_rw_signal,
    },
    views::{Decorators, label, stack},
};
use std::{
    path::{Path, PathBuf},
    rc::Rc,
};

#[derive(Clone, Copy)]
pub(super) struct ImportForm {
    import_open: RwSignal<bool>,
    create_kind: RwSignal<Option<Kind>>,
    source: RwSignal<String>,
    destination: RwSignal<String>,
    error: RwSignal<String>,
    importing: RwSignal<bool>,
}

impl ImportForm {
    pub(super) fn new() -> Self {
        Self {
            import_open: create_rw_signal(false),
            create_kind: create_rw_signal(None),
            source: create_rw_signal(String::new()),
            destination: create_rw_signal(String::from("assets/")),
            error: create_rw_signal(String::new()),
            importing: create_rw_signal(false),
        }
    }

    pub(super) fn show(self, kind: Option<Kind>, source: &str, destination: &str) {
        self.create_kind.set(kind);
        self.source.set(source.into());
        self.destination.set(destination.into());
        self.import_open.set(true);
    }

    pub(super) fn view(
        self,
        window: Rc<WindowTabData>,
        import_root: Option<PathBuf>,
        module: RwSignal<String>,
        refresh: RwSignal<u64>,
        scope: Scope,
    ) -> impl View {
        let Self {
            import_open,
            create_kind,
            source,
            destination,
            error,
            importing,
        } = self;
        let config = window.common.config;
        stack((
            label(move ||match create_kind.get(){Some(Kind::Color)=>"New color resource",Some(Kind::String)=>"New string resource",_=>"Import resource"}).style(|s| s.font_bold()),
            text_input(source, config).placeholder("Source file path / resource value"),
            text_input(destination, config).placeholder("Destination path / resource name"),
            label(move || error.get()),
            stack((
                label(|| "Cancel").on_click_stop(move |_| import_open.set(false)),
                label(move ||if create_kind.get().is_some(){"Create"}else{"Import"}).on_click_stop(move |_| {
                    let Some(root) = import_root.clone() else {
                        return;
                    };
                    let dirty = window.main_split.docs.with_untracked(|docs| {
                        docs.get(&root.join("pubspec.yaml"))
                            .is_some_and(|doc| !doc.is_pristine())
                    });
                    if dirty {
                        error.set(
                            "Save pubspec.yaml before importing an asset.".into(),
                        );
                        return;
                    }
                    if let Some(kind)=create_kind.get_untracked() {
                        let module=module.get_untracked();let module=if module.starts_with("android/"){module}else{"android/app".into()};
                        let target=root.join(module).join("src/main/res/values").join(if kind==Kind::Color{"colors.xml"}else{"strings.xml"});
                        if window.main_split.docs.with_untracked(|docs|docs.get(&target).is_some_and(|doc|!doc.is_pristine())){error.set("Save the resource definition before adding another value.".into());return;}
                    }
                    if importing.get_untracked() {
                        return;
                    }
                    importing.set(true);
                    error.set("Importing…".into());
                    let source = source.get_untracked();
                    let destination = destination.get_untracked();
                    let create=create_kind.get_untracked();let selected_module=module.get_untracked();
                    let send = floem::ext_event::create_ext_action(
                        scope,
                        move |result: Result<(), String>| {
                            importing.set(false);
                            match result {
                                Ok(()) => {
                                    import_open.set(false);
                                    error.set(String::new());
                                    refresh.update(|n| *n += 1);
                                }
                                Err(message) => error.set(message),
                            }
                        },
                    );
                    std::thread::spawn(move || {
                        send(if let Some(kind)=create{create_value(&root,&selected_module,kind,&destination,&source)}else{import_asset(&root,Path::new(&source),Path::new(&destination))})
                    });
                }),
            ))
            .style(|s| s.gap(16.0)),
        ))
        .style(move |s| {
            s.absolute()
                .inset_left(12.0)
                .inset_right(12.0)
                .inset_top(100.0)
                .padding(16.0)
                .gap(12.0)
                .flex_col()
                .border(1.0)
                .border_radius(5.0)
                .background(config.get().color(SparrowColor::PANEL_BACKGROUND))
                .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
                .z_index(50)
                .apply_if(!import_open.get(), |s| s.hide())
        })
    }
}
