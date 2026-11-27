//! GitHub authentication, OS credential store integration, and repository discovery.
//!
//! Tokens are stored exclusively in the operating system's native credential store:
//! - Windows: Windows Credential Manager (`CredWriteW` / `CredReadW` / `CredEnumerateW` / `CredDeleteW`)
//! - macOS: macOS Keychain (`/usr/bin/security`)
//!
//! Each copy of Launcher can hold several GitHub accounts (your own token plus, say, a
//! friend's for testing their private apps). Every account's token is keyed under
//! `JavacrafterLauncher/github/<install-tag>/<login>`. Copies from before multiple
//! accounts used a single `JavacrafterLauncher/github/<install-tag>` entry; that entry
//! is still read and counts as one of this copy's accounts. Entries left by other or
//! deleted copies can be enumerated and cleared from Settings.
//!
//! Raw tokens are kept only in process memory (`ACCOUNTS`) and are never serialized to
//! `data/settings.json` or sent to the WebView.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::RwLock;

use crate::settings::{app_dir, GitHubAccount, Settings};
use crate::updater::{
    agent, api_base, detect_platforms_from_assets, parse_version_lenient, select_release_asset_for_os,
    GhAsset, GhRelease,
};

/// Prefix shared by every credential written by any copy of Launcher.
pub const CRED_PREFIX: &str = "JavacrafterLauncher";
const CRED_GITHUB_PREFIX: &str = "JavacrafterLauncher/github/";

/// One signed-in account held in memory.
#[derive(Clone)]
struct Account {
    login: String,
    token: String,
    /// The credential store entry holding this token.
    key: String,
}

static ACCOUNTS: RwLock<Vec<Account>> = RwLock::new(Vec::new());

/// Which account's token can see a repository: lowercase `owner/repo` to lowercase login.
/// Filled from the app list, the private-repo listing and on-demand probes.
static REPO_ACCOUNT: RwLock<Option<HashMap<String, String>>> = RwLock::new(None);

fn accounts() -> Vec<Account> {
    ACCOUNTS.read().map(|g| g.clone()).unwrap_or_default()
}

/// True when at least one GitHub account is signed in.
pub fn has_accounts() -> bool {
    ACCOUNTS.read().is_ok_and(|g| !g.is_empty())
}

/// Logins of the signed-in accounts, default first.
pub fn account_logins() -> Vec<String> {
    accounts().into_iter().map(|a| a.login).collect()
}

fn token_of(login: &str) -> Option<String> {
    ACCOUNTS
        .read()
        .ok()
        .and_then(|g| g.iter().find(|a| a.login.eq_ignore_ascii_case(login)).map(|a| a.token.clone()))
}

/// Records which account can see `repo`, so later checks and downloads use that token.
pub fn remember_repo_account(repo: &str, login: &str) {
    if repo.is_empty() || login.is_empty() {
        return;
    }
    if let Ok(mut g) = REPO_ACCOUNT.write() {
        g.get_or_insert_with(HashMap::new)
            .insert(repo.to_ascii_lowercase(), login.to_ascii_lowercase());
    }
}

/// Forgets every repo-to-account pairing, e.g. after an account was added or removed,
/// so the next request probes again.
pub fn forget_repo_accounts() {
    if let Ok(mut g) = REPO_ACCOUNT.write() {
        *g = None;
    }
}

fn remembered_account(repo: &str) -> Option<String> {
    REPO_ACCOUNT
        .read()
        .ok()
        .and_then(|g| g.as_ref().and_then(|m| m.get(&repo.to_ascii_lowercase()).cloned()))
}

/// The token to use for `repo`: the account remembered for it, or else the first
/// account whose token GitHub lets see the repository (probed once and remembered),
/// or else the default account. None when nobody is signed in.
pub fn token_for_repo(repo: &str) -> Option<String> {
    let all = accounts();
    match all.len() {
        0 => return None,
        1 => return Some(all[0].token.clone()),
        _ => {}
    }
    if let Some(tok) = remembered_account(repo).and_then(|login| token_of(&login)) {
        return Some(tok);
    }
    let mut definitive = true;
    for a in &all {
        match probe_repo(repo, &a.token) {
            Some(200) => {
                remember_repo_account(repo, &a.login);
                return Some(a.token.clone());
            }
            Some(_) => {}
            None => definitive = false,
        }
    }
    // No token sees it: use the default, and don't probe again every check unless
    // the network was the problem.
    if definitive {
        remember_repo_account(repo, &all[0].login);
    }
    Some(all[0].token.clone())
}

