//! Native tool windows backed by workspace diagnostics, local Git and adb.
use super::kind::PanelKind;
use crate::{
    app::clickable_icon,
    config::{color::SparrowColor, icon::SparrowIcons},
    window_tab::WindowTabData,
};
use floem::{
    View,
    menu::{Menu, MenuItem},
    reactive::{
        Scope, SignalGet, SignalUpdate, SignalWith, create_effect, create_memo,
        create_rw_signal,
    },
    views::{Decorators, dyn_stack, label, scroll, stack, text_input},
};
use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    rc::Rc,
    time::{Duration, Instant},
};

// Read stdout and stderr concurrently so a full pipe cannot block timeout/kill.
pub(super) fn output(
    program: &Path,
    args: &[&str],
    cwd: Option<&Path>,
) -> Result<String, String> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("{}: {e}", program.display()))?;
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let out = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = stdout.read_to_end(&mut b);
        b
    });
    let err = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = stderr.read_to_end(&mut b);
        b
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Ok(s),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(e.to_string());
            }
            _ => {}
        }
        if start.elapsed() > Duration::from_secs(5) {
            let _ = child.kill();
            let _ = child.wait();
            break Err("Command timed out".into());
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    let bytes = out.join().unwrap_or_default();
    let errors = err.join().unwrap_or_default();
    if status?.success() {
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    } else {
        Err(String::from_utf8_lossy(&errors).trim().to_string())
    }
}
pub(super) fn adb() -> PathBuf {
    for var in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Some(root) = std::env::var_os(var) {
            let p = PathBuf::from(root).join("platform-tools/adb");
            if p.is_file() {
                return p;
            }
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        let p = PathBuf::from(home).join("Android/Sdk/platform-tools/adb");
        if p.is_file() {
            return p;
        }
    }
    PathBuf::from("adb")
}

pub fn problems(window: Rc<WindowTabData>) -> impl View {
    super::problems_view::panel(window)
}

