//! Pull request filters, result list and detail controls.

use super::record;
use crate::panel::{kind::PanelKind, reference_tools::output};
use crate::widgets::popup_menu::{Menu, MenuItem};
use crate::widgets::{sparrow_scroll, text_input};
use crate::{
    config::{color::SparrowColor, icon::SparrowIcons},
    widgets::clickable_icon,
    window_tab::WindowTabData,
};
use floem::{
    View,
    reactive::{
        Scope, SignalGet, SignalUpdate, SignalWith, create_effect, create_memo,
        create_rw_signal,
    },
    views::{Decorators, dyn_stack, empty, label, stack},
};
use std::{path::Path, rc::Rc, time::Duration};

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
        crate::panel::chrome::header(
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
        floem::views::container(
            text_input(query, config).placeholder("Search pull requests"),
        )
        .style(|s| s.margin_horiz(12.0).min_width(0.0)),
        stack((
            crate::widgets::menu_button(
                move || state.get(),
                move || {
                    let mut m = Menu::new("");
                    for value in ["open", "closed", "all"] {
                        m = m.entry(
                            MenuItem::new(value)
                                .action(move || state.set(value.into())),
                        );
                    }
                    m
                },
                config,
            ),
            crate::widgets::menu_button(
                move || {
                    author.with(|a| {
                        if a.is_empty() {
                            "Author".into()
                        } else {
                            a.clone()
                        }
                    })
                },
                move || {
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
                },
                config,
            ),
            crate::widgets::menu_button(
                move || {
                    tag.with(|t| {
                        if t.is_empty() {
                            "Label".into()
                        } else {
                            t.clone()
                        }
                    })
                },
                move || {
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
                            MenuItem::new(t.clone())
                                .action(move || tag.set(t.clone())),
                        );
                    }
                    m
                },
                config,
            ),
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
        sparrow_scroll(
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
