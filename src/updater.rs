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

use crate::settings::{app_dir, install_dir};

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

#[derive(Serialize, Deserialize)]
struct Manifest {
    version: String,
    /// Which entry is the program itself.
    exe: String,
    files: Vec<ManifestFile>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
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
    /// The release asset holding the program folder. `{version}` is replaced with
    /// the release's version for repos that put the version in the file name.
    package: String,
    /// The install folder.
    dir: PathBuf,
    /// Set for the launcher: the running exe, which the package's exe replaces
    /// even if someone renamed their copy.
    self_exe: Option<PathBuf>,
    /// True when the repo publishes update.json on an `updates` branch.
    update_json: bool,
    /// For third-party packages that don't ship a launcher-manifest.json: the
    /// program exe inside the archive, used to write a manifest on unpack.
    default_exe: Option<String>,
}

impl Target {
    pub fn launcher() -> Self {
        let install = install_dir();
        let dir = if crate::settings::is_dir_writable(&install) {
            install
        } else {
            app_dir()
        };
        Self {
            repo: REPO.into(),
            package: if cfg!(windows) {
                "Launcher-windows-x64.zip".into()
            } else {
                "Launcher-macos.zip".into()
            },
            dir,
            self_exe: if cfg!(windows) { std::env::current_exe().ok() } else { None },
            update_json: true,
            default_exe: None,
        }
    }

    /// An app published in `repo`, installed at `portableApplications/<name>`
    /// beside the launcher.
    pub fn app(repo: &str, name: &str) -> Self {
        Self {
            repo: repo.into(),
            package: if cfg!(windows) {
                format!("{name}-windows-x64.zip")
            } else {
                format!("{name}-macos.zip")
            },
            dir: app_dir().join(APPS_DIR).join(name),
            self_exe: None,
            update_json: true,
            default_exe: None,
        }
    }

    /// A third-party app published on GitHub releases, installed at
    /// `portableApplications\<name>`. `package` may contain `{version}`.
    pub fn third_party(repo: &str, name: &str, package: &str, exe: &str) -> Self {
        Self {
            repo: repo.into(),
            package: package.into(),
            dir: app_dir().join(APPS_DIR).join(name),
            self_exe: None,
            update_json: false,
            default_exe: Some(exe.into()),
        }
    }

