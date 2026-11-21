//! Mobile device presets, orientation and browser preview controls.

use super::{DEVICES, Preview, launch};
use crate::widgets::{ButtonKind, button, text_input, toggle_row};
use crate::{
    config::color::SparrowColor,
    widgets::popup_menu::{Menu, MenuItem},
    window_tab::WindowTabData,
};
use floem::{
    View,
    reactive::{Scope, SignalGet, SignalUpdate, create_rw_signal},
    views::{Decorators, empty, label, stack},
};
use std::{cell::RefCell, rc::Rc};

pub fn panel(window: Rc<WindowTabData>) -> impl View {
    let config = window.common.config;
    let device = create_rw_signal(3usize);
    let landscape = create_rw_signal(false);
    let url = create_rw_signal("http://127.0.0.1:7357".to_string());
    let status = create_rw_signal(String::new());
    let busy = create_rw_signal(false);
    let preview = Rc::new(RefCell::new(None::<Preview>));
    let session = preview.clone();
    let stop = preview.clone();
    let commands = window.common.internal_command;
    let root = window.workspace.path.clone();
    let notices = window.notifications;
    let launch_button = button(
        move || if busy.get() { "Opening…" } else { "Open interactive preview" },
        ButtonKind::Primary,
        config,
    ).on_click_stop(move |_|{
 if busy.get_untracked(){return;}busy.set(true);status.set(String::new());let session=session.clone();let send=floem::ext_event::create_ext_action(Scope::current(),move |result:Result<Preview,String>|{busy.set(false);match result{Ok(browser)=>{session.borrow_mut().replace(browser);status.set("Preview is open in its own mobile viewport. Touch, scrolling and app navigation are enabled.".into());},Err(e)=>status.set(e)}});let url=url.get_untracked();let d=DEVICES[device.get_untracked()];let rotate=landscape.get_untracked();std::thread::spawn(move ||send(launch(&url,d,rotate)));
 }).disabled(move || busy.get()).style(|s| s.padding(8.0));
    stack((crate::panel::chrome::header(window,crate::panel::kind::PanelKind::MobilePreview,"Mobile Preview",empty()),stack((
 label(||"Preview your app without an emulator").style(|s|s.font_bold()),
 crate::widgets::menu_button(
     move || DEVICES[device.get()].name,
     move || {
         let mut menu = Menu::new("Device");
         for (index, preset) in DEVICES.iter().enumerate() {
             menu = menu.entry(MenuItem::new(preset.name).action(move || device.set(index)));
         }
         menu
     },
     config,
 ),
 label(move ||{let d=DEVICES[device.get()];let (w,h)=if landscape.get(){(d.height,d.width)}else{(d.width,d.height)};format!("{w} × {h} logical pixels · {}× density",d.dpr)}),
 toggle_row("Landscape", None, landscape, |_| {}, config),
 text_input(url, config).placeholder("App URL"),
 button(|| "Start Flutter web server", ButtonKind::Outlined, config).style(|s| s.padding(8.0)).on_click_stop(move |_|{let Some(root)=root.as_deref() else{return;};if let Some(flutter)=crate::dart::sdk::find_flutter_binary(Some(root)){commands.send(crate::command::InternalCommand::NewTerminal{profile:Some(sparrow_rpc::terminal::TerminalProfile{name:"Mobile Preview server".into(),command:Some(flutter.display().to_string()),arguments:Some(vec!["run".into(),"-d".into(),"web-server".into(),"--web-hostname".into(),"127.0.0.1".into(),"--web-port".into(),"7357".into()]),workdir:lsp_types::Url::from_directory_path(root).ok(),..Default::default()})});}else{crate::panel::workflow_tools::record(notices,"Mobile Preview","Configure a Flutter SDK before starting the web server.");}}),
 launch_button,button(|| "Close preview", ButtonKind::Outlined, config).style(|s| s.padding(8.0)).on_click_stop(move |_|{stop.borrow_mut().take();status.set("Preview closed.".into());}),label(move ||status.get()),
 label(||"Uses your app’s Flutter web build. Camera, native plugins and iOS/Android OS behavior require a real device. Change a preset or orientation, then reopen to apply.").style(move |s|s.width_full().color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))),
 )).style(|s|s.flex_col().gap(14.0).padding(16.0).width_full()))).style(|s|s.absolute().size_full().flex_col())
}
