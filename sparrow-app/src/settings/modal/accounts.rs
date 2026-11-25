//! Account controls for secure remote version-control credentials.

use std::{sync::Arc, thread};

use floem::{
    View,
    ext_event::create_ext_action,
    reactive::{ReadSignal, RwSignal, Scope, SignalGet, SignalUpdate, SignalWith},
    views::{Decorators, dyn_stack, label, stack},
};
use zeroize::Zeroizing;

use crate::{
    config::{SparrowConfig, color::SparrowColor},
    panel::workflow_tools::{Notice, record},
    vcs_credentials::{self, Account, AccountProvider, AccountStatus, StorageKind},
    widgets::{ButtonKind, button, dropdown_with_label, text_input},
};

fn refresh_accounts(reload: RwSignal<u64>) {
    reload.update(|value| *value = value.wrapping_add(1));
}

fn account_summary(account: &Account) -> String {
    let validity = match &account.status {
        AccountStatus::Unknown => "not checked",
        AccountStatus::Valid => "valid",
        AccountStatus::Invalid => "invalid or expired",
    };
    let storage = match &account.storage {
        StorageKind::Keyring => "OS credential store",
        StorageKind::SessionOnly => "session-only memory",
    };
    let provider = match &account.provider {
        AccountProvider::GitHub => "GitHub-compatible",
        AccountProvider::GenericHttps => "generic HTTPS",
    };
    format!(
        "{} · {} · {provider} · {validity} · {storage}",
        account.host, account.username
    )
}