/// HTTP status of `GET /repos/{repo}` with `token`, or None when GitHub couldn't be reached.
fn probe_repo(repo: &str, token: &str) -> Option<u16> {
    let url = format!("{}/repos/{repo}", api_base());
    agent()
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("Authorization", &format!("Bearer {token}"))
        .call()
        .ok()
        .map(|r| r.status().as_u16())
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

/// This copy's single-account credential key, from before multiple accounts.
pub fn current_cred_key() -> String {
    format!("{CRED_GITHUB_PREFIX}{}", install_tag())
}

/// Credential key for one account of this copy of Launcher.
fn account_cred_key(login: &str) -> String {
    format!("{}/{}", current_cred_key(), login.to_ascii_lowercase())
}

/// True when `key` is one of this copy's credentials (legacy or per-account).
fn is_this_copy(key: &str, current: &str) -> bool {
    key == current || key.strip_prefix(current).is_some_and(|rest| rest.starts_with('/') && rest.len() > 1)
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

/// One signed-in account as the WebView sees it.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountStatus {
    pub login: String,
    pub avatar_url: Option<String>,
}

/// Non-secret auth summary sent to the WebView.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthStatus {
    pub logged_in: bool,
    /// The default account's login.
    pub username: Option<String>,
    pub avatar_url: Option<String>,
    pub accounts: Vec<AccountStatus>,
    pub store_name: &'static str,
    pub current_key: String,
}

