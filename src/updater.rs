//! Self-update from GitHub releases.
//!
//! A release is eligible when its tag parses as a semver version (a leading `v` is
//! fine) and it carries the `Launcher-windows-x64.zip` package. The newest eligible
//! release that is newer than this build wins.
//!
//! The package is the whole program folder plus `launcher-manifest.json`, which
//! lists every file with its size and SHA-256. An update is checked twice: the zip
//! against the SHA-256 GitHub publishes for the asset, then every unpacked file
//! against the manifest. Only then is anything swapped in.
//!
//! Installing never overwrites a file in use: Windows lets a running exe or a
//! loaded DLL be *renamed*, just not overwritten. So each file being replaced is
//! renamed to `<name>.launcher-old`, the new one takes its place, and the new copy
//! is started. The next launch sweeps up the leftovers.

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
/// The release asset holding the whole program folder. Built by tools\package.ps1.
pub const PACKAGE_ASSET: &str = "Launcher-windows-x64.zip";
/// Lists every file in a package; also kept beside the installed exe.
const MANIFEST: &str = "launcher-manifest.json";
const STAGE_DIR: &str = ".launcher-update";
const OLD_SUFFIX: &str = ".launcher-old";

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

#[derive(Deserialize)]
struct Manifest {
    version: String,
    /// Which entry is the launcher itself; it takes the running exe's place even
    /// if someone renamed their copy.
    exe: String,
    files: Vec<ManifestFile>,
}

#[derive(Deserialize)]
struct ManifestFile {
    path: String,
    size: u64,
    sha256: String,
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
            let asset = r.assets.iter().find(|a| a.name.eq_ignore_ascii_case(PACKAGE_ASSET))?;
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
/// GitHub digest this way, and no pre-releases, but it answers when the API won't.
/// The manifest inside the package still checks every file.
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
        asset_url: format!("https://github.com/{REPO}/releases/download/{tag}/{PACKAGE_ASSET}"),
        size: 0,
        digest: None,
    }))
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

/// Where a download is unpacked and checked, beside the exe so the swap is a
/// rename on the same drive.
fn stage_dir() -> PathBuf {
    app_dir().join(STAGE_DIR)
}

/// Written only once every file in the package has been verified.
fn ready_marker() -> PathBuf {
    stage_dir().join("ready")
}

/// The verified update waiting to be swapped in, if there is one.
pub fn staged() -> Option<PathBuf> {
    ready_marker().exists().then(stage_dir)
}

/// A package path made safe to join onto the install folder: relative, no `..`,
/// no drive letters, and never into `data\` or the staging folder.
fn safe_relative(p: &str) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for part in p.split(['/', '\\']) {
        if part.is_empty() || part == "." || part == ".." || part.contains(':') {
            return None;
        }
        out.push(part);
    }
    let first = out.components().next()?.as_os_str().to_string_lossy().to_ascii_lowercase();
    (first != "data" && first != STAGE_DIR).then_some(out)
}

fn sha256_file(path: &Path) -> std::io::Result<(u64, String)> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let size = std::io::copy(&mut file, &mut hasher)?;
    Ok((size, format!("{:x}", hasher.finalize())))
}

/// Downloads the release package, checks it against GitHub's SHA-256, unpacks it
/// and checks every file against the package's manifest. Returns the staging
/// folder. `progress` gets (bytes so far, total).
pub fn download(rel: &Release, progress: impl FnMut(u64, u64)) -> Result<PathBuf, String> {
    let stage = stage_dir();
    let _ = fs::remove_dir_all(&stage);
    let result = download_into(&stage, rel, progress);
    if result.is_err() {
        let _ = fs::remove_dir_all(&stage);
    }
    result.map(|_| stage)
}

