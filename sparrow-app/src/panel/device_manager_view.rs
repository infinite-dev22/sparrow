//! Device Manager using live daemon devices and installed emulator metadata.
use super::{kind::PanelKind, position::PanelPosition};
use crate::{
    app::clickable_icon,
    config::{color::SparrowColor, icon::SparrowIcons},
    dart::hot_reload::device_platform_icon,
    window_tab::WindowTabData,
};
use floem::{
    View,
    reactive::{SignalGet, SignalUpdate, SignalWith},
    views::{Decorators, dyn_stack, empty, label, scroll, stack, svg},
};
use std::rc::Rc;
pub fn device_manager_panel(
    window: Rc<WindowTabData>,
    _position: PanelPosition,
) -> impl View {
    let hr = window.flutter_hot_reload.clone();
    let config = window.common.config;
    let devices = hr.devices;
    let emulators = hr.emulators;
    let selected = hr.selected_device;
    let fetching = hr.is_fetching_devices;
    let refresh = hr.clone();
    let launch = hr.clone();
    stack((
        super::chrome::header(
            window,
            PanelKind::DeviceManager,
            "Device Manager",
            empty(),
        ),
        stack((
            clickable_icon(
                || SparrowIcons::IMAGE_LOADING,
                move || {
                    refresh.fetch_devices();
                },
                || false,
                move || fetching.get(),
                || "Refresh Devices and Emulators",
                config,
            ),
            label(move || if fetching.get() { "Refreshing…" } else { "" })
                .style(|s| s.font_size(11.0)),
        ))
        .style(move |s| {
            s.height(28.0)
                .items_center()
                .flex_shrink(0.0_f32)
                .border_bottom(1.0)
                .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
        }),
        stack((
            label(|| "Name")
                .style(|s| s.flex_grow(1.0_f32).flex_basis(0.0).padding_left(12.0)),
            label(|| "SDK").style(|s| s.width(90.0)),
            label(|| "Type").style(|s| s.width(70.0)),
            empty().style(|s| s.width(30.0)),
        ))
        .style(move |s| {
            s.height(24.0)
                .items_center()
                .flex_shrink(0.0_f32)
                .border_bottom(1.0)
                .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
        }),
        label(|| "No connected devices or installed emulators").style(move |s| {
            s.padding(18.0)
                .font_size(12.0)
                .color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
                .apply_if(
                    !devices.with(|d| d.is_empty())
                        || !emulators.with(|e| e.is_empty())
                        || fetching.get(),
                    |s| s.hide(),
                )
        }),
        scroll(
            stack((
                dyn_stack(
                    move || devices.get(),
                    |d| (d.id.clone(), d.name.clone(), d.sdk.clone()),
                    move |device| {
                        let id = device.id.clone();
                        let copy = device.clone();
                        let icon =
                            device_platform_icon(device.target_platform.as_deref());
                        let sdk = device.sdk.unwrap_or_else(|| "—".into());
                        let kind = if device.emulator == Some(true) {
                            "Virtual"
                        } else if device.target_platform.as_deref().is_some_and(
                            |p| {
                                p.contains("linux")
                                    || p.contains("windows")
                                    || p.contains("darwin")
                            },
                        ) {
                            "Desktop"
                        } else if device
                            .target_platform
                            .as_deref()
                            .is_some_and(|p| p.contains("web"))
                        {
                            "Web"
                        } else {
                            "Physical"
                        };
                        stack((
                            svg(move || config.get().ui_svg(icon)).style(move |s| {
                                s.size(16.0, 16.0).margin_horiz(8.0).color(
                                    config
                                        .get()
                                        .color(SparrowColor::SPARROW_ICON_ACTIVE),
                                )
                            }),
                            stack((
                                label(move || device.name.clone())
                                    .style(|s| s.text_ellipsis()),
                                label(move || {
                                    device
                                        .target_platform
                                        .clone()
                                        .unwrap_or_default()
                                })
                                .style(move |s| {
                                    s.font_size(11.0).text_ellipsis().color(
                                        config.get().color(
                                            SparrowColor::PANEL_FOREGROUND_DIM,
                                        ),
                                    )
                                }),
                            ))
                            .style(|s| {
                                s.flex_col()
                                    .flex_grow(1.0_f32)
                                    .flex_basis(0.0)
                                    .min_width(0.0)
                            }),
                            label(move || sdk.clone())
                                .style(|s| s.width(90.0).text_ellipsis()),
                            label(move || kind).style(|s| s.width(70.0)),
                            empty().style(|s| s.width(30.0)),
                        ))
                        .on_click_stop(move |_| selected.set(Some(copy.clone())))
                        .style(move |s| {
                            s.height(44.0)
                                .items_center()
                                .width_full()
                                .apply_if(
                                    selected.with(|d| {
                                        d.as_ref().is_some_and(|d| d.id == id)
                                    }),
                                    |s| {
                                        s.background(config.get().color(
                                            SparrowColor::PANEL_CURRENT_BACKGROUND,
                                        ))
                                    },
                                )
                                .hover(|s| {
                                    s.background(config.get().color(
                                        SparrowColor::PANEL_HOVERED_BACKGROUND,
                                    ))
                                })
                        })
                    },
                )
                .style(|s| s.flex_col().min_width_full()),
                dyn_stack(
                    move || emulators.get(),
                    |e| (e.id.clone(), e.name.clone()),
                    move |emulator| {
                        let hr = launch.clone();
                        let id = emulator.id.clone();
                        let platform = emulator.platform_type.unwrap_or_else(|| {
                            emulator.category.unwrap_or_default()
                        });
                        stack((
                            svg(move || {
                                config.get().ui_svg(SparrowIcons::DEVICE_MOBILE)
                            })
                            .style(move |s| {
                                s.size(16.0, 16.0).margin_horiz(8.0).color(
                                    config
                                        .get()
                                        .color(SparrowColor::SPARROW_ICON_ACTIVE),
                                )
                            }),
                            stack((
                                label(move || emulator.name.clone())
                                    .style(|s| s.text_ellipsis()),
                                label(move || platform.clone()).style(move |s| {
                                    s.font_size(11.0).text_ellipsis().color(
                                        config.get().color(
                                            SparrowColor::PANEL_FOREGROUND_DIM,
                                        ),
                                    )
                                }),
                            ))
                            .style(|s| {
                                s.flex_col()
                                    .flex_grow(1.0_f32)
                                    .flex_basis(0.0)
                                    .min_width(0.0)
                            }),
                            label(|| "—").style(|s| s.width(90.0)),
                            label(|| "Virtual").style(|s| s.width(70.0)),
                            clickable_icon(
                                || SparrowIcons::START,
                                move || hr.launch_emulator(&id),
                                || false,
                                || false,
                                || "Launch Virtual Device",
                                config,
                            ),
                        ))
                        .style(move |s| {
                            s.height(44.0).items_center().width_full().hover(|s| {
                                s.background(
                                    config.get().color(
                                        SparrowColor::PANEL_HOVERED_BACKGROUND,
                                    ),
                                )
                            })
                        })
                    },
                )
                .style(|s| s.flex_col().min_width_full()),
            ))
            .style(|s| s.flex_col().min_width_full()),
        )
        .style(|s| {
            s.flex_grow(1.0_f32)
                .flex_basis(0.0)
                .min_height(0.0)
                .width_full()
        }),
    ))
    .style(|s| s.absolute().size_full().flex_col())
    .debug_name("Device Manager Table")
}
