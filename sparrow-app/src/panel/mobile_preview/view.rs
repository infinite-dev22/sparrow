//! Flutter app and widget preview panel.
use std::rc::Rc;

use floem::{
    View,
    reactive::{
        SignalGet, SignalTrack, SignalUpdate, create_effect, create_rw_signal,
    },
    views::{
        Decorators,
        editor::{core::register::Clipboard, text::SystemClipboard},
        empty, label, stack,
    },
};

use crate::{
    config::color::SparrowColor,
    panel::kind::PanelKind,
    widgets::{
        ButtonKind, button, dropdown_with_label, sparrow_scroll, text_input,
        toggle_row,
    },
    window_tab::WindowTabData,
};

use super::{
    device::{DeviceCategory, DeviceChoice, DevicePlatform, PreviewLocale, devices},
    screen,
    server::Mode,
    session::{App, Session},
};

fn save_preview_preference(key: &str, value: bool) -> bool {
    crate::config::SparrowConfig::update_file("mobile-preview", key, value.into())
        .is_some()
}

pub fn panel(window: Rc<WindowTabData>) -> impl View {
    let config = window.common.config;
    let auto_reload_on_save =
        create_rw_signal(config.get().mobile_preview.auto_reload_on_save);
    let current_auto_reload = auto_reload_on_save;
    create_effect(move |_| {
        current_auto_reload.set(config.get().mobile_preview.auto_reload_on_save);
    });
    let notice_visible = create_rw_signal(
        !config.get().mobile_preview.approximation_notice_dismissed,
    );
    let current_notice = notice_visible;
    create_effect(move |_| {
        current_notice
            .set(!config.get().mobile_preview.approximation_notice_dismissed);
    });
    let session = Session::new(auto_reload_on_save.get_untracked());
    let active_preview = session.active;
    let panel = window.panel.clone();
    let visible = session.panel_visible;
    create_effect(move |_| {
        panel.styles.track();
        visible.set(panel.is_panel_visible(&PanelKind::MobilePreview));
    });
    let device = session.device;
    let options = create_rw_signal(false);
    let url = create_rw_signal(String::new());
    let width = create_rw_signal(320.0_f64);
    let root = window
        .workspace
        .path
        .clone()
        .filter(|_| window.workspace.kind.is_local());
    let auto_reload = auto_reload_on_save;
    let run = {
        let session = session.clone();
        move |mode| {
            let Some(root) = root.clone() else {
                session
                    .status
                    .set("Open a local Flutter project to start a preview.".into());
                return;
            };
            let Some(flutter) = crate::dart::sdk::find_flutter_binary(Some(&root))
            else {
                session
                    .status
                    .set("Configure a Flutter SDK to start previews.".into());
                return;
            };
            session.start_server(flutter, root, mode, auto_reload.get_untracked());
        }
    };
    let widgets = run.clone();
    let open = {
        let session = session.clone();
        move || {
            session.open(App {
                url: url.get_untracked().trim().into(),
                widgets: false,
            })
        }
    };
    let open_url = open.clone();
    let stop = session.clone();
    let hot_reload = session.clone();
    let hot_restart = session.clone();
    let cleanup = session.clone();
    let details = session.clone();
    let can_reload = session.can_reload;
    let mut device_choices = Vec::new();
    for category in [
        DeviceCategory::Phone,
        DeviceCategory::Tablet,
        DeviceCategory::Generic,
    ] {
        for platform in [
            DevicePlatform::Android,
            DevicePlatform::Ios,
            DevicePlatform::IpadOs,
            DevicePlatform::Generic,
        ] {
            device_choices.extend(
                devices()
                    .iter()
                    .filter(|device| {
                        device.category == category && device.platform == platform
                    })
                    .map(|device| DeviceChoice::Preset(device.id.clone())),
            );
        }
    }
    device_choices.push(DeviceChoice::Flexible);
    let presets = dropdown_with_label(
        device,
        device_choices,
        config,
        |choice| {
            choice.device().map_or_else(
                || "Flexible device".into(),
                |preset| {
                    let group = match (preset.category, preset.platform) {
                        (DeviceCategory::Phone, DevicePlatform::Android) => {
                            "Android phone (approx.)"
                        }
                        (DeviceCategory::Phone, DevicePlatform::Ios) => "iPhone",
                        (DeviceCategory::Phone, DevicePlatform::IpadOs) => "iPad",
                        (DeviceCategory::Tablet, DevicePlatform::Android) => {
                            "Android tablet (approx.)"
                        }
                        (DeviceCategory::Tablet, DevicePlatform::IpadOs) => "iPad",
                        (DeviceCategory::Generic, _) => "Generic",
                        _ => "Device",
                    };
                    format!("{group} · {}", preset.name)
                },
            )
        },
        |_| {},
    );
    let settings = stack((
        {
            let session = session.clone();
            let active_preview = active_preview;
            toggle_row(
                "Auto reload on save",
                Some("Hot reload after Dart source files change"),
                auto_reload_on_save,
                move |enabled| {
                    if !save_preview_preference("auto-reload-on-save", enabled) {
                        session.status.set(
                            "Could not save the automatic reload preference.".into(),
                        );
                    }
                    session.set_auto_reload(enabled);
                },
                config,
            )
            .style(move |s| {
                s.apply_if(
                    active_preview
                        .get()
                        .is_some_and(|preview| preview.widgets),
                    |s| s.hide(),
                )
            })
        },
        toggle_row(
            "Swipe navigation",
            Some("Turn off to use Back, Home and Recents buttons"),
            session.gestures,
            |_| {},
            config,
        ),
        toggle_row(
            "Keep status bar visible",
            None,
            session.status_sticky,
            |_| {},
            config,
        ),
        toggle_row(
            "Keep navigation bar visible",
            None,
            session.navigation_sticky,
            |_| {},
            config,
        ),
        dropdown_with_label(
            session.locale,
            PreviewLocale::ALL.to_vec(),
            config,
            |locale| format!("Locale · {locale}"),
            |_| {},
        ),
        label(move || {
            device.get().device().map_or_else(
                || "Flexible viewport dimensions are user-defined.".into(),
                |profile| {
                    format!(
                        "{} source ({}): {}. Frame estimate ({}): {}",
                        profile.name,
                        profile.source_date,
                        profile.source_url,
                        profile.frame_source_date,
                        profile.frame_source
                    )
                },
            )
        })
        .style(|s| s.width_full().min_width(0).font_size(10.0).selectable(true)),
        toggle_row("Device dark mode", None, session.dark, |_| {}, config),
        stack((
            text_input(url, config)
                .placeholder("Flutter Web URL")
                .on_enter(move || open_url())
                .style(|s| s.min_width(0).flex_basis(0).flex_grow(1.0_f32)),
            button(|| "Open", ButtonKind::Outlined, config)
                .on_click_stop(move |_| open())
                .style(|s| s.padding_horiz(8.0)),
        ))
        .style(|s| s.items_center().gap(6.0)),
        {
            let limitations_open = create_rw_signal(false);
            stack((
                button(
                    move || {
                        if limitations_open.get() {
                            "Hide preview limitations"
                        } else {
                            "Preview limitations"
                        }
                    },
                    ButtonKind::Plain,
                    config,
                )
                .on_click_stop(move |_| {
                    limitations_open.update(|open| *open = !*open);
                }),
                label(|| {
                    "This is not the native iOS/Android runtime. Plugins without web support \
                     do not work; fonts and platform-specific rendering can differ; camera, \
                     sensors, biometrics and native permission prompts are unavailable. Pinch \
                     gestures, system font-scale emulation, and saving screenshot files are \
                     unsupported. Safe-area insets and system bars are approximated overlays; \
                     use a real device for native verification."
                })
                .style(move |s| {
                    s.apply_if(!limitations_open.get(), |s| s.hide())
                        .font_size(11.0)
                        .color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
                }),
            ))
            .style(|s| s.flex_col().width_full())
        },
    ))
    .style(move |s| {
        s.flex_col()
            .gap(5.0)
            .width_full()
            .apply_if(!options.get(), |s| s.hide())
    });
    let custom = stack((
        text_input(session.custom_width, config)
            .placeholder("Width")
            .style(|s| s.min_width(0).flex_basis(0).flex_grow(1.0_f32)),
        text_input(session.custom_height, config)
            .placeholder("Height")
            .style(|s| s.min_width(0).flex_basis(0).flex_grow(1.0_f32)),
        text_input(session.custom_scale, config)
            .placeholder("Scale")
            .style(|s| s.min_width(0).flex_basis(0).flex_grow(1.0_f32)),
    ))
    .style(move |s| {
        s.width_full()
            .gap(5.0)
            .apply_if(!device.get().is_flexible(), |s| s.hide())
    });
    let landscape = session.landscape;
    let folded = session.folded;
    let status = session.status;
    let log_entries = session.logs;
    let diagnostics = session.clone();
    let recent_log = label(move || {
        let mut entries = log_entries
            .get()
            .into_iter()
            .rev()
            .take(6)
            .collect::<Vec<_>>();
        entries.reverse();
        entries.join("\n")
    })
    .style(move |s| {
        s.width_full()
            .min_width(0)
            .selectable(true)
            .font_size(11.0)
            .apply_if(log_entries.get().is_empty(), |s| s.hide())
    });
    let diagnostic_button =
        button(|| "Copy diagnostic", ButtonKind::Outlined, config)
            .on_click_stop(move |_| {
                let report = diagnostics.diagnostic_report();
                SystemClipboard::new().put_string(&report);
            })
            .style(|s| s.padding(5.0));
    let notice_session = session.clone();
    let notice = stack((
        label(|| {
            "Approximate Flutter web preview; not a replacement for testing on a device."
        })
        .style(|s| s.min_width(0).flex_grow(1.0_f32).font_size(11.0)),
        button(|| "Dismiss", ButtonKind::Plain, config)
            .on_click_stop(move |_| {
                notice_visible.set(false);
                if !save_preview_preference("approximation-notice-dismissed", true) {
                    notice_session
                        .status
                        .set("Could not save the preview notice preference.".into());
                }
            })
            .style(|s| s.padding_horiz(4.0)),
    ))
    .style(move |s| {
        s.width_full()
            .items_center()
            .apply_if(!notice_visible.get(), |s| s.hide())
    });
    stack((
        crate::panel::chrome::header(
            window,
            PanelKind::MobilePreview,
            "Flutter Preview",
            empty(),
        ),
        notice,
        sparrow_scroll(
            stack((
                stack((
                    presets.style(|s| s.min_width(0).flex_grow(1.0_f32)),
                    button(|| "Rotate", ButtonKind::Plain, config).on_click_stop(
                        move |_| landscape.update(|rotated| *rotated = !*rotated),
                    ),
                    button(|| "Options", ButtonKind::Plain, config).on_click_stop(
                        move |_| options.update(|open| *open = !*open),
                    ),
                ))
                .style(|s| s.items_center().gap(7.0).width_full()),
                custom,
                stack((
                    label(move || {
                        let Some(d) = details.display() else {
                            return "Width 200–2000 · Height 200–2500 · Scale 0.5–4"
                                .into();
                        };
                        let (w, h) = if landscape.get() {
                            (d.logical.1, d.logical.0)
                        } else {
                            d.logical
                        };
                        if device.get().is_flexible() {
                            format!("{w} × {h} logical · {:.2}×", d.scale)
                        } else {
                            format!(
                                "{} × {} panel · {w} × {h} {} · {:.2}×",
                                d.pixels.0,
                                d.pixels.1,
                                if d.derived {
                                    "calculated dp"
                                } else {
                                    "logical"
                                },
                                d.scale
                            )
                        }
                    })
                    .style(move |s| {
                        s.min_width(0).flex_grow(1.0_f32).font_size(11.0).color(
                            config.get().color(SparrowColor::PANEL_FOREGROUND_DIM),
                        )
                    }),
                    button(
                        move || if folded.get() { "Unfold" } else { "Fold" },
                        ButtonKind::Outlined,
                        config,
                    )
                    .on_click_stop(move |_| folded.update(|value| *value = !*value))
                    .style(move |s| {
                        s.padding_horiz(8.0).apply_if(
                            !device
                                .get()
                                .device()
                                .is_some_and(|preset| preset.cover.is_some()),
                            |s| s.hide(),
                        )
                    }),
                ))
                .style(|s| s.items_center().gap(5.0).width_full()),
                stack((
                    button(|| "Run app", ButtonKind::Outlined, config)
                        .on_click_stop(move |_| run(Mode::App))
                        .style(|s| s.padding(6.0)),
                    button(|| "Widget Previewer", ButtonKind::Primary, config)
                        .on_click_stop(move |_| widgets(Mode::Widgets))
                        .style(|s| s.padding(6.0)),
                    button(|| "Hot reload", ButtonKind::Outlined, config)
                        .on_click_stop(move |_| hot_reload.hot_reload())
                        .style(move |s| s.padding(6.0).apply_if(!can_reload.get(), |s| s.hide())),
                    button(|| "Hot restart", ButtonKind::Outlined, config)
                        .on_click_stop(move |_| hot_restart.hot_restart())
                        .style(move |s| s.padding(6.0).apply_if(!can_reload.get(), |s| s.hide())),
                    button(|| "Stop", ButtonKind::Plain, config)
                        .on_click_stop(move |_| stop.stop())
                        .style(|s| s.padding(6.0)),
                ))
                .style(|s| {
                    s.items_center()
                        .gap(6.0)
                        .flex_wrap(floem::style::FlexWrap::Wrap)
                }),
                label(|| {
                    "Flutter's Widget Previewer renders @Preview-annotated widgets. Device \
                     controls emulate the browser viewport; annotations can set their own \
                     size, theme, brightness and localizations. Sparrow's reload buttons and \
                     save watcher apply to Run app mode only."
                })
                .style(move |s| {
                    s.width_full()
                        .min_width(0)
                        .font_size(11.0)
                        .color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
                        .apply_if(
                            !active_preview
                                .get()
                                .is_some_and(|preview| preview.widgets),
                            |s| s.hide(),
                        )
                }),
                settings,
                label(move || status.get()).style(move |s| {
                    s.width_full()
                        .min_width(0)
                        .selectable(true)
                        .font_size(11.0)
                        .apply_if(status.get().is_empty(), |s| s.hide())
                }),
                recent_log,
                stack((
                    label(|| {
                        "Copied diagnostics omit project paths, URLs, source code and raw Flutter output."
                    })
                    .style(|s| s.min_width(0).flex_grow(1.0_f32).font_size(10.0)),
                    diagnostic_button,
                ))
                .style(|s| s.width_full().items_center().gap(6.0)),
                screen::phone(session, width, config),
            ))
            .style(|s| {
                s.flex_col()
                    .items_center()
                    .gap(8.0)
                    .padding(10.0)
                    .width_full()
            }),
        )
        .style(|s| {
            s.width_full()
                .flex_basis(0)
                .flex_grow(1.0_f32)
                .min_height(0)
        }),
    ))
    .style(|s| s.absolute().size_full().flex_col())
    .on_resize(move |rect| {
        let w = (rect.width() - 28.0).max(100.0);
        if w != width.get_untracked() {
            width.set(w);
        }
    })
    .on_cleanup(move || cleanup.shutdown())
}