    fn asset_name(&self, version: &str) -> String {
        self.package.replace("{version}", version)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The installed copy's version and exe, from its manifest. For a third-party
    /// app unpacked into `portableApplications\<name>` by hand (with no manifest
    /// yet), reads the version from the exe and writes `launcher-manifest.json`.
    pub fn installed(&self) -> Option<Installed> {
        if let Ok(raw) = fs::read(self.dir.join(MANIFEST)) {
            if let Ok(m) = serde_json::from_slice::<Manifest>(&raw) {
                return Some(Installed { version: m.version, exe: m.exe });
            }
        }
        let exe = self.default_exe.as_deref()?;
        let (version, resolved_exe) = adopt_unpacked(&self.dir, exe)?;
        Some(Installed { version, exe: resolved_exe })
    }

    /// True while the installed program is running: Windows won't let a running
    /// exe be opened for writing; on macOS we check running processes under `dir`.
    #[cfg(windows)]
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

    #[cfg(not(windows))]
    pub fn exe_in_use(&self) -> bool {
        !crate::procs::running_in(&self.dir).is_empty()
    }

    fn update_json_url(&self) -> String {
        let file = if cfg!(windows) { "update.json" } else { "update-macos.json" };
        if cfg!(debug_assertions) {
            if let Ok(base) = std::env::var("LAUNCHER_UPDATE_BASE") {
                let name = self.repo.rsplit('/').next().unwrap_or_default();
                return format!("{}/{name}/{file}", base.trim_end_matches('/'));
            }
        }
        format!("https://raw.githubusercontent.com/{}/updates/{file}", self.repo)
    }

    /// The newest published release, whatever is installed. update.json first (for
    /// repos that publish one); if it can't be read, the GitHub API; if that's
    /// rate-limited, the website. Ok(None) means nothing has been published yet.
    pub fn latest(&self, prereleases: bool) -> Result<Option<Release>, String> {
        if self.update_json {
            self.latest_via_update_json(prereleases)
                .or_else(|_| self.latest_via_api(prereleases))
        } else {
            self.latest_via_api(prereleases)
        }
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
                let want = self.asset_name(&v.to_string());
                let a = r.assets.iter().find(|a| a.name.eq_ignore_ascii_case(&want))?;
                Some((v, release_from_api(&r, a)))
            })
            .max_by(|a, b| a.0.cmp(&b.0))
            .map(|(v, mut rel)| {
                rel.version = v.to_string();
                rel
            }))
    }

    /// Fallback without the API: `/releases/latest` redirects to the newest stable
    /// release's tag page, and its assets download from a fixed URL. No notes and
    /// no pre-releases this way, but it answers when the API won't, and still pulls
    /// the published SHA-256 from the release's asset list page.
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
        let version = v.to_string();
        let package = self.asset_name(&version);
        let digest = website_asset_digest(&self.repo, &tag, &package);
        Ok(Some(Release {
            version,
            name: format!("v{v}"),
            notes: String::new(),
            published: String::new(),
            page: format!("https://github.com/{}/releases/tag/{tag}", self.repo),
            prerelease: false,
            asset_url: format!("https://github.com/{}/releases/download/{tag}/{package}", self.repo),
            size: 0,
            digest,
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
        let is_dmg = self.package.ends_with(".dmg") || rel.asset_url.ends_with(".dmg");
        let pkg_path = stage.join(if is_dmg { "package.dmg" } else { "package.zip" });

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
        let mut file = File::create(&pkg_path).map_err(|e| write_err(&pkg_path, e))?;
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
            file.write_all(&buf[..n]).map_err(|e| write_err(&pkg_path, e))?;
            hasher.update(&buf[..n]);
            done += n as u64;
            progress(done, total);
            if total > 0 && done * 10 / total > logged_tenth {
                logged_tenth = done * 10 / total;
                log(format!("Downloaded {}% ({} of {})", logged_tenth * 10, mb(done), mb(total)));
            }
        }
        file.sync_all().map_err(|e| write_err(&pkg_path, e))?;
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
        if is_dmg {
            extract_dmg(&pkg_path, &files)?;
        } else {
            extract(&pkg_path, &files)?;
        }
        if !files.join(MANIFEST).exists() {
            if let Some(exe) = &self.default_exe {
                write_manifest(&files, &rel.version, exe)?;
            }
        }
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
        let _ = fs::remove_file(&pkg_path);
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

        // An `.app` bundle moves as one piece. Once a bundle signed by another
        // developer has run, macOS (App Management) refuses any change to the files
        // inside it, but still lets the bundle itself be moved aside.
        let mut units: Vec<PathBuf> = Vec::new();
        for rel in &incoming {
            let unit = bundle_of(rel).unwrap_or_else(|| rel.clone());
            if !units.contains(&unit) {
                units.push(unit);
            }
        }

        let mut moved: Vec<(PathBuf, Option<PathBuf>)> = Vec::new();
        for rel in &units {
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
                        let _ = if done.is_dir() { fs::remove_dir_all(&done) } else { fs::remove_file(&done) };
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
        log(format!("{replaced} replaced, {} added, {retired} removed", units.len() - replaced));
        Ok(())
    }

    fn installed_manifest(&self) -> Result<Manifest, String> {
        let raw = fs::read(self.dir.join(MANIFEST))
            .map_err(|_| "Its file list (launcher-manifest.json) is missing. Reinstall it instead.".to_string())?;
        serde_json::from_slice(&raw)
            .map_err(|_| "Its file list (launcher-manifest.json) is damaged. Reinstall it instead.".to_string())
    }

    /// Where an installed file lives; the launcher's own exe may have been renamed.
    fn installed_path(&self, m: &Manifest, rel: &Path) -> PathBuf {
        match &self.self_exe {
            Some(exe) if rel == Path::new(&m.exe) => exe.clone(),
            _ => self.dir.join(rel),
        }
    }

    /// The package of one particular version, which is where repairs come from.
    fn package_url(&self, version: &str) -> String {
        let package = self.asset_name(version);
        if cfg!(debug_assertions) {
            if let Ok(base) = std::env::var("LAUNCHER_UPDATE_BASE") {
                let name = self.repo.rsplit('/').next().unwrap_or_default();
                return format!("{}/{name}/v{version}/{package}", base.trim_end_matches('/'));
            }
        }
        format!("https://github.com/{}/releases/download/v{version}/{package}", self.repo)
    }

    /// Hashes every installed program file against the installed manifest. Returns
    /// the paths that are missing or don't match. `data\` isn't part of the list:
    /// settings are meant to change. `progress` gets (files checked, total).
    pub fn verify(&self, mut progress: impl FnMut(usize, usize), log: &dyn Fn(String)) -> Result<Vec<String>, String> {
        let m = self.installed_manifest()?;
        log(format!("Checking {} files of v{} against their SHA-256", m.files.len(), m.version));
        let mut bad = Vec::new();
        for (i, f) in m.files.iter().enumerate() {
            let verdict = match safe_relative(&f.path) {
                None => Some("listed with an unsafe path"),
                Some(rel) => match sha256_file(&self.installed_path(&m, &rel)) {
                    Err(_) => Some("missing"),
                    Ok((size, _)) if size != f.size => Some("wrong size"),
                    Ok((_, hash)) if !hash.eq_ignore_ascii_case(&f.sha256) => Some("contents changed"),
                    Ok(_) => None,
                },
            };
            if let Some(why) = verdict {
                log(format!("Damaged: {} ({why})", f.path));
                bad.push(f.path.clone());
            }
            progress(i + 1, m.files.len());
        }
        log(match bad.len() {
            0 => format!("All {} files are intact", m.files.len()),
            n => format!("{n} of {} files need repairing", m.files.len()),
        });
        Ok(bad)
    }

    /// Re-downloads just the given files, from the installed version's own release
    /// package. Only the zip's index and the bytes of those files are fetched (HTTP
    /// range requests); each file must match its SHA-256 before it replaces the
    /// damaged one. `progress` gets (bytes written, total).
    pub fn repair(&self, paths: &[String], mut progress: impl FnMut(u64, u64), log: &dyn Fn(String)) -> Result<(), String> {
        let m = self.installed_manifest()?;
        let wanted: Vec<&ManifestFile> = m.files.iter().filter(|f| paths.contains(&f.path)).collect();
        if wanted.is_empty() {
            return Ok(());
        }
        let url = self.package_url(&m.version);
        log(format!("Reading the v{} package's index from {url}", m.version));

        let stage = self.stage_dir().join("repair");
        let _ = fs::remove_dir_all(&stage);
        fs::create_dir_all(&stage).map_err(|e| write_err(&stage, e))?;
        let result = self.repair_from(&m, &wanted, &url, &stage, &mut progress, log);
        let _ = fs::remove_dir_all(self.stage_dir());
        result
    }

    fn repair_from(
        &self,
        m: &Manifest,
        wanted: &[&ManifestFile],
        url: &str,
        stage: &Path,
        progress: &mut dyn FnMut(u64, u64),
        log: &dyn Fn(String),
    ) -> Result<(), String> {
        if self.package.ends_with(".dmg") || url.ends_with(".dmg") {
            return self.repair_from_dmg(m, wanted, url, stage, progress, log);
        }
        let fetched = std::rc::Rc::new(std::cell::Cell::new((0u64, 0u32)));
        let source = open_package(url, stage, fetched.clone(), log)?;
        let mut archive = zip::ZipArchive::new(source).map_err(|e| format!("Couldn't read the package's index ({e})"))?;
        let root = zip_root_prefix(&archive);
        let entry_name = |path: &str| match &root {
            Some(r) => format!("{r}/{path}"),
            None => path.to_string(),
        };

        // The repair must come from the release that's installed. Our own packages
        // carry launcher-manifest.json inside the zip; third-party ones don't, so
        // their zip index is checked for each wanted file and its size, then the
        // fetched bytes are checked against the SHA-256 recorded at install time.
        if self.default_exe.is_none() {
            let remote: Manifest = {
                let mut entry = archive
                    .by_name(MANIFEST)
                    .map_err(|_| "The release package has no file list.".to_string())?;
                let mut raw = Vec::new();
                entry.read_to_end(&mut raw).map_err(|e| format!("Couldn't read the release's file list ({e})"))?;
                serde_json::from_slice(&raw).map_err(|e| format!("The release's file list is unreadable ({e})"))?
            };
            if remote.version != m.version || wanted.iter().any(|f| !remote.files.contains(f)) {
                return Err(format!(
                    "The installed file list doesn't match the published v{}. Reinstall it instead.",
                    m.version
                ));
            }
            log("The release's file list agrees with the installed one".into());
        } else {
            let names: std::collections::HashSet<String> =
                archive.file_names().filter_map(|n| n.ok().map(|s| s.into_owned())).collect();
            if wanted.iter().any(|f| !names.contains(&entry_name(&f.path))) {
                return Err(format!(
                    "The installed file list doesn't match the published v{}. Reinstall it instead.",
                    m.version
                ));
            }
            log("The release package's index agrees with the installed file list".into());
        }

        let total: u64 = wanted.iter().map(|f| f.size).sum();
        let mut done = 0u64;
        let mut ready = Vec::new();
        for f in wanted {
            let rel = safe_relative(&f.path).ok_or_else(|| format!("Unsafe path in the file list: {}", f.path))?;
            let name = entry_name(&f.path);
            let mut entry = archive
                .by_name(&name)
                .map_err(|_| format!("{} isn't in the release package.", f.path))?;
            if entry.size() != f.size {
                return Err(format!(
                    "The installed file list doesn't match the published v{}. Reinstall it instead.",
                    m.version
                ));
            }
            #[cfg(unix)]
            let is_exec = entry.unix_mode().is_some_and(|mode| mode & 0o111 != 0)
                || f.path == m.exe
                || f.path.contains(".app/Contents/MacOS/");
            let tmp = stage.join(&rel);
            if let Some(parent) = tmp.parent() {
                fs::create_dir_all(parent).map_err(|e| write_err(parent, e))?;
            }
            let mut out = File::create(&tmp).map_err(|e| write_err(&tmp, e))?;
            let mut hasher = Sha256::new();
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                let n = entry.read(&mut buf).map_err(|e| format!("Couldn't fetch {} ({e})", f.path))?;
                if n == 0 {
                    break;
                }
                out.write_all(&buf[..n]).map_err(|e| write_err(&tmp, e))?;
                hasher.update(&buf[..n]);
                done += n as u64;
                progress(done, total);
            }
            out.sync_all().map_err(|e| write_err(&tmp, e))?;
            #[cfg(unix)]
            if is_exec {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755));
            }
            let hash = format!("{:x}", hasher.finalize());
            if !hash.eq_ignore_ascii_case(&f.sha256) {
                return Err(format!("The downloaded {} didn't match its SHA-256.", f.path));
            }
            log(format!("Fetched {} ({}), SHA-256 matches", f.path, mb(f.size)));
            ready.push((tmp, self.installed_path(m, &rel)));
        }

        for (tmp, dst) in &ready {
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent).map_err(|e| write_err(parent, e))?;
            }
            if dst.exists() {
                fs::rename(dst, aside(dst)).map_err(|e| write_err(dst, e))?;
            }
            fs::rename(tmp, dst).map_err(|e| write_err(dst, e))?;
        }
        let (bytes, requests) = fetched.get();
        if requests > 0 {
            log(format!(
                "Repaired {} files, downloading {} in {requests} requests",
                ready.len(),
                mb(bytes)
            ));
        } else {
            log(format!("Repaired {} files", ready.len()));
        }
        Ok(())
    }

    fn repair_from_dmg(
        &self,
        m: &Manifest,
        wanted: &[&ManifestFile],
        url: &str,
        stage: &Path,
        progress: &mut dyn FnMut(u64, u64),
        log: &dyn Fn(String),
    ) -> Result<(), String> {
        log("Downloading the release disk image to repair damaged files".into());
        let dmg_path = stage.join("package.dmg");
        let mut resp = agent()
            .get(url)
            .call()
            .map_err(|e| format!("Couldn't reach the release ({e})"))?;
        if resp.status() != 200 {
            return Err(format!("The release download answered HTTP {}.", resp.status().as_u16()));
        }
        let total = resp
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok()?.parse().ok())
            .unwrap_or(0);
        let mut file = File::create(&dmg_path).map_err(|e| write_err(&dmg_path, e))?;
        let mut reader = resp.body_mut().as_reader();
        let mut buf = vec![0u8; 64 * 1024];
        let mut done = 0u64;
        loop {
            let n = reader.read(&mut buf).map_err(|e| format!("Download interrupted ({e})"))?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n]).map_err(|e| write_err(&dmg_path, e))?;
            done += n as u64;
            progress(done, total);
        }
        file.sync_all().map_err(|e| write_err(&dmg_path, e))?;
        drop(file);

        let unpacked = stage.join("unpacked");
        extract_dmg(&dmg_path, &unpacked)?;
        let mut ready = Vec::new();
        for f in wanted {
            let rel = safe_relative(&f.path).ok_or_else(|| format!("Unsafe path in the file list: {}", f.path))?;
            let src = unpacked.join(&rel);
            let (size, hash) = sha256_file(&src).map_err(|_| format!("{} isn't in the release package.", f.path))?;
            if size != f.size || !hash.eq_ignore_ascii_case(&f.sha256) {
                return Err(format!(
                    "The installed file list doesn't match the published v{}. Reinstall it instead.",
                    m.version
                ));
            }
            ready.push((src, self.installed_path(m, &rel)));
        }
        for (src, dst) in &ready {
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent).map_err(|e| write_err(parent, e))?;
            }
            if dst.exists() {
                fs::rename(dst, aside(dst)).map_err(|e| write_err(dst, e))?;
            }
            fs::rename(src, dst).map_err(|e| write_err(dst, e))?;
        }
        log(format!("Repaired {} files", ready.len()));
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
                        // A whole `.app` bundle moved aside by an update. macOS
                        // won't let us delete inside one that has run, so it goes
                        // to the Trash instead.
                        if path.is_dir() && name.ends_with(OLD_SUFFIX) {
                            if fs::remove_dir_all(&path).is_err() && trash(&path).is_err() {
                                remaining += 1;
                            }
                            continue;
                        }
                        if path.is_dir() {
                            let skip = dir == root
                                && (is_data_dir(&name)
                                    || name == STAGE_DIR
                                    || name == APPS_DIR.to_ascii_lowercase());
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
    agent_with_redirects(10)
}

