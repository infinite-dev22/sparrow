//! Device Manager tool window: displays connected devices and cold emulators
//! with one-click launching, selecting, and refreshing.

use std::rc::Rc;

use floem::{
    View,
    reactive::{SignalGet, SignalUpdate, SignalWith},
    style::CursorStyle,
    views::{
        Decorators, empty, label, scroll, stack, svg, virtual_stack,
        VirtualVector,
    },
};

use super::position::PanelPosition;
use crate::{
    app::clickable_icon,
    config::{color::SparrowColor, icon::SparrowIcons},
    dart::hot_reload::{device_platform_icon, FlutterDevice, FlutterEmulator},
    window_tab::WindowTabData,
};

pub fn device_manager_panel(
    window_tab_data: Rc<WindowTabData>,
    _position: PanelPosition,
) -> impl View {
    let hot_reload = window_tab_data.flutter_hot_reload.clone();
    let config = window_tab_data.common.config;
    let ui_line_height = window_tab_data.common.ui_line_height;

    let devices = hot_reload.devices;
    let emulators = hot_reload.emulators;
    let selected_device = hot_reload.selected_device;
    let is_fetching = hot_reload.is_fetching_devices;

    stack((
        // Top Toolbar
        stack((
            label(|| "Devices & Emulators".to_string()).style(move |s| {
                let c = config.get();
                s.color(c.color(SparrowColor::PANEL_FOREGROUND_DIM))
                    .font_size(11.0)
                    .margin_left(10.0)
                    .selectable(false)
            }),
            empty().style(|s| s.flex_grow(1.0f32)),
            clickable_icon(
                || SparrowIcons::IMAGE_LOADING,
                {
                    let hr = hot_reload.clone();
                    move || hr.fetch_devices()
                },
                || false,
                move || is_fetching.get(),
                || "Refresh Devices & Emulators",
                config,
            )
            .style(|s| s.margin_right(4.0)),
        ))
        .style(move |s| {
            let c = config.get();
            s.height(26.0)
                .items_center()
                .border_bottom(1.0)
                .border_color(c.color(SparrowColor::SPARROW_BORDER))
        }),

        // Main content list
        scroll(
            stack((
                // Section: Connected Devices
                section_header("RUNNING / ATTACHED DEVICES", config),
                virtual_stack(
                    move || DeviceList::new(devices.get()),
                    move |(i, d)| (*i, d.id.clone()),
                    {
                        let _hr = hot_reload.clone();
                        move |(_, dev)| {
                            device_item_row(dev, selected_device, config)
                        }
                    },
                )
                .item_size_fixed(move || ui_line_height.get())
                .style(|s| s.flex_col().min_width_full()),

                // Section: Cold Emulators & Simulators
                section_header("VIRTUAL DEVICES / EMULATORS", config),
                virtual_stack(
                    move || EmulatorList::new(emulators.get()),
                    move |(i, e)| (*i, e.id.clone()),
                    {
                        let hr = hot_reload.clone();
                        move |(_, emu)| {
                            emulator_item_row(emu, hr.clone(), config)
                        }
                    },
                )
                .item_size_fixed(move || ui_line_height.get())
                .style(|s| s.flex_col().min_width_full()),
            ))
            .style(|s| s.flex_col().min_width_full()),
        )
        .style(|s| s.absolute().size_full()),
    ))
    .style(|s| s.absolute().size_full().flex_col())
    .debug_name("Device Manager Panel")
}

fn section_header(
    title: &'static str,
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::SparrowConfig>>,
) -> impl View {
    label(move || title.to_string()).style(move |s| {
        let c = config.get();
        s.font_size(10.0)
            .font_bold()
            .color(c.color(SparrowColor::EDITOR_DIM))
            .padding_left(10.0)
            .padding_vert(6.0)
            .selectable(false)
    })
}