#[derive(Clone, Debug, PartialEq)]
struct Commit {
    hash: String,
    subject: String,
    author: String,
    date: String,
    refs: String,
}
fn commits(text: &str) -> Vec<Commit> {
    text.lines()
        .filter_map(|line| {
            let f = line.split('\x1f').collect::<Vec<_>>();
            (f.len() == 5).then(|| Commit {
                hash: f[0].into(),
                subject: f[1].into(),
                author: f[2].into(),
                date: f[3].into(),
                refs: f[4].into(),
            })
        })
        .collect()
}
#[derive(Clone, Default)]
struct History {
    commits: Vec<Commit>,
    branches: Vec<String>,
    error: String,
}
pub fn history(window: Rc<WindowTabData>) -> impl View {
    let config = window.common.config;
    let root = window.workspace.path.clone();
    let state = create_rw_signal(History::default());
    let refresh = create_rw_signal(0u64);
    let query = create_rw_signal(String::new());
    let branch = create_rw_signal(String::new());
    let selected = create_rw_signal(String::new());
    let detail = create_rw_signal(String::new());
    let request = create_rw_signal(0u64);
    create_effect(move |_| {
        let _ = refresh.get();
        let reference = branch.get();
        let root = root.clone();
        request.update(|n| *n += 1);
        let generation = request.get_untracked();
        let send = floem::ext_event::create_ext_action(Scope::current(), move |s| {
            if request.get_untracked() == generation {
                state.set(s);
            }
        });
        std::thread::spawn(move || {
            let Some(root) = root else {
                send(History {
                    error: "Open a local Git workspace to view history".into(),
                    ..Default::default()
                });
                return;
            };
            let mut args = vec![
                "log",
                "-200",
                "--date=format:%Y-%m-%d %H:%M",
                "--format=%H%x1f%s%x1f%an%x1f%ad%x1f%D",
            ];
            if reference.is_empty() {
                args.push("--all")
            } else {
                args.push("--end-of-options");
                args.push(&reference)
            }
            args.push("--");
            let result = output(Path::new("git"), &args, Some(&root));
            let branches = output(
                Path::new("git"),
                &[
                    "for-each-ref",
                    "--format=%(refname:short)",
                    "refs/heads",
                    "refs/remotes",
                ],
                Some(&root),
            )
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect();
            send(match result {
                Ok(t) => History {
                    commits: commits(&t),
                    branches,
                    error: String::new(),
                },
                Err(error) => History {
                    error,
                    ..Default::default()
                },
            });
        });
    });
    let root = window.workspace.path.clone();
    create_effect(move |_| {
        let hash = selected.get();
        if hash.is_empty() {
            detail.set("Select commit to view changes".into());
            return;
        }
        let root = root.clone();
        detail.set("Loading commit…".into());
        let send = floem::ext_event::create_ext_action(
            Scope::current(),
            move |(hash, text): (String, String)| {
                if selected.get_untracked() == hash {
                    detail.set(text);
                }
            },
        );
        std::thread::spawn(move || {
            let text = output(
                Path::new("git"),
                &[
                    "show",
                    "--stat",
                    "--format=fuller",
                    "--no-renames",
                    &hash,
                    "--",
                ],
                root.as_deref(),
            )
            .unwrap_or_else(|e| e);
            send((hash, text));
        });
    });
    let filtered = create_memo(move |_| {
        let q = query.get().to_lowercase();
        state
            .get()
            .commits
            .into_iter()
            .filter(|c| {
                format!("{} {} {} {}", c.subject, c.author, c.hash, c.refs)
                    .to_lowercase()
                    .contains(&q)
            })
            .collect::<Vec<_>>()
    });
    create_effect(move |_| {
        let rows = filtered.get();
        let current = selected.get_untracked();
        if !current.is_empty() && !rows.iter().any(|c| c.hash == current) {
            selected.set(String::new());
        }
    });
    stack((super::chrome::header(window,PanelKind::GitHistory,"Git",label(||"Log").style(move |s|s.padding_horiz(10.0).height(24.0).items_center().border_radius(3.0).background(config.get().color(SparrowColor::SPARROW_TAB_ACTIVE_BACKGROUND)))),
        stack((
            stack((text_input(branch).placeholder("Branch or tag (empty: all)").style(move |s|s.height(26.0).width_full().padding_horiz(8.0).border(1.0).border_radius(3.0).border_color(config.get().color(SparrowColor::SPARROW_BORDER))),label(||"HEAD (Current Branch)").on_click_stop(move |_|branch.set("HEAD".into())).style(|s|s.height(26.0).padding_left(18.0).items_center()),scroll(dyn_stack(move ||state.get().branches,|b|b.clone(),move |b|{let choose=b.clone();label(move ||b.clone()).on_click_stop(move |_|branch.set(choose.clone())).style(|s|s.height(24.0).padding_left(18.0).items_center())}).style(|s|s.flex_col().width_full())).style(|s|s.width_full().flex_grow(1.0_f32)))) .style(move |s|s.width_pct(31.0).height_full().flex_col().padding(6.0).border_right(1.0).border_color(config.get().color(SparrowColor::SPARROW_BORDER))),
            stack((stack((text_input(query).placeholder("Text, author or hash").style(move |s|s.width(280.0).height(26.0).padding_horiz(8.0).border(1.0).border_radius(3.0).border_color(config.get().color(SparrowColor::SPARROW_BORDER))),clickable_icon(||SparrowIcons::IMAGE_LOADING,move ||refresh.update(|n|*n+=1),||false,||false,||"Refresh Git history",config))).style(|s|s.height(32.0).items_center()),
                label(move ||state.get().error).style(move |s|s.color(config.get().color(SparrowColor::SPARROW_ERROR)).apply_if(state.with(|s|s.error.is_empty()),|s|s.hide())),
                scroll(dyn_stack(move ||filtered.get(),|c|c.hash.clone(),move |c|{let hash=c.hash.clone();let check=hash.clone();stack((label(move ||format!("●  {}",c.subject)).style(|s|s.flex_grow(1.0_f32).flex_basis(0.0).min_width(0.0).text_ellipsis()),label(move ||c.refs.clone()).style(|s|s.max_width_pct(24.0).text_ellipsis()),label(move ||c.author.clone()).style(|s|s.width(140.0).text_ellipsis()),label(move ||c.date.clone()).style(|s|s.width(170.0).text_ellipsis()))).on_click_stop(move |_|selected.set(hash.clone())).style(move |s|s.height(24.0).width_full().items_center().gap(8.0).padding_horiz(8.0).apply_if(selected.get()==check,|s|s.background(config.get().color(SparrowColor::SPARROW_TAB_ACTIVE_BACKGROUND)))) }).style(|s|s.flex_col().width_full())).style(|s|s.flex_grow(1.0_f32).min_height(0.0).width_full()),
            )).style(|s|s.width_pct(47.0).height_full().flex_col()),
            scroll(label(move ||detail.get()).style(move |s|s.width_full().padding(16.0).color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM)))).style(move |s|s.width_pct(22.0).height_full().border_left(1.0).border_color(config.get().color(SparrowColor::SPARROW_BORDER))),
        )).style(|s|s.width_full().flex_grow(1.0_f32).min_height(0.0)),
    )).style(|s|s.absolute().size_full().flex_col().font_size(13.0))
}

