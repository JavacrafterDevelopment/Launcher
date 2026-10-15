//! Updates from GitHub releases, for the launcher itself and for the apps it
//! installs. Both work the same way; a [`Target`] says which repo and which folder.
//!
//! Releases are found through update.json on the repo's `updates` branch, served
//! by GitHub's CDN with no API rate limit. If that can't be read, the GitHub API is
//! asked, and if that's rate-limited, the website.
//!
//! The package is the whole program folder plus `launcher-manifest.json`, which
//! lists every file with its size and SHA-256. Before anything is swapped in, the
//! zip must match its published SHA-256, every unpacked file must match the
//! manifest, and (via update.json) the manifest must match the file list published
//! separately for that release.
//!
//! Installing never overwrites a file in use: Windows lets a running exe or a
//! loaded DLL be *renamed*, just not overwritten. So each file being replaced is
//! renamed to `<name>.launcher-old` and the new one takes its place. The next
//! launch sweeps up the leftovers. An app's `data\` folder is never touched.

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
/// Lists every file in a package; also kept beside the installed exe.
const MANIFEST: &str = "launcher-manifest.json";
const STAGE_DIR: &str = ".launcher-update";
const OLD_SUFFIX: &str = ".launcher-old";
/// Where installed apps live, inside the launcher's folder.
pub const APPS_DIR: &str = "portableApplications";

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
    /// The package's `sha256:<hex>`, from update.json or GitHub's asset digest.
    pub digest: Option<String>,
    /// The file list update.json published for this release. When present, the
    /// manifest inside the downloaded package must match it exactly.
    #[serde(skip)]
    pub expected: Option<Expected>,
}

impl Release {
    pub fn semver(&self) -> Option<semver::Version> {
        parse_version(&self.version)
    }
}

#[derive(Clone, Debug)]
pub struct Expected {
    exe: String,
    files: Vec<ManifestFile>,
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
    /// Which entry is the program itself.
    exe: String,
    files: Vec<ManifestFile>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct ManifestFile {
    path: String,
    size: u64,
    sha256: String,
}

#[derive(Deserialize)]
struct UpdateJson {
    channels: UpdateChannels,
}

#[derive(Deserialize)]
struct UpdateChannels {
    stable: Option<UpdateEntry>,
    beta: Option<UpdateEntry>,
}

#[derive(Deserialize)]
struct UpdateEntry {
    version: String,
    #[serde(default)]
    published: String,
    #[serde(default)]
    notes: String,
    page: String,
    package: UpdatePackage,
    exe: String,
    files: Vec<ManifestFile>,
}

#[derive(Deserialize)]
struct UpdatePackage {
    url: String,
    size: u64,
    sha256: String,
}

/// What an installed copy's manifest says about it.
pub struct Installed {
    pub version: String,
    pub exe: String,
}

/// One thing that can be installed and updated: the launcher, or an app.
pub struct Target {
    repo: String,
    /// The release asset holding the program folder.
    package: String,
    /// The install folder.
    dir: PathBuf,
    /// Set for the launcher: the running exe, which the package's exe replaces
    /// even if someone renamed their copy.
    self_exe: Option<PathBuf>,
}

impl Target {
    pub fn launcher() -> Self {
        Self {
            repo: REPO.into(),
            package: "Launcher-windows-x64.zip".into(),
            dir: app_dir(),
            self_exe: std::env::current_exe().ok(),
        }
    }