fn download_into(stage: &Path, rel: &Release, mut progress: impl FnMut(u64, u64)) -> Result<(), String> {
    fs::create_dir_all(stage).map_err(|e| write_err(stage, e))?;
    let zip_path = stage.join("package.zip");

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
    let mut file = File::create(&zip_path).map_err(|e| write_err(&zip_path, e))?;
    let mut reader = resp.body_mut().as_reader();
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut done = 0u64;

    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("Download interrupted ({e})"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| write_err(&zip_path, e))?;
        hasher.update(&buf[..n]);
        done += n as u64;
        progress(done, total);
    }
    file.sync_all().map_err(|e| write_err(&zip_path, e))?;
    drop(file);

    if total > 0 && done != total {
        return Err(format!("Download was incomplete ({done} of {total} bytes)."));
    }
    // GitHub publishes a SHA-256 for every release asset; the package has to match it.
    if let Some(expected) = rel.digest.as_deref().and_then(|d| d.strip_prefix("sha256:")) {
        let got = format!("{:x}", hasher.finalize());
        if !got.eq_ignore_ascii_case(expected) {
            return Err("The download didn't match GitHub's checksum.".into());
        }
    }

    let files = stage.join("files");
    extract(&zip_path, &files)?;
    let manifest = verify_files(&files)?;
    let _ = fs::remove_file(&zip_path);
    fs::write(ready_marker(), &manifest.version).map_err(|e| write_err(stage, e))
}

fn extract(zip_path: &Path, dest: &Path) -> Result<(), String> {
    fn damaged(e: impl std::fmt::Display) -> String {
        format!("The update package is damaged ({e})")
    }
    let file = File::open(zip_path).map_err(damaged)?;
    let mut archive = zip::ZipArchive::new(file).map_err(damaged)?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(damaged)?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().map_err(damaged)?.into_owned();
        let rel = safe_relative(&name)
            .ok_or_else(|| format!("The update package contains an unsafe path: {name}"))?;
        let out = dest.join(rel);
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).map_err(|e| write_err(parent, e))?;
        }
        let mut f = File::create(&out).map_err(|e| write_err(&out, e))?;
        std::io::copy(&mut entry, &mut f).map_err(damaged)?;
    }
    Ok(())
}

/// Every file the manifest lists must be present with exactly its size and SHA-256.
fn verify_files(files: &Path) -> Result<Manifest, String> {
    let raw = fs::read(files.join(MANIFEST)).map_err(|_| "The update package has no manifest.".to_string())?;
    let manifest: Manifest =
        serde_json::from_slice(&raw).map_err(|e| format!("The update manifest is unreadable ({e})"))?;

    if !manifest.files.iter().any(|f| f.path == manifest.exe) {
        return Err("The update package doesn't contain the launcher.".into());
    }
    for f in &manifest.files {
        let rel = safe_relative(&f.path)
            .ok_or_else(|| format!("The update manifest lists an unsafe path: {}", f.path))?;
        let (size, hash) =
            sha256_file(&files.join(rel)).map_err(|_| format!("The update package is missing {}.", f.path))?;
        if size != f.size || !hash.eq_ignore_ascii_case(&f.sha256) {
            return Err(format!("{} in the update package failed its checksum.", f.path));
        }
    }

    let mut head = [0u8; 2];
    File::open(files.join(&manifest.exe))
        .and_then(|mut f| f.read_exact(&mut head))
        .map_err(|e| format!("Couldn't read the new launcher ({e})"))?;
    if &head != b"MZ" {
        return Err("The new launcher isn't a Windows program.".into());
    }
    Ok(manifest)
}

fn write_err(path: &Path, e: std::io::Error) -> String {
    format!(
        "Couldn't write {} ({e}). Is the launcher's folder read-only?",
        path.file_name().unwrap_or_default().to_string_lossy()
    )
}

/// The name a replaced file is moved aside to. Windows lets a running exe or a
/// loaded DLL be renamed, just not overwritten, so this works for both.
fn aside(path: &Path) -> PathBuf {
    let with = |suffix: String| {
        let mut name = path.file_name().unwrap_or_default().to_os_string();
        name.push(suffix);
        path.with_file_name(name)
    };
    let first = with(OLD_SUFFIX.into());
    if !first.exists() || fs::remove_file(&first).is_ok() {
        return first;
    }
    // Still held by a copy that's exiting; pick a fresh name.
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    with(format!(".{stamp}{OLD_SUFFIX}"))
}

