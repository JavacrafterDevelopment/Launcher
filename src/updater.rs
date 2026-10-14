//! Self-update from GitHub releases.
//!
//! A release is eligible when its tag parses as a semver version (a leading `v` is
//! fine) and it carries a `Launcher.exe` asset. The newest eligible release that is
//! newer than this build wins.
//!
//! Installing never touches the running exe's bytes: Windows lets a running image
//! be *renamed*, just not overwritten. So the download is verified and staged next
//! to the exe, then the exe is renamed to `Launcher.exe.old`, the staged file takes
//! its name, and the new copy is started. The next launch sweeps up the `.old`.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use ureq::tls::{RootCerts, TlsConfig, TlsProvider};

use crate::settings::app_dir;

/// The repository this launcher publishes its own releases to.
pub const REPO: &str = "JavacrafterDevelopment/Launcher";
/// The release asset that holds the launcher itself.
pub const ASSET_NAME: &str = "Launcher.exe";

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub version: String,
    pub name: String,
    pub notes: String,
    pub published: String,
    pub page: String,
    pub prerelease: bool,
    pub asset_url: String,
    pub size: u64,
    /// GitHub's own `sha256:<hex>` for the asset, when it reports one.
    pub digest: Option<String>,
}

pub enum Check {
    UpToDate,
    Available(Release),
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    html_url: String,
    draft: bool,
    prerelease: bool,
    published_at: Option<String>,
    assets: Vec<GhAsset>,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    size: u64,
    digest: Option<String>,
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        // SChannel and the Windows trust store: no certificates to bundle or go stale.
        .tls_config(
            TlsConfig::builder()
                .provider(TlsProvider::NativeTls)
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        )
        .user_agent(format!("JavacrafterLauncher/{VERSION}"))
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_recv_response(Some(Duration::from_secs(20)))
        .http_status_as_error(false)
        .build()
        .into()
}

fn parse_version(tag: &str) -> Option<semver::Version> {
    semver::Version::parse(tag.trim().trim_start_matches(['v', 'V'])).ok()
}

pub fn current_version() -> semver::Version {
    semver::Version::parse(VERSION).expect("crate version is semver")
}

/// Debug builds can be pointed at a stand-in for the GitHub API to test updates
/// end to end without publishing anything. Release builds always use GitHub.
fn api_base() -> String {
    if cfg!(debug_assertions) {
        if let Ok(base) = std::env::var("LAUNCHER_UPDATE_API") {
            return base.trim_end_matches('/').to_string();
        }
    }
    "https://api.github.com".into()
}

/// Asks GitHub for the newest release this build could update to.
pub fn check(prereleases: bool) -> Result<Check, String> {
    let url = format!("{}/repos/{REPO}/releases?per_page=30", api_base());
    let mut resp = agent()
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .call()
        .map_err(|e| format!("Couldn't reach GitHub ({e})"))?;

    match resp.status().as_u16() {
        200 => {}
        // The anonymous API allows 60 requests an hour per IP, which a VPN or a
        // shared network can use up. The website isn't limited that way.
        403 | 429 => {
            return check_via_website().map_err(|_| {
                "GitHub's hourly request limit is used up. Try again in a little while.".into()
            });
        }
        404 => return Err(format!("The repository {REPO} wasn't found, or it's private.")),
        s => return Err(format!("GitHub answered with HTTP {s}.")),
    }

    let releases: Vec<GhRelease> = resp
        .body_mut()
        .read_json()
        .map_err(|e| format!("GitHub sent something unreadable ({e})"))?;

    let current = current_version();
    let best = releases
        .into_iter()
        .filter(|r| !r.draft && (prereleases || !r.prerelease))
        .filter_map(|r| {
            let v = parse_version(&r.tag_name)?;
            let asset = pick_asset(&r.assets)?;
            Some((v, asset_release(&r, asset)))
        })
        .max_by(|a, b| a.0.cmp(&b.0));

    Ok(match best {
        Some((v, mut rel)) if v > current => {
            rel.version = v.to_string();
            Check::Available(rel)
        }
        _ => Check::UpToDate,
    })
}

/// Fallback without the API: `/releases/latest` redirects to the newest stable
/// release's tag page, and its assets download from a fixed URL. No notes, size or
/// digest this way, and no pre-releases, but it answers when the API won't.
fn check_via_website() -> Result<Check, String> {
    use ureq::ResponseExt;

    let resp = agent()
        .head(format!("https://github.com/{REPO}/releases/latest"))
        .call()
        .map_err(|e| e.to_string())?;
    let landed = resp.get_uri().path().to_string();
    // With no releases at all it lands on /releases rather than /releases/tag/<tag>.
    let Some(tag) = landed.rsplit_once("/releases/tag/").map(|(_, t)| t.to_string()) else {
        return Ok(Check::UpToDate);
    };
    let Some(v) = parse_version(&tag) else { return Ok(Check::UpToDate) };
    if v <= current_version() {
        return Ok(Check::UpToDate);
    }
    Ok(Check::Available(Release {
        version: v.to_string(),
        name: format!("Launcher {v}"),
        notes: String::new(),
        published: String::new(),
        page: format!("https://github.com/{REPO}/releases/tag/{tag}"),
        prerelease: false,
        asset_url: format!("https://github.com/{REPO}/releases/download/{tag}/{ASSET_NAME}"),
        size: 0,
        digest: None,
    }))
}