    /// An app published as `<name>-windows-x64.zip` in `repo`, installed at
    /// `portableApplications\<name>` beside the launcher.
    pub fn app(repo: &str, name: &str) -> Self {
        Self {
            repo: repo.into(),
            package: format!("{name}-windows-x64.zip"),
            dir: app_dir().join(APPS_DIR).join(name),
            self_exe: None,
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The installed copy's version and exe, from its manifest. None if it isn't
    /// installed (or wasn't installed by the launcher).
    pub fn installed(&self) -> Option<Installed> {
        let raw = fs::read(self.dir.join(MANIFEST)).ok()?;
        let m: Manifest = serde_json::from_slice(&raw).ok()?;
        Some(Installed { version: m.version, exe: m.exe })
    }

    /// True while the installed program is running: Windows won't let a running
    /// exe be opened for writing.
    pub fn exe_in_use(&self) -> bool {
        use std::os::windows::fs::OpenOptionsExt;
        let Some(installed) = self.installed() else { return false };
        let Some(rel) = safe_relative(&installed.exe) else { return false };
        fs::OpenOptions::new()
            .write(true)
            .share_mode(0)
            .open(self.dir.join(rel))
            .is_err_and(|e| e.raw_os_error() == Some(32)) // ERROR_SHARING_VIOLATION
    }

    fn update_json_url(&self) -> String {
        if cfg!(debug_assertions) {
            if let Ok(base) = std::env::var("LAUNCHER_UPDATE_BASE") {
                let name = self.repo.rsplit('/').next().unwrap_or_default();
                return format!("{}/{name}/update.json", base.trim_end_matches('/'));
            }
        }
        format!("https://raw.githubusercontent.com/{}/updates/update.json", self.repo)
    }

    /// The newest published release, whatever is installed. update.json first;
    /// if it can't be read, the GitHub API; if that's rate-limited, the website.
    /// Ok(None) means nothing has been published yet.
    pub fn latest(&self, prereleases: bool) -> Result<Option<Release>, String> {
        self.latest_via_update_json(prereleases)
            .or_else(|_| self.latest_via_api(prereleases))
    }

    fn latest_via_update_json(&self, prereleases: bool) -> Result<Option<Release>, String> {
        let mut resp = agent().get(&self.update_json_url()).call().map_err(|e| e.to_string())?;
        if resp.status() != 200 {
            return Err(format!("HTTP {}", resp.status().as_u16()));
        }
        let doc: UpdateJson = resp.body_mut().read_json().map_err(|e| e.to_string())?;

        // With pre-releases on, whichever of the two channels is newer.
        let candidates = [Some(doc.channels.stable), prereleases.then_some(doc.channels.beta)];
        let best = candidates
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| Some((parse_version(&e.version)?, e)))
            .max_by(|a, b| a.0.cmp(&b.0));

        Ok(best.map(|(v, e)| Release {
            version: v.to_string(),
            name: format!("v{v}"),
            notes: e.notes,
            published: e.published,
            page: e.page,
            prerelease: !v.pre.is_empty(),
            asset_url: e.package.url,
            size: e.package.size,
            digest: Some(format!("sha256:{}", e.package.sha256)),
            expected: Some(Expected { exe: e.exe, files: e.files }),
        }))
    }

    fn latest_via_api(&self, prereleases: bool) -> Result<Option<Release>, String> {
        let url = format!("{}/repos/{}/releases?per_page=30", api_base(), self.repo);
        let mut resp = agent()
            .get(&url)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .call()
            .map_err(|_| "Couldn't reach GitHub. Are you online?".to_string())?;

        match resp.status().as_u16() {
            200 => {}
            // The anonymous API allows 60 requests an hour per IP, which a VPN or a
            // shared network can use up. The website isn't limited that way.
            403 | 429 => {
                return self.latest_via_website().map_err(|_| {
                    "GitHub's hourly request limit is used up. Try again in a little while.".into()
                });
            }
            404 => return Err(format!("The repository {} wasn't found, or it's private.", self.repo)),
            s => return Err(format!("GitHub answered with HTTP {s}.")),
        }

        let releases: Vec<GhRelease> = resp
            .body_mut()
            .read_json()
            .map_err(|e| format!("GitHub sent something unreadable ({e})"))?;

        Ok(releases
            .into_iter()
            .filter(|r| !r.draft && (prereleases || !r.prerelease))
            .filter_map(|r| {
                let v = parse_version(&r.tag_name)?;
                let a = r.assets.iter().find(|a| a.name.eq_ignore_ascii_case(&self.package))?;
                Some((v, release_from_api(&r, a)))
            })
            .max_by(|a, b| a.0.cmp(&b.0))
            .map(|(v, mut rel)| {
                rel.version = v.to_string();
                rel
            }))
    }

    /// Fallback without the API: `/releases/latest` redirects to the newest stable
    /// release's tag page, and its assets download from a fixed URL. No notes or
    /// published SHA-256 this way, and no pre-releases, but it answers when the API
    /// won't. The manifest inside the package still checks every file.
    fn latest_via_website(&self) -> Result<Option<Release>, String> {
        use ureq::ResponseExt;

        let resp = agent()
            .head(format!("https://github.com/{}/releases/latest", self.repo))
            .call()
            .map_err(|e| e.to_string())?;
        let landed = resp.get_uri().path().to_string();
        // With no releases at all it lands on /releases rather than /releases/tag/<tag>.
        let Some(tag) = landed.rsplit_once("/releases/tag/").map(|(_, t)| t.to_string()) else {
            return Ok(None);
        };
        let Some(v) = parse_version(&tag) else { return Ok(None) };
        Ok(Some(Release {
            version: v.to_string(),
            name: format!("v{v}"),
            notes: String::new(),
            published: String::new(),
            page: format!("https://github.com/{}/releases/tag/{tag}", self.repo),
            prerelease: false,
            asset_url: format!("https://github.com/{}/releases/download/{tag}/{}", self.repo, self.package),
            size: 0,
            digest: None,
            expected: None,
        }))
    }

    /// Where a download is unpacked and checked, inside the install folder so the
    /// swap is a rename on the same drive.
    fn stage_dir(&self) -> PathBuf {
        self.dir.join(STAGE_DIR)
    }

    /// Written only once every file in the package has been verified.
    fn ready_marker(&self) -> PathBuf {
        self.stage_dir().join("ready")
    }

    /// True when a verified update is waiting to be swapped in.
    pub fn has_staged(&self) -> bool {
        self.ready_marker().exists()
    }

    /// Downloads the release package, checks it against its published SHA-256,
    /// unpacks it and checks every file. Leaves it staged for [`Target::apply`].
    /// `progress` gets (bytes so far, total); `log` gets a line for each step.
    pub fn download(&self, rel: &Release, progress: impl FnMut(u64, u64), log: &dyn Fn(String)) -> Result<(), String> {
        let stage = self.stage_dir();
        let _ = fs::remove_dir_all(&stage);
        let result = self.download_into(&stage, rel, progress, log);
        if result.is_err() {
            let _ = fs::remove_dir_all(&stage);
        }
        result
    }

    fn download_into(
        &self,
        stage: &Path,
        rel: &Release,
        mut progress: impl FnMut(u64, u64),
        log: &dyn Fn(String),
    ) -> Result<(), String> {
        log(format!("Downloading v{} from {}", rel.version, rel.asset_url));
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
        log(match total {
            0 => "Size not published; going by what the server sends".to_string(),
            n => format!("Package is {}", mb(n)),
        });
        let mut file = File::create(&zip_path).map_err(|e| write_err(&zip_path, e))?;
        let mut reader = resp.body_mut().as_reader();
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 64 * 1024];
        let mut done = 0u64;
        let mut logged_tenth = 0;

        loop {
            let n = reader.read(&mut buf).map_err(|e| format!("Download interrupted ({e})"))?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n]).map_err(|e| write_err(&zip_path, e))?;
            hasher.update(&buf[..n]);
            done += n as u64;
            progress(done, total);
            if total > 0 && done * 10 / total > logged_tenth {
                logged_tenth = done * 10 / total;
                log(format!("Downloaded {}% ({} of {})", logged_tenth * 10, mb(done), mb(total)));
            }
        }
        file.sync_all().map_err(|e| write_err(&zip_path, e))?;
        drop(file);

