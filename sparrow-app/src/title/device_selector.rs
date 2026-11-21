//! Flutter device chip and chooser popup.

use floem::{
    View,
    kurbo::Point,
    peniko::Color,
    reactive::{ReadSignal, RwSignal, SignalGet, SignalUpdate, create_effect},
    style::{CursorStyle, Display},
    text::Weight,
    views::{Decorators, container, dyn_stack, empty, label, scroll, stack, svg},
};
use std::{rc::Rc, sync::Arc};

use crate::dart::hot_reload::FlutterDevice;
use crate::{
    config::{SparrowConfig, color::SparrowColor, icon::SparrowIcons},
    dart::FlutterHotReloadData,
    widgets::divider::Divider,
    window_tab::WindowTabData,
};

pub(super) fn device_selector(
    hot_reload: FlutterHotReloadData,
    config: ReadSignal<Arc<SparrowConfig>>,
    origin: RwSignal<Point>,
    device_menu_visible: RwSignal<bool>,
) -> impl View {
    // Fetch devices when the app starts if it's a flutter project
    create_effect({
        let value = hot_reload.clone();
        move |_| {
            if value.is_flutter_project() {
                value.fetch_devices();
            }
        }
    });

    let value = hot_reload.clone();
    let is_flutter = move || value.is_flutter_project();

    container(
        stack((
            // Device platform icon on the chip
            svg(move || {
                let c = config.get();
                let icon_key = if let Some(dev) = hot_reload.selected_device.get() {
                    crate::dart::hot_reload::device_platform_icon(
                        dev.target_platform.as_deref(),
                    )
                } else {
                    SparrowIcons::DEVICE_DESKTOP
                };
                c.ui_svg(icon_key)
            })
            .style(move |s| {
                let c = config.get();
                let size = c.ui.icon_size() as f32 - 2.0;
                s.size(size, size)
                    .margin_right(6.0)
                    .color(Color::from_rgb8(0x5B, 0xCF, 0xF4))
            }),
            label(move || {
                if let Some(dev) = hot_reload.selected_device.get() {
                    device_display_name(&dev)
                } else {
                    "No Device".to_string()
                }
            })
            .style({
                let value = is_flutter.clone();
                move |s| s.selectable(false).apply_if(!value(), |s| s.hide())
            }),
            svg(move || config.get().ui_svg(SparrowIcons::DROPDOWN_ARROW)).style({
                let value = is_flutter.clone();
                move |s| {
                    let c = config.get();
                    let size = c.ui.icon_size() as f32;
                    s.size(size, size)
                        .margin_left(4.0)
                        .color(c.color(SparrowColor::SPARROW_ICON_INACTIVE))
                        .apply_if(!value(), |s| s.hide())
                }
            }),
        ))
        .style(|s| s.items_center()),
    )
    .on_move(move |pt| {
        origin.set(pt);
    })
    .on_click_stop(move |_| {
        // Toggle the custom overlay
        device_menu_visible.update(|v| *v = !*v);
    })
    .style(move |s| {
        let c = config.get();
        s.height(28.0)
            .padding_horiz(10.0)
            .margin_right(8.0)
            .items_center()
            .border_radius(6.0)
            .apply_if(!is_flutter(), |s| s.hide())
            .hover(|s| {
                s.cursor(CursorStyle::Pointer)
                    .background(c.color(SparrowColor::PANEL_HOVERED_BACKGROUND))
            })
    })
    .debug_name("Flutter Device Chip")
}

