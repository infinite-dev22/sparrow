//! Secure storage and lookup for remote version-control credentials.

use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::PathBuf,
    sync::{LazyLock, Mutex},
};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use zeroize::Zeroizing;

const KEYRING_SERVICE: &str = "dev.sparrow.vcs";
const ACCOUNTS_FILE: &str = "vcs-accounts.toml";

static ACCOUNTS_LOCK: Mutex<()> = Mutex::new(());
static KEYRING_LOCK: Mutex<()> = Mutex::new(());
static SESSION_TOKENS: LazyLock<
    Mutex<HashMap<(String, String), Zeroizing<String>>>,
> = LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountStatus {
    Unknown,
    Valid,
    Invalid,
}

impl Default for AccountStatus {
    fn default() -> Self {
        Self::Unknown
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageKind {
    Keyring,
    SessionOnly,
}

impl Default for StorageKind {
    fn default() -> Self {
        Self::Keyring
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountProvider {
    GitHub,
    GenericHttps,
}

impl Default for AccountProvider {
    fn default() -> Self {
        Self::GitHub
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Account {
    pub host: String,
    pub username: String,
    #[serde(default)]
    pub provider: AccountProvider,
    #[serde(default)]
    pub status: AccountStatus,
    #[serde(default)]
    pub storage: StorageKind,
    #[serde(default)]
    pub active: bool,
}

#[derive(Default, Deserialize, Serialize)]
struct AccountFile {
    #[serde(default)]
    accounts: Vec<Account>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RemoteHost {
    Https(String),
    Ssh(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApiError {
    pub message: String,
    pub auth_required: bool,
    pub invalid_token: bool,
}

pub struct ApiResponse<T> {
    pub data: T,
    pub oauth_scopes: Option<String>,
}

pub struct ValidatedToken {
    pub username: String,
    pub oauth_scopes: Option<String>,
}

pub trait HostProvider {
    fn api_base(&self, host: &str) -> Result<String, String>;
    fn token_creation_page(&self, host: &str) -> Result<String, String>;
}

pub struct GitHubProvider;

impl HostProvider for GitHubProvider {
    fn api_base(&self, host: &str) -> Result<String, String> {
        let host = normalize_host(host)?;
        if host == "github.com" {
            Ok("https://api.github.com".into())
        } else {
            Ok(format!("https://{host}/api/v3"))
        }
    }

    fn token_creation_page(&self, host: &str) -> Result<String, String> {
        Ok(format!(
            "https://{}/settings/personal-access-tokens/new",
            normalize_host(host)?
        ))
    }
}

pub fn token_creation_page(host: &str) -> Result<String, String> {
    GitHubProvider.token_creation_page(host)
}

fn accounts_path() -> Result<PathBuf, String> {
    sparrow_core::directory::Directory::config_directory()
        .map(|dir| dir.join(ACCOUNTS_FILE))
        .ok_or_else(|| "Sparrow's configuration directory is unavailable.".into())
}

fn read_accounts_from(path: &std::path::Path) -> Result<Vec<Account>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(path).map_err(|_| {
        "Unable to read version-control account metadata.".to_string()
    })?;
    let file: AccountFile = toml::from_str(&text)
        .map_err(|_| "Version-control account metadata is invalid.".to_string())?;
    Ok(file.accounts)
}

fn write_accounts_to(
    path: &std::path::Path,
    accounts: &[Account],
) -> Result<(), String> {
    let content = toml::to_string(&AccountFile {
        accounts: accounts.to_vec(),
    })
    .map_err(|_| "Unable to encode version-control account metadata.".to_string())?;
    let parent = path.parent().ok_or_else(|| {
        "Sparrow's configuration directory is unavailable.".to_string()
    })?;
    fs::create_dir_all(parent).map_err(|_| {
        "Unable to create Sparrow's configuration directory.".to_string()
    })?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|_| "Unable to prepare account metadata safely.".to_string())?;
    temporary
        .write_all(content.as_bytes())
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|_| {
            "Unable to write version-control account metadata.".to_string()
        })?;
    temporary.persist(path).map_err(|_| {
        "Unable to save version-control account metadata.".to_string()
    })?;
    Ok(())
}

pub fn accounts() -> Result<Vec<Account>, String> {
    let _guard = ACCOUNTS_LOCK.lock().map_err(|_| {
        "Version-control account storage is unavailable.".to_string()
    })?;
    read_accounts_from(&accounts_path()?)
}

pub fn normalize_host(input: &str) -> Result<String, String> {
    let input = input.trim();
    if input.is_empty() || input.contains('/') || input.contains('@') {
        return Err("Enter a host name only, such as github.com.".into());
    }
    let parsed = url::Url::parse(&format!("https://{input}"))
        .map_err(|_| "Enter a valid HTTPS host name.".to_string())?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.path() != "/"
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err("Enter a host name only, without credentials or a path.".into());
    }
    let mut host = parsed.host_str().unwrap().to_ascii_lowercase();
    if let Some(port) = parsed.port() {
        host.push(':');
        host.push_str(&port.to_string());
    }
    Ok(host)
}

pub fn remote_host(remote: &str) -> Result<RemoteHost, String> {
    let remote = remote.trim();
    if let Some((_, address)) = remote.rsplit_once('@')
        && !remote.contains("://")
    {
        let host = address.split(':').next().unwrap_or_default();
        return Ok(RemoteHost::Ssh(normalize_host(host)?));
    }
    let parsed = url::Url::parse(remote).map_err(|_| {
        "Remote must use HTTPS or SSH; embedded credentials are not allowed."
            .to_string()
    })?;
    let has_token_query = parsed.query_pairs().any(|(key, _)| {
        matches!(
            key.to_ascii_lowercase().as_str(),
            "access_token" | "token" | "password" | "secret" | "api_key" | "apikey"
        )
    });
    if !parsed.username().is_empty()
        || parsed.password().is_some()
        || has_token_query
    {
        return Err(
            "Remove embedded credentials from the remote URL and use Version Control Accounts."
                .into(),
        );
    }
    match parsed.scheme() {
        "https" => Ok(RemoteHost::Https(normalize_host(
            parsed.host_str().unwrap_or_default(),
        )?)),
        "ssh" => Ok(RemoteHost::Ssh(normalize_host(
            parsed.host_str().unwrap_or_default(),
        )?)),
        _ => Err(
            "Remote must use HTTPS or SSH; embedded credentials are not allowed."
                .into(),
        ),
    }
}

fn keyring_entry(host: &str, username: &str) -> Result<keyring::Entry, String> {
    let identity = format!("{}:{host}{username}", host.len());
    keyring::Entry::new(KEYRING_SERVICE, &identity).map_err(|_| {
        "The operating-system credential store is unavailable.".to_string()
    })
}

fn set_keyring_password(
    host: &str,
    username: &str,
    token: &str,
) -> Result<(), String> {
    let _guard = KEYRING_LOCK.lock().map_err(|_| {
        "Operating-system credential storage is unavailable.".to_string()
    })?;
    keyring_entry(host, username)?
        .set_password(token)
        .map_err(|_| {
            "The operating-system credential store rejected this secret.".to_string()
        })
}

fn get_keyring_password(
    host: &str,
    username: &str,
) -> Result<Zeroizing<String>, String> {
    let _guard = KEYRING_LOCK.lock().map_err(|_| {
        "Operating-system credential storage is unavailable.".to_string()
    })?;
    keyring_entry(host, username)?
        .get_password()
        .map(Zeroizing::new)
        .map_err(|_| {
            "The saved token is unavailable in the operating-system credential store.".into()
        })
}

fn delete_keyring_password(host: &str, username: &str) -> Result<(), String> {
    let _guard = KEYRING_LOCK.lock().map_err(|_| {
        "Operating-system credential storage is unavailable.".to_string()
    })?;
    match keyring_entry(host, username)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err(
            "Unable to remove the credential from the operating-system store."
                .into(),
        ),
    }
}

fn account_key(host: &str, username: &str) -> (String, String) {
    (host.to_string(), username.to_string())
}

/// Store a validated token in the OS keyring, falling back only to a visibly
/// reported in-memory credential when the secure store is unavailable.
pub fn save_validated_token(
    host: &str,
    username: &str,
    token: Zeroizing<String>,
) -> Result<Account, String> {
    save_token(
        host,
        username,
        token,
        AccountProvider::GitHub,
        AccountStatus::Valid,
        true,
    )
}

pub fn save_validated_token_with_fallback(
    host: &str,
    username: &str,
    token: Zeroizing<String>,
    allow_session_fallback: bool,
) -> Result<Account, String> {
    save_token(
        host,
        username,
        token,
        AccountProvider::GitHub,
        AccountStatus::Valid,
        allow_session_fallback,
    )
}

/// Store a generic HTTPS Git credential without claiming that the host's token
/// or username was API-validated.
pub fn save_generic_token(
    host: &str,
    username: &str,
    token: Zeroizing<String>,
) -> Result<Account, String> {
    save_token(
        host,
        username,
        token,
        AccountProvider::GenericHttps,
        AccountStatus::Unknown,
        true,
    )
}

pub fn save_generic_token_with_fallback(
    host: &str,
    username: &str,
    token: Zeroizing<String>,
    allow_session_fallback: bool,
) -> Result<Account, String> {
    save_token(
        host,
        username,
        token,
        AccountProvider::GenericHttps,
        AccountStatus::Unknown,
        allow_session_fallback,
    )
}

fn save_token(
    host: &str,
    username: &str,
    token: Zeroizing<String>,
    provider: AccountProvider,
    status: AccountStatus,
    allow_session_fallback: bool,
) -> Result<Account, String> {
    let host = normalize_host(host)?;
    let username = username.trim().to_string();
    if username.is_empty() {
        return Err("Enter a username for this version-control account.".into());
    }
    let _guard = ACCOUNTS_LOCK.lock().map_err(|_| {
        "Version-control account storage is unavailable.".to_string()
    })?;
    let path = accounts_path()?;
    let mut accounts = read_accounts_from(&path)?;
    let existing_account = accounts
        .iter()
        .find(|account| account.host == host && account.username == username)
        .cloned();
    let previous_keyring_token = match existing_account
        .as_ref()
        .map(|account| &account.storage)
    {
        Some(StorageKind::Keyring) => Some(get_keyring_password(&host, &username)?),
        _ => None,
    };
    let previous_session_token = SESSION_TOKENS
        .lock()
        .map_err(|_| "In-memory credential storage is unavailable.".to_string())?
        .get(&account_key(&host, &username))
        .map(|token| Zeroizing::new(token.to_string()));
    let existing_active = accounts
        .iter()
        .find(|account| account.host == host && account.active)
        .map(|account| account.username.clone());

    let storage = match set_keyring_password(&host, &username, token.as_str()) {
        Ok(()) => StorageKind::Keyring,
        Err(error) if !allow_session_fallback => {
            return Err(format!(
                "{error} Enable session-only fallback or configure an OS credential store."
            ));
        }
        Err(error)
            if matches!(
                existing_account.as_ref().map(|account| &account.storage),
                Some(StorageKind::Keyring)
            ) =>
        {
            return Err(format!(
                "The existing secure credential could not be updated safely: {error}"
            ));
        }
        Err(_) => {
            SESSION_TOKENS
                .lock()
                .map_err(|_| {
                    "In-memory credential storage is unavailable.".to_string()
                })?
                .insert(account_key(&host, &username), token);
            StorageKind::SessionOnly
        }
    };

    let active = existing_active
        .as_deref()
        .is_none_or(|active| active == username);
    let account = Account {
        host: host.clone(),
        username: username.clone(),
        provider,
        status,
        storage,
        active,
    };
    if let Some(existing) = accounts
        .iter_mut()
        .find(|existing| existing.host == host && existing.username == username)
    {
        *existing = account.clone();
    } else {
        accounts.push(account.clone());
    }
    if let Err(error) = write_accounts_to(&path, &accounts) {
        let mut rollback_error = None;
        if let Ok(mut session_tokens) = SESSION_TOKENS.lock() {
            match previous_session_token {
                Some(token) => {
                    session_tokens.insert(account_key(&host, &username), token);
                }
                None => {
                    session_tokens.remove(&account_key(&host, &username));
                }
            }
        } else {
            rollback_error =
                Some("in-memory credential rollback failed".to_string());
        }
        if matches!(account.storage, StorageKind::Keyring) {
            let restored = match previous_keyring_token {
                Some(token) => {
                    set_keyring_password(&host, &username, token.as_str())
                }
                None => delete_keyring_password(&host, &username),
            };
            if restored.is_err() {
                rollback_error = Some("OS credential rollback failed".to_string());
            }
        }
        return Err(rollback_error.map_or(error.clone(), |rollback| {
            format!("{error}; {rollback}, so inspect this account before retrying.")
        }));
    }
    if account.storage == StorageKind::Keyring {
        SESSION_TOKENS
            .lock()
            .map_err(|_| "In-memory credential storage is unavailable.".to_string())?
            .remove(&account_key(&host, &username));
    }
    Ok(account)
}

pub fn set_active(host: &str, username: &str) -> Result<(), String> {
    let host = normalize_host(host)?;
    let _guard = ACCOUNTS_LOCK.lock().map_err(|_| {
        "Version-control account storage is unavailable.".to_string()
    })?;
    let path = accounts_path()?;
    let mut accounts = read_accounts_from(&path)?;
    let mut found = false;
    for account in &mut accounts {
        if account.host == host {
            account.active = account.username == username;
            found |= account.active;
        }
    }
    if !found {
        return Err("That account is no longer available.".into());
    }
    write_accounts_to(&path, &accounts)
}

pub fn set_status(
    host: &str,
    username: &str,
    status: AccountStatus,
) -> Result<(), String> {
    let host = normalize_host(host)?;
    let _guard = ACCOUNTS_LOCK.lock().map_err(|_| {
        "Version-control account storage is unavailable.".to_string()
    })?;
    let path = accounts_path()?;
    let mut accounts = read_accounts_from(&path)?;
    let Some(account) = accounts
        .iter_mut()
        .find(|account| account.host == host && account.username == username)
    else {
        return Ok(());
    };
    account.status = status;
    write_accounts_to(&path, &accounts)
}

pub fn remove(host: &str, username: &str) -> Result<(), String> {
    let host = normalize_host(host)?;
    let _guard = ACCOUNTS_LOCK.lock().map_err(|_| {
        "Version-control account storage is unavailable.".to_string()
    })?;
    let path = accounts_path()?;
    let mut accounts = read_accounts_from(&path)?;
    let removed = accounts
        .iter()
        .find(|account| account.host == host && account.username == username)
        .cloned();
    let Some(removed) = removed else {
        return Ok(());
    };
    let original_accounts = accounts.clone();
    accounts
        .retain(|account| !(account.host == host && account.username == username));
    if !accounts
        .iter()
        .any(|account| account.host == host && account.active)
        && let Some(next) = accounts.iter_mut().find(|account| account.host == host)
    {
        next.active = true;
    }
    write_accounts_to(&path, &accounts)?;
    let cleanup = match removed.storage {
        StorageKind::Keyring => delete_keyring_password(&host, username),
        StorageKind::SessionOnly => SESSION_TOKENS
            .lock()
            .map_err(|_| "In-memory credential storage is unavailable.".to_string())
            .map(|mut tokens| {
                tokens.remove(&account_key(&host, username));
            }),
    };
    if let Err(error) = cleanup {
        return match write_accounts_to(&path, &original_accounts) {
            Ok(()) => Err(error),
            Err(rollback) => Err(format!(
                "{error}; account metadata rollback also failed: {rollback}"
            )),
        };
    }
    Ok(())
}

pub fn active_account(host: &str) -> Result<Option<Account>, String> {
    let host = normalize_host(host)?;
    let active = accounts()?
        .into_iter()
        .filter(|account| account.host == host && account.active)
        .collect::<Vec<_>>();
    match active.as_slice() {
        [] => Ok(None),
        [account] => Ok(Some(account.clone())),
        _ => Err("Multiple accounts are marked active for this host; choose one in Settings.".into()),
    }
}

pub fn token(account: &Account) -> Result<Zeroizing<String>, String> {
    let key = account_key(&account.host, &account.username);
    if account.storage == StorageKind::SessionOnly {
        return SESSION_TOKENS
            .lock()
            .map_err(|_| "In-memory credential storage is unavailable.".to_string())?
            .get(&key)
            .map(|token| Zeroizing::new(token.to_string()))
            .ok_or_else(|| {
                "This session-only token is unavailable; add it again.".into()
            });
    }
    get_keyring_password(&account.host, &account.username)
}

pub fn active_token(
    host: &str,
) -> Result<Option<(Account, Zeroizing<String>)>, String> {
    let Some(account) = active_account(host)? else {
        return Ok(None);
    };
    let token = token(&account)?;
    Ok(Some((account, token)))
}

pub fn active_github_token(
    host: &str,
) -> Result<Option<(Account, Zeroizing<String>)>, String> {
    let Some(account) = active_account(host)? else {
        return Ok(None);
    };
    if account.provider != AccountProvider::GitHub {
        return Err(
            "The selected account is configured for generic Git only; choose a GitHub-compatible account for Pull Requests.".into(),
        );
    }
    let token = token(&account)?;
    Ok(Some((account, token)))
}

fn api_base(host: &str) -> Result<String, String> {
    GitHubProvider.api_base(host)
}

fn api_error(
    status: reqwest::StatusCode,
    headers: &reqwest::header::HeaderMap,
    has_token: bool,
    host: &str,
) -> ApiError {
    let rate_limited = headers
        .get("x-ratelimit-remaining")
        .and_then(|value| value.to_str().ok())
        == Some("0");
    let sso_required = headers
        .get("x-github-sso")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("required"));
    let permissions = headers
        .get("x-accepted-github-permissions")
        .and_then(|value| value.to_str().ok());
    let (message, auth_required, invalid_token) = match status.as_u16() {
        401 => (
            "GitHub rejected this token; it may be invalid, expired, or revoked. Replace it in Version Control Accounts.".into(),
            !has_token,
            has_token,
        ),
        403 if sso_required => (
            "This token needs organization SSO authorization before it can access the repository.".into(),
            true,
            false,
        ),
        403 if rate_limited => (
            "GitHub API rate limit reached. Wait before refreshing, or use an authenticated account.".into(),
            !has_token,
            false,
        ),
        403 => {
            let detail = permissions.map_or_else(String::new, |value| {
                format!(" GitHub reports the required permission as: {value}.")
            });
            (
                format!("GitHub denied access. Check repository access and token permissions.{detail}"),
                !has_token || permissions.is_some(),
                false,
            )
        }
        404 => (
            "GitHub could not find this repository or the selected account cannot access it. Verify the repository, or manage the account and token.".into(),
            true,
            false,
        ),
        422 => (
            "GitHub could not create this pull request. Check the head/base branches and required repository permissions.".into(),
            false,
            false,
        ),
        429 => (
            "GitHub rate limit reached. Wait before retrying or reduce the number of refreshes.".into(),
            false,
            false,
        ),
        _ => (
            format!("GitHub returned HTTP {} while handling this request.", status.as_u16()),
            false,
            false,
        ),
    };
    let message = if auth_required {
        format!(
            "{message} Add an account for {host} in Settings > Version Control > Accounts."
        )
    } else {
        message
    };
    ApiError {
        message,
        auth_required,
        invalid_token,
    }
}

fn github_api_request_at<T: DeserializeOwned>(
    base: &str,
    host: &str,
    method: reqwest::Method,
    path: &str,
    query: &[(&str, &str)],
    token: Option<&str>,
    body: Option<serde_json::Value>,
) -> Result<ApiResponse<T>, ApiError> {
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(12))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("Sparrow-Dart-Flutter-IDE")
        .build()
        .map_err(|error| ApiError {
            message: format!(
                "Unable to prepare GitHub request: {}",
                error.without_url()
            ),
            auth_required: false,
            invalid_token: false,
        })?;
    let url = format!("{}/{path}", base.trim_end_matches('/'));
    let mut request = client
        .request(method, url)
        .query(query)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(body) = body {
        request = request.json(&body);
    }
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    let response = request.send().map_err(|error| ApiError {
        message: format!("Unable to reach {host}: {}", error.without_url()),
        auth_required: false,
        invalid_token: false,
    })?;
    if !response.status().is_success() {
        return Err(api_error(
            response.status(),
            response.headers(),
            token.is_some(),
            host,
        ));
    }
    let oauth_scopes = response
        .headers()
        .get("x-oauth-scopes")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let data = response.json().map_err(|_| ApiError {
        message: "The GitHub response was unreadable.".into(),
        auth_required: false,
        invalid_token: false,
    })?;
    Ok(ApiResponse { data, oauth_scopes })
}

pub fn github_api_json<T: DeserializeOwned>(
    host: &str,
    path: &str,
    query: &[(&str, &str)],
    token: Option<&str>,
) -> Result<T, ApiError> {
    let base = api_base(host).map_err(|message| ApiError {
        message,
        auth_required: false,
        invalid_token: false,
    })?;
    let host = normalize_host(host).unwrap_or_else(|_| "GitHub".into());
    github_api_request_at(
        &base,
        &host,
        reqwest::Method::GET,
        path,
        query,
        token,
        None,
    )
    .map(|response| response.data)
}

pub fn github_api_post_json<T: DeserializeOwned, B: Serialize>(
    host: &str,
    path: &str,
    body: &B,
    token: &str,
) -> Result<T, ApiError> {
    let base = api_base(host).map_err(|message| ApiError {
        message,
        auth_required: false,
        invalid_token: false,
    })?;
    let host = normalize_host(host).unwrap_or_else(|_| "GitHub".into());
    let body = serde_json::to_value(body).map_err(|_| ApiError {
        message: "Unable to prepare GitHub request body.".into(),
        auth_required: false,
        invalid_token: false,
    })?;
    github_api_request_at(
        &base,
        &host,
        reqwest::Method::POST,
        path,
        &[],
        Some(token),
        Some(body),
    )
    .map(|response| response.data)
}

pub fn validate_github_token(host: &str, token: &str) -> Result<String, ApiError> {
    validate_github_token_details(host, token).map(|result| result.username)
}

pub fn validate_github_token_details(
    host: &str,
    token: &str,
) -> Result<ValidatedToken, ApiError> {
    #[derive(Deserialize)]
    struct User {
        login: String,
    }
    let base = api_base(host).map_err(|message| ApiError {
        message,
        auth_required: false,
        invalid_token: false,
    })?;
    let host = normalize_host(host).unwrap_or_else(|_| "GitHub".into());
    let response: ApiResponse<User> = github_api_request_at(
        &base,
        &host,
        reqwest::Method::GET,
        "user",
        &[],
        Some(token),
        None,
    )?;
    Ok(ValidatedToken {
        username: response.data.login,
        oauth_scopes: response.oauth_scopes,
    })
}

pub fn redact(value: &str, secret: &str) -> String {
    if secret.is_empty() {
        value.to_string()
    } else {
        value.replace(secret, "[REDACTED]")
    }
}

fn host_from_prompt(prompt: &str) -> Option<String> {
    let url = prompt
        .split(['\'', '"'])
        .find(|part| part.starts_with("https://"))?;
    let parsed = url::Url::parse(url).ok()?;
    let mut host = parsed.host_str()?.to_string();
    if let Some(port) = parsed.port() {
        host.push(':');
        host.push_str(&port.to_string());
    }
    normalize_host(&host).ok()
}

fn session_credential(host: &str) -> Option<(String, Zeroizing<String>)> {
    let count = std::env::var("SPARROW_GIT_SESSION_COUNT")
        .ok()?
        .parse::<usize>()
        .ok()?
        .min(128);
    for index in 0..count {
        let suffix = index.to_string();
        let stored_host =
            std::env::var(format!("SPARROW_GIT_SESSION_HOST_{suffix}")).ok()?;
        if normalize_host(&stored_host).ok().as_deref() != Some(host) {
            continue;
        }
        let username =
            std::env::var(format!("SPARROW_GIT_SESSION_USERNAME_{suffix}")).ok()?;
        let token =
            std::env::var(format!("SPARROW_GIT_SESSION_TOKEN_{suffix}")).ok()?;
        return Some((username, Zeroizing::new(token)));
    }
    None
}

/// Entry point used only when Git launches Sparrow as `GIT_ASKPASS`.
pub fn askpass() -> i32 {
    if std::env::var_os("SPARROW_GIT_ASKPASS_MODE").is_none() {
        return 1;
    }
    let prompt = std::env::args().nth(1).unwrap_or_default();
    let Some(host) = host_from_prompt(&prompt) else {
        return 1;
    };
    let session = session_credential(&host);
    let prompt = prompt.to_ascii_lowercase();
    if prompt.contains("username") {
        let username = session.map(|(username, _)| username).or_else(|| {
            active_account(&host)
                .ok()
                .flatten()
                .map(|account| account.username)
        });
        let Some(username) = username else {
            return 1;
        };
        println!("{username}");
        return 0;
    }
    if prompt.contains("password") {
        let token = session
            .map(|(_, token)| token)
            .or_else(|| active_token(&host).ok().flatten().map(|(_, token)| token));
        let Some(token) = token else {
            return 1;
        };
        println!("{}", token.as_str());
        return 0;
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Read, net::TcpListener, thread};

    #[test]
    fn account_metadata_never_contains_or_serializes_a_token() {
        let account = Account {
            host: "github.com".into(),
            username: "sparrow-user".into(),
            provider: AccountProvider::GitHub,
            status: AccountStatus::Valid,
            storage: StorageKind::Keyring,
            active: true,
        };
        let encoded = toml::to_string(&AccountFile {
            accounts: vec![account],
        })
        .unwrap();
        assert!(encoded.contains("sparrow-user"));
        assert!(!encoded.contains("ghp_test_secret"));
    }

    #[test]
    fn host_parser_rejects_embedded_credentials_and_paths() {
        assert_eq!(normalize_host(" GitHub.com ").unwrap(), "github.com");
        assert!(normalize_host("user:token@github.com").is_err());
        assert!(normalize_host("github.com/api/v3").is_err());
        assert_eq!(
            token_creation_page("github.com").unwrap(),
            "https://github.com/settings/personal-access-tokens/new"
        );
    }

    #[test]
    fn remote_parser_distinguishes_secure_https_and_ssh_without_credentials() {
        assert_eq!(
            remote_host("https://github.com/owner/repo.git").unwrap(),
            RemoteHost::Https("github.com".into())
        );
        assert_eq!(
            remote_host("git@github.com:owner/repo.git").unwrap(),
            RemoteHost::Ssh("github.com".into())
        );
        assert!(
            remote_host("https://user:token@github.com/owner/repo.git").is_err()
        );
        assert!(
            remote_host("https://github.com/owner/repo?access_token=secret")
                .is_err()
        );
    }

    #[test]
    fn error_redaction_removes_an_exact_token() {
        assert_eq!(
            redact("request failed for secret-value", "secret-value"),
            "request failed for [REDACTED]"
        );
        assert_eq!(redact("no secret", ""), "no secret");
    }

    #[test]
    fn account_metadata_round_trips_without_secrets() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("vcs-accounts.toml");
        let account = Account {
            host: "github.com".into(),
            username: "ada".into(),
            provider: AccountProvider::GenericHttps,
            status: AccountStatus::Unknown,
            storage: StorageKind::SessionOnly,
            active: true,
        };
        write_accounts_to(&path, std::slice::from_ref(&account)).unwrap();
        assert_eq!(read_accounts_from(&path).unwrap(), vec![account]);
        assert!(!fs::read_to_string(path).unwrap().contains("token"));
    }

    #[test]
    fn authenticated_api_requests_use_bearer_headers() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            let size = stream.read(&mut request).unwrap();
            let request =
                String::from_utf8_lossy(&request[..size]).to_ascii_lowercase();
            let body = r#"{"login":"ada"}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nX-OAuth-Scopes: repo, read:user\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
            request
        });
        let response: ApiResponse<serde_json::Value> = github_api_request_at(
            &format!("http://{address}"),
            "github.com",
            reqwest::Method::GET,
            "user",
            &[],
            Some("session-secret"),
            None,
        )
        .unwrap();
        let request = server.join().unwrap();
        assert_eq!(response.data["login"], "ada");
        assert_eq!(response.oauth_scopes.as_deref(), Some("repo, read:user"));
        assert!(request.contains("authorization: bearer session-secret"));
    }

    #[test]
    fn authenticated_post_sends_json_and_does_not_expose_the_token_in_the_url() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 1024];
            let mut body_end = None;
            loop {
                let size = stream.read(&mut buffer).unwrap();
                request.extend_from_slice(&buffer[..size]);
                if body_end.is_none()
                    && let Some(header_end) =
                        request.windows(4).position(|w| w == b"\r\n\r\n")
                {
                    let headers = String::from_utf8_lossy(&request[..header_end]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length: ")
                                .and_then(|value| value.parse::<usize>().ok())
                        })
                        .unwrap_or_default();
                    body_end = Some(header_end + 4 + length);
                }
                if body_end.is_some_and(|end| request.len() >= end) {
                    break;
                }
            }
            let request = String::from_utf8_lossy(&request).to_ascii_lowercase();
            let body = r#"{"number":42}"#;
            write!(
                stream,
                "HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
            request
        });
        let body = serde_json::json!({"title": "Add feature", "head": "work", "base": "main"});
        let response: ApiResponse<serde_json::Value> = github_api_request_at(
            &format!("http://{address}"),
            "github.com",
            reqwest::Method::POST,
            "repos/owner/repo/pulls",
            &[],
            Some("session-secret"),
            Some(body),
        )
        .unwrap();
        let request = server.join().unwrap();
        assert_eq!(response.data["number"], 42);
        assert!(request.contains("authorization: bearer session-secret"));
        assert!(request.contains("/repos/owner/repo/pulls "));
        assert!(request.contains("\"title\":\"add feature\""));
        assert!(!request.contains("session-secret?"));
    }

    #[test]
    fn github_errors_are_actionable_and_do_not_echo_tokens() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "x-accepted-github-permissions",
            "pull_requests=read".parse().unwrap(),
        );
        let error =
            api_error(reqwest::StatusCode::FORBIDDEN, &headers, true, "github.com");
        assert!(error.message.contains("pull_requests=read"));
        assert!(!error.message.contains("session-secret"));
        assert!(error.auth_required);
        assert!(!error.invalid_token);
    }

    #[test]
    fn git_askpass_prompt_uses_the_https_host_only() {
        assert_eq!(
            host_from_prompt("Username for 'https://github.com': "),
            Some("github.com".into())
        );
        assert_eq!(
            host_from_prompt("Password for 'https://ada@git.example.com:8443': "),
            Some("git.example.com:8443".into())
        );
        assert_eq!(host_from_prompt("Enter passphrase for key"), None);
    }

    #[test]
    fn github_provider_supports_public_and_enterprise_api_hosts() {
        assert_eq!(api_base("github.com").unwrap(), "https://api.github.com");
        assert_eq!(
            api_base("git.example.com:8443").unwrap(),
            "https://git.example.com:8443/api/v3"
        );
        assert_eq!(
            token_creation_page("github.com").unwrap(),
            "https://github.com/settings/personal-access-tokens/new"
        );
    }

    #[test]
    fn invalid_token_and_rate_limit_errors_are_distinct_and_sanitized() {
        let invalid = api_error(
            reqwest::StatusCode::UNAUTHORIZED,
            &reqwest::header::HeaderMap::new(),
            true,
            "github.com",
        );
        assert!(invalid.invalid_token);
        assert!(!invalid.auth_required);
        assert!(!invalid.message.contains("ghp_sensitive"));

        let limited = api_error(
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            &reqwest::header::HeaderMap::new(),
            true,
            "github.com",
        );
        assert!(limited.message.contains("rate limit"));
        assert!(!limited.invalid_token);
    }
}