        if total > 0 && done != total {
            return Err(format!("Download was incomplete ({done} of {total} bytes)."));
        }
        if let Some(expected) = rel.digest.as_deref().and_then(|d| d.strip_prefix("sha256:")) {
            let got = format!("{:x}", hasher.finalize());
            if !got.eq_ignore_ascii_case(expected) {
                return Err("The download didn't match its published checksum.".into());
            }
            log(format!("Package SHA-256 matches the published one ({}…)", &got[..12]));
        } else {
            log("No published SHA-256 for the package; relying on the per-file checks".into());
        }

        let files = stage.join("files");
        log("Unpacking".into());
        extract(&zip_path, &files)?;
        let manifest = verify_files(&files)?;
        log(format!("Checked all {} files against their SHA-256 in the manifest", manifest.files.len()));
        // The package's own manifest came out of the zip, so on its own it only shows
        // the zip is self-consistent. update.json's list comes separately; a package
        // whose list differs from it in any file, size or hash is refused.
        if let Some(expected) = &rel.expected {
            let sorted = |files: &[ManifestFile]| {
                let mut v = files.to_vec();
                v.sort_by(|a, b| a.path.cmp(&b.path));
                v
            };
            if manifest.exe != expected.exe || sorted(&manifest.files) != sorted(&expected.files) {
                return Err("The update package doesn't match its published file list.".into());
            }
            log("File list matches the one published in update.json".into());
        }
        let _ = fs::remove_file(&zip_path);
        fs::write(self.ready_marker(), &manifest.version).map_err(|e| write_err(stage, e))
    }

    /// Swaps every file in the staged package into the install folder, moving the
    /// files it replaces aside, and retires files the new version no longer ships.
    /// Any failure puts back what was already moved.
    pub fn apply(&self, log: &dyn Fn(String)) -> Result<(), String> {
        let stage = self.stage_dir();
        let files = stage.join("files");
        // Checked again here: a staged update may have sat on disk since a previous run.
        let manifest = verify_files(&files)?;
        let exe_rel = PathBuf::from(&manifest.exe);
        let target = |rel: &Path| match &self.self_exe {
            Some(exe) if rel == exe_rel => exe.clone(),
            _ => self.dir.join(rel),
        };

        let incoming: Vec<PathBuf> = manifest
            .files
            .iter()
            .filter_map(|f| safe_relative(&f.path))
            .chain(std::iter::once(PathBuf::from(MANIFEST)))
            .collect();
        let previous = installed_files(&self.dir);
        log(format!("Moving {} files into {}", incoming.len(), self.dir.display()));
        let mut replaced = 0;

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
                Ok(backup) => {
                    replaced += backup.is_some() as usize;
                    moved.push((dst, backup));
                }
                Err(e) => {
                    log(format!("Couldn't place {}; putting the previous files back", rel.display()));
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
        let mut retired = 0;
        for rel in previous {
            if !keep.contains(&rel.to_string_lossy().to_lowercase()) {
                let p = self.dir.join(&rel);
                if p.is_file() {
                    log(format!("Removing {} (no longer part of the app)", rel.display()));
                    retired += fs::rename(&p, aside(&p)).is_ok() as usize;
                }
            }
        }

        let _ = fs::remove_dir_all(&stage);
        log(format!("{replaced} files replaced, {} added, {retired} removed", incoming.len() - replaced));
        Ok(())
    }

    /// Removes what the last update moved aside and any half-finished download.
    /// Replaced files may still be held for a moment by a copy that's exiting, so
    /// the deleting retries briefly off-thread. `data\`, the staging folder and the
    /// launcher's `portableApplications\` are never walked into.
    pub fn sweep_leftovers(&self) {
        // A download that never reached the ready marker is useless. Cleared here,
        // before a check can start a new one in the same folder.
        if !self.has_staged() {
            let _ = fs::remove_dir_all(self.stage_dir());
        }
        // 0.1.0 moved its exe aside as `Launcher.exe.old`; clear those up too.
        let legacy = self
            .self_exe
            .as_ref()
            .map(|exe| exe.file_name().unwrap_or_default().to_string_lossy().to_ascii_lowercase());
        let root = self.dir.clone();

        std::thread::spawn(move || {
            for _ in 0..20 {
                let mut remaining = 0;
                let mut pending = vec![root.clone()];
                while let Some(dir) = pending.pop() {
                    let Ok(entries) = fs::read_dir(&dir) else { continue };
                    for e in entries.flatten() {
                        let name = e.file_name().to_string_lossy().to_ascii_lowercase();
                        let path = e.path();
                        if path.is_dir() {
                            let skip = dir == root
                                && ["data", STAGE_DIR, &APPS_DIR.to_ascii_lowercase()].contains(&name.as_str());
                            if !skip {
                                pending.push(path);
                            }
                            continue;
                        }
                        let legacy_old = legacy.as_ref().is_some_and(|l| {
                            dir == root
                                && name.starts_with(l.as_str())
                                && (name.ends_with(".old") || name.ends_with(".download"))
                        });
                        if (name.ends_with(OLD_SUFFIX) || legacy_old) && fs::remove_file(&path).is_err() {
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

/// Debug builds can be pointed at a stand-in for the GitHub API to test the
/// fallback without GitHub. Release builds always use GitHub.
fn api_base() -> String {
    if cfg!(debug_assertions) {
        if let Ok(base) = std::env::var("LAUNCHER_UPDATE_API") {
            return base.trim_end_matches('/').to_string();
        }
    }
    "https://api.github.com".into()
}

fn release_from_api(r: &GhRelease, a: &GhAsset) -> Release {
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
        expected: None,
    }
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
        return Err("The update package doesn't contain its program.".into());
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
        .map_err(|e| format!("Couldn't read the new program ({e})"))?;
    if &head != b"MZ" {
        return Err("The new program isn't a Windows program.".into());
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
fn installed_files(dir: &Path) -> Vec<PathBuf> {
    fs::read(dir.join(MANIFEST))
        .ok()
        .and_then(|raw| serde_json::from_slice::<Manifest>(&raw).ok())
        .map(|m| m.files.iter().filter_map(|f| safe_relative(&f.path)).collect())
        .unwrap_or_default()
}

pub fn relaunch(exe: &Path) -> Result<(), String> {
    std::process::Command::new(exe)
        .arg("--updated")
        .current_dir(app_dir())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Couldn't start the new version ({e})"))
}

fn mb(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_048_576.0)
}
