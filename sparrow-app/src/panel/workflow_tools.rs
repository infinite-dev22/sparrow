//! Dart/Flutter workflow surfaces. All cards and rows represent observed data.
use crate::popup_menu::PopupMenuExt;
use crate::popup_menu::{Menu, MenuItem};
use super::{kind::PanelKind, reference_tools::output};
use crate::{
    app::clickable_icon,
    config::{color::SparrowColor, icon::SparrowIcons},
    dart::hot_reload::FlutterRunState,
    window_tab::WindowTabData,
};
use floem::{
    View,
    reactive::{
        RwSignal, Scope, SignalGet, SignalUpdate, SignalWith, create_effect,
        create_memo, create_rw_signal,
    },
    views::{Decorators, dyn_stack, empty, label, scroll, stack, svg, text_input},
};
use std::{
    cell::RefCell,
    path::Path,
    process::{Child, Command, Stdio},
    rc::Rc,
    time::Duration,
};
#[derive(Clone, Debug)]
pub struct Notice {
    pub id: u64,
    pub resolution_id: Option<u64>,
    pub time: String,
    pub title: String,
    pub body: String,
}
pub fn record(notices: RwSignal<Vec<Notice>>, title: &str, body: &str) {
    notices.update(|items| {
        let id = items.last().map_or(1, |n| n.id + 1);
        items.push(Notice {
            id,
            resolution_id: None,
            time: chrono::Local::now().format("%H:%M").to_string(),
            title: title.into(),
            body: body.into(),
        });
        if items.len() > 200 {
            items.remove(0);
        }
    });
}
/// One completion per process, even if automatic and manual requests joined it.
pub fn record_pub_get_result(notices:RwSignal<Vec<Notice>>,id:u64,result:Result<(),String>){
    notices.update(|items|{
        if items.iter().any(|notice|notice.resolution_id==Some(id)){return;}
        let next=items.last().map_or(1,|n|n.id+1);
        items.push(Notice{id:next,resolution_id:Some(id),time:chrono::Local::now().format("%H:%M").to_string(),title:"Pub Get".into(),body:match result{Ok(())=>"Package dependencies resolved.".into(),Err(error)=>error}});
        if items.len()>200 {items.remove(0);}
    });
}
pub fn notifications(window: Rc<WindowTabData>) -> impl View {
    let config=window.common.config;
    let notices=window.notifications;
    let expanded=create_rw_signal(std::collections::HashSet::<u64>::new());
    let open_groups=create_rw_signal(std::collections::HashSet::<String>::new());
    let message=window.flutter_hot_reload.message;
    create_effect(move |last:Option<Option<String>>|{
        let current=message.get();
        if current.is_some()&&last.as_ref()!=Some(&current){record(notices,"Flutter",current.as_deref().unwrap());} current
    });
    let groups=create_memo(move |_|{
        let mut groups=Vec::<String>::new();
        for notice in notices.get().iter().rev(){if !groups.contains(&notice.title){groups.push(notice.title.clone());}}
        groups
    });
    stack((
        super::chrome::header(window,PanelKind::Notifications,"Notifications",empty()),
        stack((label(||"Timeline").style(move |s|s.color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))),
            empty().style(|s|s.flex_grow(1.0_f32)),
            label(||"Clear all").on_click_stop(move |_|{notices.set(Vec::new());expanded.set(Default::default());open_groups.set(Default::default());})
                .style(move |s|s.color(config.get().color(SparrowColor::EDITOR_LINK)))
        )).style(|s|s.height(30.0).items_center().padding_horiz(16.0).width_full()),
        label(||"No notifications").style(move |s|s.padding(24.0).apply_if(!notices.with(|n|n.is_empty()),|s|s.hide())),
        scroll(dyn_stack(move ||groups.get(),|title|title.clone(),move |title|{
            let count_title=title.clone();let toggle_title=title.clone();let dismiss_title=title.clone();let rows_title=title.clone();
            stack((
                stack((label(move ||{
                    let count=notices.with(|items|items.iter().filter(|n|n.title==count_title).count());
                    format!("{} {}  ({count})",if open_groups.with(|g|g.contains(&count_title)){"⌄"}else{"›"},count_title)
                }).style(|s|s.flex_grow(1.0_f32).font_weight(floem::text::Weight(600))),
                    clickable_icon(||SparrowIcons::CLOSE,move ||notices.update(|items|items.retain(|n|n.title!=dismiss_title)),||false,||false,||"Dismiss group",config)
                )).on_click_stop(move |_|open_groups.update(|groups|{if !groups.remove(&toggle_title){groups.insert(toggle_title.clone());}}))
                  .style(|s|s.height(28.0).width_full().items_center().padding_horiz(6.0)),
                dyn_stack(move ||{
                    let all=open_groups.with(|groups|groups.contains(&rows_title));
                    notices.with(|items|items.iter().rev().filter(|n|n.title==rows_title).take(if all{usize::MAX}else{1}).cloned().collect::<Vec<_>>())
                },|notice|notice.id,move |notice|{
                    let id=notice.id;let body=notice.body.clone();let long=body.chars().count()>360||body.lines().count()>5;
                    stack((
                        stack((label(move ||notice.time.clone()).style(move |s|s.flex_grow(1.0_f32).font_size(11.0).color(config.get().color(SparrowColor::EDITOR_DIM))),
                            clickable_icon(||SparrowIcons::CLOSE,move ||notices.update(|items|items.retain(|n|n.id!=id)),||false,||false,||"Dismiss notification",config)
                        )).style(|s|s.items_center().width_full()),
                        label(move ||if long&&!expanded.with(|ids|ids.contains(&id)){
                            let text=body.lines().take(5).collect::<Vec<_>>().join("\n");
                            format!("{}…",text.chars().take(360).collect::<String>())
                        }else{body.clone()}).style(|s|s.width_full().min_width(0.0).margin_top(4.0)),
                        label(move ||if expanded.with(|ids|ids.contains(&id)){"Show less"}else{"Show more"})
                            .on_click_stop(move |_|expanded.update(|ids|{if !ids.remove(&id){ids.insert(id);}}))
                            .style(move |s|s.margin_top(6.0).color(config.get().color(SparrowColor::EDITOR_LINK)).apply_if(!long,|s|s.hide()))
                    )).style(move |s|s.flex_col().width_full().padding(10.0).margin_bottom(6.0).border_radius(5.0)
                        .background(config.get().color(SparrowColor::PANEL_HOVERED_BACKGROUND).with_alpha(0.35)))
                }).style(|s|s.flex_col().width_full()),
            )).style(|s|s.flex_col().width_full().margin_bottom(10.0))
        }).style(|s|s.flex_col().width_full().padding_horiz(10.0))).style(|s|s.flex_grow(1.0_f32).min_height(0.0).width_full())
    )).style(|s|s.absolute().size_full().flex_col().font_size(13.0))
}

