//! Connected device cards and mirroring controls.

use super::{Notice, record};
use crate::panel::kind::PanelKind;
use crate::widgets::popup_menu::PopupMenuExt;
use crate::widgets::popup_menu::{Menu, MenuItem};
use crate::{
    config::{color::SparrowColor, icon::SparrowIcons},
    dart::hot_reload::FlutterRunState,
    widgets::clickable_icon,
    window_tab::WindowTabData,
};
use floem::{
    View,
    reactive::{
        RwSignal, SignalGet, SignalUpdate, SignalWith, create_memo, create_rw_signal,
    },
    views::{Decorators, dyn_stack, empty, label, stack, svg},
};
use std::{
    cell::RefCell,
    process::{Child, Command, Stdio},
    rc::Rc,
    time::Duration,
};
use crate::widgets::sparrow_scroll;

fn watch_mirror(
    child: Rc<RefCell<Option<Child>>>,
    status: RwSignal<String>,
    notices: RwSignal<Vec<Notice>>,
    alive: Rc<std::cell::Cell<bool>>,
) {
    floem::action::exec_after(Duration::from_secs(1), move |_| {
        if !alive.get() {
            return;
        }
        let result = child
            .borrow_mut()
            .as_mut()
            .and_then(|c| c.try_wait().ok().flatten());
        if let Some(exit) = result {
            child.borrow_mut().take();
            status.set(String::new());
            record(
                notices,
                "Running Devices",
                &format!("Device mirroring exited: {exit}"),
            );
        } else if child.borrow().is_some() {
            watch_mirror(child.clone(), status, notices, alive.clone());
        }
    });
}