pub fn device_selector_overlay(
    window_tab_data: Rc<WindowTabData>,
    origin: RwSignal<Point>,
) -> impl View {
    let visible = window_tab_data.device_menu_visible;
    let config = window_tab_data.common.config;
    let hot_reload = window_tab_data.flutter_hot_reload.clone();

    let close_menu = {
        let visible = window_tab_data.device_menu_visible;
        move || visible.set(false)
    };

    let value = hot_reload.clone();
    let device_row = move |dev: FlutterDevice, is_current: bool| {
        let dev_clone = dev.clone();
        let hr = value.clone();
        let close = close_menu.clone();
        let config_get = config;

        let icon_key = crate::dart::hot_reload::device_platform_icon(
            dev.target_platform.as_deref(),
        );
        let platform_desc = dev
            .target_platform
            .clone()
            .unwrap_or_else(|| "desktop".to_string());

        container(
            stack((
                // Icon
                svg(move || {
                    let c = config_get.get();
                    c.ui_svg(icon_key)
                })
                .style(move |s| {
                    let c = config_get.get();
                    let size = c.ui.icon_size() as f32;
                    s.size(size, size).margin_right(10.0).color(if is_current {
                        c.color(SparrowColor::EDITOR_FOREGROUND)
                    } else {
                        c.color(SparrowColor::EDITOR_DIM)
                    })
                }),
                // Device Name & target platform
                stack((
                    label(move || dev_clone.name.clone()).style(move |s| {
                        let c = config_get.get();
                        s.font_size(13.0)
                            .font_weight(if is_current {
                                Weight::BOLD
                            } else {
                                Weight::NORMAL
                            })
                            .color(c.color(SparrowColor::EDITOR_FOREGROUND))
                    }),
                    label(move || format!("• {}", platform_desc)).style(move |s| {
                        let c = config_get.get();
                        s.font_size(11.0)
                            .margin_left(6.0)
                            .color(c.color(SparrowColor::PANEL_FOREGROUND_DIM))
                    }),
                ))
                .style(|s| s.items_center().flex_row().flex_grow(1.0_f32)),
                // Checkmark if current
                label(move || if is_current { "✓" } else { "" }).style(move |s| {
                    let c = config_get.get();
                    s.font_size(13.0)
                        .font_weight(Weight::BOLD)
                        .color(c.color(SparrowColor::EDITOR_FOREGROUND))
                }),
            ))
            .style(|s| s.items_center().width_full()),
        )
        .on_click_stop(move |_| {
            hr.selected_device.set(Some(dev.clone()));
            close();
        })
        .style(move |s| {
            let c = config_get.get();
            s.width_full()
                .height(32.0)
                .padding_horiz(10.0)
                .items_center()
                .apply_if(is_current, |s| {
                    s.background(
                        c.color(SparrowColor::SPARROW_BUTTON_PRIMARY_BACKGROUND)
                            .multiply_alpha(0.25),
                    )
                })
                .hover(|s| {
                    s.background(c.color(SparrowColor::PANEL_HOVERED_BACKGROUND))
                        .cursor(CursorStyle::Pointer)
                })
        })
    };

    let emu_value = hot_reload.clone();
    let emulator_row = move |emu: crate::dart::hot_reload::FlutterEmulator| {
        let emu_clone = emu.clone();
        let hr = emu_value.clone();
        let close = close_menu.clone();
        let config_get = config;

        container(
            stack((
                svg(move || config_get.get().ui_svg(SparrowIcons::DEVICE_MOBILE))
                    .style(move |s| {
                        let c = config_get.get();
                        let size = c.ui.icon_size() as f32;
                        s.size(size, size)
                            .margin_right(10.0)
                            .color(c.color(SparrowColor::EDITOR_DIM))
                    }),
                label(move || emu_clone.name.clone()).style(move |s| {
                    let c = config_get.get();
                    s.font_size(13.0)
                        .color(c.color(SparrowColor::EDITOR_FOREGROUND))
                        .flex_grow(1.0_f32)
                }),
                label(|| "Launch".to_string()).style(move |s| {
                    let c = config_get.get();
                    s.font_size(11.0).font_weight(Weight::BOLD).color(
                        c.color(SparrowColor::SPARROW_BUTTON_PRIMARY_BACKGROUND),
                    )
                }),
            ))
            .style(|s| s.items_center().width_full()),
        )
        .on_click_stop(move |_| {
            hr.launch_emulator(&emu.id);
            close();
        })
        .style(move |s| {
            let c = config_get.get();
            s.width_full()
                .height(32.0)
                .padding_horiz(10.0)
                .items_center()
                .hover(|s| {
                    s.background(c.color(SparrowColor::PANEL_HOVERED_BACKGROUND))
                        .cursor(CursorStyle::Pointer)
                })
        })
    };

    stack((
        // Background to catch outside clicks
        empty()
            .on_click_stop(move |_| close_menu())
            .style(|s| s.absolute().size_full()),
        // Menu Panel
        container(
            scroll(
                stack((
                    // Section header: Running Devices
                    label(|| "Running Devices".to_string()).style(move |s| {
                        let c = config.get();
                        s.font_size(11.0)
                            .font_weight(Weight::BOLD)
                            .padding_horiz(10.0)
                            .padding_vert(4.0)
                            .color(c.color(SparrowColor::PANEL_FOREGROUND_DIM))
                    }),
                    // Device List
                    dyn_stack(
                        move || hot_reload.devices.get(),
                        |dev| dev.id.clone(),
                        move |dev| {
                            let is_current = hot_reload
                                .selected_device
                                .get_untracked()
                                .map(|d| d.id == dev.id)
                                .unwrap_or(false);
                            device_row(dev, is_current)
                        },
                    )
                    .style(|s| s.flex_col().width_full()),
                    // If no devices found
                    label(|| "No running devices detected".to_string()).style({
                        let hr = hot_reload.clone();
                        move |s| {
                            let c = config.get();
                            let is_empty = hr.devices.get().is_empty();
                            s.font_size(12.0)
                                .padding_horiz(10.0)
                                .padding_vert(4.0)
                                .color(c.color(SparrowColor::EDITOR_DIM))
                                .apply_if(!is_empty, |s| s.hide())
                        }
                    }),
                    // Emulators section (if any available)
                    stack((
                        Divider::new(config).spacing(6.0).into_view(),
                        label(|| "Available Emulators".to_string()).style(
                            move |s| {
                                let c = config.get();
                                s.font_size(11.0)
                                    .font_weight(Weight::BOLD)
                                    .padding_horiz(10.0)
                                    .padding_vert(4.0)
                                    .color(
                                        c.color(SparrowColor::PANEL_FOREGROUND_DIM),
                                    )
                            },
                        ),
                        dyn_stack(
                            {
                                let hr = hot_reload.clone();
                                move || hr.emulators.get()
                            },
                            |emu| emu.id.clone(),
                            move |emu| emulator_row(emu),
                        )
                        .style(|s| s.flex_col().width_full()),
                    ))
                    .style({
                        let hr = hot_reload.clone();
                        move |s| {
                            let is_empty = hr.emulators.get().is_empty();
                            s.flex_col()
                                .width_full()
                                .apply_if(is_empty, |s| s.hide())
                        }
                    }),
                    Divider::new(config).spacing(6.0).into_view(),
                    // Refresh Devices Action
                    container(
                        stack((
                            svg(move || {
                                config
                                    .get()
                                    .ui_svg(SparrowIcons::FLUTTER_HOT_RESTART)
                            })
                            .style(move |s| {
                                let c = config.get();
                                let size = c.ui.icon_size() as f32;
                                s.size(size, size)
                                    .margin_right(10.0)
                                    .color(c.color(SparrowColor::EDITOR_FOREGROUND))
                            }),
                            label(|| "Refresh Devices".to_string()).style(
                                move |s| {
                                    s.font_size(13.0).color(
                                        config
                                            .get()
                                            .color(SparrowColor::EDITOR_FOREGROUND),
                                    )
                                },
                            ),
                        ))
                        .style(|s| s.items_center()),
                    )
                    .on_click_stop({
                        let hr = hot_reload.clone();
                        let close = close_menu.clone();
                        move |_| {
                            hr.fetch_devices();
                            close();
                        }
                    })
                    .style(move |s| {
                        let c = config.get();
                        s.width_full()
                            .height(32.0)
                            .padding_horiz(10.0)
                            .items_center()
                            .hover(|s| {
                                s.background(
                                    c.color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                                )
                                .cursor(CursorStyle::Pointer)
                            })
                    }),
                    // Restart Flutter Daemon Action
                    container(
                        stack((
                            svg(move || config.get().ui_svg(SparrowIcons::FLUTTER))
                                .style(move |s| {
                                    let c = config.get();
                                    let size = c.ui.icon_size() as f32;
                                    s.size(size, size).margin_right(10.0).color(
                                        c.color(SparrowColor::EDITOR_FOREGROUND),
                                    )
                                }),
                            label(|| "Restart Flutter Daemon".to_string()).style(
                                move |s| {
                                    s.font_size(13.0).color(
                                        config
                                            .get()
                                            .color(SparrowColor::EDITOR_FOREGROUND),
                                    )
                                },
                            ),
                        ))
                        .style(|s| s.items_center()),
                    )
                    .on_click_stop(move |_| {
                        hot_reload.restart_daemon();
                        close_menu();
                    })
                    .style(move |s| {
                        let c = config.get();
                        s.width_full()
                            .height(32.0)
                            .padding_horiz(10.0)
                            .items_center()
                            .hover(|s| {
                                s.background(
                                    c.color(SparrowColor::PANEL_HOVERED_BACKGROUND),
                                )
                                .cursor(CursorStyle::Pointer)
                            })
                    }),
                ))
                .style(|s| s.flex_col().width_full().padding_vert(4.0)),
            )
            .style(|s| s.max_height(400.0).width_full()),
        )
        .on_click_stop(|_| {}) // Prevent clicks inside menu from closing it
        .style(move |s| {
            let c = config.get();
            let pt = origin.get();
            let th = window_tab_data.title_height.get();

            s.absolute()
                .margin_left(pt.x as f32)
                // Anchor perfectly below the title bar, offset slightly to align with the chip
                .margin_top(th as f32 + 2.0)
                .width(300.0)
                .padding(4.0)
                .background(c.color(SparrowColor::PANEL_BACKGROUND))
                .border(1.0)
                .border_radius(6.0)
                .border_color(c.color(SparrowColor::SPARROW_BORDER))
                .box_shadow_blur(8.0)
                .box_shadow_color(c.color(SparrowColor::SPARROW_DROPDOWN_SHADOW))
        }),
    ))
    .style(move |s| {
        s.absolute()
            .size_full()
            .display(if visible.get() {
                Display::Flex
            } else {
                Display::None
            })
            .z_index(100)
    })
}

fn device_display_name(device: &FlutterDevice) -> String {
    match device.target_platform.as_deref() {
        Some(platform)
            if platform.starts_with("linux")
                || platform.starts_with("darwin")
                || platform.starts_with("windows") =>
        {
            format!("{} (desktop)", device.name)
        }
        Some(platform) if platform.starts_with("web") => {
            format!("{} (web)", device.name)
        }
        _ => device.name.clone(),
    }
}
