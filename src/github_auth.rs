//! GitHub authentication, OS credential store integration, and repository discovery.
//!
//! Tokens are stored exclusively in the operating system's native credential store:
//! - Windows: Windows Credential Manager (`CredWriteW` / `CredReadW` / `CredEnumerateW` / `CredDeleteW`)
//! - macOS: macOS Keychain (`/usr/bin/security`)
//!
//! Each copy of Launcher keys its credential under `JavacrafterLauncher/github/<install-tag>`
//! so multiple or previously deleted copies can be enumerated and cleared cleanly from
//! Settings without leaving stale or duplicated entries behind.
//!
//! Raw tokens are kept only in process memory (`TOKEN`) and are never serialized to
//! `data/settings.json` or sent to the WebView.

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::RwLock;

use crate::settings::{app_dir, Settings};
use crate::updater::{
    agent, api_base, detect_platforms_from_assets, parse_version_lenient, select_release_asset_for_os,
    GhAsset, GhRelease,
};

/// Prefix shared by every credential written by any copy of Launcher.
pub const CRED_PREFIX: &str = "JavacrafterLauncher";
const CRED_GITHUB_PREFIX: &str = "JavacrafterLauncher/github/";

/// Optional compile-time GitHub OAuth App Client ID for Device Flow login.
/// Can be set at build time via `LAUNCHER_GITHUB_CLIENT_ID`, or configured at
/// runtime in Settings → GitHub Account.
pub const BUILTIN_OAUTH_CLIENT_ID: &str = match option_env!("LAUNCHER_GITHUB_CLIENT_ID") {
    Some(id) => id,
    None => "",
};

static TOKEN: RwLock<Option<String>> = RwLock::new(None);

/// Returns the currently active GitHub token from process memory, if signed in.
pub fn current_token() -> Option<String> {
    TOKEN.read().ok().and_then(|g| g.clone())
}

/// Replaces the in-memory GitHub token.
pub fn set_current_token(token: Option<String>) {
    if let Ok(mut g) = TOKEN.write() {
        *g = token.filter(|t| !t.trim().is_empty());
    }
}

/// Human-readable name of the credential store for this OS.
pub fn store_name() -> &'static str {
    if cfg!(windows) {
        "Windows Credential Manager"
    } else if cfg!(target_os = "macos") {
        "macOS Keychain"
    } else {
        "Local credential store"
    }
}

/// Stable 8-hex-char tag for this copy's `app_dir()`, matching `instance.rs`'s FNV-1a scheme.
pub fn install_tag() -> String {
    let key = app_dir().to_string_lossy().to_lowercase();
    let hash = key
        .bytes()
        .fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
    format!("{:08x}", (hash ^ (hash >> 32)) as u32)
}

/// Credential key/service name for this copy of Launcher.
pub fn current_cred_key() -> String {
    format!("{CRED_GITHUB_PREFIX}{}", install_tag())
}

/// One Launcher credential entry discovered in the OS credential store (no secret included).
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StoredCredential {
    pub key: String,
    pub account: String,
    pub method: String,
    pub install_dir: String,
    pub is_current: bool,
    pub dir_exists: bool,
}

/// Non-secret auth summary sent to the WebView.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthStatus {
    pub logged_in: bool,
    pub username: Option<String>,
    pub avatar_url: Option<String>,
    pub method: Option<String>,
    pub store_name: &'static str,
    pub has_builtin_oauth: bool,
    pub current_key: String,
}

impl AuthStatus {
    pub fn from_settings(settings: &Settings) -> Self {
        let logged_in = current_token().is_some();
        Self {
            logged_in,
            username: if logged_in { settings.github_user.clone() } else { None },
            avatar_url: if logged_in { settings.github_avatar.clone() } else { None },
            method: if logged_in { settings.github_method.clone() } else { None },
            store_name: store_name(),
            has_builtin_oauth: !BUILTIN_OAUTH_CLIENT_ID.trim().is_empty(),
            current_key: current_cred_key(),
        }
    }
}

/// Metadata returned when validating a GitHub token via `/user`.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubUser {
    pub login: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, alias = "avatar_url")]
    pub avatar_url: Option<String>,
}

/// Summary of a GitHub repository shown in the private-repo picker.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoSummary {
    pub full_name: String,
    pub name: String,
    pub owner: String,
    pub description: String,
    pub private: bool,
    pub updated_at: String,
    pub html_url: String,
}

/// Preview shown in the "Where should this app appear?" popup before adding a repo.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoPreview {
    pub repo: String,
    pub name: String,
    pub publisher: String,
    pub tagline: String,
    pub private: bool,
    pub own_format: bool,
    pub platforms: Vec<String>,
    pub latest_version: Option<String>,
    pub matched_asset: Option<String>,
    pub has_releases: bool,
}

