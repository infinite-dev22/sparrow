//! Mobile Preview panel and device settings.
use std::rc::Rc;

use floem::{
    View,
    reactive::{
        SignalGet, SignalTrack, SignalUpdate, create_effect, create_rw_signal,
    },
    views::{Decorators, empty, label, stack},
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
    device::{DEVICES, DeviceChoice},
    screen,
    server::Mode,
    session::{App, Session},
};

pub fn panel(window: Rc<WindowTabData>) -> impl View {
    let config = window.common.config;
    let session = Session::new();
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
            session.start_server(flutter, root, mode);
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
    let cleanup = session.clone();
    let details = session.clone();
    let device_choices = DEVICES
        .iter()
        .map(|preset| DeviceChoice::Preset(preset.id))
        .chain(std::iter::once(DeviceChoice::Flexible))
        .collect::<Vec<_>>();
    let presets = dropdown_with_label(
        device,
        device_choices,
        config,
        |choice| {
            choice.device().map_or_else(
                || "Flexible device".into(),
                |preset| preset.name.into(),
            )
        },
        |_| {},
    );
    let settings = stack((
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
    stack((
        crate::panel::chrome::header(
            window,
            PanelKind::MobilePreview,
            "Mobile Preview",
            empty(),
        ),
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
                    button(|| "Run app", ButtonKind::Primary, config)
                        .on_click_stop(move |_| run(Mode::App))
                        .style(|s| s.padding(6.0)),
                    button(|| "Widget Previews", ButtonKind::Outlined, config)
                        .on_click_stop(move |_| widgets(Mode::Widgets))
                        .style(|s| s.padding(6.0)),
                    button(|| "Stop", ButtonKind::Plain, config)
                        .on_click_stop(move |_| stop.stop())
                        .style(|s| s.padding(6.0)),
                ))
                .style(|s| {
                    s.items_center()
                        .gap(6.0)
                        .flex_wrap(floem::style::FlexWrap::Wrap)
                }),
                settings,
                label(move || status.get()).style(move |s| {
                    s.width_full()
                        .min_width(0)
                        .selectable(true)
                        .font_size(11.0)
                        .apply_if(status.get().is_empty(), |s| s.hide())
                }),
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