fn agent_with_redirects(redirects: u32) -> ureq::Agent {
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
        .max_redirects(redirects)
        .build()
        .into()
}

/// A release package on the server, read in pieces with HTTP range requests so
/// the zip reader can pick out single files without downloading the rest. Blocks
/// already fetched are kept, since the zip's index is read more than once.
struct RangeReader {
    url: String,
    len: u64,
    pos: u64,
    blocks: std::collections::HashMap<u64, Vec<u8>>,
    /// Bytes actually downloaded and requests made, for the log.
    fetched: std::rc::Rc<std::cell::Cell<(u64, u32)>>,
}

const RANGE_BLOCK: u64 = 256 * 1024;

impl RangeReader {
    fn block(&mut self, index: u64) -> std::io::Result<&[u8]> {
        if !self.blocks.contains_key(&index) {
            let start = index * RANGE_BLOCK;
            let end = (start + RANGE_BLOCK).min(self.len) - 1;
            let mut resp = agent()
                .get(&self.url)
                .header("Range", format!("bytes={start}-{end}"))
                .call()
                .map_err(std::io::Error::other)?;
            if resp.status() != 206 {
                return Err(std::io::Error::other(format!("HTTP {} for a partial download", resp.status().as_u16())));
            }
            let bytes = resp.body_mut().read_to_vec().map_err(std::io::Error::other)?;
            let (b, n) = self.fetched.get();
            self.fetched.set((b + bytes.len() as u64, n + 1));
            self.blocks.insert(index, bytes);
        }
        Ok(&self.blocks[&index])
    }
}