/// Response from GitHub's OAuth Device Flow initiation endpoint.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCode {
    #[serde(alias = "device_code")]
    pub device_code: String,
    #[serde(alias = "user_code")]
    pub user_code: String,
    #[serde(alias = "verification_uri")]
    pub verification_uri: String,
    #[serde(alias = "expires_in")]
    pub expires_in: u64,
    #[serde(default = "default_poll_interval")]
    pub interval: u64,
}

fn default_poll_interval() -> u64 {
    5
}

pub enum DevicePoll {
    Pending,
    SlowDown(u64),
    Token(String),
    Error(String),
}

/// Loads this copy's saved GitHub token from the OS credential store on startup.
pub fn init_from_store(settings: &mut Settings) {
    let key = current_cred_key();
    match os_store::read_token(&key) {
        Ok(Some((token, account, method))) => {
            set_current_token(Some(token));
            if settings.github_user.is_none() && !account.is_empty() {
                settings.github_user = Some(account);
            }
            if settings.github_method.is_none() && !method.is_empty() {
                settings.github_method = Some(method);
            }
        }
        _ => {
            set_current_token(None);
            if settings.github_user.is_some()
                || settings.github_avatar.is_some()
                || settings.github_method.is_some()
            {
                settings.github_user = None;
                settings.github_avatar = None;
                settings.github_method = None;
                let _ = settings.save();
            }
        }
    }
}

/// Saves the token in the OS credential store for this copy of Launcher and activates it.
pub fn save_active_token(token: &str, username: &str, method: &str) -> Result<(), String> {
    let key = current_cred_key();
    let comment = encode_comment(method, &app_dir().display().to_string());
    os_store::write_token(&key, username, &comment, token)?;
    set_current_token(Some(token.to_string()));
    Ok(())
}

/// Signs out this copy of Launcher by removing its entry from the OS credential store.
pub fn sign_out(account: Option<&str>) -> Result<(), String> {
    let key = current_cred_key();
    set_current_token(None);
    os_store::delete_entry(&key, account)
}

/// Lists every Launcher credential currently in the OS credential store (including old copies).
pub fn list_credentials() -> Result<Vec<StoredCredential>, String> {
    let cur = current_cred_key();
    let mut entries = os_store::list_entries()?;
    for e in &mut entries {
        e.is_current = e.key == cur;
        e.dir_exists = !e.install_dir.is_empty() && Path::new(&e.install_dir).exists();
    }
    // Current copy first, then newest/alphabetical.
    entries.sort_by(|a, b| b.is_current.cmp(&a.is_current).then_with(|| a.key.cmp(&b.key)));
    Ok(entries)
}

/// Deletes one specific credential entry from the OS credential store.
/// If it was the current copy's entry, also clears the in-memory token.
pub fn delete_credential(key: &str, account: Option<&str>) -> Result<bool, String> {
    if !key.starts_with(CRED_PREFIX) {
        return Err("Refusing to delete a credential that does not belong to Launcher.".into());
    }
    let was_current = key == current_cred_key();
    os_store::delete_entry(key, account)?;
    if was_current {
        set_current_token(None);
    }
    Ok(was_current)
}

/// Deletes every Launcher credential belonging to other or deleted copies of Launcher,
/// keeping this copy's active credential intact.
pub fn clear_old_credentials() -> Result<usize, String> {
    let cur = current_cred_key();
    let entries = os_store::list_entries()?;
    let mut removed = 0;
    for e in entries {
        if e.key != cur {
            if os_store::delete_entry(&e.key, Some(&e.account)).is_ok() {
                removed += 1;
            }
        }
    }
    Ok(removed)
}

/// Deletes EVERY Launcher credential from the OS credential store (current + old/duplicates)
/// and signs out this copy.
pub fn clear_all_credentials() -> Result<usize, String> {
    let entries = os_store::list_entries()?;
    let mut removed = 0;
    for e in entries {
        if os_store::delete_entry(&e.key, Some(&e.account)).is_ok() {
            removed += 1;
        }
    }
    // Also attempt direct deletion of this copy's key in case enumeration missed it.
    let cur = current_cred_key();
    if removed == 0 && os_store::delete_entry(&cur, None).is_ok() {
        removed = 1;
    }
    set_current_token(None);
    Ok(removed)
}

fn encode_comment(method: &str, dir: &str) -> String {
    format!("{method}|{dir}")
}