fn device_item_row(
    dev: FlutterDevice,
    selected: floem::reactive::RwSignal<Option<FlutterDevice>>,
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::SparrowConfig>>,
) -> impl View {
    let dev_clone = dev.clone();
    let dev_id = dev.id.clone();
    let dev_name = dev.name.clone();
    let is_active = move || {
        selected.with(|s| s.as_ref().map(|d| d.id == dev_id).unwrap_or(false))
    };

    let platform_icon = device_platform_icon(dev.target_platform.as_deref());

    stack((
        svg(move || config.get().ui_svg(platform_icon)).style(move |s| {
            let c = config.get();
            let size = c.ui.icon_size() as f32;
            s.size(size, size)
                .margin_right(8.0)
                .color(c.color(SparrowColor::SPARROW_ICON_ACTIVE))
        }),
        label(move || dev_name.clone()).style(|s| {
            s.selectable(false)
                .text_ellipsis()
                .flex_grow(1.0_f32)
        }),
        label(move || {
            if is_active() {
                "● Active".to_string()
            } else {
                "".to_string()
            }
        })
        .style(move |s| {
            let c = config.get();
            s.color(c.color(SparrowColor::SPARROW_WARN))
                .font_size(10.0)
                .margin_right(10.0)
                .selectable(false)
        }),
    ))
    .on_click_stop(move |_| {
        selected.set(Some(dev_clone.clone()));
    })
    .style(move |s| {
        let c = config.get();
        s.items_center()
            .padding_left(16.0)
            .padding_right(10.0)
            .cursor(CursorStyle::Pointer)
            .hover(|s| {
                s.background(c.color(SparrowColor::PANEL_HOVERED_BACKGROUND))
            })
    })
}

fn emulator_item_row(
    emu: FlutterEmulator,
    hot_reload: crate::dart::FlutterHotReloadData,
    config: floem::reactive::ReadSignal<std::sync::Arc<crate::config::SparrowConfig>>,
) -> impl View {
    let emu_id = emu.id.clone();
    let emu_name = emu.name.clone();
    let emu_category = emu.category.unwrap_or_else(|| "mobile".to_string());

    let icon_name = if emu_category.contains("mobile") {
        SparrowIcons::DEVICE_MOBILE
    } else {
        SparrowIcons::DEVICE_DESKTOP
    };

    stack((
        svg(move || config.get().ui_svg(icon_name)).style(move |s| {
            let c = config.get();
            let size = c.ui.icon_size() as f32;
            s.size(size, size)
                .margin_right(8.0)
                .color(c.color(SparrowColor::SPARROW_ICON_ACTIVE))
        }),
        label(move || emu_name.clone()).style(|s| {
            s.selectable(false)
                .text_ellipsis()
                .flex_grow(1.0_f32)
        }),
        label(|| "Cold Boot ▶".to_string()).style(move |s| {
            let c = config.get();
            s.font_size(10.0)
                .color(c.color(SparrowColor::EDITOR_LINK))
                .margin_right(10.0)
                .selectable(false)
        }),
    ))
    .on_click_stop({
        let hr = hot_reload.clone();
        let id = emu_id.clone();
        move |_| {
            hr.launch_emulator(&id);
        }
    })
    .style(move |s| {
        let c = config.get();
        s.items_center()
            .padding_left(16.0)
            .padding_right(10.0)
            .cursor(CursorStyle::Pointer)
            .hover(|s| {
                s.background(c.color(SparrowColor::PANEL_HOVERED_BACKGROUND))
            })
    })
}

struct DeviceList {
    items: Vec<(usize, FlutterDevice)>,
}

impl DeviceList {
    fn new(devices: Vec<FlutterDevice>) -> Self {
        Self {
            items: devices.into_iter().enumerate().collect(),
        }
    }
}

impl VirtualVector<(usize, FlutterDevice)> for DeviceList {
    fn total_len(&self) -> usize {
        self.items.len()
    }

    fn slice(&mut self, range: std::ops::Range<usize>) -> impl Iterator<Item = (usize, FlutterDevice)> {
        let start = range.start.min(self.items.len());
        let end = range.end.min(self.items.len());
        self.items[start..end].iter().cloned()
    }
}

struct EmulatorList {
    items: Vec<(usize, FlutterEmulator)>,
}

impl EmulatorList {
    fn new(emulators: Vec<FlutterEmulator>) -> Self {
        Self {
            items: emulators.into_iter().enumerate().collect(),
        }
    }
}

impl VirtualVector<(usize, FlutterEmulator)> for EmulatorList {
    fn total_len(&self) -> usize {
        self.items.len()
    }

    fn slice(&mut self, range: std::ops::Range<usize>) -> impl Iterator<Item = (usize, FlutterEmulator)> {
        let start = range.start.min(self.items.len());
        let end = range.end.min(self.items.len());
        self.items[start..end].iter().cloned()
    }
}
