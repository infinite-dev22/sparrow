//! Extra dialogs for the Run/Debug Configurations window, modelled on the IDE:
//!
//! * **Environment Variables** – user table (+ − copy paste), "include system
//!   variables" checkbox and a read-only system table. Round-trips with the
//!   single-line `KEY=value; KEY2=value` field.
//! * **Add New Task** popup – Launch Web Browser, Run Another Configuration,
//!   Run External Tool, Run Gradle Task, Build Artifacts.
//! * **Launch Web Browser** dialog – browser dropdown + URL.
//!
//! They are plain overlays (absolute, full-size, painted over the dialog), so
//! they do not interfere with `ModalFocus` tab order. Escape closes them.

use super::Dialog;
use crate::{
    config::color::SparrowColor,
    dart::run_config::{self},
    widgets::{
        ButtonKind, button, dropdown_with_label, labeled_checkbox, sparrow_scroll,
        text_input,
    },
};
use floem::{
    IntoView, View,
    reactive::{
        RwSignal, Scope, SignalGet, SignalUpdate, SignalWith, create_effect,
    },
    views::{Decorators, dyn_stack, empty, label, stack},
};
use std::rc::Rc;

// ───────────────────────── pure logic ─────────────────────────

/// `"A=1; B=two words"` → `[("A","1"),("B","two words")]`.
pub(super) fn parse_env(text: &str) -> Vec<(String, String)> {
    text.split(';')
        .filter_map(|part| {
            let (name, value) = part.trim().split_once('=')?;
            let name = name.trim();
            (!name.is_empty()).then(|| (name.to_string(), value.trim().to_string()))
        })
        .collect()
}