pub(super) fn view(
    config: ReadSignal<Arc<SparrowConfig>>,
    notices: RwSignal<Vec<Notice>>,
) -> impl View {
    let cx = Scope::current();
    let accounts = cx.create_rw_signal(Vec::<Account>::new());
    let host = cx.create_rw_signal("github.com".to_string());
    let provider = cx.create_rw_signal("github".to_string());
    let username = cx.create_rw_signal(String::new());
    let token = cx.create_rw_signal(String::new());
    let status = cx.create_rw_signal(String::new());
    let reload = cx.create_rw_signal(0_u64);
    let working = cx.create_rw_signal(false);
    let allow_session_fallback = cx.create_rw_signal(true);

    cx.create_effect(move |_| {
        let _ = reload.get();
        let send = create_ext_action(Scope::current(), move |result| match result {
            Ok(items) => accounts.set(items),
            Err(error) => status.set(error),
        });
        thread::spawn(move || send(vcs_credentials::accounts()));
    });

    let save_host = host;
    let save_provider = provider;
    let save_username = username;
    let save_token = token;
    let save_status = status;
    let save_working = working;
    let save_reload = reload;
    let save_fallback = allow_session_fallback;
    let add = button(
        move || {
            if save_provider.get() == "github" {
                "Test & Add / Replace Token"
            } else {
                "Save Generic HTTPS Credential"
            }
        },
        ButtonKind::Primary,
        config,
    )
        .on_click_stop(move |_| {
            if save_working.get_untracked() {
                return;
            }
            let host_value = save_host.get_untracked();
            let provider_value = save_provider.get_untracked();
            let username_value = save_username.get_untracked();
            let allow_fallback = save_fallback.get_untracked();
            let mut secret = String::new();
            save_token.update(|value| secret = std::mem::take(value));
            if secret.trim().is_empty() {
                save_status.set("Paste a token to validate and add this account.".into());
                return;
            }
            let secret = Zeroizing::new(secret);
            save_working.set(true);
            save_status.set(if provider_value == "github" {
                "Validating token with GitHub…".into()
            } else {
                "Saving generic HTTPS credential…".into()
            });
            let done = create_ext_action(
                Scope::current(),
                move |result: Result<(Account, String), String>| {
                save_working.set(false);
                match result {
                    Ok((account, message)) => {
                        if account.storage == StorageKind::SessionOnly {
                            record(notices, "Version Control Account", &message);
                        }
                        save_status.set(message);
                    }
                    Err(error) => save_status.set(error),
                }
                refresh_accounts(save_reload);
            },
            );
            thread::spawn(move || {
                let result: Result<(Account, String), String> =
                    vcs_credentials::normalize_host(&host_value).and_then(|normalized| {
                        let (account, validation_note) = match provider_value.as_str() {
                            "github" => {
                                let validation = vcs_credentials::validate_github_token_details(
                                    &normalized,
                                    secret.as_str(),
                                )
                                .map_err(|error| error.message)?;
                                let note = match validation.oauth_scopes {
                                    Some(scopes) if !scopes.trim().is_empty() => format!(
                                        "Classic OAuth scopes reported: {scopes}."
                                    ),
                                    Some(_) => "GitHub reported no classic OAuth scopes; token type cannot be inferred.".into(),
                                    None => "GitHub did not report classic OAuth scopes; token type cannot be inferred.".into(),
                                };
                                let account = vcs_credentials::save_validated_token_with_fallback(
                                    &normalized,
                                    &validation.username,
                                    secret,
                                    allow_fallback,
                                )?;
                                (account, note)
                            }
                            "generic" => {
                                let account = vcs_credentials::save_generic_token_with_fallback(
                                    &normalized,
                                    &username_value,
                                    secret,
                                    allow_fallback,
                                )?;
                                (
                                    account,
                                    "The host has no standard token-validation endpoint; Sparrow will use this credential for HTTPS Git only.".into(),
                                )
                            }
                            _ => return Err("Select a supported account provider.".into()),
                        };
                        let storage_note: String = match &account.storage {
                            StorageKind::Keyring => "Secret saved in the operating-system credential store.".into(),
                            StorageKind::SessionOnly => "Secure storage is unavailable; the secret is kept only in Sparrow's memory until it exits.".into(),
                        };
                        Ok((account, format!("{validation_note} {storage_note}")))
                    });
                done(result);
            });
        });

    let link_host = host;
    let link_status = status;
    let token_page = button(
        || "Create token on this host…",
        ButtonKind::Outlined,
        config,
    )
    .on_click_stop(move |_| {
        let host = link_host.get_untracked();
        match vcs_credentials::token_creation_page(&host) {
            Ok(url) => {
                let send = create_ext_action(Scope::current(), move |result| {
                    if let Err(error) = result {
                        link_status.set(error);
                    }
                });
                thread::spawn(move || {
                    send(open::that(url).map_err(|_| {
                        "Unable to open the token page in your browser.".to_string()
                    }));
                });
            }
            Err(error) => link_status.set(error),
        }
    })
    .style(move |s| s.apply_if(provider.get() != "github", |s| s.hide()));

    let provider_picker = dropdown_with_label(
        provider,
        vec!["github".to_string(), "generic".to_string()],
        config,
        |provider| match provider.as_str() {
            "github" => "GitHub / GitHub Enterprise".into(),
            _ => "Generic HTTPS Git host".into(),
        },
        |_| {},
    );
    let fallback_toggle = button(
        move || {
            if allow_session_fallback.get() {
                "✓ Allow session-only memory fallback (default)"
            } else {
                "Allow session-only memory fallback"
            }
        },
        ButtonKind::Plain,
        config,
    )
    .on_click_stop(move |_| {
        allow_session_fallback.update(|enabled| *enabled = !*enabled);
    });

    let account_list = dyn_stack(
        move || accounts.get(),
        |account| (account.host.clone(), account.username.clone()),
        move |account| {
            let select_host = account.host.clone();
            let select_user = account.username.clone();
            let select_status = status;
            let select_reload = reload;
            let is_active = account.active;
            let select = button(
                move || if is_active { "Selected" } else { "Use for Git" },
                if account.active {
                    ButtonKind::Plain
                } else {
                    ButtonKind::Outlined
                },
                config,
            )
            .on_click_stop(move |_| {
                let host = select_host.clone();
                let username = select_user.clone();
                let send = create_ext_action(Scope::current(), move |result| {
                    match result {
                        Ok(()) => select_status.set("Active account updated.".into()),
                        Err(error) => select_status.set(error),
                    }
                    refresh_accounts(select_reload);
                });
                thread::spawn(move || send(vcs_credentials::set_active(&host, &username)));
            });

            let check_account = account.clone();
            let check_status = status;
            let check_reload = reload;
            let verify = button(|| "Test", ButtonKind::Outlined, config)
                .on_click_stop(move |_| {
                    let account = check_account.clone();
                    check_status.set("Checking saved credential…".into());
                    let send = create_ext_action(Scope::current(), move |message: String| {
                        check_status.set(message);
                        refresh_accounts(check_reload);
                    });
                    thread::spawn(move || {
                        let mut invalid_token = false;
                        let result = if account.provider == AccountProvider::GenericHttps {
                            vcs_credentials::token(&account).map(|_| {
                                "Generic HTTPS credential is available to Git; the host has no standard API validation endpoint.".to_string()
                            })
                        } else {
                            vcs_credentials::token(&account).and_then(|secret| {
                                let validation = vcs_credentials::validate_github_token_details(
                                    &account.host,
                                    secret.as_str(),
                                )
                                .map_err(|error| {
                                    invalid_token = error.invalid_token;
                                    error.message
                                })?;
                                if validation.username != account.username {
                                    return Err("The saved token belongs to a different account; replace it.".into());
                                }
                                vcs_credentials::set_status(
                                    &account.host,
                                    &account.username,
                                    AccountStatus::Valid,
                                )?;
                                Ok("Credential is valid.".into())
                            })
                        };
                        if invalid_token {
                            let _ = vcs_credentials::set_status(
                                &account.host,
                                &account.username,
                                AccountStatus::Invalid,
                            );
                        }
                        send(result.unwrap_or_else(|error| error));
                    });
                });

            let remove_host = account.host.clone();
            let remove_user = account.username.clone();
            let remove_status = status;
            let remove_reload = reload;
            let replace_host = host;
            let replace_provider = provider;
            let replace_username = username;
            let replace_token = token;
            let replace_status = status;
            let replace_account = account.clone();
            let replace = button(|| "Replace", ButtonKind::Outlined, config)
                .on_click_stop(move |_| {
                    replace_host.set(replace_account.host.clone());
                    replace_username.set(replace_account.username.clone());
                    replace_provider.set(match &replace_account.provider {
                        AccountProvider::GitHub => "github".into(),
                        AccountProvider::GenericHttps => "generic".into(),
                    });
                    replace_token.set(String::new());
                    replace_status.set(format!(
                        "Paste a replacement token for {} and use the test/save action.",
                        replace_account.username
                    ));
                });
            let remove = button(|| "Remove", ButtonKind::Outlined, config)
                .on_click_stop(move |_| {
                    let host = remove_host.clone();
                    let username = remove_user.clone();
                    let send = create_ext_action(Scope::current(), move |result| {
                        match result {
                            Ok(()) => remove_status.set("Account and stored token removed.".into()),
                            Err(error) => remove_status.set(error),
                        }
                        refresh_accounts(remove_reload);
                    });
                    thread::spawn(move || send(vcs_credentials::remove(&host, &username)));
                });

            stack((
                label(move || account_summary(&account)).style(move |s| {
                    s.flex_grow(1.0_f32)
                        .min_width(0.0)
                        .text_ellipsis()
                        .color(config.get().color(SparrowColor::PANEL_FOREGROUND))
                }),
                select,
                verify,
                replace,
                remove,
            ))
            .style(|s| s.width_full().items_center().gap(6.0).padding_vert(6.0))
        },
    )
    .style(|s| s.flex_col().width_full());

    stack((
        stack((
            label(|| "Remote Accounts").style(|s| s.font_bold().font_size(14.0)),
            label(|| "GitHub.com and GitHub Enterprise tokens are tested before saving. Secrets are kept in the OS credential store; if unavailable, Sparrow will tell you when it can keep a token in memory for this session only.")
                .style(move |s| s.width_full().color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))),
            label(|| "Host").style(|s| s.font_bold()),
            text_input(host, config).placeholder("github.com or your GitHub Enterprise host"),
            label(|| "Provider").style(|s| s.font_bold()),
            provider_picker,
            text_input(username, config)
                .placeholder("Username required for generic HTTPS Git")
                .style(move |s| s.apply_if(provider.get() != "generic", |s| s.hide())),
            label(|| "Personal access token (masked while typing)").style(|s| s.font_bold()),
        ))
        .style(|s| s.flex_col().width_full().gap(8.0)),
        stack((
            text_input(token, config)
                .placeholder("Token is never saved in settings or repository configuration")
                .style(|s| s.color(floem::peniko::Color::TRANSPARENT)),
            label(move || {
                let count = token.with(|value| value.chars().count());
                if count == 0 {
                    "No token entered".into()
                } else {
                    format!("{} token characters hidden", "•".repeat(count.min(32)))
                }
            })
            .style(move |s| {
                s.font_size(11.0)
                    .color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
            }),
            fallback_toggle,
            label(|| "If disabled, Sparrow refuses to save when the operating-system store is unavailable. No plaintext or passphrase-encrypted file fallback is used.")
                .style(move |s| s.width_full().color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))),
            add,
            token_page,
            label(move || {
                if provider.get() == "github" {
                    "GitHub fine-grained tokens need Pull requests: read for PR listing/details, Pull requests: write to create, Issues: read for conversation comments, Contents: read for clone/fetch, and Contents: write for push. Classic PATs need repo (Git) and read:user (identity validation); authorize organization SSO when required."
                } else {
                    "Generic HTTPS credentials are not API-validated. Sparrow supplies the username and token to Git's askpass flow; SSH remotes use your SSH agent instead."
                }
            })
            .style(move |s| s.width_full().color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))),
            label(move || status.get()).style(move |s| {
                s.width_full()
                    .text_ellipsis()
                    .color(config.get().color(SparrowColor::PANEL_FOREGROUND_DIM))
            }),
            label(|| "Saved Accounts").style(|s| s.font_bold().margin_top(12.0)),
            account_list,
        ))
        .style(|s| s.flex_col().width_full().gap(8.0)),
    ))
    .style(|s| s.flex_col().width_full().gap(8.0).padding_vert(12.0))
}