fn pick_asset(assets: &[GhAsset]) -> Option<&GhAsset> {
    assets
        .iter()
        .find(|a| a.name.eq_ignore_ascii_case(ASSET_NAME))
        .or_else(|| {
            assets.iter().find(|a| {
                let n = a.name.to_ascii_lowercase();
                n.ends_with(".exe") && n.contains("launcher")
            })
        })
}

fn asset_release(r: &GhRelease, a: &GhAsset) -> Release {
    Release {
        version: String::new(),
        name: r.name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| r.tag_name.clone()),
        notes: r.body.clone().unwrap_or_default(),
        published: r.published_at.clone().unwrap_or_default(),
        page: r.html_url.clone(),
        prerelease: r.prerelease,
        asset_url: a.browser_download_url.clone(),
        size: a.size,
        digest: a.digest.clone(),
    }
}

fn exe_path() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|e| format!("Couldn't locate the launcher's own file ({e})"))
}

fn sibling(suffix: &str) -> Result<PathBuf, String> {
    let exe = exe_path()?;
    let mut name = exe.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    Ok(exe.with_file_name(name))
}

/// The verified download, waiting to be swapped in.
pub fn staged_path() -> Result<PathBuf, String> {
    sibling(".update")
}

/// Downloads the release beside the exe (same volume, so the swap is a rename),
/// verifies it, and returns the staged path. `progress` gets (bytes so far, total).
pub fn download(rel: &Release, mut progress: impl FnMut(u64, u64)) -> Result<PathBuf, String> {
    let part = sibling(".download")?;
    let staged = staged_path()?;

    let mut resp = agent()
        .get(&rel.asset_url)
        .call()
        .map_err(|e| format!("Download failed ({e})"))?;
    if resp.status() != 200 {
        return Err(format!("Download failed: HTTP {}", resp.status().as_u16()));
    }

    // The website fallback doesn't know the size up front; the response does.
    let total = match rel.size {
        0 => resp
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok()?.parse().ok())
            .unwrap_or(0),
        n => n,
    };
    let mut file = File::create(&part).map_err(|e| write_err(&part, e))?;
    let mut reader = resp.body_mut().as_reader();
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut done = 0u64;
    let mut head = [0u8; 2];

    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("Download interrupted ({e})"))?;
        if n == 0 {
            break;
        }
        if done == 0 && n >= 2 {
            head.copy_from_slice(&buf[..2]);
        }
        file.write_all(&buf[..n]).map_err(|e| write_err(&part, e))?;
        hasher.update(&buf[..n]);
        done += n as u64;
        progress(done, total);
    }
    file.sync_all().map_err(|e| write_err(&part, e))?;
    drop(file);

    let verify = || -> Result<(), String> {
        if total > 0 && done != total {
            return Err(format!("Download was incomplete ({done} of {total} bytes)."));
        }
        if &head != b"MZ" {
            return Err("The downloaded file isn't a Windows program.".into());
        }
        if let Some(expected) = rel.digest.as_deref().and_then(|d| d.strip_prefix("sha256:")) {
            let got = format!("{:x}", hasher.clone().finalize());
            if !got.eq_ignore_ascii_case(expected) {
                return Err("The download didn't match GitHub's checksum.".into());
            }
        }
        Ok(())
    };
    if let Err(e) = verify() {
        let _ = fs::remove_file(&part);
        return Err(e);
    }

    let _ = fs::remove_file(&staged);
    fs::rename(&part, &staged).map_err(|e| write_err(&staged, e))?;
    Ok(staged)
}

fn write_err(path: &Path, e: std::io::Error) -> String {
    format!(
        "Couldn't write {} ({e}). Is the launcher's folder read-only?",
        path.file_name().unwrap_or_default().to_string_lossy()
    )
}

/// Swaps the staged download in for the running exe. Returns the exe path.
pub fn apply(staged: &Path) -> Result<PathBuf, String> {
    let exe = exe_path()?;
    let mut old = sibling(".old")?;

    // A previous .old may still be held by a copy that's exiting; pick a fresh name.
    if old.exists() && fs::remove_file(&old).is_err() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        old = sibling(&format!(".{stamp}.old"))?;
    }

    fs::rename(&exe, &old).map_err(|e| write_err(&exe, e))?;
    if let Err(e) = fs::rename(staged, &exe) {
        // Put the original back so the launcher still opens next time.
        let _ = fs::rename(&old, &exe);
        return Err(write_err(&exe, e));
    }
    Ok(exe)
}

pub fn relaunch(exe: &Path) -> Result<(), String> {
    std::process::Command::new(exe)
        .arg("--updated")
        .current_dir(app_dir())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Couldn't start the new version ({e})"))
}

/// Removes the previous version and any abandoned download. The old exe may still
/// be running for a moment while it exits, so this retries briefly off-thread.
pub fn sweep_leftovers() {
    let Ok(exe) = exe_path() else { return };
    let stem = exe.file_name().unwrap_or_default().to_string_lossy().to_ascii_lowercase();
    std::thread::spawn(move || {
        for _ in 0..20 {
            let mut remaining = 0;
            if let Ok(entries) = fs::read_dir(app_dir()) {
                for e in entries.flatten() {
                    let name = e.file_name().to_string_lossy().to_ascii_lowercase();
                    let leftover = name.starts_with(&stem)
                        && (name.ends_with(".old") || name.ends_with(".download"));
                    if leftover && fs::remove_file(e.path()).is_err() {
                        remaining += 1;
                    }
                }
            }
            if remaining == 0 {
                return;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    });
}
