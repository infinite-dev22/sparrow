//! Device Manager using live daemon devices and installed emulator metadata.
use super::{kind::PanelKind, position::PanelPosition};
use crate::{
    widgets::clickable_icon,
    config::{color::SparrowColor, icon::SparrowIcons},
    dart::hot_reload::device_platform_icon,
    window_tab::WindowTabData,
};
use floem::{
    View,
    reactive::{SignalGet, SignalUpdate, SignalWith},
    views::{Decorators, dyn_stack, empty, label, stack, svg},
};
use std::rc::Rc;
use crate::widgets::sparrow_scroll;

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
            s.height(30.0)
                .items_center()
                .flex_shrink(0.0_f32)
                .border_bottom(1.0)
                .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
        }),
        stack((
            label(|| "Name")
                .style(|s| s.flex_grow(1.0_f32).flex_basis(0.0).padding_left(12.0)),
            label(|| "API").style(|s| s.width(44.0)),
            label(|| "Type").style(|s| s.width(52.0)),
            empty().style(|s| s.width(48.0)),
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
        sparrow_scroll(
            stack((
                dyn_stack(
                    move || devices.get(),
                    |d| (d.id.clone(), d.name.clone(), d.sdk.clone()),
                    move |device| {
                        let id = device.id.clone();
                        let copy = device.clone();
                        let icon =
                            device_platform_icon(device.target_platform.as_deref());
                        let sdk = android_api(device.sdk.as_deref())
                            .unwrap_or("—")
                            .to_string();
                        let detail =
                            [device.sdk.clone(), device.target_platform.clone()]
                                .into_iter()
                                .flatten()
                                .collect::<Vec<_>>()
                                .join(" | ");
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
                        } else if device.target_platform.is_some() {
                            "Physical"
                        } else {
                            "—"
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
                                label(move || detail.clone()).style(move |s| {
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
                            label(move || sdk.clone()).style(|s| {
                                s.width(44.0).flex_shrink(0.0_f32).text_ellipsis()
                            }),
                            label(move || kind).style(|s| s.width(52.0)),
                            empty().style(|s| s.width(48.0)),
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
                            label(|| "—").style(|s| s.width(44.0)),
                            label(|| "Virtual").style(|s| s.width(52.0)),
                            clickable_icon(
                                || SparrowIcons::START,
                                move || hr.launch_emulator(&id),
                                || false,
                                || false,
                                || "Launch Virtual Device",
                                config,
                            ).style(|s|s.width(48.0)),
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
    .style(|s| s.absolute().size_full().flex_col().font_size(13.0))
    .debug_name("Device Manager Table")
}

// Only show an API level explicitly supplied by the SDK, never infer it
// from an Android release or from an emulator's name.
fn android_api(sdk: Option<&str>) -> Option<&str> {
    let api = sdk?.split_once("API ")?.1;
    let end = api
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(api.len());
    (end > 0).then_some(&api[..end])
}
#[cfg(test)]
mod tests {
    use super::android_api;
    #[test]
    fn api_requires_explicit_metadata() {
        assert_eq!(android_api(Some("Android 16 (API 36)")), Some("36"));
        assert_eq!(android_api(Some("Android 17 (API 37.2)")), Some("37.2"));
        assert_eq!(android_api(Some("Fedora Linux 43")), None);
        assert_eq!(android_api(None), None);
    }
}