/// Paths the installed copy's manifest says it shipped, if it has one.
fn installed_files() -> Vec<PathBuf> {
    fs::read(app_dir().join(MANIFEST))
        .ok()
        .and_then(|raw| serde_json::from_slice::<Manifest>(&raw).ok())
        .map(|m| m.files.iter().filter_map(|f| safe_relative(&f.path)).collect())
        .unwrap_or_default()
}

/// Swaps every file in the verified package into the install folder, moving the
/// files it replaces aside, and retires files the new version no longer ships.
/// Any failure puts back what was already moved. Returns the exe path.
pub fn apply(stage: &Path) -> Result<PathBuf, String> {
    let files = stage.join("files");
    // Checked again here: a staged update may have sat on disk since a previous run.
    let manifest = verify_files(&files)?;
    let dir = app_dir();
    let exe = exe_path()?;
    let exe_rel = PathBuf::from(&manifest.exe);
    let target = |rel: &Path| if rel == exe_rel { exe.clone() } else { dir.join(rel) };

    let incoming: Vec<PathBuf> = manifest
        .files
        .iter()
        .filter_map(|f| safe_relative(&f.path))
        .chain(std::iter::once(PathBuf::from(MANIFEST)))
        .collect();
    let previous = installed_files();

    let mut moved: Vec<(PathBuf, Option<PathBuf>)> = Vec::new();
    for rel in &incoming {
        let (src, dst) = (files.join(rel), target(rel));
        let step = || -> std::io::Result<Option<PathBuf>> {
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent)?;
            }
            let backup = if dst.exists() {
                let b = aside(&dst);
                fs::rename(&dst, &b)?;
                Some(b)
            } else {
                None
            };
            if let Err(e) = fs::rename(&src, &dst) {
                if let Some(b) = &backup {
                    let _ = fs::rename(b, &dst);
                }
                return Err(e);
            }
            Ok(backup)
        };
        match step() {
            Ok(backup) => moved.push((dst, backup)),
            Err(e) => {
                // Undo in reverse so the old version is whole again.
                for (done, backup) in moved.into_iter().rev() {
                    let _ = fs::remove_file(&done);
                    if let Some(b) = backup {
                        let _ = fs::rename(b, &done);
                    }
                }
                return Err(write_err(&dst, e));
            }
        }
    }

    // Files the old version shipped that the new one doesn't: moved aside, and
    // deleted with the rest of the leftovers on the next start.
    let keep: std::collections::HashSet<String> =
        incoming.iter().map(|p| p.to_string_lossy().to_lowercase()).collect();
    for rel in previous {
        if !keep.contains(&rel.to_string_lossy().to_lowercase()) {
            let p = dir.join(&rel);
            if p.is_file() {
                let _ = fs::rename(&p, aside(&p));
            }
        }
    }

    let _ = fs::remove_dir_all(stage);
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

/// Removes what the last update moved aside and any half-finished download.
/// Replaced files may still be held for a moment by the copy that's exiting, so
/// the deleting retries briefly off-thread.
pub fn sweep_leftovers() {
    // A download that never reached the ready marker is useless. Cleared here,
    // before the launch-time check can start a new one in the same folder.
    if staged().is_none() {
        let _ = fs::remove_dir_all(stage_dir());
    }
    let Ok(exe) = exe_path() else { return };
    // 0.1.0 moved its exe aside as `Launcher.exe.old`; clear those up too.
    let legacy = exe.file_name().unwrap_or_default().to_string_lossy().to_ascii_lowercase();

    std::thread::spawn(move || {
        let root = app_dir();
        for _ in 0..20 {
            let mut remaining = 0;
            let mut pending = vec![root.clone()];
            while let Some(dir) = pending.pop() {
                let Ok(entries) = fs::read_dir(&dir) else { continue };
                for e in entries.flatten() {
                    let name = e.file_name().to_string_lossy().to_ascii_lowercase();
                    let path = e.path();
                    if path.is_dir() {
                        if dir != root || (name != "data" && name != STAGE_DIR) {
                            pending.push(path);
                        }
                        continue;
                    }
                    let leftover = name.ends_with(OLD_SUFFIX)
                        || (dir == root
                            && name.starts_with(&legacy)
                            && (name.ends_with(".old") || name.ends_with(".download")));
                    if leftover && fs::remove_file(&path).is_err() {
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