const DEVICES_ART: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 180 112" fill="none" stroke-width="3"><rect x="25" y="25" width="104" height="61" rx="7" stroke="#5889d8"/><path d="M65 88v12m-22 1h58" stroke="#5889d8"/><rect x="111" y="43" width="32" height="60" rx="7" stroke="#4b9b69"/><circle cx="127" cy="92" r="2" fill="#4b9b69"/><rect x="8" y="66" width="51" height="36" rx="5" stroke="#4b9b69"/></svg>"##;
pub fn running_devices(window: Rc<WindowTabData>) -> impl View {
    let config = window.common.config;
    let notices = window.notifications;
    let hr = window.flutter_hot_reload.clone();
    let selected = hr.selected_device;
    let devices = hr.devices;
    let state = hr.state;
    let manager = window.clone();
    let refresh = hr.clone();
    let add = hr.clone();
    let mirror: Rc<RefCell<Option<Child>>> = Rc::new(RefCell::new(None));
    let mirror_alive = Rc::new(std::cell::Cell::new(true));
    let cleanup_alive = mirror_alive.clone();
    let cleanup = mirror.clone();
    let stop = mirror.clone();
    let mirror_status = create_rw_signal(String::new());
    let rows = create_memo(move |_| {
        devices
            .get()
            .into_iter()
            .filter(|d| {
                d.target_platform
                    .as_deref()
                    .is_some_and(|p| p.contains("android") || p.contains("ios"))
                    || (state.get() == FlutterRunState::Running
                        && selected
                            .with(|s| s.as_ref().is_some_and(|s| s.id == d.id)))
            })
            .collect::<Vec<_>>()
    });
    stack((crate::panel::chrome::header(window,PanelKind::RunningDevices,"Running Devices",empty()),
 stack((clickable_icon(||SparrowIcons::ADD,move ||{},||false,||false,||"Select device or launch emulator",config).ide_popout_menu(move ||{let mut m=Menu::new("");for d in add.devices.get_untracked(){let choice=d.clone();m=m.entry(MenuItem::new(&d.name).action(move ||selected.set(Some(choice.clone()))));}for e in add.emulators.get_untracked(){let hr=add.clone();m=m.entry(MenuItem::new(format!("Launch {}",e.name)).action(move ||hr.launch_emulator(&e.id)));}m}),label(||"Device Manager").on_click_stop(move |_|manager.show_panel(PanelKind::DeviceManager)).style(move |s|s.color(config.get().color(SparrowColor::EDITOR_LINK))),clickable_icon(||SparrowIcons::IMAGE_LOADING,move ||refresh.fetch_devices(),||false,||false,||"Refresh Flutter devices",config))).style(|s|s.height(30.0).gap(8.0).items_center().padding_horiz(12.0)),
 stack((svg(||DEVICES_ART.to_string()).style(|s|s.size(180.0,112.0).margin_bottom(20.0)),label(||"Connect an Android device via USB or Wi-Fi to mirror it. Use + to select a Flutter target or launch an installed emulator.").style(|s|s.width(346.0).max_width_full()),label(||"Android mirroring opens in a separate scrcpy window. Start a Flutter session to display its active target here.").style(|s|s.width(346.0).max_width_full().margin_top(18.0)))).style(move |s|s.flex_col().flex_grow(1.0_f32).width_full().items_center().justify_center().padding(18.0).padding_bottom(116.0).line_height(1.4).color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM)).apply_if(!rows.with(|r|r.is_empty()),|s|s.hide())),
 sparrow_scroll(dyn_stack(move ||rows.get(),|d|d.id.clone(),move |d|{let select=d.clone();let is_android=d.target_platform.as_deref().is_some_and(|p|p.contains("android"));let mirror=mirror.clone();let mirror_alive=mirror_alive.clone();let id=d.id.clone();stack((label(move ||d.name.clone()).style(|s|s.font_bold()),label(move ||d.target_platform.clone().unwrap_or_default()).style(|s|s.margin_top(6.0)),label(||"Select Flutter target").on_click_stop(move |_|selected.set(Some(select.clone()))).style(move |s|s.margin_top(10.0).color(config.get().color(SparrowColor::EDITOR_LINK))),label(||"Mirror Android device").on_click_stop(move |_|{if let Some(mut old)=mirror.borrow_mut().take(){let _=old.kill();let _=old.wait();}match Command::new("scrcpy").args(["-s",&id,"--window-title","Sparrow · Flutter device"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn(){Ok(child)=>{mirror.borrow_mut().replace(child);mirror_status.set("Mirroring opened in scrcpy".into());watch_mirror(mirror.clone(),mirror_status,notices,mirror_alive.clone());},Err(e)=>record(notices,"Running Devices",&format!("Unable to start scrcpy: {e}"))}}).style(move |s|s.margin_top(10.0).color(config.get().color(SparrowColor::EDITOR_LINK)).apply_if(!is_android,|s|s.hide())))).style(move |s|s.flex_col().width_full().padding(16.0).border_bottom(1.0).border_color(config.get().color(SparrowColor::SPARROW_BORDER)))}).style(|s|s.flex_col().width_full())).style(move |s|s.width_full().flex_grow(1.0_f32).min_height(0.0).apply_if(rows.with(|r|r.is_empty()),|s|s.hide())),
 label(move ||mirror_status.get()).style(move |s|s.padding_horiz(12.0).apply_if(mirror_status.with(|s|s.is_empty()),|s|s.hide())),label(||"Stop mirroring").on_click_stop(move |_|{if let Some(mut child)=stop.borrow_mut().take(){let _=child.kill();let _=child.wait();}mirror_status.set(String::new());}).style(move |s|s.height(28.0).padding_horiz(12.0).color(config.get().color(SparrowColor::EDITOR_LINK)).apply_if(mirror_status.with(|s|s.is_empty()),|s|s.hide())),
 )).on_cleanup(move ||{cleanup_alive.set(false);if let Some(mut child)=cleanup.borrow_mut().take(){let _=child.kill();let _=child.wait();}}).style(|s|s.absolute().size_full().flex_col().font_size(13.0))
}