fn github_repository(remote: &str) -> Option<String> {
    let path = if let Some(p) = remote.trim().strip_prefix("git@github.com:") {
        p.to_string()
    } else {
        let u = url::Url::parse(remote.trim()).ok()?;
        if u.scheme() != "https" || u.host_str() != Some("github.com") {
            return None;
        }
        u.path().trim_start_matches('/').to_string()
    };
    let path = path.trim_end_matches('/').trim_end_matches(".git");
    let parts = path.split('/').collect::<Vec<_>>();
    (parts.len() == 2
        && parts.iter().all(|p| {
            !p.is_empty()
                && *p != "."
                && *p != ".."
                && p.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
        }))
    .then(|| path.to_string())
}
#[derive(Clone, PartialEq, serde::Deserialize)]
struct User {
    login: String,
}
#[derive(Clone, PartialEq, serde::Deserialize)]
struct Tag {
    name: String,
}
#[derive(Clone, PartialEq, serde::Deserialize)]
struct Pull {
    number: u64,
    title: String,
    html_url: String,
    user: User,
    labels: Vec<Tag>,
    state: String,
}
#[derive(Clone, Default)]
struct Pulls {
    repository: String,
    rows: Vec<Pull>,
    error: String,
}
fn load_pulls(root: &Path, state: &str) -> Pulls {
    let remote = match output(
        Path::new("git"),
        &["remote", "get-url", "origin"],
        Some(root),
    ) {
        Ok(r) => r,
        Err(e) => {
            return Pulls {
                error: e,
                ..Default::default()
            };
        }
    };
    let Some(repository) = github_repository(&remote) else {
        return Pulls {
            error: "Pull Requests currently supports GitHub remotes only.".into(),
            ..Default::default()
        };
    };
    let fetch = || -> Result<Vec<Pull>, String> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(12))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("Sparrow-Dart-Flutter-IDE")
            .build()
            .map_err(|e| e.to_string())?;
        let url = format!("https://api.github.com/repos/{repository}/pulls");
        let mut request = client
            .get(url)
            .query(&[("state", state), ("per_page", "100")])
            .header("Accept", "application/vnd.github+json");
        if let Ok(token) =
            std::env::var("GITHUB_TOKEN").or_else(|_| std::env::var("GH_TOKEN"))
        {
            request = request.bearer_auth(token);
        }
        let response = request
            .send()
            .map_err(|e| format!("Unable to reach GitHub: {}", e.without_url()))?;
        if !response.status().is_success() {
            return Err(format!(
                "GitHub returned {}. Check repository access or API rate limits. Private repositories require GITHUB_TOKEN or GH_TOKEN in Sparrow's environment.",
                response.status()
            ));
        }
        response.json().map_err(|_| {
            "GitHub returned an unreadable pull-request response.".into()
        })
    };
    match fetch() {
        Ok(rows) => Pulls {
            repository,
            rows,
            error: String::new(),
        },
        Err(error) => Pulls {
            repository,
            error,
            ..Default::default()
        },
    }
}
pub fn pull_requests(window: Rc<WindowTabData>) -> impl View {
    let config = window.common.config;
    let notices = window.notifications;
    let root = window.workspace.path.clone();
    let data = create_rw_signal(Pulls::default());
    let query = create_rw_signal(String::new());
    let author = create_rw_signal(String::new());
    let tag = create_rw_signal(String::new());
    let state = create_rw_signal("open".to_string());
    let refresh = create_rw_signal(0u64);
    let generation = create_rw_signal(0u64);
    let loading = create_rw_signal(false);
    let panel = window.panel.clone();
    let visible = create_memo(move |_| {
        let _ = panel.styles.get();
        let _ = panel.panels.get();
        panel.is_panel_visible(&PanelKind::PullRequests)
    });
    create_effect(move |_| {
        let _ = refresh.get();
        let state = state.get();
        if !visible.get() {
            return;
        }
        generation.update(|n| *n += 1);
        let request = generation.get_untracked();
        loading.set(true);
        let root = root.clone();
        let send = floem::ext_event::create_ext_action(
            Scope::current(),
            move |result: Pulls| {
                if generation.get_untracked() == request {
                    loading.set(false);
                    if !result.error.is_empty() {
                        record(notices, "Pull Requests", &result.error);
                    }
                    data.set(result);
                }
            },
        );
        std::thread::spawn(move || {
            send(root.as_deref().map(|r|load_pulls(r,&state)).unwrap_or_else(||Pulls{error:"Open a local Dart or Flutter project with a GitHub remote.".into(),..Default::default()}));
        });
    });
    let filtered = create_memo(move |_| {
        let q = query.get().to_lowercase();
        data.get()
            .rows
            .into_iter()
            .filter(|p| {
                (author.with(|a| a.is_empty() || *a == p.user.login))
                    && tag.with(|t| {
                        t.is_empty() || p.labels.iter().any(|l| l.name == *t)
                    })
                    && format!("{} {} {}", p.title, p.number, p.user.login)
                        .to_lowercase()
                        .contains(&q)
            })
            .collect::<Vec<_>>()
    });
    stack((
        super::chrome::header(
            window,
            PanelKind::PullRequests,
            "Pull Requests",
            empty(),
        ),
        stack((
            label(move || {
                data.with(|d| {
                    if d.repository.is_empty() {
                        "GitHub repository".into()
                    } else {
                        d.repository.clone()
                    }
                })
            })
            .style(|s| s.flex_grow(1.0_f32).min_width(0.0).text_ellipsis()),
            clickable_icon(
                || SparrowIcons::IMAGE_LOADING,
                move || refresh.update(|n| *n += 1),
                || false,
                move || loading.get(),
                || "Refresh pull requests",
                config,
            ),
        ))
        .style(|s| {
            s.height(30.0)
                .width_full()
                .padding_horiz(12.0)
                .items_center()
        }),
        text_input(query)
            .placeholder("Search pull requests")
            .style(move |s| {
                s.height(26.0)
                    .margin_horiz(12.0)
                    .padding_horiz(8.0)
                    .border(1.0)
                    .border_radius(3.0)
                    .border_color(
                        config.get().color(SparrowColor::SPARROW_TAB_ACTIVE_BORDER),
                    )
            }),
        stack((
            label(move || format!("{} ⌄", state.get())).ide_popout_menu(move || {
                let mut m = Menu::new("");
                for value in ["open", "closed", "all"] {
                    m = m.entry(
                        MenuItem::new(value).action(move || state.set(value.into())),
                    );
                }
                m
            }),
            label(move || {
                author.with(|a| {
                    if a.is_empty() {
                        "Author ⌄".into()
                    } else {
                        a.clone()
                    }
                })
            })
            .ide_popout_menu(move || {
                let mut m = Menu::new("").entry(
                    MenuItem::new("All authors")
                        .action(move || author.set(String::new())),
                );
                let mut authors = data.with_untracked(|d| {
                    d.rows
                        .iter()
                        .map(|p| p.user.login.clone())
                        .collect::<Vec<_>>()
                });
                authors.sort();
                authors.dedup();
                for a in authors {
                    m = m.entry(
                        MenuItem::new(a.clone())
                            .action(move || author.set(a.clone())),
                    );
                }
                m
            }),
            label(move || {
                tag.with(|t| {
                    if t.is_empty() {
                        "Label ⌄".into()
                    } else {
                        t.clone()
                    }
                })
            })
            .ide_popout_menu(move || {
                let mut m = Menu::new("").entry(
                    MenuItem::new("All labels")
                        .action(move || tag.set(String::new())),
                );
                let mut tags = data.with_untracked(|d| {
                    d.rows
                        .iter()
                        .flat_map(|p| p.labels.iter().map(|t| t.name.clone()))
                        .collect::<Vec<_>>()
                });
                tags.sort();
                tags.dedup();
                for t in tags {
                    m = m.entry(
                        MenuItem::new(t.clone()).action(move || tag.set(t.clone())),
                    );
                }
                m
            }),
        ))
        .style(|s| s.height(36.0).gap(14.0).items_center().padding_horiz(16.0)),
        label(move || {
            if loading.get() {
                "Loading pull requests…".into()
            } else {
                data.get().error
            }
        })
        .style(move |s| {
            s.width_full()
                .padding(16.0)
                .color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
                .apply_if(!loading.get() && data.with(|d| d.error.is_empty()), |s| {
                    s.hide()
                })
        }),
        stack((
            label(|| "No pull requests matching filters"),
            label(|| "Clear filters")
                .on_click_stop(move |_| {
                    query.set(String::new());
                    author.set(String::new());
                    tag.set(String::new());
                    state.set("open".into());
                })
                .style(move |s| {
                    s.margin_top(12.0)
                        .color(config.get().color(SparrowColor::EDITOR_LINK))
                }),
        ))
        .style(move |s| {
            s.flex_col()
                .width_full()
                .padding_vert(100.0)
                .items_center()
                .color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
                .apply_if(
                    loading.get()
                        || !data.with(|d| d.error.is_empty())
                        || !filtered.with(|p| p.is_empty()),
                    |s| s.hide(),
                )
        }),
        scroll(
            dyn_stack(
                move || filtered.get(),
                |p| p.number,
                move |p| {
                    let url = p.html_url;
                    stack((
                        label(move || p.title.clone()).style(|s| s.width_full()),
                        label(move || {
                            format!("#{} · {} · {}", p.number, p.user.login, p.state)
                        })
                        .style(move |s| {
                            s.font_size(11.0).margin_top(6.0).color(
                                config
                                    .get()
                                    .color(SparrowColor::PANEL_FOREGROUND_DIM),
                            )
                        }),
                    ))
                    .on_click_stop(move |_| {
                        if url.starts_with("https://github.com/") {
                            if let Err(e) = open::that(&url) {
                                record(
                                    notices,
                                    "Pull Requests",
                                    &format!("Unable to open browser: {e}"),
                                );
                            }
                        }
                    })
                    .style(move |s| {
                        s.flex_col()
                            .width_full()
                            .padding(12.0)
                            .border_bottom(1.0)
                            .border_color(
                                config.get().color(SparrowColor::SPARROW_BORDER),
                            )
                            .hover(|s| {
                                s.background(
                                    config.get().color(
                                        SparrowColor::PANEL_HOVERED_BACKGROUND,
                                    ),
                                )
                            })
                    })
                },
            )
            .style(|s| s.flex_col().width_full()),
        )
        .style(|s| s.flex_grow(1.0_f32).min_height(0.0).width_full()),
        label(|| "First 100 results · select a request to open on GitHub").style(
            move |s| {
                s.width_full()
                    .padding(8.0)
                    .font_size(11.0)
                    .color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
            },
        ),
    ))
    .style(|s| s.absolute().size_full().flex_col().font_size(13.0))
}

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
    stack((super::chrome::header(window,PanelKind::RunningDevices,"Running Devices",empty()),
 stack((clickable_icon(||SparrowIcons::ADD,move ||{},||false,||false,||"Select device or launch emulator",config).ide_popout_menu(move ||{let mut m=Menu::new("");for d in add.devices.get_untracked(){let choice=d.clone();m=m.entry(MenuItem::new(&d.name).action(move ||selected.set(Some(choice.clone()))));}for e in add.emulators.get_untracked(){let hr=add.clone();m=m.entry(MenuItem::new(format!("Launch {}",e.name)).action(move ||hr.launch_emulator(&e.id)));}m}),label(||"Device Manager").on_click_stop(move |_|manager.show_panel(PanelKind::DeviceManager)).style(move |s|s.color(config.get().color(SparrowColor::EDITOR_LINK))),clickable_icon(||SparrowIcons::IMAGE_LOADING,move ||refresh.fetch_devices(),||false,||false,||"Refresh Flutter devices",config))).style(|s|s.height(30.0).gap(8.0).items_center().padding_horiz(12.0)),
 stack((svg(||DEVICES_ART.to_string()).style(|s|s.size(180.0,112.0).margin_bottom(20.0)),label(||"Connect an Android device via USB or Wi-Fi to mirror it. Use + to select a Flutter target or launch an installed emulator.").style(|s|s.width(346.0).max_width_full()),label(||"Android mirroring opens in a separate scrcpy window. Start a Flutter session to display its active target here.").style(|s|s.width(346.0).max_width_full().margin_top(18.0)))).style(move |s|s.flex_col().flex_grow(1.0_f32).width_full().items_center().justify_center().padding(18.0).padding_bottom(116.0).line_height(1.4).color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM)).apply_if(!rows.with(|r|r.is_empty()),|s|s.hide())),
 scroll(dyn_stack(move ||rows.get(),|d|d.id.clone(),move |d|{let select=d.clone();let is_android=d.target_platform.as_deref().is_some_and(|p|p.contains("android"));let mirror=mirror.clone();let mirror_alive=mirror_alive.clone();let id=d.id.clone();stack((label(move ||d.name.clone()).style(|s|s.font_bold()),label(move ||d.target_platform.clone().unwrap_or_default()).style(|s|s.margin_top(6.0)),label(||"Select Flutter target").on_click_stop(move |_|selected.set(Some(select.clone()))).style(move |s|s.margin_top(10.0).color(config.get().color(SparrowColor::EDITOR_LINK))),label(||"Mirror Android device").on_click_stop(move |_|{if let Some(mut old)=mirror.borrow_mut().take(){let _=old.kill();let _=old.wait();}match Command::new("scrcpy").args(["-s",&id,"--window-title","Sparrow · Flutter device"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn(){Ok(child)=>{mirror.borrow_mut().replace(child);mirror_status.set("Mirroring opened in scrcpy".into());watch_mirror(mirror.clone(),mirror_status,notices,mirror_alive.clone());},Err(e)=>record(notices,"Running Devices",&format!("Unable to start scrcpy: {e}"))}}).style(move |s|s.margin_top(10.0).color(config.get().color(SparrowColor::EDITOR_LINK)).apply_if(!is_android,|s|s.hide())))).style(move |s|s.flex_col().width_full().padding(16.0).border_bottom(1.0).border_color(config.get().color(SparrowColor::SPARROW_BORDER)))}).style(|s|s.flex_col().width_full())).style(move |s|s.width_full().flex_grow(1.0_f32).min_height(0.0).apply_if(rows.with(|r|r.is_empty()),|s|s.hide())),
 label(move ||mirror_status.get()).style(move |s|s.padding_horiz(12.0).apply_if(mirror_status.with(|s|s.is_empty()),|s|s.hide())),label(||"Stop mirroring").on_click_stop(move |_|{if let Some(mut child)=stop.borrow_mut().take(){let _=child.kill();let _=child.wait();}mirror_status.set(String::new());}).style(move |s|s.height(28.0).padding_horiz(12.0).color(config.get().color(SparrowColor::EDITOR_LINK)).apply_if(mirror_status.with(|s|s.is_empty()),|s|s.hide())),
 )).on_cleanup(move ||{cleanup_alive.set(false);if let Some(mut child)=cleanup.borrow_mut().take(){let _=child.kill();let _=child.wait();}}).style(|s|s.absolute().size_full().flex_col().font_size(13.0))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires a real GitHub workspace and network access"]
    fn reads_installed_flutter_repository_pull_requests() {
        let root = std::env::var("SPARROW_PR_TEST_WORKSPACE")
            .expect("set a local Flutter workspace");
        let result = load_pulls(Path::new(&root), "open");
        assert!(result.error.is_empty(), "{}", result.error);
        assert!(!result.repository.is_empty());
    }
    #[test]
    fn recognizes_github_remotes_only() {
        assert_eq!(
            github_repository("git@github.com:owner/flutter-app.git"),
            Some("owner/flutter-app".into())
        );
        assert_eq!(
            github_repository("https://github.com/owner/app.git"),
            Some("owner/app".into())
        );
        assert!(github_repository("https://example.com/owner/app").is_none());
        assert!(github_repository("https://github.com/owner/app/extra").is_none());
        assert!(github_repository("https://github.com/../app").is_none());
    }
}