impl AuthStatus {
    pub fn from_settings(settings: &Settings) -> Self {
        let accounts: Vec<AccountStatus> = account_logins()
            .into_iter()
            .map(|login| {
                let avatar_url = settings
                    .github_accounts
                    .iter()
                    .find(|a| a.login.eq_ignore_ascii_case(&login))
                    .and_then(|a| a.avatar_url.clone());
                AccountStatus { login, avatar_url }
            })
            .collect();
        Self {
            logged_in: !accounts.is_empty(),
            username: accounts.first().map(|a| a.login.clone()),
            avatar_url: accounts.first().and_then(|a| a.avatar_url.clone()),
            accounts,
            store_name: store_name(),
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
    /// Login of the signed-in account that can see this repository.
    pub account: String,
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
    /// Login of the signed-in account whose token reached the repository, if any.
    pub account: Option<String>,
}

/// Loads this copy's saved GitHub accounts from the OS credential store on startup, in
/// the order Settings lists them. Returns the logins Settings remembered whose token is
/// no longer in the store (removed from outside Launcher); Settings is updated to match.
pub fn init_from_store(settings: &mut Settings) -> Vec<String> {
    let cur = current_cred_key();
    let mut keys: Vec<String> = os_store::list_entries()
        .unwrap_or_default()
        .into_iter()
        .map(|e| e.key)
        .filter(|k| is_this_copy(k, &cur))
        .collect();
    // Enumeration can come back short; read the expected keys directly too.
    let expected = std::iter::once(cur.clone())
        .chain(settings.github_accounts.iter().map(|a| account_cred_key(&a.login)));
    for k in expected {
        if !keys.contains(&k) {
            keys.push(k);
        }
    }

    let mut found: Vec<Account> = Vec::new();
    for key in keys {
        let Ok(Some((token, account, _method))) = os_store::read_token(&key) else {
            continue;
        };
        let login = if !account.trim().is_empty() {
            account
        } else if let Some(l) = key.strip_prefix(&format!("{cur}/")) {
            l.to_string()
        } else if let Some(first) = settings.github_accounts.first() {
            // A legacy entry saved without a username: it belonged to the one account.
            first.login.clone()
        } else {
            "github".to_string()
        };
        match found.iter_mut().find(|a| a.login.eq_ignore_ascii_case(&login)) {
            // Prefer the per-account entry over the legacy one for the same login.
            Some(existing) => {
                if existing.key == cur {
                    *existing = Account { login, token, key };
                }
            }
            None => found.push(Account { login, token, key }),
        }
    }

    // Settings order first, then anything only the store knew about.
    let mut ordered: Vec<Account> = Vec::new();
    let mut removed = Vec::new();
    for meta in &settings.github_accounts {
        if let Some(pos) = found.iter().position(|a| a.login.eq_ignore_ascii_case(&meta.login)) {
            ordered.push(found.remove(pos));
        } else {
            removed.push(meta.login.clone());
        }
    }
    ordered.extend(found);

    let before = settings.github_accounts.clone();
    settings.github_accounts = ordered
        .iter()
        .map(|a| GitHubAccount {
            login: a.login.clone(),
            avatar_url: before
                .iter()
                .find(|m| m.login.eq_ignore_ascii_case(&a.login))
                .and_then(|m| m.avatar_url.clone()),
        })
        .collect();
    if settings.github_accounts != before {
        let _ = settings.save();
    }
    if let Ok(mut g) = ACCOUNTS.write() {
        *g = ordered;
    }
    forget_repo_accounts();
    removed
}

/// Saves the PAT in the OS credential store under its own account key and activates it
/// alongside any accounts already signed in. Signing in again as the same login replaces
/// that account's token. Returns true when the account is new.
pub fn add_account(token: &str, username: &str) -> Result<bool, String> {
    let key = account_cred_key(username);
    let comment = encode_comment("pat", &app_dir().display().to_string());
    os_store::write_token(&key, username, &comment, token)?;
    let mut stale_key = None;
    let is_new = {
        let mut g = ACCOUNTS.write().map_err(|_| "Account list is unavailable.".to_string())?;
        if let Some(a) = g.iter_mut().find(|a| a.login.eq_ignore_ascii_case(username)) {
            if a.key != key {
                stale_key = Some(a.key.clone());
            }
            *a = Account { login: username.to_string(), token: token.to_string(), key };
            false
        } else {
            g.push(Account { login: username.to_string(), token: token.to_string(), key });
            true
        }
    };
    // The same account was saved under the old single-account key: drop that copy.
    if let Some(old) = stale_key {
        let _ = os_store::delete_entry(&old, Some(username));
    }
    forget_repo_accounts();
    Ok(is_new)
}

/// Signs one account out of this copy of Launcher, removing its token from the OS
/// credential store. Returns false when no such account was signed in.
pub fn remove_account(login: &str) -> Result<bool, String> {
    let removed = {
        let mut g = ACCOUNTS.write().map_err(|_| "Account list is unavailable.".to_string())?;
        g.iter()
            .position(|a| a.login.eq_ignore_ascii_case(login))
            .map(|pos| g.remove(pos))
    };
    forget_repo_accounts();
    match removed {
        Some(a) => {
            os_store::delete_entry(&a.key, Some(&a.login))?;
            Ok(true)
        }
        None => Ok(false),
    }
}

/// Lists every Launcher credential currently in the OS credential store (including old copies).
pub fn list_credentials() -> Result<Vec<StoredCredential>, String> {
    let cur = current_cred_key();
    let mut entries = os_store::list_entries()?;
    for e in &mut entries {
        e.is_current = is_this_copy(&e.key, &cur);
        e.dir_exists = !e.install_dir.is_empty() && Path::new(&e.install_dir).exists();
    }
    // This copy's entries first, then alphabetical.
    entries.sort_by(|a, b| b.is_current.cmp(&a.is_current).then_with(|| a.key.cmp(&b.key)));
    Ok(entries)
}

/// Deletes one specific credential entry from the OS credential store. If it held one
/// of this copy's signed-in accounts, that account is signed out and its login returned.
pub fn delete_credential(key: &str, account: Option<&str>) -> Result<Option<String>, String> {
    if !key.starts_with(CRED_PREFIX) {
        return Err("Refusing to delete a credential that does not belong to Launcher.".into());
    }
    os_store::delete_entry(key, account)?;
    let removed = ACCOUNTS.write().ok().and_then(|mut g| {
        g.iter().position(|a| a.key == key).map(|pos| g.remove(pos).login)
    });
    if removed.is_some() {
        forget_repo_accounts();
    }
    Ok(removed)
}

/// Deletes every Launcher credential belonging to other or deleted copies of Launcher,
/// keeping this copy's accounts intact.
pub fn clear_old_credentials() -> Result<usize, String> {
    let cur = current_cred_key();
    let entries = os_store::list_entries()?;
    let mut removed = 0;
    for e in entries {
        if !is_this_copy(&e.key, &cur) && os_store::delete_entry(&e.key, Some(&e.account)).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

/// Deletes EVERY Launcher credential from the OS credential store (this copy's accounts
/// and old/duplicate copies) and signs every account out. Returns how many entries were
/// removed and the logins that were signed out.
pub fn clear_all_credentials() -> Result<(usize, Vec<String>), String> {
    let entries = os_store::list_entries()?;
    let mut removed = 0;
    let mut deleted_keys = Vec::new();
    for e in entries {
        if os_store::delete_entry(&e.key, Some(&e.account)).is_ok() {
            removed += 1;
            deleted_keys.push(e.key);
        }
    }
    // Also delete this copy's keys directly in case enumeration missed them.
    let signed_in = ACCOUNTS.write().map(|mut g| std::mem::take(&mut *g)).unwrap_or_default();
    for a in &signed_in {
        if !deleted_keys.contains(&a.key) && os_store::delete_entry(&a.key, Some(&a.login)).is_ok() {
            removed += 1;
        }
    }
    forget_repo_accounts();
    Ok((removed, signed_in.into_iter().map(|a| a.login).collect()))
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

/* ------------------------------------------------------------- GitHub API */

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

/// Lists private repositories every signed-in account can see, each tagged with the
/// account that sees it (the first, when several do). An account whose listing fails is
/// skipped; the call fails only when every account does.
pub fn fetch_private_repos() -> Result<Vec<RepoSummary>, String> {
    let all = accounts();
    if all.is_empty() {
        return Err("Sign in to GitHub first to browse private repositories.".into());
    }
    let mut out: Vec<RepoSummary> = Vec::new();
    let mut first_err = None;
    let mut any_ok = false;
    for a in &all {
        match fetch_private_repos_for(&a.token, &a.login) {
            Ok(repos) => {
                any_ok = true;
                for r in repos {
                    if !out.iter().any(|o| o.full_name.eq_ignore_ascii_case(&r.full_name)) {
                        remember_repo_account(&r.full_name, &a.login);
                        out.push(r);
                    }
                }
            }
            Err(e) => {
                let e = if all.len() > 1 { format!("@{}: {e}", a.login) } else { e };
                first_err.get_or_insert(e);
            }
        }
    }
    match (any_ok, first_err) {
        (false, Some(e)) => Err(e),
        _ => Ok(out),
    }
}

fn fetch_private_repos_for(token: &str, login: &str) -> Result<Vec<RepoSummary>, String> {
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
                            account: login.to_string(),
                        });
                    }
                }
                if count < 100 {
                    break;
                }
            }
            401 => return Err("This GitHub token was rejected or has expired. Remove it and add a new one.".into()),
            403 => return Err("This GitHub token doesn't have permission to list private repositories (needs 'repo' or Contents read access).".into()),
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