fn decode_comment(comment: &str) -> (String, String) {
    if let Some((method, dir)) = comment.split_once('|') {
        (method.trim().to_string(), dir.trim().to_string())
    } else if !comment.trim().is_empty() {
        ("unknown".into(), comment.trim().to_string())
    } else {
        ("unknown".into(), String::new())
    }
}

/* ----------------------------------------------------- GitHub OAuth & API */

fn form_encode(val: &str) -> String {
    let mut out = String::with_capacity(val.len());
    for b in val.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Resolves the effective OAuth Client ID (built-in constant or user-configured in Settings).
pub fn resolve_oauth_client_id(settings: &Settings) -> Option<String> {
    let builtin = BUILTIN_OAUTH_CLIENT_ID.trim();
    if !builtin.is_empty() {
        return Some(builtin.to_string());
    }
    settings
        .github_oauth_client_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

/// Starts GitHub's OAuth Device Flow (`POST https://github.com/login/device/code`).
pub fn start_device_flow(client_id: &str) -> Result<DeviceCode, String> {
    let client_id = client_id.trim();
    if client_id.is_empty() {
        return Err("Enter a GitHub OAuth App Client ID first, or sign in with a Personal Access Token.".into());
    }
    let body = format!(
        "client_id={}&scope={}",
        form_encode(client_id),
        form_encode("repo read:user")
    );
    let mut resp = agent()
        .post("https://github.com/login/device/code")
        .header("Accept", "application/json")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .send(&body)
        .map_err(|_| "Couldn't reach GitHub to start sign-in. Are you online?".to_string())?;
    let status = resp.status().as_u16();
    let v: serde_json::Value = resp
        .body_mut()
        .read_json()
        .map_err(|e| format!("GitHub returned an unreadable OAuth response ({e})."))?;
    if status != 200 || v.get("error").is_some() {
        let desc = v["error_description"]
            .as_str()
            .or_else(|| v["error"].as_str())
            .unwrap_or("GitHub rejected the OAuth Client ID. Make sure Device Flow is enabled on your GitHub OAuth App.");
        return Err(desc.to_string());
    }
    serde_json::from_value::<DeviceCode>(v)
        .map_err(|e| format!("Unexpected OAuth device code response ({e})."))
}

/// Polls GitHub once for Device Flow authorization completion.
pub fn poll_device_token(client_id: &str, device_code: &str) -> DevicePoll {
    let body = format!(
        "client_id={}&device_code={}&grant_type={}",
        form_encode(client_id),
        form_encode(device_code),
        form_encode("urn:ietf:params:oauth:grant-type:device_code")
    );
    let mut resp = match agent()
        .post("https://github.com/login/oauth/access_token")
        .header("Accept", "application/json")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .send(&body)
    {
        Ok(r) => r,
        Err(_) => return DevicePoll::Pending, // transient network blip
    };
    let Ok(v) = resp.body_mut().read_json::<serde_json::Value>() else {
        return DevicePoll::Pending;
    };
    if let Some(tok) = v["access_token"].as_str().filter(|s| !s.is_empty()) {
        return DevicePoll::Token(tok.to_string());
    }
    match v["error"].as_str().unwrap_or("") {
        "authorization_pending" => DevicePoll::Pending,
        "slow_down" => DevicePoll::SlowDown(v["interval"].as_u64().unwrap_or(10)),
        "expired_token" => DevicePoll::Error("The sign-in code expired. Try signing in again.".into()),
        "access_denied" => DevicePoll::Error("GitHub sign-in was cancelled.".into()),
        other => {
            let desc = v["error_description"].as_str().unwrap_or(other);
            DevicePoll::Error(if desc.is_empty() {
                "GitHub sign-in failed.".into()
            } else {
                desc.to_string()
            })
        }
    }
}

/// Validates a GitHub token and fetches the authenticated user profile.
pub fn fetch_user(token: &str) -> Result<GitHubUser, String> {
    let token = token.trim();
    if token.is_empty() {
        return Err("Paste a GitHub Personal Access Token first.".into());
    }
    let url = format!("{}/user", api_base());
    let mut resp = agent()
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("Authorization", &format!("Bearer {token}"))
        .call()
        .map_err(|_| "Couldn't reach GitHub. Are you online?".to_string())?;
    match resp.status().as_u16() {
        200 => resp
            .body_mut()
            .read_json::<GitHubUser>()
            .map_err(|e| format!("Couldn't read GitHub profile ({e}).")),
        401 => Err("That GitHub token was rejected (HTTP 401). Check that it is valid and hasn't expired.".into()),
        403 => Err("GitHub denied access for this token (HTTP 403).".into()),
        s => Err(format!("GitHub answered with HTTP {s}.")),
    }
}

#[derive(Deserialize)]
struct GhRepoItem {
    full_name: String,
    name: String,
    owner: GhOwner,
    description: Option<String>,
    private: bool,
    updated_at: Option<String>,
    html_url: String,
}

#[derive(Deserialize)]
struct GhOwner {
    login: String,
}

/// Lists private repositories accessible to the signed-in user.
pub fn fetch_private_repos() -> Result<Vec<RepoSummary>, String> {
    let Some(token) = current_token() else {
        return Err("Sign in to GitHub first to browse private repositories.".into());
    };
    let mut out = Vec::new();
    for page in 1..=2 {
        let url = format!(
            "{}/user/repos?visibility=private&affiliation=owner,collaborator,organization_member&sort=updated&per_page=100&page={page}",
            api_base()
        );
        let mut resp = agent()
            .get(&url)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("Authorization", &format!("Bearer {token}"))
            .call()
            .map_err(|_| "Couldn't reach GitHub. Are you online?".to_string())?;
        match resp.status().as_u16() {
            200 => {
                let items: Vec<GhRepoItem> = resp
                    .body_mut()
                    .read_json()
                    .map_err(|e| format!("Couldn't read repository list ({e})."))?;
                let count = items.len();
                for r in items {
                    if r.private {
                        out.push(RepoSummary {
                            full_name: r.full_name,
                            name: r.name,
                            owner: r.owner.login,
                            description: r.description.unwrap_or_default(),
                            private: true,
                            updated_at: r.updated_at.unwrap_or_default(),
                            html_url: r.html_url,
                        });
                    }
                }
                if count < 100 {
                    break;
                }
            }
            401 => return Err("Your GitHub sign-in expired. Sign in again.".into()),
            403 => return Err("Your GitHub token doesn't have permission to list private repositories (needs 'repo' or Contents read access).".into()),
            s => return Err(format!("GitHub answered with HTTP {s}.")),
        }
    }
    Ok(out)
}

/// Parses a GitHub repository URL or `"owner/repo"` string into `"owner/repo"`.
pub fn parse_github_repo(input: &str) -> Result<String, String> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err("Paste a GitHub repository link (for example, https://github.com/owner/repo).".into());
    }
    let without_scheme = raw
        .strip_prefix("https://")
        .or_else(|| raw.strip_prefix("http://"))
        .unwrap_or(raw);
    let path = if let Some(rest) = without_scheme
        .strip_prefix("www.github.com/")
        .or_else(|| without_scheme.strip_prefix("github.com/"))
    {
        rest
    } else if let Some(rest) = raw.strip_prefix("git@github.com:") {
        rest
    } else if !without_scheme.contains('.') && without_scheme.contains('/') {
        without_scheme
    } else {
        return Err("That doesn't look like a GitHub repository link. Expected https://github.com/owner/repo.".into());
    };

    let clean = path.split(['?', '#']).next().unwrap_or(path).trim_matches('/');
    let mut parts = clean.split('/').filter(|s| !s.is_empty());
    let Some(owner) = parts.next() else {
        return Err("Missing repository owner in GitHub link.".into());
    };
    let Some(repo_raw) = parts.next() else {
        return Err("Missing repository name in GitHub link.".into());
    };
    let repo = repo_raw.strip_suffix(".git").unwrap_or(repo_raw);
    let valid_part = |s: &str| {
        !s.is_empty()
            && !s.starts_with('.')
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    };
    if !valid_part(owner) || !valid_part(repo) {
        return Err("That GitHub repository name contains invalid characters.".into());
    }
    Ok(format!("{owner}/{repo}"))
}