fn adb_devices(text: &str) -> Vec<String> {
    text.lines()
        .skip(1)
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            let id = f.next()?;
            (f.next() == Some("device")).then(|| id.to_string())
        })
        .collect()
}

#[derive(Clone, Default)]
struct Logs {
    devices: Vec<String>,
    text: String,
    error: String,
}
pub fn logcat(window: Rc<WindowTabData>) -> impl View {
    let config = window.common.config;
    let state = create_rw_signal(Logs::default());
    let device = create_rw_signal(String::new());
    let query = create_rw_signal(String::new());
    let paused = create_rw_signal(false);
    let tick = create_rw_signal(0u64);
    let epoch = create_rw_signal(0u64);
    let alive = Rc::new(std::cell::Cell::new(true));
    let busy = create_rw_signal(false);
    let panel = window.panel.clone();
    let live = alive.clone();
    create_effect(move |_| {
        let _ = tick.get();
        let serial = device.get();
        let paused_now = paused.get();
        let _ = panel.styles.get();
        let _ = panel.panels.get();
        if !panel.is_panel_visible(&PanelKind::Logcat)
            || paused_now
            || busy.get_untracked()
        {
            return;
        }
        busy.set(true);
        let generation = epoch.get_untracked();
        let expected_device = serial.clone();
        let live = live.clone();
        let send = floem::ext_event::create_ext_action(
            Scope::current(),
            move |result: Logs| {
                busy.set(false);
                if !live.get() {
                    return;
                }
                if generation == epoch.get_untracked()
                    && device.get_untracked() == expected_device
                    && !paused.get_untracked()
                {
                    state.set(result);
                }
                let live = live.clone();
                floem::action::exec_after(Duration::from_secs(2), move |_| {
                    if live.get() {
                        tick.update(|n| *n += 1);
                    }
                });
            },
        );
        std::thread::spawn(move || {
            let bin = adb();
            let result = output(&bin, &["devices"], None);
            let devices = match result {
                Ok(t) => adb_devices(&t),
                Err(error) => {
                    send(Logs {
                        error,
                        ..Default::default()
                    });
                    return;
                }
            };
            let text = if !serial.is_empty() && devices.contains(&serial) {
                output(
                    &bin,
                    &[
                        "-s",
                        &serial,
                        "logcat",
                        "-d",
                        "-t",
                        "500",
                        "-v",
                        "threadtime",
                    ],
                    None,
                )
            } else {
                Ok(String::new())
            };
            send(match text {
                Ok(text) => Logs {
                    devices,
                    text,
                    error: String::new(),
                },
                Err(error) => Logs {
                    devices,
                    error,
                    ..Default::default()
                },
            });
        });
    });
    let shown = create_memo(move |_| {
        let q = query.get().to_lowercase();
        state.with(|s| {
            s.text
                .lines()
                .filter(|l| l.to_lowercase().contains(&q))
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
    });
    stack((
        super::chrome::header(
            window,
            PanelKind::Logcat,
            "Logcat",
            label(|| "Logcat").style(move |s| {
                s.padding_horiz(12.0)
                    .height(24.0)
                    .items_center()
                    .border_radius(3.0)
                    .background(
                        config
                            .get()
                            .color(SparrowColor::SPARROW_TAB_ACTIVE_BACKGROUND),
                    )
            }),
        ),
        stack((
            label(move || {
                if device.with(|d| d.is_empty()) {
                    if state.with(|s| s.devices.is_empty()) {
                        "No connected devices".into()
                    } else {
                        "Select Android device".into()
                    }
                } else {
                    device.get()
                }
            })
            .popout_menu(move || {
                let mut menu = Menu::new("");
                for serial in state.get_untracked().devices {
                    let text = serial.clone();
                    menu = menu.entry(MenuItem::new(text).action(move || {
                        device.set(serial.clone());
                    }));
                }
                menu
            })
            .style(move |s| {
                s.width(396.0)
                    .height(32.0)
                    .items_center()
                    .padding_horiz(12.0)
                    .border(1.0)
                    .border_radius(4.0)
                    .border_color(
                        config.get().color(SparrowColor::SPARROW_TAB_ACTIVE_BORDER),
                    )
                    .apply_if(state.with(|s| s.devices.is_empty()), |s| {
                        s.color(config.get().color(SparrowColor::SPARROW_ERROR))
                    })
            }),
            text_input(query)
                .placeholder("Filter logs (text)")
                .style(move |s| {
                    s.height(30.0)
                        .flex_grow(1.0_f32)
                        .min_width(0.0)
                        .padding_horiz(8.0)
                        .border(1.0)
                        .border_color(
                            config.get().color(SparrowColor::SPARROW_BORDER),
                        )
                }),
        ))
        .style(|s| {
            s.width_full()
                .height(48.0)
                .items_center()
                .gap(12.0)
                .padding_horiz(8.0)
        }),
        stack((
            stack((
                clickable_icon(
                    || SparrowIcons::CLOSE,
                    move || {
                        epoch.update(|n| *n += 1);
                        state.update(|s| s.text.clear());
                        paused.set(true);
                    },
                    || false,
                    || false,
                    || "Clear displayed logs and pause",
                    config,
                ),
                label(move || if paused.get() { "▶" } else { "Ⅱ" })
                    .on_click_stop(move |_| paused.update(|v| *v = !*v))
                    .style(|s| s.size(26.0, 26.0).items_center().justify_center()),
                clickable_icon(
                    || SparrowIcons::IMAGE_LOADING,
                    move || {
                        paused.set(false);
                        tick.update(|n| *n += 1);
                    },
                    || false,
                    move || busy.get(),
                    || "Refresh Logcat",
                    config,
                ),
            ))
            .style(|s| s.width(32.0).height_full().flex_col().items_center()),
            scroll(
                stack((
                    label(move || state.get().error).style(move |s| {
                        s.color(config.get().color(SparrowColor::SPARROW_ERROR))
                            .apply_if(state.with(|s| s.error.is_empty()), |s| {
                                s.hide()
                            })
                    }),
                    dyn_stack(
                        move || {
                            shown.get().into_iter().enumerate().collect::<Vec<_>>()
                        },
                        |(i, l)| (*i, l.clone()),
                        move |(_, line)| {
                            label(move || line.clone()).style(move |s| {
                                s.height(20.0).text_ellipsis().font_family(
                                    config.get().terminal.font_family.clone(),
                                )
                            })
                        },
                    )
                    .style(|s| s.flex_col().width_full()),
                ))
                .style(|s| s.flex_col().width_full()),
            )
            .style(|s| s.flex_grow(1.0_f32).min_width(0.0).height_full()),
        ))
        .style(|s| s.width_full().flex_grow(1.0_f32).min_height(0.0)),
    ))
    .on_cleanup(move || alive.set(false))
    .style(|s| s.absolute().size_full().flex_col().font_size(13.0))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn excludes_unavailable_adb_devices() {
        assert_eq!(
            adb_devices(
                "List of devices attached\nserial1\tdevice\nserial2\toffline\nserial3\tunauthorized\n"
            ),
            vec!["serial1"]
        );
        assert!(adb_devices("List of devices attached\n\n").is_empty());
    }
    #[test]
    fn parses_real_git_fields() {
        let c =
            commits("abc\x1fA subject\x1fAuthor\x1f2026-11-15\x1fHEAD -> main\n");
        assert_eq!(c[0].author, "Author");
        assert_eq!(c[0].refs, "HEAD -> main");
        assert!(commits("malformed").is_empty());
    }
}