    // Try the account remembered for this repo first, then every other signed-in
    // account in order, so a friend's private repo is found with the friend's token.
    let mut candidates: Vec<(String, String)> = accounts().into_iter().map(|a| (a.login, a.token)).collect();
    if let Some(login) = remembered_account(&repo_slug) {
        if let Some(pos) = candidates.iter().position(|(l, _)| l.eq_ignore_ascii_case(&login)) {
            let c = candidates.remove(pos);
            candidates.insert(0, c);
        }
    }
    let signed_in = !candidates.is_empty();

    let repo_url = format!("{}/repos/{repo_slug}", api_base());
    let mut status = 0u16;
    let mut found = None;
    let attempts: Vec<Option<(String, String)>> = if signed_in {
        candidates.into_iter().map(Some).collect()
    } else {
        vec![None]
    };
    for cand in attempts {
        let mut req = agent()
            .get(&repo_url)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28");
        if let Some((_, tok)) = &cand {
            req = req.header("Authorization", &format!("Bearer {tok}"));
        }
        let resp = req
            .call()
            .map_err(|_| "Couldn't reach GitHub. Are you online?".to_string())?;
        status = resp.status().as_u16();
        if status == 200 {
            found = Some((resp, cand));
            break;
        }
        // 401/403/404 with one token: another account may still have access.
        if !matches!(status, 401 | 403 | 404) {
            break;
        }
    }
    let Some((mut resp, chosen)) = found else {
        return Err(match status {
            401 => "A saved GitHub token was rejected or has expired. Remove it in Settings and add a new one.".into(),
            403 | 429 if !signed_in => {
                "GitHub's anonymous rate limit was reached. Sign in to GitHub below for 5,000 requests/hour.".into()
            }
            403 | 429 => "GitHub denied access to that repository.".into(),
            404 if signed_in => {
                format!("Repository {repo_slug} wasn't found, or none of your GitHub accounts have access to it. To use a friend's private repo, add their token in Settings.")
            }
            404 => format!("Repository {repo_slug} wasn't found. If it's a private repo, sign in to GitHub at the bottom of Settings first."),
            s => format!("GitHub answered with HTTP {s}."),
        });
    };
    let (account, token) = match chosen {
        Some((login, tok)) => (Some(login), Some(tok)),
        None => (None, None),
    };
    let meta: GhRepoItem = resp
        .body_mut()
        .read_json()
        .map_err(|e| format!("Couldn't read repository metadata ({e})."))?;
    if let Some(login) = &account {
        remember_repo_account(&meta.full_name, login);
    }

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
        account,
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
    fn this_copy_owns_its_legacy_and_per_account_keys_only() {
        let cur = "JavacrafterLauncher/github/1234abcd";
        assert!(is_this_copy(cur, cur));
        assert!(is_this_copy("JavacrafterLauncher/github/1234abcd/alice", cur));
        assert!(!is_this_copy("JavacrafterLauncher/github/1234abcd/", cur));
        assert!(!is_this_copy("JavacrafterLauncher/github/1234abcdef", cur));
        assert!(!is_this_copy("JavacrafterLauncher/github/deadbeef/alice", cur));
    }