/// Inspects a GitHub repository (public or private) to populate the category-chooser popup.
pub fn inspect_repo(input: &str, prereleases: bool) -> Result<RepoPreview, String> {
    let repo_slug = parse_github_repo(input)?;
    let token = current_token();

    let repo_url = format!("{}/repos/{repo_slug}", api_base());
    let mut req = agent()
        .get(&repo_url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(tok) = &token {
        req = req.header("Authorization", &format!("Bearer {tok}"));
    }
    let mut resp = req
        .call()
        .map_err(|_| "Couldn't reach GitHub. Are you online?".to_string())?;
    match resp.status().as_u16() {
        200 => {}
        401 => return Err("Your GitHub sign-in expired. Sign in again in Settings.".into()),
        403 | 429 => {
            return Err(if token.is_some() {
                "GitHub denied access to that repository.".into()
            } else {
                "GitHub's anonymous rate limit was reached. Sign in to GitHub below for 5,000 requests/hour.".into()
            });
        }
        404 => {
            return Err(if token.is_some() {
                format!("Repository {repo_slug} wasn't found, or your account doesn't have access to it.")
            } else {
                format!("Repository {repo_slug} wasn't found. If it's a private repo, sign in to GitHub at the bottom of Settings first.")
            });
        }
        s => return Err(format!("GitHub answered with HTTP {s}.")),
    }
    let meta: GhRepoItem = resp
        .body_mut()
        .read_json()
        .map_err(|e| format!("Couldn't read repository metadata ({e})."))?;

    // Check whether this repo publishes Launcher's `updates` branch (`update.json` / `update-macos.json`).
    let own_format = has_updates_branch(&meta.full_name, token.as_deref());

    // Fetch recent releases to detect platforms and preview the newest downloadable version.
    let rel_url = format!("{}/repos/{}/releases?per_page=15", api_base(), meta.full_name);
    let mut rel_req = agent()
        .get(&rel_url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(tok) = &token {
        rel_req = rel_req.header("Authorization", &format!("Bearer {tok}"));
    }
    let releases: Vec<GhRelease> = rel_req
        .call()
        .ok()
        .filter(|r| r.status() == 200)
        .and_then(|mut r| r.body_mut().read_json().ok())
        .unwrap_or_default();

    let eligible: Vec<&GhRelease> = releases
        .iter()
        .filter(|r| !r.draft && (prereleases || !r.prerelease))
        .collect();
    let has_releases = !eligible.is_empty();

    let mut all_assets: Vec<&GhAsset> = Vec::new();
    for r in eligible.iter().take(5) {
        all_assets.extend(r.assets.iter());
    }
    let mut platforms = detect_platforms_from_assets(&all_assets);
    if platforms.is_empty() {
        // No releases yet or unlabelled asset names: keep both enabled so future releases work.
        platforms = vec!["Windows".into(), "MacOS".into()];
    }

    let best = eligible
        .iter()
        .filter_map(|r| {
            let v = parse_version_lenient(&r.tag_name)?;
            let asset = select_release_asset_for_os(&r.assets, cfg!(target_os = "macos"));
            Some((v, asset.map(|a| a.name.clone())))
        })
        .max_by(|a, b| a.0.cmp(&b.0));

    let (latest_version, matched_asset) = match best {
        Some((v, asset)) => (Some(v.to_string()), asset),
        None => (None, None),
    };

    Ok(RepoPreview {
        repo: meta.full_name,
        name: meta.name,
        publisher: meta.owner.login,
        tagline: meta.description.unwrap_or_default(),
        private: meta.private,
        own_format,
        platforms,
        latest_version,
        matched_asset,
        has_releases,
    })
}

fn has_updates_branch(repo: &str, token: Option<&str>) -> bool {
    let file = if cfg!(windows) { "update.json" } else { "update-macos.json" };
    let url = format!("{}/repos/{repo}/contents/{file}?ref=updates", api_base());
    let mut req = agent()
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(tok) = token {
        req = req.header("Authorization", &format!("Bearer {tok}"));
    }
    req.call().is_ok_and(|r| r.status() == 200)
}

/* --------------------------------------------------- OS Credential Store */

#[cfg(windows)]
mod os_store {
    use super::{decode_comment, StoredCredential, CRED_PREFIX};
    use windows_sys::Win32::Security::Credentials::{
        CredDeleteW, CredEnumerateW, CredFree, CredReadW, CredWriteW, CREDENTIALW,
        CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    unsafe fn from_pwstr(ptr: *const u16) -> String {
        if ptr.is_null() {
            return String::new();
        }
        let mut len = 0usize;
        while *ptr.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len))
    }

    pub fn write_token(key: &str, account: &str, comment: &str, token: &str) -> Result<(), String> {
        let mut target_w = wide(key);
        let mut user_w = wide(account);
        let mut comment_w = wide(comment);
        let mut blob = token.as_bytes().to_vec();

        let cred = CREDENTIALW {
            Flags: 0,
            Type: CRED_TYPE_GENERIC,
            TargetName: target_w.as_mut_ptr(),
            Comment: comment_w.as_mut_ptr(),
            LastWritten: unsafe { std::mem::zeroed() },
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_mut_ptr(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            AttributeCount: 0,
            Attributes: std::ptr::null_mut(),
            TargetAlias: std::ptr::null_mut(),
            UserName: user_w.as_mut_ptr(),
        };

        let ok = unsafe { CredWriteW(&cred, 0) };
        if ok == 0 {
            return Err(format!(
                "Couldn't save credentials to Windows Credential Manager ({}).",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }

    pub fn read_token(key: &str) -> Result<Option<(String, String, String)>, String> {
        let target_w = wide(key);
        let mut ptr: *mut CREDENTIALW = std::ptr::null_mut();
        let ok = unsafe { CredReadW(target_w.as_ptr(), CRED_TYPE_GENERIC, 0, &mut ptr) };
        if ok == 0 || ptr.is_null() {
            return Ok(None);
        }
        let res = unsafe {
            let c = &*ptr;
            let bytes = if c.CredentialBlob.is_null() || c.CredentialBlobSize == 0 {
                &[]
            } else {
                std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize)
            };
            let token = String::from_utf8_lossy(bytes).into_owned();
            let account = from_pwstr(c.UserName);
            let comment = from_pwstr(c.Comment);
            let (method, _) = decode_comment(&comment);
            CredFree(ptr as *const _);
            (token, account, method)
        };
        if res.0.is_empty() {
            Ok(None)
        } else {
            Ok(Some(res))
        }
    }

    pub fn delete_entry(key: &str, _account: Option<&str>) -> Result<(), String> {
        let target_w = wide(key);
        let ok = unsafe { CredDeleteW(target_w.as_ptr(), CRED_TYPE_GENERIC, 0) };
        if ok == 0 {
            let err = std::io::Error::last_os_error();
            // ERROR_NOT_FOUND (1168) is fine — already deleted.
            if err.raw_os_error() == Some(1168) {
                return Ok(());
            }
            return Err(format!("Couldn't remove credential from Windows Credential Manager ({err})."));
        }
        Ok(())
    }

    pub fn list_entries() -> Result<Vec<StoredCredential>, String> {
        let filter = wide(&format!("{CRED_PREFIX}*"));
        let mut count = 0u32;
        let mut list_ptr: *mut *mut CREDENTIALW = std::ptr::null_mut();
        let ok = unsafe { CredEnumerateW(filter.as_ptr(), 0, &mut count, &mut list_ptr) };
        if ok == 0 || list_ptr.is_null() {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(1168) {
                return Ok(Vec::new());
            }
            return Ok(Vec::new());
        }
        let mut out = Vec::with_capacity(count as usize);
        unsafe {
            let slice = std::slice::from_raw_parts(list_ptr, count as usize);
            for &cptr in slice {
                if cptr.is_null() {
                    continue;
                }
                let c = &*cptr;
                let key = from_pwstr(c.TargetName);
                if !key.starts_with(CRED_PREFIX) {
                    continue;
                }
                let account = from_pwstr(c.UserName);
                let comment = from_pwstr(c.Comment);
                let (method, install_dir) = decode_comment(&comment);
                out.push(StoredCredential {
                    key,
                    account,
                    method,
                    install_dir,
                    is_current: false,
                    dir_exists: false,
                });
            }
            CredFree(list_ptr as *const _);
        }
        Ok(out)
    }
}

#[cfg(target_os = "macos")]
mod os_store {
    use super::{parse_keychain_dump, StoredCredential};
    use std::io::Write;
    use std::process::{Command, Stdio};

    fn escape_sec_arg(s: &str) -> String {
        let mut out = String::with_capacity(s.len() + 2);
        out.push('"');
        for ch in s.chars() {
            match ch {
                '\\' | '"' => {
                    out.push('\\');
                    out.push(ch);
                }
                '\n' | '\r' => {}
                c => out.push(c),
            }
        }
        out.push('"');
        out
    }

    pub fn write_token(key: &str, account: &str, comment: &str, token: &str) -> Result<(), String> {
        // Clear any existing items for this copy first so re-logging in with a different
        // GitHub username never leaves a duplicate Keychain item behind.
        let _ = delete_entry(key, None);

        // Pass the command through `security -i` stdin so the token never appears in process args.
        let line = format!(
            "add-generic-password -U -s {} -a {} -l {} -j {} -w {}\n",
            escape_sec_arg(key),
            escape_sec_arg(account),
            escape_sec_arg("Javacrafter Launcher (GitHub)"),
            escape_sec_arg(comment),
            escape_sec_arg(token),
        );
        let mut child = Command::new("/usr/bin/security")
            .arg("-i")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Couldn't start macOS Keychain utility ({e})."))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(line.as_bytes())
                .map_err(|e| format!("Couldn't write to macOS Keychain ({e})."))?;
        }
        let out = child
            .wait_with_output()
            .map_err(|e| format!("Couldn't save credentials to macOS Keychain ({e})."))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
            return Err(if err.is_empty() {
                "Couldn't save credentials to macOS Keychain.".into()
            } else {
                format!("Couldn't save credentials to macOS Keychain ({err}).")
            });
        }
        Ok(())
    }

    pub fn read_token(key: &str) -> Result<Option<(String, String, String)>, String> {
        let out = Command::new("/usr/bin/security")
            .args(["find-generic-password", "-s", key, "-w"])
            .output()
            .map_err(|e| format!("Couldn't read macOS Keychain ({e})."))?;
        if !out.status.success() {
            return Ok(None);
        }
        let token = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if token.is_empty() {
            return Ok(None);
        }
        let meta = list_entries()
            .ok()
            .and_then(|list| list.into_iter().find(|e| e.key == key));
        let (account, method) = meta
            .map(|m| (m.account, m.method))
            .unwrap_or_else(|| (String::new(), "unknown".into()));
        Ok(Some((token, account, method)))
    }

    pub fn delete_entry(key: &str, account: Option<&str>) -> Result<(), String> {
        // Loop so duplicate items with the same service/account are all removed.
        for _ in 0..16 {
            let mut cmd = Command::new("/usr/bin/security");
            cmd.args(["delete-generic-password", "-s", key]);
            if let Some(acct) = account.filter(|a| !a.is_empty()) {
                cmd.args(["-a", acct]);
            }
            let status = cmd
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map_err(|e| format!("Couldn't access macOS Keychain ({e})."))?;
            if !status.success() {
                break;
            }
        }
        Ok(())
    }

    pub fn list_entries() -> Result<Vec<StoredCredential>, String> {
        // Without `-d`, `dump-keychain` reads metadata attributes only (no Keychain password prompt).
        let out = Command::new("/usr/bin/security")
            .arg("dump-keychain")
            .output()
            .map_err(|e| format!("Couldn't inspect macOS Keychain ({e})."))?;
        let text = String::from_utf8_lossy(&out.stdout);
        Ok(parse_keychain_dump(&text))
    }
}

/// Parses `security dump-keychain` output for `JavacrafterLauncher*` generic password entries.
/// Kept outside `#[cfg(target_os = "macos")]` so it is unit-tested on every platform.
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
fn parse_keychain_dump(dump: &str) -> Vec<StoredCredential> {
    let mut results = Vec::new();
    let mut in_genp = false;
    let mut svce = String::new();
    let mut acct = String::new();
    let mut icmt = String::new();

    let flush = |in_genp: bool, svce: &mut String, acct: &mut String, icmt: &mut String, out: &mut Vec<StoredCredential>| {
        if in_genp && svce.starts_with(CRED_PREFIX) {
            let (method, install_dir) = decode_comment(icmt);
            out.push(StoredCredential {
                key: std::mem::take(svce),
                account: std::mem::take(acct),
                method,
                install_dir,
                is_current: false,
                dir_exists: false,
            });
        } else {
            svce.clear();
            acct.clear();
        }
        icmt.clear();
    };

    for raw_line in dump.lines() {
        let line = raw_line.trim();
        if line.starts_with("keychain:") {
            flush(in_genp, &mut svce, &mut acct, &mut icmt, &mut results);
            in_genp = false;
        } else if let Some(cls) = line.strip_prefix("class:") {
            in_genp = cls.trim().contains("\"genp\"");
        } else if in_genp {
            if let Some(val) = parse_keychain_attr(line, "\"svce\"") {
                svce = val;
            } else if let Some(val) = parse_keychain_attr(line, "\"acct\"") {
                acct = val;
            } else if let Some(val) = parse_keychain_attr(line, "\"icmt\"") {
                icmt = val;
            }
        }
    }
    flush(in_genp, &mut svce, &mut acct, &mut icmt, &mut results);
    results
}

#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
fn parse_keychain_attr(line: &str, name: &str) -> Option<String> {
    let rest = line.strip_prefix(name)?;
    let (_, rhs) = rest.split_once('=')?;
    let rhs = rhs.trim();
    if let Some(quoted) = rhs.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        return Some(quoted.replace("\\\"", "\"").replace("\\\\", "\\"));
    }
    if let Some(hex_part) = rhs.strip_prefix("0x") {
        let hex = hex_part.split_whitespace().next()?;
        if hex.len() % 2 == 0 {
            let mut bytes = Vec::with_capacity(hex.len() / 2);
            for i in (0..hex.len()).step_by(2) {
                let b = u8::from_str_radix(&hex[i..i + 2], 16).ok()?;
                bytes.push(b);
            }
            return String::from_utf8(bytes).ok();
        }
    }
    None
}

#[cfg(all(not(windows), not(target_os = "macos")))]
mod os_store {
    use super::{decode_comment, StoredCredential, CRED_PREFIX};
    use crate::settings::data_dir;
    use serde::{Deserialize, Serialize};
    use std::fs;
    use std::path::PathBuf;

    #[derive(Serialize, Deserialize)]
    struct LocalEntry {
        key: String,
        account: String,
        comment: String,
        token: String,
    }

    fn store_path() -> PathBuf {
        data_dir().join(".github-credentials.json")
    }

    fn load_all() -> Vec<LocalEntry> {
        fs::read(store_path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn save_all(entries: &[LocalEntry]) -> Result<(), String> {
        let dir = data_dir();
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = store_path();
        let bytes = serde_json::to_vec_pretty(entries).map_err(|e| e.to_string())?;
        fs::write(&path, bytes).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }

    pub fn write_token(key: &str, account: &str, comment: &str, token: &str) -> Result<(), String> {
        let mut all = load_all();
        all.retain(|e| e.key != key);
        all.push(LocalEntry {
            key: key.into(),
            account: account.into(),
            comment: comment.into(),
            token: token.into(),
        });
        save_all(&all)
    }

    pub fn read_token(key: &str) -> Result<Option<(String, String, String)>, String> {
        let all = load_all();
        Ok(all.into_iter().find(|e| e.key == key).map(|e| {
            let (method, _) = decode_comment(&e.comment);
            (e.token, e.account, method)
        }))
    }

    pub fn delete_entry(key: &str, account: Option<&str>) -> Result<(), String> {
        let mut all = load_all();
        all.retain(|e| !(e.key == key && account.is_none_or(|a| a.is_empty() || e.account == a)));
        save_all(&all)
    }

    pub fn list_entries() -> Result<Vec<StoredCredential>, String> {
        Ok(load_all()
            .into_iter()
            .filter(|e| e.key.starts_with(CRED_PREFIX))
            .map(|e| {
                let (method, install_dir) = decode_comment(&e.comment);
                StoredCredential {
                    key: e.key,
                    account: e.account,
                    method,
                    install_dir,
                    is_current: false,
                    dir_exists: false,
                }
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_github_repo_variants() {
        assert_eq!(parse_github_repo("https://github.com/JavacrafterDevelopment/Launcher").unwrap(), "JavacrafterDevelopment/Launcher");
        assert_eq!(parse_github_repo("https://github.com/owner/repo.git").unwrap(), "owner/repo");
        assert_eq!(parse_github_repo("https://github.com/owner/repo/releases/tag/v1.0.0").unwrap(), "owner/repo");
        assert_eq!(parse_github_repo("github.com/owner/my_app/").unwrap(), "owner/my_app");
        assert_eq!(parse_github_repo("git@github.com:owner/private-tool.git").unwrap(), "owner/private-tool");
        assert_eq!(parse_github_repo("owner/simple-repo").unwrap(), "owner/simple-repo");
        assert!(parse_github_repo("https://gitlab.com/owner/repo").is_err());
        assert!(parse_github_repo("https://github.com/onlyowner").is_err());
        assert!(parse_github_repo("").is_err());
    }

    #[test]
    fn parse_macos_keychain_dump_finds_current_and_old_copies() {
        let sample = r#"
keychain: "/Users/alice/Library/Keychains/login.keychain-db"
version: 512
class: "genp"
attributes:
    0x00000007 <blob>="Some Other App"
    "acct"<blob>="someone"
    "svce"<blob>="OtherService"
keychain: "/Users/alice/Library/Keychains/login.keychain-db"
version: 512
class: "genp"
attributes:
    0x00000007 <blob>="Javacrafter Launcher (GitHub)"
    "acct"<blob>="alice"
    "icmt"<blob>="oauth|/Applications/Launcher"
    "svce"<blob>="JavacrafterLauncher/github/11223344"
keychain: "/Users/alice/Library/Keychains/login.keychain-db"
version: 512
class: "genp"
attributes:
    0x00000007 <blob>="Javacrafter Launcher (GitHub)"
    "acct"<blob>="alice"
    "icmt"<blob>=0x7061747C2F55736572732F616C6963652F4F6C64  "pat|/Users/alice/Old"
    "svce"<blob>="JavacrafterLauncher/github/deadbeef"
"#;
        let items = parse_keychain_dump(sample);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].key, "JavacrafterLauncher/github/11223344");
        assert_eq!(items[0].account, "alice");
        assert_eq!(items[0].method, "oauth");
        assert_eq!(items[0].install_dir, "/Applications/Launcher");
        assert_eq!(items[1].key, "JavacrafterLauncher/github/deadbeef");
        assert_eq!(items[1].method, "pat");
        assert_eq!(items[1].install_dir, "/Users/alice/Old");
    }
}