/// Inverse of [`parse_env`]; rows without a name are dropped.
pub(super) fn format_env(rows: &[(String, String)]) -> String {
    rows.iter()
        .filter(|(name, _)| !name.trim().is_empty())
        .map(|(name, value)| format!("{}={}", name.trim(), value))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Clipboard text may be one `K=V` per line (what the IDE copies) or the
/// `;`-separated single-line form.
pub(super) fn parse_pasted(text: &str) -> Vec<(String, String)> {
    text.lines()
        .flat_map(|line| line.split(';'))
        .filter_map(|part| {
            let (name, value) = part.trim().split_once('=')?;
            let name = name.trim();
            (!name.is_empty()).then(|| (name.to_string(), value.trim().to_string()))
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Browser {
    Default,
    Chrome,
    Firefox,
    Edge,
    Safari,
}

impl Browser {
    pub(super) fn all() -> Vec<Browser> {
        let mut all = vec![
            Browser::Default,
            Browser::Chrome,
            Browser::Firefox,
            Browser::Edge,
        ];
        if cfg!(target_os = "macos") {
            all.push(Browser::Safari);
        }
        all
    }
    pub(super) fn title(self) -> &'static str {
        match self {
            Browser::Default => "Default",
            Browser::Chrome => "Chrome",
            Browser::Firefox => "Firefox",
            Browser::Edge => "Edge",
            Browser::Safari => "Safari",
        }
    }
}

fn with_scheme(url: &str) -> String {
    let url = url.trim();
    if url.contains("://") {
        url.to_string()
    } else {
        format!("http://{url}")
    }
}

/// Program + argument string for a before-launch task opening `url`.
pub(super) fn browser_command(
    browser: Browser,
    url: &str,
    os: &str,
) -> (String, String) {
    let url = with_scheme(url);
    match os {
        "macos" => match browser {
            Browser::Default => ("open".into(), url),
            Browser::Chrome => {
                ("open".into(), format!("-a \"Google Chrome\" {url}"))
            }
            Browser::Firefox => ("open".into(), format!("-a Firefox {url}")),
            Browser::Edge => ("open".into(), format!("-a \"Microsoft Edge\" {url}")),
            Browser::Safari => ("open".into(), format!("-a Safari {url}")),
        },
        "windows" => match browser {
            Browser::Default | Browser::Safari => (
                "rundll32".into(),
                format!("url.dll,FileProtocolHandler {url}"),
            ),
            Browser::Chrome => ("cmd".into(), format!("/C start chrome {url}")),
            Browser::Firefox => ("cmd".into(), format!("/C start firefox {url}")),
            Browser::Edge => ("cmd".into(), format!("/C start msedge {url}")),
        },
        _ => match browser {
            Browser::Default | Browser::Safari => ("xdg-open".into(), url),
            Browser::Chrome => ("google-chrome".into(), url),
            Browser::Firefox => ("firefox".into(), url),
            Browser::Edge => ("microsoft-edge".into(), url),
        },
    }
}

fn current_os() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(windows) {
        "windows"
    } else {
        "linux"
    }
}

// ───────────────────────── state ─────────────────────────

/// Page of the Add New Task popup.
pub(super) const ADD_CLOSED: u8 = 0;
pub(super) const ADD_MENU: u8 = 1;
pub(super) const ADD_CONFIGS: u8 = 2;
pub(super) const ADD_BROWSER: u8 = 3;

#[derive(Clone, Copy)]
pub(super) struct Popups {
    pub env: RwSignal<bool>,
    /// Whether the Add New Task popup is open; `add` is which page it shows.
    pub add_open: RwSignal<bool>,
    pub add: RwSignal<u8>,
}

impl Popups {
    pub(super) fn open_add(&self) {
        self.add.set(ADD_MENU);
        self.add_open.set(true);
    }
    pub(super) fn is_open(&self) -> bool {
        self.env.get_untracked() || self.add_open.get_untracked()
    }
    pub(super) fn close_all(&self) {
        self.env.set(false);
        self.add_open.set(false);
    }

    pub(super) fn new(cx: Scope) -> Self {
        Self {
            env: cx.create_rw_signal(false),
            add_open: cx.create_rw_signal(false),
            add: cx.create_rw_signal(ADD_CLOSED),
        }
    }
}

fn add_preset(d: &Rc<Dialog>, program: &str, args: &str) {
    d.edit_task(0);
    d.fields[6].set(program.into());
    d.fields[7].set(args.into());
    d.capture();
}

// ───────────────────────── shared chrome ─────────────────────────

/// Inner layout only: `floating_search` supplies the frame, shadow and radius.
fn panel_style(
    _config: floem::reactive::ReadSignal<
        std::sync::Arc<crate::config::SparrowConfig>,
    >,
    s: floem::style::Style,
) -> floem::style::Style {
    s.flex_col().padding(14.0).gap(10.0)
}

// ───────────────────────── Environment Variables ─────────────────────────

#[derive(Clone, Copy)]
struct Row {
    id: usize,
    name: RwSignal<String>,
    value: RwSignal<String>,
}

pub(super) fn env_dialog(d: Rc<Dialog>, popups: Popups, cx: Scope) -> impl View {
    let config = d.window.common.config;
    let open = popups.env;
    let rows = cx.create_rw_signal(Vec::<Row>::new());
    let next_id = cx.create_rw_signal(0usize);
    let selected = cx.create_rw_signal(None::<usize>);
    let include_system = cx.create_rw_signal(true);
    let system = cx.create_rw_signal(Vec::<(String, String)>::new());
    let field = d.fields[5];

    let add_row = move |name: String, value: String| {
        let id = next_id.get_untracked();
        next_id.set(id + 1);
        let row = Row {
            id,
            name: cx.create_rw_signal(name),
            value: cx.create_rw_signal(value),
        };
        rows.update(|rows| rows.push(row));
        selected.set(Some(id));
    };

    create_effect(move |_| {
        if !open.get() {
            return;
        }
        rows.set(Vec::new());
        selected.set(None);
        for (name, value) in parse_env(&field.get_untracked()) {
            add_row(name, value);
        }
        selected.set(None);
        let mut vars = std::env::vars().collect::<Vec<_>>();
        vars.sort_by_key(|(name, _)| name.to_lowercase());
        system.set(vars);
    });

    let snapshot = move || {
        rows.with_untracked(|rows| {
            rows.iter()
                .map(|row| (row.name.get_untracked(), row.value.get_untracked()))
                .collect::<Vec<_>>()
        })
    };

    let tool = move |text: &'static str,
                     enabled: Box<dyn Fn() -> bool>,
                     run: Box<dyn Fn()>| {
        label(move || text)
            .on_click_stop(move |_| {
                if enabled() {
                    run()
                }
            })
            .style(move |s| {
                let c = config.get();
                s.size(24.0, 22.0)
                    .items_center()
                    .justify_center()
                    .border_radius(4.0)
                    .hover(|s| {
                        s.background(c.color(SparrowColor::PANEL_HOVERED_BACKGROUND))
                    })
            })
    };

    let toolbar = stack((
        tool(
            "+",
            Box::new(|| true),
            Box::new(move || add_row(String::new(), String::new())),
        ),
        tool(
            "−",
            Box::new(move || selected.get_untracked().is_some()),
            Box::new(move || {
                if let Some(id) = selected.get_untracked() {
                    rows.update(|rows| rows.retain(|row| row.id != id));
                    selected.set(None);
                }
            }),
        ),
        tool(
            "⧉",
            Box::new(|| true),
            Box::new(move || {
                let text = snapshot()
                    .iter()
                    .filter(|(n, _)| !n.trim().is_empty())
                    .map(|(n, v)| format!("{n}={v}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                crate::menus::copy_text(&text);
            }),
        ),
        tool(
            "⎘",
            Box::new(|| true),
            Box::new(move || {
                if let Ok(text) = floem::Clipboard::get_contents() {
                    for (name, value) in parse_pasted(&text) {
                        add_row(name, value);
                    }
                }
            }),
        ),
    ))
    .style(|s| s.gap(2.0));

    let user_table =
        stack((
            stack((
                label(|| "Name").style(|s| s.width_pct(40.0)),
                label(|| "Value").style(|s| s.flex_grow(1.0_f32)),
            ))
            .style(move |s| {
                s.height(24.0)
                    .width_full()
                    .items_center()
                    .padding_horiz(8.0)
                    .border_bottom(1.0)
                    .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
                    .color(config.get().color(SparrowColor::EDITOR_DIM))
            }),
            sparrow_scroll(
                dyn_stack(
                    move || rows.get(),
                    |row| row.id,
                    move |row| {
                        stack((
                            text_input(row.name, config)
                                .placeholder("NAME")
                                .style(|s| s.width_pct(40.0)),
                            text_input(row.value, config)
                                .placeholder("value")
                                .style(|s| s.flex_grow(1.0_f32).min_width(0.0)),
                        ))
                        .on_click_cont(move |_| selected.set(Some(row.id)))
                        .style(move |s| {
                            s.width_full()
                                .items_center()
                                .gap(4.0)
                                .padding_horiz(4.0)
                                .padding_vert(1.0)
                                .apply_if(selected.get() == Some(row.id), |s| {
                                    s.background(config.get().color(
                                        SparrowColor::PANEL_HOVERED_BACKGROUND,
                                    ))
                                })
                        })
                    },
                )
                .style(|s| s.flex_col().width_full()),
            )
            .style(|s| s.width_full().flex_grow(1.0_f32).min_height(0.0)),
        ))
        .style(move |s| {
            s.flex_col()
                .width_full()
                .height(170.0)
                .border(1.0)
                .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
        });

    let system_table = stack((
        stack((
            label(|| "Name").style(|s| s.width_pct(40.0)),
            label(|| "Value").style(|s| s.flex_grow(1.0_f32)),
        ))
        .style(move |s| {
            s.height(24.0)
                .width_full()
                .items_center()
                .padding_horiz(8.0)
                .border_bottom(1.0)
                .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
                .color(config.get().color(SparrowColor::EDITOR_DIM))
        }),
        sparrow_scroll(
            dyn_stack(
                move || system.get().into_iter().enumerate().collect::<Vec<_>>(),
                |(i, (name, _))| (*i, name.clone()),
                move |(_, (name, value))| {
                    stack((
                        label(move || name.clone())
                            .style(|s| s.width_pct(40.0).text_ellipsis()),
                        label(move || value.clone()).style(|s| {
                            s.flex_grow(1.0_f32).min_width(0.0).text_ellipsis()
                        }),
                    ))
                    .style(move |s| {
                        s.width_full()
                            .height(22.0)
                            .items_center()
                            .padding_horiz(8.0)
                            .color(config.get().color(SparrowColor::EDITOR_DIM))
                    })
                },
            )
            .style(|s| s.flex_col().width_full()),
        )
        .style(|s| s.width_full().flex_grow(1.0_f32).min_height(0.0)),
    ))
    .style(move |s| {
        s.flex_col()
            .width_full()
            .height(170.0)
            .border(1.0)
            .border_color(config.get().color(SparrowColor::SPARROW_BORDER))
    });

    let ok = button(|| "OK", ButtonKind::Primary, config)
        .style(|s| s.min_width(72.0).height(24.0).padding_horiz(14.0))
        .on_click_stop(move |_| {
            field.set(format_env(&snapshot()));
            open.set(false);
        });
    let cancel = button(|| "Cancel", ButtonKind::Outlined, config)
        .style(|s| s.min_width(72.0).height(24.0).padding_horiz(14.0))
        .on_click_stop(move |_| open.set(false));

    let panel = stack((
        label(|| "Environment Variables")
            .style(|s| s.width_full().justify_center().font_bold()),
        stack((
            label(|| "User environment variables:"),
            empty().style(|s| s.flex_grow(1.0_f32)),
            toolbar,
        ))
        .style(|s| s.items_center().width_full()),
        user_table,
        labeled_checkbox(
            move || include_system.get(),
            move || "Include system environment variables",
            move |v| include_system.set(v),
            config,
        ),
        label(|| "System environment variables:")
            .style(move |s| s.apply_if(!include_system.get(), |s| s.hide())),
        system_table.style(move |s| s.apply_if(!include_system.get(), |s| s.hide())),
        stack((empty().style(|s| s.flex_grow(1.0_f32)), ok, cancel))
            .style(|s| s.gap(10.0).width_full().items_center()),
    ))
    .on_click_stop(|_| {})
    .style(move |s| panel_style(config, s).width_full());

    crate::widgets::floating_search::floating_search(
        panel,
        open,
        config,
        None,
        || true,
        |s| s.width(640.0),
    )
    .style(move |s| s.z_index(260).apply_if(!open.get(), |s| s.hide()))
}

// ───────────────────────── Add New Task ─────────────────────────

pub(super) fn add_task_popup(d: Rc<Dialog>, popups: Popups, cx: Scope) -> impl View {
    let config = d.window.common.config;
    let page = popups.add;
    let browser = cx.create_rw_signal(Browser::Default);
    let url = cx.create_rw_signal(String::from("http://localhost:8080"));
    let error = cx.create_rw_signal(String::new());
    let add_open = popups.add_open;
    let close = move || add_open.set(false);

    create_effect(move |_| {
        if page.get() == ADD_BROWSER {
            error.set(String::new());
        }
    });

    let item = move |glyph: &'static str,
                     title: String,
                     enabled: bool,
                     run: Rc<dyn Fn()>| {
        stack((
            label(move || glyph).style(move |s| {
                s.width(22.0)
                    .color(config.get().color(SparrowColor::EDITOR_LINK))
            }),
            label(move || title.clone()),
        ))
        .on_click_stop(move |_| {
            if enabled {
                run()
            }
        })
        .style(move |s| {
            let c = config.get();
            s.height(26.0)
                .width_full()
                .items_center()
                .padding_horiz(10.0)
                .border_radius(4.0)
                .apply_if(!enabled, |s| s.color(c.color(SparrowColor::EDITOR_DIM)))
                .hover(|s| {
                    s.background(c.color(SparrowColor::PANEL_HOVERED_BACKGROUND))
                })
        })
        .into_any()
    };

    let menu = {
        let d = d.clone();
        stack((
            label(|| "Add New Task").style(|s| {
                s.width_full()
                    .justify_center()
                    .font_bold()
                    .margin_bottom(4.0)
            }),
            item(
                "◎",
                "Launch Web Browser".into(),
                true,
                Rc::new(move || page.set(ADD_BROWSER)),
            ),
            item(
                "▷",
                "Run Another Configuration".into(),
                true,
                Rc::new(move || page.set(ADD_CONFIGS)),
            ),
            item("⚙", "Run External Tool".into(), true, {
                let d = d.clone();
                Rc::new(move || {
                    add_preset(&d, "", "");
                    close();
                })
            }),
            item("◆", "Run Gradle Task".into(), true, {
                let d = d.clone();
                Rc::new(move || {
                    add_preset(
                        &d,
                        if cfg!(windows) {
                            "android\\gradlew.bat"
                        } else {
                            "./android/gradlew"
                        },
                        "assembleDebug",
                    );
                    close();
                })
            }),
            item("▣", "Build Artifacts".into(), true, {
                let d = d.clone();
                Rc::new(move || {
                    add_preset(&d, "flutter", "build apk");
                    close();
                })
            }),
        ))
        .style(move |s| {
            panel_style(config, s)
                .gap(1.0)
                .padding(8.0)
                .width_full()
                .apply_if(page.get() != ADD_MENU, |s| s.hide())
        })
    };

    // Run Another Configuration → pick which one.
    let configs = {
        let d = d.clone();
        stack((
            label(|| "Run Another Configuration").style(|s| {
                s.width_full()
                    .justify_center()
                    .font_bold()
                    .margin_bottom(4.0)
            }),
            dyn_stack(
                {
                    let d = d.clone();
                    move || {
                        if page.get() != ADD_CONFIGS {
                            return Vec::new();
                        }
                        d.capture();
                        d.drafts
                            .get_untracked()
                            .iter()
                            .enumerate()
                            .filter_map(|(i, draft)| {
                                let mut config = draft.build().ok()?;
                                if let Some(root) =
                                    d.window.workspace.path.as_deref()
                                {
                                    config.cwd = Some(
                                        run_config::launch_directory(
                                            root,
                                            config.cwd.as_deref(),
                                        )
                                        .display()
                                        .to_string(),
                                    );
                                }
                                let args = run_config::join_args(&[
                                    "--run-config-task".into(),
                                    serde_json::to_string(&config).ok()?,
                                ]);
                                Some((i, config.name.clone(), args))
                            })
                            .collect::<Vec<_>>()
                    }
                },
                |(i, name, _)| (*i, name.clone()),
                {
                    let d = d.clone();
                    move |(_, name, args)| {
                        let d = d.clone();
                        item(
                            "▷",
                            name,
                            true,
                            Rc::new(move || {
                                add_preset(&d, "${sparrow}", &args);
                                close();
                            }),
                        )
                    }
                },
            )
            .style(|s| s.flex_col().width_full()),
            label(|| "No valid configurations to run.").style(move |s| {
                s.padding(10.0)
                    .color(config.get().color(SparrowColor::EDITOR_DIM))
                    .apply_if(d.drafts.with(|drafts| !drafts.is_empty()), |s| {
                        s.hide()
                    })
            }),
        ))
        .style(move |s| {
            panel_style(config, s)
                .gap(1.0)
                .padding(8.0)
                .width_full()
                .apply_if(page.get() != ADD_CONFIGS, |s| s.hide())
        })
    };

    // Launch Web Browser dialog.
    let browser_dialog = {
        let d = d.clone();
        let ok = button(|| "OK", ButtonKind::Primary, config)
            .style(|s| s.min_width(72.0).height(24.0).padding_horiz(14.0))
            .on_click_stop(move |_| {
                let text = url.get_untracked();
                if text.trim().is_empty() {
                    error.set("Enter a URL to open.".into());
                    return;
                }
                let chosen = browser.get_untracked();
                let (program, args) = browser_command(chosen, &text, current_os());
                add_preset(&d, &program, &args);
                close();
            });
        let cancel = button(|| "Cancel", ButtonKind::Outlined, config)
            .style(|s| s.min_width(72.0).height(24.0).padding_horiz(14.0))
            .on_click_stop(move |_| close());
        stack((
            label(|| "Launch Web Browser")
                .style(|s| s.width_full().justify_center().font_bold()),
            stack((
                label(|| "Browser:").style(|s| s.width(70.0)),
                dropdown_with_label(
                    browser,
                    Browser::all(),
                    config,
                    |browser| browser.title().to_string(),
                    |_| {},
                )
                .style(|s| s.min_width(140.0)),
            ))
            .style(|s| s.items_center().width_full()),
            stack((
                label(|| "URL:").style(|s| s.width(70.0)),
                text_input(url, config)
                    .placeholder("http://localhost:8080")
                    .style(|s| s.flex_grow(1.0_f32).min_width(0.0)),
            ))
            .style(|s| s.items_center().width_full()),
            label(move || error.get()).style(move |s| {
                s.color(config.get().color(SparrowColor::SPARROW_ERROR))
                    .apply_if(error.with(|e| e.is_empty()), |s| s.hide())
            }),
            stack((empty().style(|s| s.flex_grow(1.0_f32)), ok, cancel))
                .style(|s| s.gap(10.0).width_full().items_center()),
        ))
        .style(move |s| {
            panel_style(config, s).apply_if(page.get() != ADD_BROWSER, |s| s.hide())
        })
    };

    let content = stack((menu, configs, browser_dialog)).on_click_stop(|_| {});
    crate::widgets::floating_search::floating_search(
        content,
        popups.add_open,
        config,
        None,
        || true,
        move |s| {
            s.width(match page.get() {
                ADD_MENU => 300.0,
                ADD_CONFIGS => 320.0,
                _ => 460.0,
            })
        },
    )
    .style(move |s| {
        s.z_index(260)
            .apply_if(!popups.add_open.get(), |s| s.hide())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_round_trip() {
        let rows = parse_env("A=1; B=two words ; =skip; C=x=y;junk");
        assert_eq!(
            rows,
            vec![
                ("A".into(), "1".into()),
                ("B".into(), "two words".into()),
                ("C".into(), "x=y".into())
            ]
        );
        assert_eq!(format_env(&rows), "A=1; B=two words; C=x=y");
        assert_eq!(parse_env(&format_env(&rows)), rows);
        assert_eq!(format_env(&[("".into(), "v".into())]), "");
        assert!(parse_env("").is_empty());
    }

    #[test]
    fn paste_accepts_lines_and_semicolons() {
        let rows = parse_pasted("A=1\nB=2; C=3\r\n\nnoequals\n");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[2], ("C".into(), "3".into()));
    }

    #[test]
    fn browser_commands_per_os() {
        assert_eq!(
            browser_command(Browser::Default, "localhost:3000", "linux"),
            ("xdg-open".into(), "http://localhost:3000".into())
        );
        assert_eq!(
            browser_command(Browser::Chrome, "https://a.b", "macos").1,
            "-a \"Google Chrome\" https://a.b"
        );
        assert_eq!(
            browser_command(Browser::Edge, "x.y", "windows"),
            ("cmd".into(), "/C start msedge http://x.y".into())
        );
        assert_eq!(
            browser_command(Browser::Default, "https://a", "windows").0,
            "rundll32"
        );
    }
}