    /// Runs against the file-backed store, so only where that store is the real one.
    #[cfg(all(not(windows), not(target_os = "macos")))]
    #[test]
    fn several_accounts_sign_in_and_out_independently() {
        let _ = clear_all_credentials();
        // A token saved by a single-account copy, plus one account that has since vanished.
        os_store::write_token(&current_cred_key(), "alice", "pat|here", "tok-a").unwrap();
        let mut settings = Settings {
            github_accounts: vec![
                GitHubAccount { login: "alice".into(), avatar_url: Some("a.png".into()) },
                GitHubAccount { login: "gone".into(), avatar_url: None },
            ],
            ..Settings::default()
        };
        assert_eq!(init_from_store(&mut settings), vec!["gone".to_string()]);
        assert_eq!(account_logins(), vec!["alice"]);
        assert_eq!(settings.github_accounts.len(), 1);
        assert_eq!(settings.github_accounts[0].avatar_url.as_deref(), Some("a.png"));

        // A friend's token is added beside it instead of replacing it.
        assert!(add_account("tok-b", "Bob").unwrap());
        assert_eq!(account_logins(), vec!["alice", "Bob"]);
        remember_repo_account("Bob/Secret", "bob");
        assert_eq!(token_for_repo("bob/secret").as_deref(), Some("tok-b"));

        // Re-adding alice replaces her token and moves it off the legacy key.
        assert!(!add_account("tok-a2", "alice").unwrap());
        let keys: Vec<String> = list_credentials().unwrap().into_iter().map(|c| c.key).collect();
        assert!(!keys.contains(&current_cred_key()), "{keys:?}");
        assert!(keys.contains(&account_cred_key("alice")) && keys.contains(&account_cred_key("bob")));

        // Signing the friend out leaves alice signed in; reloading finds the same.
        assert!(remove_account("bob").unwrap());
        assert_eq!(account_logins(), vec!["alice"]);
        settings.github_accounts.push(GitHubAccount { login: "Bob".into(), avatar_url: None });
        assert_eq!(init_from_store(&mut settings), vec!["Bob".to_string()]);
        assert_eq!(token_for_repo("any/repo").as_deref(), Some("tok-a2"));

        let (_, signed_out) = clear_all_credentials().unwrap();
        assert_eq!(signed_out, vec!["alice"]);
        assert!(!has_accounts());
        let _ = std::fs::remove_file(crate::settings::data_dir().join("settings.json"));
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