impl Read for RangeReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.pos >= self.len || buf.is_empty() {
            return Ok(0);
        }
        let (index, offset) = (self.pos / RANGE_BLOCK, (self.pos % RANGE_BLOCK) as usize);
        let block = self.block(index)?;
        let n = buf.len().min(block.len().saturating_sub(offset));
        buf[..n].copy_from_slice(&block[offset..offset + n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl std::io::Seek for RangeReader {
    fn seek(&mut self, to: std::io::SeekFrom) -> std::io::Result<u64> {
        use std::io::SeekFrom;
        let pos = match to {
            SeekFrom::Start(p) => p as i64,
            SeekFrom::End(d) => self.len as i64 + d,
            SeekFrom::Current(d) => self.pos as i64 + d,
        };
        if pos < 0 {
            return Err(std::io::Error::other("seek before the start of the package"));
        }
        self.pos = pos as u64;
        Ok(self.pos)
    }
}

/// Either the package on the server, read in ranges, or (if the server won't do
/// ranges) the whole package downloaded to disk.
trait ReadSeek: Read + std::io::Seek {}
impl<T: Read + std::io::Seek> ReadSeek for T {}

fn open_package(
    url: &str,
    stage: &Path,
    fetched: std::rc::Rc<std::cell::Cell<(u64, u32)>>,
    log: &dyn Fn(String),
) -> Result<Box<dyn ReadSeek>, String> {
    // Redirects are followed by hand: GitHub sends release downloads on to its
    // storage host, and the Range header has to reach that final address.
    let mut url = url.to_string();
    let no_redirects = agent_with_redirects(0);
    for _ in 0..8 {
        let mut resp = no_redirects
            .get(&url)
            .header("Range", "bytes=0-0")
            .call()
            .map_err(|e| format!("Couldn't reach the release ({e})"))?;
        match resp.status().as_u16() {
            301 | 302 | 303 | 307 | 308 => {
                url = resp
                    .headers()
                    .get("location")
                    .and_then(|v| v.to_str().ok())
                    .ok_or("The release download redirected nowhere.")?
                    .to_string();
            }
            206 => {
                let len = resp
                    .headers()
                    .get("content-range")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.rsplit('/').next()?.parse::<u64>().ok())
                    .ok_or("The server didn't say how big the package is.")?;
                log(format!("Package is {}; fetching only the parts needed", mb(len)));
                return Ok(Box::new(RangeReader { url, len, pos: 0, blocks: Default::default(), fetched }));
            }
            200 => {
                // No partial downloads here: take the whole package, then still
                // replace only the damaged files.
                log("The server won't send parts of the package; downloading all of it".into());
                let path = stage.join("package.zip");
                let mut file = File::create(&path).map_err(|e| write_err(&path, e))?;
                std::io::copy(&mut resp.body_mut().as_reader(), &mut file)
                    .map_err(|e| format!("Download interrupted ({e})"))?;
                drop(file);
                let file = File::open(&path).map_err(|e| write_err(&path, e))?;
                return Ok(Box::new(file));
            }
            404 => return Err("That version's release package is no longer on GitHub. Reinstall instead.".into()),
            s => return Err(format!("The release download answered HTTP {s}.")),
        }
    }
    Err("The release download redirected too many times.".into())
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

/// Pulls the `sha256:<hex>` digest for `package` from GitHub's HTML asset list
/// fragment when the REST API is rate-limited.
fn website_asset_digest(repo: &str, tag: &str, package: &str) -> Option<String> {
    let url = format!("https://github.com/{repo}/releases/expanded_assets/{tag}");
    let mut resp = agent().get(&url).call().ok()?;
    if resp.status() != 200 {
        return None;
    }
    let html = resp.body_mut().read_to_string().ok()?;
    let marker = format!("digest for {package}\"");
    let after = &html[html.find(&marker)? + marker.len()..];
    let val = &after[after.find("value=\"sha256:")? + 7..];
    let digest = val.split('"').next()?;
    (digest.len() == 71 && digest.starts_with("sha256:")).then(|| digest.to_string())
}

/// Portable user-data folders (`data\` and `<App>CraftData\`), which updates,
/// verifies and leftover sweeps never touch.
fn is_data_dir(lower: &str) -> bool {
    lower == "data" || lower == "portable-data" || lower.ends_with("craftdata")
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
    (!is_data_dir(&first) && first != STAGE_DIR).then_some(out)
}

fn sha256_file(path: &Path) -> std::io::Result<(u64, String)> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let size = std::io::copy(&mut file, &mut hasher)?;
    Ok((size, format!("{:x}", hasher.finalize())))
}

/// When every file in a zip sits inside one top-level folder (as third-party
/// portable archives often do: `app-1.0.0-windows-x64/app.exe`), returns that
/// folder name so unpacking strips it instead of nesting another folder.
fn zip_root_prefix<R: Read + std::io::Seek>(archive: &zip::ZipArchive<R>) -> Option<String> {
    let mut root: Option<String> = None;
    for name in archive.file_names() {
        let name = name.ok()?;
        if name.ends_with(['/', '\\']) {
            continue;
        }
        let mut parts = name.split(['/', '\\']).filter(|p| !p.is_empty());
        let first = parts.next()?;
        // A file at the archive root means there is no single wrapper folder.
        parts.next()?;
        match &root {
            None => root = Some(first.to_string()),
            Some(r) if r == first => {}
            Some(_) => return None,
        }
    }
    root
}

fn strip_root<'a>(name: &'a str, root: Option<&str>) -> &'a str {
    match root {
        Some(r) => name
            .strip_prefix(r)
            .map(|rest| rest.trim_start_matches(['/', '\\']))
            .unwrap_or(name),
        None => name,
    }
}

fn extract(zip_path: &Path, dest: &Path) -> Result<(), String> {
    fn damaged(e: impl std::fmt::Display) -> String {
        format!("The update package is damaged ({e})")
    }
    let file = File::open(zip_path).map_err(damaged)?;
    let mut archive = zip::ZipArchive::new(file).map_err(damaged)?;
    let root = zip_root_prefix(&archive);
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(damaged)?;
        if entry.is_dir() {
            continue;
        }
        #[cfg(unix)]
        let unix_mode = entry.unix_mode();
        let raw_name = entry.name().map_err(damaged)?.into_owned();
        let name = strip_root(&raw_name, root.as_deref());
        let rel = safe_relative(name)
            .ok_or_else(|| format!("The update package contains an unsafe path: {raw_name}"))?;
        let out = dest.join(&rel);
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).map_err(|e| write_err(parent, e))?;
        }
        let mut f = File::create(&out).map_err(|e| write_err(&out, e))?;
        std::io::copy(&mut entry, &mut f).map_err(damaged)?;
        drop(f);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let rel_str = rel.to_string_lossy();
            let is_exec = unix_mode.is_some_and(|m| m & 0o111 != 0) || rel_str.contains(".app/Contents/MacOS/");
            if is_exec {
                let _ = fs::set_permissions(&out, fs::Permissions::from_mode(0o755));
            }
        }
    }
    Ok(())
}

