//! Pull request filters, result list and detail controls.

use super::record;
use crate::panel::{kind::PanelKind, reference_tools::output};
use crate::widgets::{
    ButtonKind, button, dropdown_with_label, sparrow_scroll, text_input,
};
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
    views::{Decorators, dyn_container, dyn_stack, empty, label, stack},
};
use std::{path::Path, rc::Rc};

fn github_repository(remote: &str) -> Option<(String, String)> {
    let remote = remote.trim();
    let remote_host = crate::vcs_credentials::remote_host(remote).ok()?;
    let host = match remote_host {
        crate::vcs_credentials::RemoteHost::Https(host)
        | crate::vcs_credentials::RemoteHost::Ssh(host) => host,
    };
    let path = if let Some((_, address)) = remote.rsplit_once('@')
        && !remote.contains("://")
    {
        address.split_once(':')?.1.to_string()
    } else {
        url::Url::parse(remote)
            .ok()?
            .path()
            .trim_start_matches('/')
            .to_string()
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
    .then(|| (host, path.to_string()))
}

fn safe_pull_url(value: &str, host: &str) -> bool {
    let Ok(url) = url::Url::parse(value) else {
        return false;
    };
    url.scheme() == "https"
        && url.host_str() == Some(host)
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
        && !url.query_pairs().any(|(key, _)| {
            matches!(
                key.to_ascii_lowercase().as_str(),
                "access_token"
                    | "token"
                    | "password"
                    | "secret"
                    | "api_key"
                    | "apikey"
            )
        })
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
struct PullDetails {
    number: u64,
    title: String,
    body: String,
    state: String,
    head: String,
    base: String,
    files: Vec<PullFile>,
    comments: Vec<PullComment>,
    error: String,
    truncated: bool,
}

fn mark_invalid_token(
    error: &crate::vcs_credentials::ApiError,
    credential: &Option<(
        crate::vcs_credentials::Account,
        zeroize::Zeroizing<String>,
    )>,
) {
    if error.invalid_token
        && let Some((account, _)) = credential.as_ref()
    {
        let _ = crate::vcs_credentials::set_status(
            &account.host,
            &account.username,
            crate::vcs_credentials::AccountStatus::Invalid,
        );
    }
}

#[derive(Clone, serde::Deserialize)]
struct PullFile {
    filename: String,
    status: String,
    additions: u32,
    deletions: u32,
    patch: Option<String>,
}

#[derive(Clone, serde::Deserialize)]
struct PullComment {
    user: User,
    body: String,
    created_at: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    line: Option<u32>,
}

#[derive(serde::Deserialize)]
struct PullDetailsResponse {
    #[serde(default)]
    body: Option<String>,
    state: String,
    head: BranchRef,
    base: BranchRef,
}

#[derive(serde::Deserialize)]
struct BranchRef {
    #[serde(rename = "ref")]
    branch: String,
}

#[derive(serde::Serialize)]
struct NewPullRequest<'a> {
    title: &'a str,
    head: &'a str,
    base: &'a str,
    body: &'a str,
}

#[derive(serde::Deserialize)]
struct CreatedPullRequest {
    number: u64,
    html_url: String,
}
#[derive(Clone, Default)]
struct Pulls {
    host: String,
    repository: String,
    rows: Vec<Pull>,
    error: String,
    auth_required: bool,
    truncated: bool,
}

fn fetch_pull_pages(
    host: &str,
    repository: &str,
    state: &str,
    token: Option<&str>,
) -> Result<(Vec<Pull>, bool), crate::vcs_credentials::ApiError> {
    let mut rows = Vec::new();
    for page in 1..=20 {
        let page_number = page.to_string();
        let query = [
            ("state", state),
            ("per_page", "100"),
            ("page", page_number.as_str()),
        ];
        let batch: Vec<Pull> = crate::vcs_credentials::github_api_json(
            host,
            &format!("repos/{repository}/pulls"),
            &query,
            token,
        )?;
        if batch.len() < 100 {
            rows.extend(batch);
            return Ok((rows, false));
        }
        rows.extend(batch);
    }
    Ok((rows, true))
}

fn fetch_detail_pages<T: serde::de::DeserializeOwned>(
    host: &str,
    path: &str,
    token: Option<&str>,
) -> Result<(Vec<T>, bool), crate::vcs_credentials::ApiError> {
    let mut items = Vec::new();
    for page in 1..=5 {
        let page_number = page.to_string();
        let query = [("per_page", "100"), ("page", page_number.as_str())];
        let batch: Vec<T> =
            crate::vcs_credentials::github_api_json(host, path, &query, token)?;
        if batch.len() < 100 {
            items.extend(batch);
            return Ok((items, false));
        }
        items.extend(batch);
    }
    Ok((items, true))
}

fn load_pull_details(host: &str, repository: &str, pull: &Pull) -> PullDetails {
    let credential = match crate::vcs_credentials::active_github_token(host) {
        Ok(credential) => credential,
        Err(error) => {
            return PullDetails {
                number: pull.number,
                title: pull.title.clone(),
                error,
                ..Default::default()
            };
        }
    };
    let token = credential.as_ref().map(|(_, token)| token.as_str());
    let prefix = format!("repos/{repository}/pulls/{}", pull.number);
    let response: PullDetailsResponse =
        match crate::vcs_credentials::github_api_json(host, &prefix, &[], token) {
            Ok(response) => response,
            Err(error) => {
                mark_invalid_token(&error, &credential);
                return PullDetails {
                    number: pull.number,
                    title: pull.title.clone(),
                    error: error.message,
                    ..Default::default()
                };
            }
        };
    let files_path = format!("{prefix}/files");
    let comments_path =
        format!("repos/{repository}/issues/{}/comments", pull.number);
    let review_comments_path = format!("{prefix}/comments");
    let (mut files, files_truncated) =
        match fetch_detail_pages::<PullFile>(host, &files_path, token) {
            Ok(result) => result,
            Err(error) => {
                mark_invalid_token(&error, &credential);
                return PullDetails {
                    number: pull.number,
                    title: pull.title.clone(),
                    error: error.message,
                    ..Default::default()
                };
            }
        };
    let (mut comments, comments_truncated) =
        match fetch_detail_pages::<PullComment>(host, &comments_path, token) {
            Ok(result) => result,
            Err(error) => {
                mark_invalid_token(&error, &credential);
                return PullDetails {
                    number: pull.number,
                    title: pull.title.clone(),
                    error: error.message,
                    ..Default::default()
                };
            }
        };
    let (mut review_comments, review_truncated) = match fetch_detail_pages::<
        PullComment,
    >(
        host,
        &review_comments_path,
        token,
    ) {
        Ok(result) => result,
        Err(error) => {
            mark_invalid_token(&error, &credential);
            return PullDetails {
                number: pull.number,
                title: pull.title.clone(),
                error: error.message,
                ..Default::default()
            };
        }
    };
    if let Some((_, token)) = credential.as_ref() {
        let secret = token.as_str();
        for file in &mut files {
            if let Some(patch) = &mut file.patch {
                *patch = crate::vcs_credentials::redact(patch, secret);
            }
        }
        for comment in comments.iter_mut().chain(review_comments.iter_mut()) {
            comment.body = crate::vcs_credentials::redact(&comment.body, secret);
        }
    }
    comments.append(&mut review_comments);
    let mut body = response.body.unwrap_or_default();
    if let Some((_, token)) = credential.as_ref() {
        body = crate::vcs_credentials::redact(&body, token.as_str());
    }
    PullDetails {
        number: pull.number,
        title: pull.title.clone(),
        body,
        state: response.state,
        head: response.head.branch,
        base: response.base.branch,
        files,
        comments,
        error: String::new(),
        truncated: files_truncated || comments_truncated || review_truncated,
    }
}

fn pull_details_text(details: &PullDetails) -> String {
    use std::fmt::Write;
    if !details.error.is_empty() {
        return format!("Pull request #{}\n{}", details.number, details.error);
    }
    let mut text = format!(
        "#{} · {}\n{} → {} · {}\n\n{}\n\nFILES\n",
        details.number,
        details.title,
        details.head,
        details.base,
        details.state,
        if details.body.trim().is_empty() {
            "No description provided."
        } else {
            details.body.trim()
        }
    );
    for file in &details.files {
        let _ = writeln!(
            text,
            "\n{} · {} · +{} -{}",
            file.filename, file.status, file.additions, file.deletions
        );
        let _ = writeln!(
            text,
            "{}",
            file.patch
                .as_deref()
                .unwrap_or("Patch unavailable (binary or large file).")
        );
    }
    let _ = writeln!(text, "\nCOMMENTS\n");
    for comment in &details.comments {
        let location = comment.path.as_deref().map_or_else(String::new, |path| {
            format!(
                " · {path}{}",
                comment
                    .line
                    .map_or(String::new(), |line| format!(":{line}"))
            )
        });
        let _ = writeln!(
            text,
            "{} · {}{}\n{}\n",
            comment.user.login, comment.created_at, location, comment.body
        );
    }
    if details.truncated {
        text.push_str(
            "\nSome files or comments were omitted after the 500-item safety limit.",
        );
    }
    let output_truncated = text.chars().count() > 60_000;
    let mut output = text.chars().take(60_000).collect::<String>();
    if output_truncated {
        output.push_str("\n\n[PR detail output truncated for display.]");
    }
    output
}

fn create_github_pull_request(
    host: &str,
    repository: &str,
    title: &str,
    head: &str,
    base: &str,
    body: &str,
) -> Result<(u64, String), String> {
    if title.trim().is_empty() || head.trim().is_empty() || base.trim().is_empty() {
        return Err("Title, head branch, and base branch are required.".into());
    }
    let credential = crate::vcs_credentials::active_github_token(host)?;
    let Some((account, token)) = credential else {
        return Err(format!(
            "Add a GitHub account for {host} with Pull requests: write permission to create a request."
        ));
    };
    let request = NewPullRequest {
        title: title.trim(),
        head: head.trim(),
        base: base.trim(),
        body,
    };
    let path = format!("repos/{repository}/pulls");
    let created: CreatedPullRequest =
        match crate::vcs_credentials::github_api_post_json(
            host,
            &path,
            &request,
            token.as_str(),
        ) {
            Ok(created) => created,
            Err(error) => {
                mark_invalid_token(&error, &Some((account.clone(), token)));
                return Err(error.message);
            }
        };
    if !safe_pull_url(&created.html_url, host) {
        return Err(
            "GitHub returned a pull-request URL outside the configured host.".into(),
        );
    }
    Ok((created.number, created.html_url))
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
    let Some((host, repository)) = github_repository(&remote) else {
        return Pulls {
            error: "Pull Requests currently supports GitHub remotes only.".into(),
            ..Default::default()
        };
    };
    let credential = match crate::vcs_credentials::active_github_token(&host) {
        Ok(credential) => credential,
        Err(error) => {
            return Pulls {
                host,
                repository,
                error,
                auth_required: true,
                ..Default::default()
            };
        }
    };
    let token = credential.as_ref().map(|(_, token)| token.as_str());
    match fetch_pull_pages(&host, &repository, state, token) {
        Ok((rows, truncated)) => Pulls {
            host,
            repository,
            rows,
            error: String::new(),
            truncated,
            auth_required: false,
        },
        Err(error) => {
            if error.invalid_token
                && let Some((account, _)) = credential.as_ref()
            {
                let _ = crate::vcs_credentials::set_status(
                    &account.host,
                    &account.username,
                    crate::vcs_credentials::AccountStatus::Invalid,
                );
            }
            Pulls {
                host,
                repository,
                error: error.message,
                auth_required: error.auth_required || error.invalid_token,
                truncated: false,
                ..Default::default()
            }
        }
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
    let selected_pull = create_rw_signal(None::<Pull>);
    let pull_details = create_rw_signal(PullDetails::default());
    let detail_loading = create_rw_signal(false);
    let detail_generation = create_rw_signal(0u64);
    let create_open = create_rw_signal(false);
    let create_title = create_rw_signal(String::new());
    let create_head = create_rw_signal(String::new());
    let create_base = create_rw_signal("main".to_string());
    let create_body = create_rw_signal(String::new());
    let create_status = create_rw_signal(String::new());
    let creating = create_rw_signal(false);
    let panel = window.panel.clone();
    let settings_window = window.clone();
    let create_settings_window = window.clone();
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
    let new_pull_button = button(|| "New PR", ButtonKind::Outlined, config)
        .on_click_stop(move |_| {
            create_status.set(String::new());
            create_open.set(true);
        })
        .style(move |s| {
            s.apply_if(data.with(|pulls| pulls.repository.is_empty()), |s| s.hide())
        });

    let create_request_button = button(|| "Create Pull Request", ButtonKind::Primary, config)
        .on_click_stop(move |_| {
            if creating.get_untracked() {
                return;
            }
            let (host, repository) = data.with_untracked(|pulls| {
                (pulls.host.clone(), pulls.repository.clone())
            });
            let title = create_title.get_untracked();
            let head = create_head.get_untracked();
            let base = create_base.get_untracked();
            let body = create_body.get_untracked();
            if title.trim().is_empty() || head.trim().is_empty() || base.trim().is_empty() {
                create_status.set("Title, head branch, and base branch are required.".into());
                return;
            }
            creating.set(true);
            create_status.set("Creating pull request…".into());
            let send = floem::ext_event::create_ext_action(
                Scope::current(),
                move |result: Result<String, String>| {
                    creating.set(false);
                    match result {
                        Ok(message) => {
                            create_status.set(message.clone());
                            record(notices, "Pull Requests", &message);
                            create_open.set(false);
                            refresh.update(|value| *value = value.wrapping_add(1));
                        }
                        Err(error) => create_status.set(error),
                    }
                },
            );
            std::thread::spawn(move || {
                let result = create_github_pull_request(
                    &host,
                    &repository,
                    &title,
                    &head,
                    &base,
                    &body,
                )
                .map(|(number, url)| {
                    if let Err(error) = open::that(url) {
                        format!("Created pull request #{number}, but could not open the browser: {error}")
                    } else {
                        format!("Created and opened pull request #{number}.")
                    }
                });
                send(result);
            });
        });

    let create_form = stack((
        label(|| "Create pull request").style(|s| s.font_bold()),
        text_input(create_title, config).placeholder("Title"),
        text_input(create_head, config)
            .placeholder("Head branch (branch or owner:branch)"),
        text_input(create_base, config).placeholder("Base branch (default: main)"),
        text_input(create_body, config).placeholder("Description"),
        stack((
            create_request_button,
            button(|| "Manage accounts", ButtonKind::Outlined, config)
                .on_click_stop(move |_| {
                    create_settings_window
                        .settings_search_query
                        .set("accounts".into());
                    create_settings_window.settings_modal_visible.set(true);
                }),
            button(|| "Cancel", ButtonKind::Outlined, config)
                .on_click_stop(move |_| create_open.set(false)),
        ))
        .style(|s| s.gap(8.0)),
        label(move || create_status.get()).style(move |s| {
            s.width_full()
                .color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
        }),
    ))
    .style(move |s| {
        s.flex_col()
            .width_full()
            .gap(8.0)
            .padding(12.0)
            .apply_if(!create_open.get() || selected_pull.get().is_some(), |s| {
                s.hide()
            })
    });

    let details_panel = stack((
        stack((
            button(|| "Back to list", ButtonKind::Outlined, config)
                .on_click_stop(move |_| selected_pull.set(None)),
            label(move || {
                selected_pull.with(|pull| {
                    pull.as_ref()
                        .map(|pull| format!("#{} · {}", pull.number, pull.title))
                        .unwrap_or_default()
                })
            })
            .style(|s| s.flex_grow(1.0_f32).min_width(0.0).text_ellipsis()),
        ))
        .style(|s| s.width_full().items_center().gap(8.0)),
        label(move || {
            if detail_loading.get() {
                "Loading diff and comments…".into()
            } else {
                String::new()
            }
        })
        .style(move |s| {
            s.color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
                .apply_if(!detail_loading.get(), |s| s.hide())
        }),
        sparrow_scroll(label(move || pull_details_text(&pull_details.get()))),
    ))
    .style(move |s| {
        s.flex_col()
            .flex_grow(1.0_f32)
            .min_height(0.0)
            .width_full()
            .gap(8.0)
            .padding(12.0)
            .apply_if(selected_pull.get().is_none(), |s| s.hide())
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
            new_pull_button,
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
            dropdown_with_label(
                state,
                ["open".into(), "closed".into(), "all".into()],
                config,
                |value| value.clone(),
                |_| {},
            ),
            dyn_container(
                move || {
                    let mut authors = data
                        .get()
                        .rows
                        .iter()
                        .map(|pull| pull.user.login.clone())
                        .collect::<Vec<_>>();
                    let selected = author.get();
                    if !selected.is_empty() {
                        authors.push(selected);
                    }
                    authors.sort();
                    authors.dedup();
                    authors.insert(0, String::new());
                    authors
                },
                move |authors| {
                    dropdown_with_label(
                        author,
                        authors,
                        config,
                        |value| {
                            if value.is_empty() {
                                "Author".into()
                            } else {
                                value.clone()
                            }
                        },
                        |_| {},
                    )
                },
            ),
            dyn_container(
                move || {
                    let mut tags = data
                        .get()
                        .rows
                        .iter()
                        .flat_map(|pull| {
                            pull.labels.iter().map(|label| label.name.clone())
                        })
                        .collect::<Vec<_>>();
                    let selected = tag.get();
                    if !selected.is_empty() {
                        tags.push(selected);
                    }
                    tags.sort();
                    tags.dedup();
                    tags.insert(0, String::new());
                    tags
                },
                move |tags| {
                    dropdown_with_label(
                        tag,
                        tags,
                        config,
                        |value| {
                            if value.is_empty() {
                                "Label".into()
                            } else {
                                value.clone()
                            }
                        },
                        |_| {},
                    )
                },
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
        button(
            || "Add or replace account in Settings",
            ButtonKind::Outlined,
            config,
        )
        .on_click_stop(move |_| {
            settings_window.settings_search_query.set("accounts".into());
            settings_window.settings_modal_visible.set(true);
        })
        .style(move |s| {
            s.margin_horiz(16.0)
                .apply_if(!data.with(|pulls| pulls.auth_required), |s| s.hide())
        }),
        create_form,
        label(move || "Showing the first 2,000 pull requests.").style(move |s| {
            s.padding_horiz(16.0)
                .color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
                .apply_if(!data.with(|pulls| pulls.truncated), |s| s.hide())
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
                    let url = p.html_url.clone();
                    let allowed_host = data.with(|pulls| pulls.host.clone());
                    let details_pull = p.clone();
                    let select_details = selected_pull;
                    let detail_signal = pull_details;
                    let detail_loading_signal = detail_loading;
                    let detail_generation_signal = detail_generation;
                    let pull_data = data;
                    let detail_button = button(|| "Details", ButtonKind::Outlined, config)
                        .on_click_stop(move |_| {
                            let pull = details_pull.clone();
                            let (host, repository) = pull_data.with_untracked(|pulls| {
                                (pulls.host.clone(), pulls.repository.clone())
                            });
                            select_details.set(Some(pull.clone()));
                            detail_signal.set(PullDetails {
                                number: pull.number,
                                title: pull.title.clone(),
                                ..Default::default()
                            });
                            detail_loading_signal.set(true);
                            detail_generation_signal.update(|generation| *generation += 1);
                            let request = detail_generation_signal.get_untracked();
                            let send = floem::ext_event::create_ext_action(
                                Scope::current(),
                                move |result: PullDetails| {
                                    if detail_generation_signal.get_untracked() == request {
                                        detail_loading_signal.set(false);
                                        detail_signal.set(result);
                                    }
                                },
                            );
                            std::thread::spawn(move || {
                                send(load_pull_details(&host, &repository, &pull));
                            });
                        });
                    let open_button = button(|| "Open", ButtonKind::Plain, config)
                        .on_click_stop(move |_| {
                            if safe_pull_url(&url, &allowed_host)
                                && let Err(error) = open::that(&url)
                            {
                                record(
                                    notices,
                                    "Pull Requests",
                                    &format!("Unable to open browser: {error}"),
                                );
                            }
                        });
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
                        stack((detail_button, open_button))
                            .style(|s| s.gap(6.0).items_center()),
                    ))
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
        .style(move |s| {
            s.flex_grow(1.0_f32)
                .min_height(0.0)
                .width_full()
                .apply_if(selected_pull.get().is_some(), |s| s.hide())
        }),
        details_panel,
        label(|| "Select a request to inspect its diff and comments, or open it in a browser.").style(move |s| {
            s.width_full()
                .padding(8.0)
                .font_size(11.0)
                .color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
                .apply_if(selected_pull.get().is_some(), |s| s.hide())
        }),
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
    fn recognizes_github_and_enterprise_remotes() {
        assert_eq!(
            github_repository("git@github.com:owner/flutter-app.git"),
            Some(("github.com".into(), "owner/flutter-app".into()))
        );
        assert_eq!(
            github_repository("https://github.com/owner/app.git"),
            Some(("github.com".into(), "owner/app".into()))
        );
        assert_eq!(
            github_repository("https://git.example.com/owner/app"),
            Some(("git.example.com".into(), "owner/app".into()))
        );
        assert!(github_repository("http://github.com/owner/app").is_none());
        assert!(github_repository("https://github.com/owner/app/extra").is_none());
        assert!(github_repository("https://github.com/../app").is_none());
        assert!(
            github_repository("https://user:token@github.com/owner/app").is_none()
        );
    }

    #[test]
    fn pull_details_include_diff_and_conversation_content() {
        let details = PullDetails {
            number: 7,
            title: "Update widget".into(),
            body: "Adds a loading state.".into(),
            state: "open".into(),
            head: "feature".into(),
            base: "main".into(),
            files: vec![PullFile {
                filename: "lib/widget.dart".into(),
                status: "modified".into(),
                additions: 3,
                deletions: 1,
                patch: Some("+ loading = true".into()),
            }],
            comments: vec![PullComment {
                user: User {
                    login: "reviewer".into(),
                },
                body: "Looks good.".into(),
                created_at: "2026-11-25".into(),
                path: Some("lib/widget.dart".into()),
                line: Some(12),
            }],
            error: String::new(),
            truncated: false,
        };
        let text = pull_details_text(&details);
        assert!(text.contains("lib/widget.dart · modified · +3 -1"));
        assert!(text.contains("+ loading = true"));
        assert!(text.contains("reviewer · 2026-11-25 · lib/widget.dart:12"));
        assert!(text.contains("Looks good."));
    }

    #[test]
    fn pull_creation_rejects_missing_required_fields_without_network() {
        let error = create_github_pull_request(
            "github.com",
            "owner/repo",
            "",
            "head",
            "main",
            "",
        )
        .unwrap_err();
        assert!(error.contains("Title, head branch, and base branch are required."));
    }

    #[test]
    fn pull_browser_urls_must_match_the_repository_host_without_credentials() {
        assert!(safe_pull_url(
            "https://github.com/owner/repo/pull/12",
            "github.com"
        ));
        assert!(!safe_pull_url(
            "https://github.com.evil.test/owner/repo/pull/12",
            "github.com"
        ));
        assert!(!safe_pull_url(
            "https://user:secret@github.com/owner/repo/pull/12",
            "github.com"
        ));
        assert!(!safe_pull_url(
            "https://github.com/owner/repo/pull/12?token=secret",
            "github.com"
        ));
    }
}