/// Mounts a macOS `.dmg` read-only without showing it in Finder, copies the `.app`
/// bundle (and any regular files) into `dest`, writes `portable.txt`, and unmounts.
#[cfg(target_os = "macos")]
fn extract_dmg(dmg_path: &Path, dest: &Path) -> Result<(), String> {
    let mnt = dmg_path.with_extension("mnt");
    let _ = fs::remove_dir_all(&mnt);
    fs::create_dir_all(&mnt).map_err(|e| write_err(&mnt, e))?;
    let status = std::process::Command::new("hdiutil")
        .args(["attach", "-nobrowse", "-readonly", "-noverify", "-noautoopen", "-mountpoint"])
        .arg(&mnt)
        .arg(dmg_path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| format!("Couldn't mount the disk image ({e})"))?;
    if !status.success() {
        let _ = fs::remove_dir_all(&mnt);
        return Err("The downloaded disk image couldn't be mounted.".into());
    }

    let copy_res = copy_dmg_contents(&mnt, dest);
    let _ = std::process::Command::new("hdiutil")
        .args(["detach"])
        .arg(&mnt)
        .arg("-force")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    let _ = fs::remove_dir_all(&mnt);
    copy_res?;

    // Drop portable.txt next to the .app bundle so Tauri/storytold apps know they
    // can operate in portable mode when supported.
    let _ = fs::write(dest.join("portable.txt"), b"portable mode\n");
    Ok(())
}

#[cfg(target_os = "macos")]
fn copy_dmg_contents(mnt: &Path, dest: &Path) -> Result<(), String> {
    fs::create_dir_all(dest).map_err(|e| write_err(dest, e))?;
    let entries = fs::read_dir(mnt).map_err(|e| format!("Couldn't read the mounted disk image ({e})"))?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        // Skip hidden metadata (.DS_Store, .VolumeIcon.icns, .Trashes, .fseventsd) and the Applications symlink.
        if name_str.starts_with('.') || name_str.eq_ignore_ascii_case("Applications") {
            continue;
        }
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        copy_tree(&entry.path(), &dest.join(&name))?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn copy_tree(src: &Path, dst: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let meta = fs::symlink_metadata(src).map_err(|e| format!("Couldn't read {} ({e})", src.display()))?;
    if meta.file_type().is_symlink() {
        // Resolve symlinks inside .app bundles to regular files/dirs so verify_files can hash every file.
        let target = fs::metadata(src).map_err(|e| format!("Couldn't resolve symlink {} ({e})", src.display()))?;
        if target.is_dir() {
            fs::create_dir_all(dst).map_err(|e| write_err(dst, e))?;
            for entry in fs::read_dir(src).map_err(|e| format!("Couldn't read {} ({e})", src.display()))?.flatten() {
                copy_tree(&entry.path(), &dst.join(entry.file_name()))?;
            }
        } else if target.is_file() {
            if let Some(p) = dst.parent() {
                fs::create_dir_all(p).map_err(|e| write_err(p, e))?;
            }
            fs::copy(src, dst).map_err(|e| write_err(dst, e))?;
        }
        return Ok(());
    }
    if meta.is_dir() {
        fs::create_dir_all(dst).map_err(|e| write_err(dst, e))?;
        for entry in fs::read_dir(src).map_err(|e| format!("Couldn't read {} ({e})", src.display()))?.flatten() {
            copy_tree(&entry.path(), &dst.join(entry.file_name()))?;
        }
    } else if meta.is_file() {
        if let Some(p) = dst.parent() {
            fs::create_dir_all(p).map_err(|e| write_err(p, e))?;
        }
        fs::copy(src, dst).map_err(|e| write_err(dst, e))?;
        let mode = meta.permissions().mode();
        if mode & 0o111 != 0 || dst.to_string_lossy().contains(".app/Contents/MacOS/") {
            let _ = fs::set_permissions(dst, fs::Permissions::from_mode(0o755));
        }
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn extract_dmg(_dmg_path: &Path, _dest: &Path) -> Result<(), String> {
    Err("macOS disk images (.dmg) can only be unpacked on macOS.".into())
}

/// Finds the main executable inside a `.app` bundle in `dir` if the expected
/// `exe` path doesn't match the bundle's exact binary capitalization.
///
/// Always reads the `.app/Contents/MacOS` directory entries rather than relying
/// on `dir.join(exe).is_file()`, because macOS's default APFS volume is
/// case-insensitive (`is_file()` returns true for `.../MacOS/photocraft` even
/// when the on-disk entry returned by `read_dir` is `.../MacOS/PhotoCraft`).
fn resolve_bundle_exe(dir: &Path, exe: &str) -> String {
    let Ok(entries) = fs::read_dir(dir) else { return exe.to_string() };
    let mut fallback = None;
    for e in entries.flatten() {
        let path = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if path.is_dir() && name.ends_with(".app") {
            let stem = name.trim_end_matches(".app");
            let macos_dir = path.join("Contents").join("MacOS");
            if let Ok(bins) = fs::read_dir(&macos_dir) {
                for bin in bins.flatten() {
                    if bin.path().is_file() {
                        let bin_name = bin.file_name().to_string_lossy().into_owned();
                        let candidate = format!("{name}/Contents/MacOS/{bin_name}");
                        if candidate.eq_ignore_ascii_case(exe) || bin_name.eq_ignore_ascii_case(stem) {
                            return candidate;
                        }
                        if fallback.is_none() {
                            fallback = Some(candidate);
                        }
                    }
                }
            }
        }
    }
    fallback.unwrap_or_else(|| exe.to_string())
}

/// Writes `launcher-manifest.json` for a third-party package after it has been
/// checked against its release checksum and unpacked, so verify, repair and
/// updates work identically to first-party packages.
fn write_manifest(files_dir: &Path, version: &str, exe: &str) -> Result<(), String> {
    let mut resolved_exe = resolve_bundle_exe(files_dir, exe);
    let mut files = Vec::new();
    let mut pending = vec![files_dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let entries = fs::read_dir(&dir).map_err(|e| write_err(&dir, e))?;
        for e in entries {
            let e = e.map_err(|err| write_err(&dir, err))?;
            let path = e.path();
            let rel = path
                .strip_prefix(files_dir)
                .map_err(|_| "Unexpected path while building the manifest.".to_string())?;
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            if safe_relative(&rel_str).is_none() {
                continue;
            }
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            if rel_str == MANIFEST || rel_str.ends_with(OLD_SUFFIX) {
                continue;
            }
            let (size, sha256) = sha256_file(&path).map_err(|err| write_err(&path, err))?;
            files.push(ManifestFile { path: rel_str, size, sha256 });
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    if !files.iter().any(|f| f.path == resolved_exe) {
        if let Some(found) = files.iter().find(|f| f.path.eq_ignore_ascii_case(&resolved_exe)) {
            resolved_exe = found.path.clone();
        }
    }
    let manifest = Manifest { version: version.to_string(), exe: resolved_exe, files };
    let json = serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;
    let out = files_dir.join(MANIFEST);
    fs::write(&out, json).map_err(|e| write_err(&out, e))
}

/// Adopts a third-party folder that was unpacked by hand into `dir` without a
/// `launcher-manifest.json`. If the zip's wrapper folder (e.g.
/// `photocraft-0.8.2-windows-x64-portable\`) was kept, flattens it first.
fn adopt_unpacked(dir: &Path, exe: &str) -> Option<(String, String)> {
    let mut folder_ver: Option<String> = None;
    let resolved = resolve_bundle_exe(dir, exe);
    if !dir.join(&resolved).is_file() {
        let mut subdirs = Vec::new();
        for e in fs::read_dir(dir).ok()?.flatten() {
            let path = e.path();
            if path.is_dir() {
                let name = e.file_name().to_string_lossy().to_ascii_lowercase();
                if !is_data_dir(&name) && name != STAGE_DIR && !name.ends_with(".app") {
                    subdirs.push(path);
                }
            }
        }
        if subdirs.len() == 1 {
            let sub_exe = resolve_bundle_exe(&subdirs[0], exe);
            if subdirs[0].join(&sub_exe).is_file() {
                let wrapper = &subdirs[0];
                folder_ver = wrapper
                    .file_name()
                    .and_then(|n| n.to_str())
                    .and_then(version_in_name);
                for e in fs::read_dir(wrapper).ok()?.flatten() {
                    let dst = dir.join(e.file_name());
                    let _ = fs::rename(e.path(), dst);
                }
                let _ = fs::remove_dir(wrapper);
            }
        }
    }
    let resolved_exe = resolve_bundle_exe(dir, exe);
    let exe_path = dir.join(&resolved_exe);
    if !exe_path.is_file() {
        return None;
    }
    let version = pe_version(&exe_path)
        .or_else(|| plist_version(&exe_path))
        .or(folder_ver)?;
    write_manifest(dir, &version, &resolved_exe).ok()?;
    Some((version, resolved_exe))
}

fn version_in_name(name: &str) -> Option<String> {
    name.split(['-', '_'])
        .find_map(parse_version)
        .map(|v| v.to_string())
}

/// Reads `CFBundleShortVersionString` from `<App>.app/Contents/Info.plist` when
/// `exe_path` is `<App>.app/Contents/MacOS/<bin>`.
fn plist_version(exe_path: &Path) -> Option<String> {
    let contents = exe_path.parent()?.parent()?;
    let plist = fs::read_to_string(contents.join("Info.plist")).ok()?;
    let idx = plist.find("CFBundleShortVersionString")?;
    let after = &plist[idx..];
    let start = after.find("<string>")? + 8;
    let end = after[start..].find("</string>")?;
    parse_version(&after[start..start + end]).map(|v| v.to_string())
}

/// Reads the `major.minor.patch` file version from a Windows PE executable's
/// `.rsrc` section (`VS_FIXEDFILEINFO`).
fn pe_version(path: &Path) -> Option<String> {
    use std::io::Seek;
    let mut file = File::open(path).ok()?;
    let mut head = [0u8; 4096];
    let n = file.read(&mut head).ok()?;
    let buf = &head[..n];
    if buf.get(0..2) != Some(b"MZ") {
        return None;
    }
    let pe = u32::from_le_bytes(buf.get(0x3c..0x40)?.try_into().ok()?) as usize;
    if buf.get(pe..pe + 4) != Some(b"PE\0\0") {
        return None;
    }
    let sections = u16::from_le_bytes(buf.get(pe + 6..pe + 8)?.try_into().ok()?) as usize;
    let opt_size = u16::from_le_bytes(buf.get(pe + 20..pe + 22)?.try_into().ok()?) as usize;
    let table = pe + 24 + opt_size;
    let mut rsrc_span = None;
    for i in 0..sections {
        let s = table + i * 40;
        let name = buf.get(s..s + 8)?;
        if name.starts_with(b".rsrc") {
            let size = u32::from_le_bytes(buf.get(s + 16..s + 20)?.try_into().ok()?) as usize;
            let ptr = u32::from_le_bytes(buf.get(s + 20..s + 24)?.try_into().ok()?) as u64;
            rsrc_span = Some((ptr, size.min(8 * 1024 * 1024)));
            break;
        }
    }
    let (ptr, size) = rsrc_span?;
    file.seek(std::io::SeekFrom::Start(ptr)).ok()?;
    let mut rsrc = vec![0u8; size];
    file.read_exact(&mut rsrc).ok()?;

    // VS_FIXEDFILEINFO magic (0xFEEF04BD) followed by struc version 1.0 (0x00010000).
    const SIG: [u8; 8] = [0xbd, 0x04, 0xef, 0xfe, 0x00, 0x00, 0x01, 0x00];
    let pos = (0..rsrc.len().saturating_sub(24))
        .step_by(4)
        .find(|&i| rsrc[i..i + 8] == SIG)?;
    let ms = u32::from_le_bytes(rsrc[pos + 8..pos + 12].try_into().ok()?);
    let ls = u32::from_le_bytes(rsrc[pos + 12..pos + 16].try_into().ok()?);
    Some(format!("{}.{}.{}", ms >> 16, ms & 0xffff, ls >> 16))
}

/// Every file the manifest lists must be present with exactly its size and SHA-256.
fn verify_files(files: &Path) -> Result<Manifest, String> {
    let raw = fs::read(files.join(MANIFEST)).map_err(|_| "The update package has no manifest.".to_string())?;
    let manifest: Manifest =
        serde_json::from_slice(&raw).map_err(|e| format!("The update manifest is unreadable ({e})"))?;

    if !manifest.files.iter().any(|f| f.path.eq_ignore_ascii_case(&manifest.exe)) {
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

    let exe_path = files.join(&manifest.exe);
    let mut head = [0u8; 4];
    let n = File::open(&exe_path)
        .and_then(|mut f| f.read(&mut head))
        .map_err(|e| format!("Couldn't read the new program ({e})"))?;
    let is_pe = n >= 2 && &head[..2] == b"MZ";
    let is_macho = n == 4
        && matches!(
            u32::from_be_bytes(head),
            0xFEEDFACF | 0xCFFAEDFE | 0xFEEDFACE | 0xCEFAEDFE | 0xCAFEBABE | 0xBEBAFECA | 0xCAFEBABF | 0xBFBAFECA
        );
    if cfg!(windows) && !is_pe {
        return Err("The new program isn't a Windows program.".into());
    }
    if !cfg!(windows) && !is_macho && !is_pe {
        return Err("The new program isn't a valid macOS application binary.".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&exe_path, fs::Permissions::from_mode(0o755));
    }
    Ok(manifest)
}

fn write_err(path: &Path, e: std::io::Error) -> String {
    // EPERM inside a bundle is macOS App Management guarding an app from another
    // developer, not a read-only folder.
    if cfg!(target_os = "macos") && e.raw_os_error() == Some(1) {
        if let Some(bundle) = path.ancestors().find(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("app"))) {
            return format!(
                "macOS won't let the launcher change files inside {}. Reinstall it instead, \
                 or allow Launcher in System Settings › Privacy & Security › App Management.",
                bundle.file_name().unwrap_or_default().to_string_lossy()
            );
        }
    }
    format!(
        "Couldn't write {} ({e}). Is the launcher's folder read-only?",
        path.file_name().unwrap_or_default().to_string_lossy()
    )
}

/// The top-level `.app` bundle a package path lies inside, on macOS.
fn bundle_of(rel: &Path) -> Option<PathBuf> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let mut parts = rel.components();
    let first = parts.next()?.as_os_str();
    let is_app = Path::new(first).extension().is_some_and(|x| x.eq_ignore_ascii_case("app"));
    (is_app && parts.next().is_some()).then(|| PathBuf::from(first))
}

/// Moves `path` to the Trash, as Finder's Move to Trash does. For app bundles
/// macOS won't let the launcher delete the contents of.
#[cfg(target_os = "macos")]
pub fn trash(path: &Path) -> Result<(), String> {
    use objc2_foundation::{NSFileManager, NSString, NSURL};
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    NSFileManager::defaultManager()
        .trashItemAtURL_resultingItemURL_error(&url, None)
        .map_err(|e| e.localizedDescription().to_string())
}

#[cfg(not(target_os = "macos"))]
pub fn trash(_path: &Path) -> Result<(), String> {
    Err("There's no Trash to move it to here.".into())
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
    if !first.exists() || fs::remove_file(&first).is_ok() || fs::remove_dir_all(&first).is_ok() {
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
    #[cfg(target_os = "macos")]
    let exe_buf = {
        let target = Target::launcher();
        let rel = target
            .installed()
            .map(|i| i.exe)
            .unwrap_or_else(|| "Launcher.app/Contents/MacOS/Launcher".into());
        let candidate = target.dir().join(rel);
        if candidate.is_file() {
            if let Some(bundle) = candidate
                .ancestors()
                .find(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("app")))
            {
                let _ = std::process::Command::new("xattr").arg("-cr").arg(bundle).status();
            }
            candidate
        } else {
            exe.to_path_buf()
        }
    };
    #[cfg(not(target_os = "macos"))]
    let exe_buf = exe.to_path_buf();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&exe_buf, fs::Permissions::from_mode(0o755));
    }
    std::process::Command::new(&exe_buf)
        .arg("--updated")
        .current_dir(app_dir())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Couldn't start the new version ({e})"))
}

fn mb(bytes: u64) -> String {
    match bytes {
        b if b < 1024 => format!("{b} bytes"),
        b if b < 1_048_576 => format!("{:.0} KB", b as f64 / 1024.0),
        b => format!("{:.1} MB", b as f64 / 1_048_576.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Damages an installed app, then verifies and repairs it from its real GitHub
    /// release. Network-bound, so it only runs when asked for:
    ///
    ///   set LAUNCHER_TEST_APP_DIR=<a folder holding an installed Lighthouse>
    ///   cargo test repair_from_github -- --ignored --nocapture
    #[test]
    #[ignore]
    fn repair_from_github() {
        let dir = PathBuf::from(std::env::var("LAUNCHER_TEST_APP_DIR").expect("LAUNCHER_TEST_APP_DIR"));
        let target = Target {
            repo: "JavacrafterDevelopment/Lighthouse".into(),
            package: "Lighthouse-windows-x64.zip".into(),
            dir: dir.clone(),
            self_exe: None,
            update_json: true,
            default_exe: None,
        };
        let log = |line: String| println!("  {line}");

        // One file with its contents changed, one deleted outright.
        fs::write(dir.join("wwwroot/app.css"), b"not the real stylesheet").unwrap();
        fs::remove_file(dir.join("Lighthouse.dll")).unwrap();

        println!("verify:");
        let damaged = target.verify(|_, _| {}, &log).unwrap();
        assert_eq!(damaged.len(), 2, "{damaged:?}");

        println!("repair:");
        target.repair(&damaged, |_, _| {}, &log).unwrap();

        println!("verify again:");
        assert!(target.verify(|_, _| {}, &log).unwrap().is_empty());
    }

    #[test]
    fn third_party_unpack_and_verify() {
        let tmp = std::env::temp_dir().join(format!("launcher-tp-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let zip_path = tmp.join("pkg.zip");
        {
            let file = File::create(&zip_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let opts = zip::write::SimpleFileOptions::default();
            zip.start_file("photocraft-0.9.0-windows-x64-portable/photocraft.exe", opts).unwrap();
            zip.write_all(b"MZfake-exe-bytes").unwrap();
            zip.start_file("photocraft-0.9.0-windows-x64-portable/portable.txt", opts).unwrap();
            zip.write_all(b"portable mode").unwrap();
            zip.finish().unwrap();
        }

        let files = tmp.join("files");
        extract(&zip_path, &files).unwrap();
        write_manifest(&files, "0.9.0", "photocraft.exe").unwrap();
        let m = verify_files(&files).unwrap();
        assert_eq!(m.version, "0.9.0");
        assert_eq!(m.exe, "photocraft.exe");
        assert_eq!(m.files.len(), 2);

        let install_dir = tmp.join("installed");
        let target = Target {
            repo: "storytold/photocraft".into(),
            package: "photocraft-{version}-windows-x64-portable.zip".into(),
            dir: install_dir.clone(),
            self_exe: None,
            update_json: false,
            default_exe: Some("photocraft.exe".into()),
        };
        let stage_files = target.stage_dir().join("files");
        fs::create_dir_all(target.stage_dir()).unwrap();
        fs::rename(&files, &stage_files).unwrap();
        fs::write(target.ready_marker(), "0.9.0").unwrap();
        target.apply(&|_| {}).unwrap();

        assert!(target.verify(|_, _| {}, &|_| {}).unwrap().is_empty());
        fs::write(install_dir.join("portable.txt"), b"tampered").unwrap();
        let bad = target.verify(|_, _| {}, &|_| {}).unwrap();
        assert_eq!(bad, vec!["portable.txt".to_string()]);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn adopt_unpacked_old_version() {
        let tmp = std::env::temp_dir().join(format!("launcher-adopt-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let wrapper = tmp.join("photocraft-0.8.2-windows-x64-portable");
        fs::create_dir_all(&wrapper).unwrap();
        fs::write(wrapper.join("photocraft.exe"), b"MZfake").unwrap();
        fs::write(wrapper.join("portable.txt"), b"portable").unwrap();

        let target = Target {
            repo: "storytold/photocraft".into(),
            package: "photocraft-{version}-windows-x64-portable.zip".into(),
            dir: tmp.clone(),
            self_exe: None,
            update_json: false,
            default_exe: Some("photocraft.exe".into()),
        };

        let inst = target.installed().expect("adopted unpacked folder");
        assert_eq!(inst.version, "0.8.2");
        assert_eq!(inst.exe, "photocraft.exe");
        assert!(tmp.join(MANIFEST).is_file());
        assert!(tmp.join("photocraft.exe").is_file());

        // Also check that pe_version reads a real Windows system binary's .rsrc.
        let cmd = Path::new(r"C:\Windows\System32\cmd.exe");
        if cmd.is_file() {
            assert!(pe_version(cmd).is_some());
        }

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    #[ignore]
    fn photocraft_live_check_and_repair() {
        let tmp = std::env::temp_dir().join(format!("launcher-photocraft-live-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let target = Target {
            repo: "storytold/photocraft".into(),
            package: "photocraft-{version}-windows-x64-portable.zip".into(),
            dir: tmp.clone(),
            self_exe: None,
            update_json: false,
            default_exe: Some("photocraft.exe".into()),
        };
        let log = |line: String| println!("  {line}");

        let rel = target.latest(false).unwrap().expect("photocraft release found");
        println!("latest photocraft: v{} ({})", rel.version, rel.asset_url);
        assert!(rel.digest.as_deref().unwrap_or_default().starts_with("sha256:"));

        // Seed a minimal manifest for v0.9.0 with portable.txt so we can exercise
        // HTTP Range repair against the live 80 MB GitHub asset in ~512 KB.
        let m = Manifest {
            version: "0.9.0".into(),
            exe: "photocraft.exe".into(),
            files: vec![ManifestFile {
                path: "portable.txt".into(),
                size: 527,
                sha256: "765cf8f8ffc8ee57f83f31e5eb2d453183e2e96ba97417ab9238717ccb27183b".into(),
            }],
        };
        fs::write(tmp.join(MANIFEST), serde_json::to_vec_pretty(&m).unwrap()).unwrap();
        let bad = target.verify(|_, _| {}, &log).unwrap();
        assert_eq!(bad, vec!["portable.txt".to_string()]);

        target.repair(&bad, |_, _| {}, &log).unwrap();
        assert!(target.verify(|_, _| {}, &log).unwrap().is_empty());

        let _ = fs::remove_dir_all(&tmp);
    }

    /// macOS App Management blocks changes inside a bundle that has run; a locked
    /// file stands in for that. The update has to swap the bundle whole.
    #[test]
    #[cfg(target_os = "macos")]
    fn apply_replaces_locked_bundle_whole() {
        let tmp = std::env::temp_dir().join(format!("launcher-bundle-swap-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let target = Target {
            repo: "JavacrafterDevelopment/PhotoCraft".into(),
            package: "PhotoCraft-macos.zip".into(),
            dir: tmp.clone(),
            self_exe: None,
            update_json: true,
            default_exe: None,
        };
        let exe = "PhotoCraft.app/Contents/MacOS/PhotoCraft";
        let macho: &[u8] = &[0xCA, 0xFE, 0xBA, 0xBE, 0, 0, 0, 1];
        let lay_out = |root: &Path, version: &str, extra: &str| {
            fs::create_dir_all(root.join("PhotoCraft.app/Contents/MacOS")).unwrap();
            fs::create_dir_all(root.join("PhotoCraft.app/Contents/_CodeSignature")).unwrap();
            fs::write(root.join(exe), [macho, version.as_bytes()].concat()).unwrap();
            fs::write(root.join("PhotoCraft.app/Contents/_CodeSignature/CodeResources"), version).unwrap();
            fs::write(root.join(extra), version).unwrap();
            fs::write(root.join("portable.txt"), b"portable mode\n").unwrap();
            write_manifest(root, version, exe).unwrap();
        };

        lay_out(&tmp, "1.0.0", "PhotoCraft.app/Contents/Old.txt");
        let locked = tmp.join("PhotoCraft.app/Contents/_CodeSignature/CodeResources");
        assert!(std::process::Command::new("chflags").arg("uchg").arg(&locked).status().unwrap().success());

        lay_out(&target.stage_dir().join("files"), "2.0.0", "PhotoCraft.app/Contents/New.txt");
        fs::write(target.ready_marker(), "2.0.0").unwrap();

        let result = target.apply(&|line| println!("  {line}"));
        let _ = std::process::Command::new("chflags").arg("-R").arg("nouchg").arg(&tmp).status();
        result.unwrap();

        let bundle = tmp.join("PhotoCraft.app/Contents");
        assert_eq!(fs::read_to_string(bundle.join("_CodeSignature/CodeResources")).unwrap(), "2.0.0");
        assert!(bundle.join("New.txt").is_file());
        assert!(!bundle.join("Old.txt").exists(), "the old bundle's files went with it");
        assert!(tmp.join(format!("PhotoCraft.app{OLD_SUFFIX}/Contents/Old.txt")).is_file());
        assert_eq!(target.installed().unwrap().version, "2.0.0");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn macos_bundle_case_insensitive_manifest_and_verify() {
        let tmp = std::env::temp_dir().join(format!("launcher-macos-bundle-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let macos_dir = tmp.join("PhotoCraft.app/Contents/MacOS");
        fs::create_dir_all(&macos_dir).unwrap();
        // Universal Mach-O fat header (0xCAFEBABE) on macOS/Unix, or MZ on Windows so verify_files accepts it.
        let magic: &[u8] = if cfg!(windows) { b"MZfake-exe" } else { &[0xCA, 0xFE, 0xBA, 0xBE, 0, 0, 0, 1] };
        fs::write(macos_dir.join("PhotoCraft"), magic).unwrap();
        fs::write(
            tmp.join("PhotoCraft.app/Contents/Info.plist"),
            b"<plist><dict><key>CFBundleShortVersionString</key><string>0.11.1</string></dict></plist>",
        )
        .unwrap();

        // Even when passed lowercase `photocraft`, resolve_bundle_exe + write_manifest must record
        // the exact on-disk casing `PhotoCraft.app/Contents/MacOS/PhotoCraft`.
        write_manifest(&tmp, "0.11.1", "PhotoCraft.app/Contents/MacOS/photocraft").unwrap();
        let m = verify_files(&tmp).unwrap();
        assert_eq!(m.exe, "PhotoCraft.app/Contents/MacOS/PhotoCraft");
        assert!(m.files.iter().any(|f| f.path == m.exe));

        let _ = fs::remove_dir_all(&tmp);
    }
}
