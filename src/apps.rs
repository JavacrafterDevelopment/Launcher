//! The library: the apps the launcher can install, each into its own folder under
//! `portableApplications\`.
//!
//! Updates for apps are found automatically but never installed automatically:
//! every check runs on launch (and on demand), and the card shows an Update button.
//! Installing and updating use the same verified download and swap as the
//! launcher's own updates (see [`crate::updater`]), and an app's `data\` folder is
//! never touched, so its settings survive.

use serde::Serialize;
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tao::event_loop::EventLoopProxy;

use crate::procs;
use crate::updater::{Release, Target};
use crate::UserEvent;

#[cfg(target_os = "macos")]
pub const CURRENT_OS: &str = "MacOS";
#[cfg(not(target_os = "macos"))]
pub const CURRENT_OS: &str = "Windows";

pub struct AppInfo {
    pub id: &'static str,
    /// What the card and the log call it.
    pub name: &'static str,
    /// Its folder under portableApplications, when that isn't `name`: an app
    /// renamed after people installed it keeps the folder it was installed
    /// in, so updating never moves it or its data. Its release packages are
    /// named after the folder too (`<folder>-windows-x64.zip`), and on macOS
    /// its data folder (`<folder>Data`).
    pub folder: Option<&'static str>,
    pub repo: &'static str,
    pub tagline: &'static str,
    pub icon: Option<&'static [u8]>,
    pub platforms: &'static [&'static str],
    pub stop: Stop,
    pub source: Source,
    pub category: Category,
    /// A .bat in the app's folder that starts it, offered as "Launch with .bat"
    /// beside Launch once an installed version has it. Windows only.
    pub bat: Option<&'static str>,
}

impl AppInfo {
    /// The folder it's installed in, under portableApplications.
    pub fn folder(&self) -> &'static str {
        self.folder.unwrap_or(self.name)
    }
}

#[derive(Clone, Copy)]
pub enum Source {
    /// Built and packaged with `tools/package.ps1` or `tools/package-macos.sh`.
    Own,
}

/// Which tab an app is listed under.
#[derive(Clone, Copy)]
pub enum Category {
    Apps,
    /// Games, installed and launched like any app.
    Games,
}

impl Category {
    fn id(self) -> &'static str {
        match self {
            Category::Apps => "apps",
            Category::Games => "games",
        }
    }

    /// Anything that isn't `"games"` is listed under Apps, including the
    /// `"temporary"` section older versions had.
    pub fn from_id(s: &str) -> Self {
        match s {
            "games" => Category::Games,
            _ => Category::Apps,
        }
    }
}

/// How the Stop button asks an app to quit.
#[derive(Clone, Copy)]
pub enum Stop {
    /// Run the app's exe again with this argument; it tells the running copy to
    /// exit properly. For apps that run as administrator, which the launcher can't
    /// close or end itself.
    QuitArg { arg: &'static str, since: &'static str },
    /// Ask its windows to close, as clicking ✕ would.
    CloseWindows,
}

pub const CATALOG: &[AppInfo] = &[
    AppInfo {
        id: "lighthouse",
        name: "Lighthouse",
        folder: None,
        repo: "JavacrafterDevelopment/Lighthouse",
        tagline: "Instant search across every file and folder on every drive, with Explorer's own icons.",
        icon: Some(include_bytes!("../assets/lighthouse.png")),
        platforms: &["Windows"],
        // Runs as administrator, and its close button only hides it to the tray.
        stop: Stop::QuitArg { arg: "--quit", since: "0.0.2" },
        source: Source::Own,
        category: Category::Apps,
        bat: None,
    },
    AppInfo {
        id: "screenshotapp",
        name: "ScreenshotApp",
        folder: None,
        repo: "JavacrafterDevelopment/ScreenshotApp",
        tagline: "Timed and hotkey screenshots, numbered and saved wherever you like.",
        icon: Some(include_bytes!("../assets/screenshotapp.png")),
        platforms: &["Windows"],
        stop: Stop::CloseWindows,
        source: Source::Own,
        category: Category::Apps,
        bat: None,
    },
    AppInfo {
        id: "rustwinutil",
        name: "RustWinUtil",
        folder: None,
        repo: "JavacrafterDevelopment/RustWinUtil",
        tagline: "Chris Titus Tech's WinUtil, rebuilt in Rust: install apps, apply tweaks, fix Windows.",
        icon: Some(include_bytes!("../assets/rustwinutil.png")),
        platforms: &["Windows"],
        // Runs as administrator, so the launcher can't close it itself.
        stop: Stop::QuitArg { arg: "--quit", since: "0.0.1" },
        source: Source::Own,
        category: Category::Apps,
        bat: None,
    },
    AppInfo {
        id: "kittyconnect",
        // Called KittyConnect until 0.13.0: it keeps that folder (and the
        // repo, its packages' names and its data folders), so it updates in
        // place and nobody loses their hubs or settings.
        name: "Portside",
        folder: Some("KittyConnect"),
        repo: "JavacrafterDevelopment/KittyConnect",
        tagline: "Hubs, voice rooms, file sharing and game hosting with friends over encrypted TailCat tunnels. No account, no router setup.",
        icon: Some(include_bytes!("../assets/portside.png")),
        // macOS from KittyConnect 0.8.0. On a Mac its data goes in
        // KittyConnectData beside the app, which `shell_open` points it at.
        platforms: &["Windows", "MacOS"],
        // Closing its window ends every TailCat tunnel it opened.
        stop: Stop::CloseWindows,
        source: Source::Own,
        category: Category::Apps,
        // Ships from KittyConnect 0.7.1.
        bat: Some("Start KittyConnect.bat"),
    },
    AppInfo {
        id: "inkcognito",
        name: "Ink-cognito",
        folder: None,
        repo: "JavacrafterDevelopment/Ink-cognito",
        tagline: "A 3D multiplayer game: host a world, share its TailCat address, and walk the plate with friends.",
        icon: Some(include_bytes!("../assets/inkcognito.png")),
        // Its release workflow builds Windows and macOS packages.
        platforms: &["Windows", "MacOS"],
        // Closing its window ends the game and its TailCat session.
        stop: Stop::CloseWindows,
        source: Source::Own,
        category: Category::Games,
        bat: None,
    },
];

/// How long to wait for an app to quit when asked before ending it outright.
const STOP_GRACE: Duration = Duration::from_secs(10);
/// What an app's quit argument exits with when the app is in the middle of
/// something it shouldn't drop (RustWinUtil running a job) and is asking in its
/// own window whether to close once it's done. Ending it would cut that work off,
/// so the launcher leaves it running and notices when it does quit.
const QUIT_DEFERRED: i32 = 2;
/// How long a just-launched app counts as "starting" before it's seen running.
/// Generous, because Lighthouse may be sitting at an elevation prompt.
const START_GRACE: Duration = Duration::from_secs(45);

/// What an app is busy with, if anything.
#[derive(Clone, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum Task {
    Idle,
    Checking,
    Queued,
    Downloading { done: u64, total: u64 },
    Installing,
    Verifying { done: usize, total: usize },
    Stopping,
    Repairing { done: u64, total: u64 },
    Error { message: String },
}

pub enum AppEvent {
    Checked(Result<Option<Release>, String>),
    Progress(u64, u64),
    /// The download is verified; the files are being swapped in.
    Installing,
    Log(String),
    Finished(Result<(), String>),
    VerifyProgress(usize, usize),
    /// The installed files that are missing or don't match their SHA-256.
    Verified(Result<Vec<String>, String>),
    RepairProgress(u64, u64),
    Repaired(Result<(), String>),
    Stopped(Result<(), String>),
    /// Asked to quit, the app said it's busy and is asking in its own window instead.
    StopDeferred(String),
}

/// One line of an app's activity log, shown in the drawer under its card.
#[derive(Clone, Serialize)]
struct LogLine {
    /// Unix milliseconds; the page shows it as local time.
    t: u128,
    text: String,
    /// "info", "ok" or "error".
    kind: &'static str,
}

/// Enough to follow an install; older lines fall off the top.
const LOG_LINES: usize = 300;

#[derive(Clone)]
enum RuntimeSource {
    Own,
    /// Added from Settings by pasting a GitHub link: the release asset is picked
    /// automatically for this OS (see `updater::select_release_asset_for_os`).
    GitHubAuto {
        own_format: bool,
        private: bool,
    },
}

#[derive(Clone)]
struct RuntimeApp {
    id: String,
    name: String,
    folder: String,
    repo: String,
    tagline: String,
    publisher: String,
    platforms: Vec<String>,
    stop: Stop,
    source: RuntimeSource,
    category: Category,
    bat: Option<String>,
    custom: bool,
    private: bool,
}

impl RuntimeApp {
    fn from_builtin(info: &'static AppInfo) -> Self {
        let source = match info.source {
            Source::Own => RuntimeSource::Own,
        };
        Self {
            id: info.id.into(),
            name: info.name.into(),
            folder: info.folder().into(),
            repo: info.repo.into(),
            tagline: info.tagline.into(),
            publisher: publisher_of(info).into(),
            platforms: info.platforms.iter().map(|p| (*p).into()).collect(),
            stop: info.stop,
            source,
            category: info.category,
            bat: info.bat.map(String::from),
            custom: false,
            private: false,
        }
    }

    fn from_custom(c: &crate::settings::CustomApp) -> Self {
        let platforms = if c.platforms.is_empty() {
            vec!["Windows".into(), "MacOS".into()]
        } else {
            c.platforms.clone()
        };
        let publisher = if c.publisher.trim().is_empty() {
            c.repo.split('/').next().unwrap_or("GitHub").to_string()
        } else {
            c.publisher.clone()
        };
        Self {
            id: c.id.clone(),
            name: c.name.clone(),
            folder: c.folder.clone(),
            repo: c.repo.clone(),
            tagline: c.tagline.clone(),
            publisher,
            platforms,
            stop: Stop::CloseWindows,
            source: RuntimeSource::GitHubAuto { own_format: c.own_format, private: c.private },
            category: Category::from_id(&c.category),
            bat: None,
            custom: true,
            private: c.private,
        }
    }

    fn folder(&self) -> &str {
        &self.folder
    }

    fn supported_on_current_os(&self) -> bool {
        self.platforms.iter().any(|p| p.eq_ignore_ascii_case(CURRENT_OS))
    }
}

struct AppState {
    info: RuntimeApp,
    installed: Option<String>,
    latest: Option<Release>,
    /// True once a check has answered, so "no release yet" can be told apart
    /// from "not checked yet".
    checked: bool,
    task: Task,
    log: Vec<LogLine>,
    /// Files the last verify found missing or changed; Repair fetches these.
    damaged: Vec<String>,
    /// Whether its exe is running right now, however it was started.
    running: bool,
    /// Set when Launch is pressed, until the app is seen running.
    starting: Option<Instant>,
}

/// Embedded app icons encoded once for `window.BOOT.icons`, so snapshots sent on
/// every download progress tick stay small.
pub fn icons() -> Value {
    let mut map = serde_json::Map::new();
    for info in CATALOG {
        if let Some(bytes) = info.icon {
            map.insert(info.id.into(), Value::String(png_data_url(bytes)));
        }
    }
    Value::Object(map)
}

/// Brand colours `(accent, ink_light, ink_dark)` for the Modern theme: gives Javacrafter apps
/// distinct accents, and hashes custom apps onto a curated palette.
fn app_colors(id: &str) -> (&'static str, &'static str, &'static str) {
    match id {
        "lighthouse" => ("#d99326", "#9a630a", "#f2bc5a"),
        "screenshotapp" => ("#1fa5c9", "#0e7290", "#5fd0ee"),
        "rustwinutil" => ("#e0622d", "#a83e12", "#ff9668"),
        "kittyconnect" => ("#2d81ff", "#1659c4", "#74aaff"),
        "inkcognito" => ("#8b5cf6", "#6a3fd6", "#b69cff"),
        other => {
            const PALETTE: &[(&str, &str, &str)] = &[
                ("#2d81ff", "#1659c4", "#74aaff"),
                ("#8b5cf6", "#6a3fd6", "#b69cff"),
                ("#1fa5c9", "#0e7290", "#5fd0ee"),
                ("#1f9d55", "#12703a", "#4fd68a"),
                ("#e0622d", "#a83e12", "#ff9668"),
                ("#d99326", "#9a630a", "#f2bc5a"),
                ("#e0368f", "#b0206c", "#ff7ab8"),
                ("#3f5fe0", "#2a44b8", "#8ea2ff"),
            ];
            let hash = other.bytes().fold(0usize, |h, b| h.wrapping_mul(31).wrapping_add(b as usize));
            PALETTE[hash % PALETTE.len()]
        }
    }
}

fn publisher_of(info: &AppInfo) -> &'static str {
    match info.source {
        Source::Own => "Javacrafter",
    }
}

impl AppState {
    fn from_runtime(info: RuntimeApp, root: &std::path::Path) -> Self {
        let mut a = Self {
            info,
            installed: None,
            latest: None,
            checked: false,
            task: Task::Idle,
            log: Vec::new(),
            damaged: Vec::new(),
            running: false,
            starting: None,
        };
        a.refresh_installed(root);
        a
    }

    fn target_in(&self, root: &std::path::Path) -> Target {
        match &self.info.source {
            RuntimeSource::Own => Target::app_in(&self.info.repo, self.info.folder(), root),
            RuntimeSource::GitHubAuto { own_format, private } => {
                Target::custom_in(&self.info.repo, self.info.folder(), *own_format, *private, root)
            }
        }
    }

    /// Resolves where this app lives: prefers an already-installed copy in `root` or the default
    /// `portableApplications/` folder so updates stay in their current location; new installs go
    /// into `root`.
    fn target(&self, root: &std::path::Path) -> Target {
        let preferred = self.target_in(root);
        if preferred.installed().is_some() {
            return preferred;
        }
        let default_root = crate::settings::app_dir().join(crate::updater::APPS_DIR);
        if root != default_root {
            let legacy = self.target_in(&default_root);
            if legacy.installed().is_some() {
                return legacy;
            }
        }
        preferred
    }

    fn refresh_installed(&mut self, root: &std::path::Path) {
        self.installed = self.target(root).installed().map(|i| i.version);
    }

    /// The app's start .bat, if it has one and the installed version shipped it.
    fn bat_path(&self, root: &std::path::Path) -> Option<std::path::PathBuf> {
        let name = self.info.bat.as_deref().filter(|_| cfg!(windows) && self.installed.is_some())?;
        Some(self.target(root).dir().join(name)).filter(|p| p.is_file())
    }

    fn update_available(&self) -> bool {
        let latest = self.latest.as_ref().and_then(|r| r.semver());
        let installed = self
            .installed
            .as_deref()
            .and_then(crate::updater::parse_version_lenient);
        matches!((latest, installed), (Some(l), Some(i)) if l > i)
    }

    fn busy(&self) -> bool {
        matches!(
            self.task,
            Task::Checking
                | Task::Queued
                | Task::Downloading { .. }
                | Task::Installing
                | Task::Verifying { .. }
                | Task::Repairing { .. }
                | Task::Stopping
        )
    }

    fn downloading(&self) -> bool {
        matches!(self.task, Task::Downloading { .. } | Task::Installing)
    }

    fn log(&mut self, kind: &'static str, text: impl Into<String>) {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        self.log.push(LogLine { t, text: text.into(), kind });
        if self.log.len() > LOG_LINES {
            self.log.drain(..self.log.len() - LOG_LINES);
        }
    }
}

pub struct Library {
    apps: Vec<AppState>,
    proxy: EventLoopProxy<UserEvent>,
    install_root: std::path::PathBuf,
    max_downloads: u8,
    queue: Vec<String>,
}

impl Library {
    pub fn new(
        proxy: EventLoopProxy<UserEvent>,
        install_root: std::path::PathBuf,
        max_downloads: u8,
        custom_apps: &[crate::settings::CustomApp],
    ) -> Self {
        let mut apps: Vec<AppState> = CATALOG
            .iter()
            .map(|info| AppState::from_runtime(RuntimeApp::from_builtin(info), &install_root))
            .collect();
        for c in custom_apps {
            if !apps.iter().any(|a| a.info.id == c.id) {
                apps.push(AppState::from_runtime(RuntimeApp::from_custom(c), &install_root));
            }
        }
        Self { apps, proxy, install_root, max_downloads, queue: Vec::new() }
    }

    /// Synchronizes user-added custom apps with the live library, preserving state for
    /// existing entries and kicking off a release check for newly added ones.
    pub fn sync_custom_apps(&mut self, custom_apps: &[crate::settings::CustomApp], prereleases: bool) {
        let keep_ids: std::collections::HashSet<&str> = custom_apps.iter().map(|c| c.id.as_str()).collect();
        self.apps.retain(|a| !a.info.custom || keep_ids.contains(a.info.id.as_str()));
        self.queue.retain(|qid| self.apps.iter().any(|a| a.info.id == *qid));

        let mut added_ids = Vec::new();
        for c in custom_apps {
            if let Some(existing) = self.apps.iter_mut().find(|a| a.info.id == c.id) {
                existing.info = RuntimeApp::from_custom(c);
                existing.refresh_installed(&self.install_root);
            } else {
                let state = AppState::from_runtime(RuntimeApp::from_custom(c), &self.install_root);
                added_ids.push(state.info.id.clone());
                self.apps.push(state);
            }
        }
        for id in added_ids {
            self.check_one(&id, prereleases);
        }
    }

    pub fn has_repo(&self, repo: &str) -> bool {
        self.apps.iter().any(|a| a.info.repo.eq_ignore_ascii_case(repo))
    }

    pub fn existing_ids(&self) -> Vec<String> {
        self.apps.iter().map(|a| a.info.id.clone()).collect()
    }

    pub fn existing_folders(&self) -> Vec<String> {
        self.apps.iter().map(|a| a.info.folder.clone()).collect()
    }

    /// Checks on disk whether the app is currently installed (updating its cached state).
    pub fn is_installed(&mut self, id: &str) -> bool {
        let Some(a) = self.apps.iter_mut().find(|a| a.info.id == id) else {
            return false;
        };
        a.refresh_installed(&self.install_root);
        a.installed.is_some()
    }

    pub fn app_name(&self, id: &str) -> Option<String> {
        self.apps.iter().find(|a| a.info.id == id).map(|a| a.info.name.clone())
    }

    pub fn set_install_root(&mut self, root: std::path::PathBuf) {
        self.install_root = root;
        for a in &mut self.apps {
            a.refresh_installed(&self.install_root);
        }
    }

    pub fn set_max_downloads(&mut self, max_downloads: u8) {
        self.max_downloads = max_downloads;
        self.drain_queue();
    }

    fn active_downloads(&self) -> usize {
        self.apps.iter().filter(|a| a.downloading()).count()
    }

    /// Clears what earlier updates left behind in each installed app's folder.
    pub fn sweep(&self) {
        for a in self.apps.iter().filter(|a| a.installed.is_some()) {
            a.target(&self.install_root).sweep_leftovers();
        }
    }

    pub fn snapshot(&self) -> Value {
        let apps: Vec<Value> = self
            .apps
            .iter()
            .map(|a| {
                let (accent, ink_light, ink_dark) = app_colors(&a.info.id);
                json!({
                    "id": a.info.id,
                    "name": a.info.name,
                    "folder": a.info.folder(),
                    "tagline": a.info.tagline,
                    "repo": a.info.repo,
                    "publisher": a.info.publisher,
                    "accent": accent,
                    "inkLight": ink_light,
                    "inkDark": ink_dark,
                    "platforms": a.info.platforms,
                    "supported": a.info.supported_on_current_os(),
                    "thirdParty": a.info.custom,
                    "custom": a.info.custom,
                    "private": a.info.private,
                    "category": a.info.category.id(),
                    "installed": a.installed,
                    "latest": a.latest,
                    "checked": a.checked,
                    "updateAvailable": a.update_available(),
                    "task": a.task,
                    "log": a.log,
                    "damaged": a.damaged,
                    "running": a.running,
                    "starting": a.starting.is_some(),
                    "bat": a.bat_path(&self.install_root).is_some(),
                })
            })
            .collect();
        json!({ "type": "apps", "apps": apps })
    }

    fn get(&mut self, id: &str) -> Option<&mut AppState> {
        self.apps.iter_mut().find(|a| a.info.id == id)
    }

    /// Checks a single app for its latest release.
    pub fn check_one(&mut self, id: &str, prereleases: bool) {
        let root = self.install_root.clone();
        let proxy = self.proxy.clone();
        let Some(a) = self.get(id) else { return };
        if a.busy() {
            return;
        }
        a.refresh_installed(&root);
        if !a.info.supported_on_current_os() {
            a.checked = true;
            a.task = Task::Idle;
            return;
        }
        a.task = Task::Checking;
        let (app_id, target) = (a.info.id.clone(), a.target(&root));
        std::thread::spawn(move || {
            let result = target.latest(prereleases);
            let _ = proxy.send_event(UserEvent::App(app_id, AppEvent::Checked(result)));
        });
    }

    /// Re-checks private apps and any apps that previously failed (e.g. after signing in to GitHub).
    pub fn recheck_private_or_failed(&mut self, prereleases: bool) {
        let ids: Vec<String> = self
            .apps
            .iter()
            .filter(|a| !a.busy() && (a.info.private || matches!(a.task, Task::Error { .. })))
            .map(|a| a.info.id.clone())
            .collect();
        for id in ids {
            self.check_one(&id, prereleases);
        }
    }

    /// Looks for new releases of every app supported on the current OS. Finding one
    /// only shows the Update button; nothing is downloaded until it's pressed.
    pub fn check_all(&mut self, prereleases: bool) {
        let root = self.install_root.clone();
        for a in self.apps.iter_mut().filter(|a| !a.busy()) {
            a.refresh_installed(&root);
            if !a.info.supported_on_current_os() {
                a.checked = true;
                a.task = Task::Idle;
                continue;
            }
            a.task = Task::Checking;
            let (id, target, proxy) = (a.info.id.clone(), a.target(&root), self.proxy.clone());
            std::thread::spawn(move || {
                let result = target.latest(prereleases);
                let _ = proxy.send_event(UserEvent::App(id, AppEvent::Checked(result)));
            });
        }
    }

    /// Queues or starts installing/updating an app, respecting `max_downloads`.
    pub fn install(&mut self, id: &str) {
        let root = self.install_root.clone();
        let active = self.active_downloads();
        let limit_hit = self.max_downloads > 0 && active >= self.max_downloads as usize;

        let Some(a) = self.get(id) else { return };
        if a.busy() {
            return;
        }
        if !a.info.supported_on_current_os() {
            let message = format!(
                "{} is only available for {}.",
                a.info.name,
                a.info.platforms.join(", ")
            );
            a.log("error", message.clone());
            a.task = Task::Error { message };
            return;
        }
        if a.latest.is_none() {
            a.task = Task::Error { message: "No release has been found to install yet.".into() };
            return;
        }
        let target = a.target(&root);
        if target.exe_in_use() {
            let message = format!("{} is running. Close it, then update.", a.info.name);
            a.log("error", message.clone());
            a.task = Task::Error { message };
            return;
        }

        if limit_hit {
            let owned_id = a.info.id.clone();
            a.log.clear();
            a.log("info", "Waiting for another download to finish…");
            a.task = Task::Queued;
            if !self.queue.iter().any(|q| q == &owned_id) {
                self.queue.push(owned_id);
            }
            return;
        }
        self.start_install(id);
    }

    /// Starts updating every eligible installed app that has a newer release available.
    pub fn update_all(&mut self) {
        let ids: Vec<String> = self
            .apps
            .iter()
            .filter(|a| a.installed.is_some() && a.update_available() && a.info.supported_on_current_os() && !a.busy() && !a.running)
            .map(|a| a.info.id.clone())
            .collect();
        for id in ids {
            self.install(&id);
        }
    }

    /// Cancels a queued download before it has begun.
    pub fn cancel_queued(&mut self, id: &str) {
        self.queue.retain(|q| q != id);
        if let Some(a) = self.get(id) {
            if matches!(a.task, Task::Queued) {
                a.task = Task::Idle;
                a.log("info", "Removed from download queue");
            }
        }
    }

    fn drain_queue(&mut self) {
        while !self.queue.is_empty() && (self.max_downloads == 0 || self.active_downloads() < self.max_downloads as usize) {
            let next_id = self.queue.remove(0);
            if let Some(a) = self.get(&next_id) {
                if !matches!(a.task, Task::Queued) {
                    continue;
                }
                a.task = Task::Idle;
            }
            self.start_install(&next_id);
        }
    }

    fn start_install(&mut self, id: &str) {
        let proxy = self.proxy.clone();
        let root = self.install_root.clone();
        let Some(a) = self.get(id) else { return };
        if a.busy() {
            return;
        }
        let Some(release) = a.latest.clone() else {
            a.task = Task::Error { message: "No release has been found to install yet.".into() };
            return;
        };
        let target = a.target(&root);
        if target.exe_in_use() {
            let message = format!("{} is running. Close it, then update.", a.info.name);
            a.log("error", message.clone());
            a.task = Task::Error { message };
            return;
        }
        // Each install or update starts a fresh log.
        a.log.clear();
        match &a.installed {
            Some(from) => a.log("info", format!("Updating {} from v{from} to v{}", a.info.name, release.version)),
            None => a.log("info", format!("Installing {} v{}", a.info.name, release.version)),
        }
        a.task = Task::Downloading { done: 0, total: release.size };
        let id = a.info.id.clone();
        std::thread::spawn(move || {
            let send = |e: AppEvent| {
                let _ = proxy.send_event(UserEvent::App(id.clone(), e));
            };
            let log = |line: String| send(AppEvent::Log(line));
            let mut last_pct = u64::MAX;
            let progress = |done, total| {
                // One message per percent is plenty for a progress bar.
                let pct = if total > 0 { done * 100 / total } else { done / 1_000_000 };
                if total > 0 && done >= total {
                    // Downloaded; the unpacking and checking that follow are part
                    // of installing as far as anyone watching is concerned.
                    send(AppEvent::Installing);
                } else if pct != last_pct {
                    last_pct = pct;
                    send(AppEvent::Progress(done, total));
                }
            };
            let result = target.download(&release, progress, &log).and_then(|_| {
                send(AppEvent::Installing);
                target.apply(&log)
            });
            send(AppEvent::Finished(result));
        });
    }

    pub fn on_event(&mut self, id: &str, event: AppEvent) {
        let root = self.install_root.clone();
        let Some(a) = self.get(id) else { return };
        // A verify that finds damage goes straight on to repairing it.
        let mut repair_next = false;
        let mut drain_next = false;
        match event {
            AppEvent::Checked(Ok(latest)) => {
                a.latest = latest;
                a.checked = true;
                a.task = Task::Idle;
            }
            AppEvent::Checked(Err(message)) => {
                a.checked = true;
                let message = format!("Couldn't check for updates: {message}");
                a.log("error", message.clone());
                a.task = Task::Error { message };
            }
            AppEvent::Installing => a.task = Task::Installing,
            AppEvent::Progress(done, total) => a.task = Task::Downloading { done, total },
            AppEvent::Log(line) => a.log("info", line),
            AppEvent::Finished(Ok(())) => {
                a.refresh_installed(&root);
                a.damaged.clear();
                let done = format!("Done: {} v{} is installed", a.info.name, a.installed.as_deref().unwrap_or("?"));
                a.log("ok", done);
                a.task = Task::Idle;
                drain_next = true;
            }
            AppEvent::Finished(Err(message)) => {
                a.refresh_installed(&root);
                a.log("error", format!("Stopped: {message}"));
                a.task = Task::Error { message };
                drain_next = true;
            }
            AppEvent::VerifyProgress(done, total) => a.task = Task::Verifying { done, total },
            AppEvent::Verified(Ok(damaged)) => {
                a.damaged = damaged;
                a.log(if a.damaged.is_empty() { "ok" } else { "error" }, match a.damaged.len() {
                    0 => "Verify finished: nothing to repair".to_string(),
                    n => format!(
                        "Verify finished: {n} file{} damaged; fetching just {} again",
                        if n == 1 { "" } else { "s" },
                        if n == 1 { "that one" } else { "those" }
                    ),
                });
                a.task = Task::Idle;
                repair_next = !a.damaged.is_empty();
            }
            AppEvent::Verified(Err(message)) => {
                a.log("error", format!("Couldn't verify: {message}"));
                a.task = Task::Error { message };
            }
            AppEvent::RepairProgress(done, total) => a.task = Task::Repairing { done, total },
            AppEvent::Repaired(Ok(())) => {
                a.damaged.clear();
                a.log("ok", format!("Done: {} is repaired", a.info.name));
                a.task = Task::Idle;
            }
            AppEvent::Repaired(Err(message)) => {
                a.log("error", format!("Repair stopped: {message}"));
                a.task = Task::Error { message };
            }
            AppEvent::StopDeferred(message) => {
                a.running = a.target(&root).exe_in_use();
                a.log("info", message);
                a.task = Task::Idle;
            }
            AppEvent::Stopped(result) => {
                a.running = a.target(&root).exe_in_use();
                a.task = match result {
                    Ok(()) => {
                        a.log("ok", format!("{} has stopped", a.info.name));
                        Task::Idle
                    }
                    Err(message) => {
                        a.log("error", message.clone());
                        Task::Error { message }
                    }
                };
            }
        }
        if repair_next {
            self.repair(id);
        }
        if drain_next {
            self.drain_queue();
        }
    }

    /// Checks every installed file against its SHA-256. Damaged files are only
    /// reported; Repair fixes them.
    pub fn verify(&mut self, id: &str) {
        let proxy = self.proxy.clone();
        let root = self.install_root.clone();
        let Some(a) = self.get(id) else { return };
        if a.busy() || a.installed.is_none() {
            return;
        }
        a.log.clear();
        a.log("info", format!("Verifying {} v{}", a.info.name, a.installed.as_deref().unwrap_or("?")));
        a.task = Task::Verifying { done: 0, total: 0 };
        let (id, target) = (a.info.id.clone(), a.target(&root));
        std::thread::spawn(move || {
            let send = |e: AppEvent| {
                let _ = proxy.send_event(UserEvent::App(id.clone(), e));
            };
            let mut last = 0;
            let progress = |done, total| {
                // A message every few files keeps the page current without flooding it.
                if done == total || done - last >= 5 {
                    last = done;
                    send(AppEvent::VerifyProgress(done, total));
                }
            };
            let result = target.verify(progress, &|line| send(AppEvent::Log(line)));
            send(AppEvent::Verified(result));
        });
    }

    /// Re-downloads only the files the last verify found damaged.
    pub fn repair(&mut self, id: &str) {
        let proxy = self.proxy.clone();
        let root = self.install_root.clone();
        let Some(a) = self.get(id) else { return };
        if a.busy() || a.damaged.is_empty() {
            return;
        }
        let target = a.target(&root);
        if target.exe_in_use() {
            let message = format!("{} is running. Close it, then repair.", a.info.name);
            a.log("error", message.clone());
            a.task = Task::Error { message };
            return;
        }
        a.log("info", format!("Repairing {} file{}", a.damaged.len(), if a.damaged.len() == 1 { "" } else { "s" }));
        a.task = Task::Repairing { done: 0, total: 0 };
        let (id, paths) = (a.info.id.clone(), a.damaged.clone());
        std::thread::spawn(move || {
            let send = |e: AppEvent| {
                let _ = proxy.send_event(UserEvent::App(id.clone(), e));
            };
            let mut last_pct = u64::MAX;
            let progress = |done, total| {
                let pct = if total > 0 { done * 100 / total } else { 0 };
                if pct != last_pct {
                    last_pct = pct;
                    send(AppEvent::RepairProgress(done, total));
                }
            };
            let result = target.repair(&paths, progress, &|line| send(AppEvent::Log(line)));
            send(AppEvent::Repaired(result));
        });
    }

    /// Notices apps starting and stopping, whether through this launcher or not.
    /// Returns true when anything changed and the page needs a new snapshot.
    pub fn refresh_running(&mut self) -> bool {
        let root = self.install_root.clone();
        let mut changed = false;
        for a in self.apps.iter_mut().filter(|a| a.installed.is_some()) {
            let running = a.target(&root).exe_in_use();
            if running != a.running {
                a.running = running;
                changed = true;
            }
            let starting = a.starting.is_some_and(|t| !running && t.elapsed() < START_GRACE);
            if starting != a.starting.is_some() {
                a.starting = None;
                changed = true;
            }
        }
        changed
    }

    pub fn launch(&mut self, id: &str) {
        self.start(id, false);
    }

    /// Starts the app through the .bat in its folder instead of its exe.
    pub fn launch_bat(&mut self, id: &str) {
        self.start(id, true);
    }

    fn start(&mut self, id: &str, bat: bool) {
        let root = self.install_root.clone();
        let Some(a) = self.get(id) else { return };
        let target = a.target(&root);
        let Some(installed) = target.installed() else { return };
        // The button turns into Stop while it runs; this covers a click that
        // arrived just before that, or an app started some other way.
        if a.running || a.starting.is_some() || target.exe_in_use() {
            a.running = target.exe_in_use();
            a.log("info", format!("{} is already running", a.info.name));
            return;
        }
        let exe = match bat {
            false => target.dir().join(&installed.exe),
            true => match a.bat_path(&root) {
                Some(p) => p,
                None => {
                    a.log("error", format!("This version of {} has no .bat to start it with", a.info.name));
                    return;
                }
            },
        };
        match shell_open(&a.info, &exe, target.dir()) {
            Ok(()) => {
                a.log("info", format!("Started {}", exe.display()));
                a.starting = Some(Instant::now());
            }
            Err(message) => {
                let message = format!("Couldn't start {}: {message}", a.info.name);
                a.log("error", message.clone());
                a.task = Task::Error { message };
            }
        }
    }

    /// Asks a running app to quit, and ends it if it hasn't after a while.
    pub fn stop(&mut self, id: &str) {
        let proxy = self.proxy.clone();
        let root = self.install_root.clone();
        let Some(a) = self.get(id) else { return };
        if a.busy() || !a.running {
            return;
        }
        let (id, name, how, target) = (a.info.id.clone(), a.info.name.clone(), a.info.stop, a.target(&root));
        let Some(installed) = target.installed() else { return };
        a.task = Task::Stopping;
        a.starting = None;
        std::thread::spawn(move || {
            let log = |line: String| {
                let _ = proxy.send_event(UserEvent::App(id.clone(), AppEvent::Log(line)));
            };
            let dir = target.dir().to_path_buf();
            // Versions from before an app learned its quit argument would just start
            // another copy if given it, so those get their windows closed instead.
            let how = match how {
                Stop::QuitArg { since, .. }
                    if semver::Version::parse(&installed.version).ok() < semver::Version::parse(since).ok() =>
                {
                    Stop::CloseWindows
                }
                other => other,
            };
            match how {
                Stop::QuitArg { arg, .. } => {
                    log(format!("Asking {name} to quit ({} {arg})", installed.exe));
                    let asked = std::process::Command::new(dir.join(&installed.exe))
                        .arg(arg)
                        .current_dir(&dir)
                        .status();
                    match asked {
                        Err(e) => log(format!("Couldn't ask it ({e})")),
                        Ok(status) if status.code() == Some(QUIT_DEFERRED) => {
                            let message = format!(
                                "{name} is in the middle of a job, so it's asking in its own window whether to close once it's done"
                            );
                            let _ = proxy.send_event(UserEvent::App(id.clone(), AppEvent::StopDeferred(message)));
                            return;
                        }
                        Ok(_) => {}
                    }
                }
                Stop::CloseWindows => {
                    let pids = procs::running_in(&dir);
                    let asked = procs::close_windows(&pids);
                    log(format!("Asking {name} to close ({asked} window{})", if asked == 1 { "" } else { "s" }));
                }
            }

            let gone = |within: Duration| {
                let until = Instant::now() + within;
                while Instant::now() < until {
                    if !target.exe_in_use() {
                        return true;
                    }
                    std::thread::sleep(Duration::from_millis(250));
                }
                !target.exe_in_use()
            };
            let result = if gone(STOP_GRACE) {
                Ok(())
            } else {
                log(format!("{name} didn't quit within {} seconds; ending it", STOP_GRACE.as_secs()));
                let ended = procs::terminate(&procs::running_in(&dir));
                if ended > 0 && gone(Duration::from_secs(3)) {
                    Ok(())
                } else {
                    Err(format!(
                        "Couldn't stop {name}. If it's running as administrator, quit it from its own window or tray."
                    ))
                }
            };
            let _ = proxy.send_event(UserEvent::App(id, AppEvent::Stopped(result)));
        });
    }

    pub fn open_folder(&self, id: &str) {
        if let Some(a) = self.apps.iter().find(|a| a.info.id == id) {
            let target = a.target(&self.install_root);
            if target.dir().exists() {
                crate::open_in_shell(target.dir());
            }
        }
    }

    /// Deletes the app's folder, its settings included. The page asks first.
    pub fn uninstall(&mut self, id: &str) {
        let root = self.install_root.clone();
        let Some(a) = self.get(id) else { return };
        if a.busy() {
            return;
        }
        let target = a.target(&root);
        if target.exe_in_use() {
            let message = format!("{} is running. Close it, then uninstall.", a.info.name);
            a.log("error", message.clone());
            a.task = Task::Error { message };
            return;
        }
        // On macOS the folder goes to the Trash whole: once an app from another
        // developer has run, macOS won't let the launcher delete inside its bundle,
        // and deleting file by file would stop partway with the app half gone.
        #[cfg(target_os = "macos")]
        let result = {
            a.log("info", format!("Moving {} to the Trash", target.dir().display()));
            crate::updater::trash(target.dir())
        };
        #[cfg(not(target_os = "macos"))]
        let result = {
            a.log("info", format!("Deleting {}", target.dir().display()));
            std::fs::remove_dir_all(target.dir()).map_err(|e| e.to_string())
        };
        a.refresh_installed(&root);
        a.task = match result {
            Ok(()) => {
                a.log("ok", format!("{} is uninstalled", a.info.name));
                Task::Idle
            }
            Err(e) => {
                let message = format!("Couldn't remove every file ({e}).");
                a.log("error", message.clone());
                Task::Error { message }
            }
        };
    }
}

/// Starts a program the way Explorer would. Unlike spawning it directly, this
/// lets an app that asks for administrator rights show the UAC prompt.
#[cfg(windows)]
fn shell_open(_info: &RuntimeApp, exe: &std::path::Path, dir: &std::path::Path) -> Result<(), String> {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let wide = |s: &std::ffi::OsStr| {
        use std::os::windows::ffi::OsStrExt;
        s.encode_wide().chain(Some(0)).collect::<Vec<u16>>()
    };
    let (verb, file, cwd) = (wide("open".as_ref()), wide(exe.as_os_str()), wide(dir.as_os_str()));
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            cwd.as_ptr(),
            SW_SHOWNORMAL,
        )
    };
    // Values above 32 mean success; below are legacy error codes.
    if result as usize > 32 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().to_string())
    }
}

#[cfg(not(windows))]
fn shell_open(info: &RuntimeApp, exe: &std::path::Path, dir: &std::path::Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(exe, std::fs::Permissions::from_mode(0o755));

    // Point apps that support it at `<dir>/<Name>Data` so their settings stay inside
    // the app's portable folder on macOS rather than ~/Library/Application Support.
    // Apps differ in what they read (`<ID>_CONFIG_DIR`, `<ID>_DATA_DIR` or
    // `<ID>_PROFILE_DIR`), so all three are set; apps that read none ignore them.
    let id = info.id.to_ascii_uppercase();
    let portable_data = dir.join(format!("{}Data", info.folder()));
    let vars = [format!("{id}_CONFIG_DIR"), format!("{id}_DATA_DIR"), format!("{id}_PROFILE_DIR")];

    if let Some(bundle) = exe
        .ancestors()
        .find(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("app")))
    {
        let _ = std::process::Command::new("xattr").arg("-cr").arg(bundle).status();
        let mut open = std::process::Command::new("open");
        for var in &vars {
            open.arg("--env").arg(format!("{var}={}", portable_data.display()));
        }
        let status = open.arg(bundle).current_dir(dir).status().map_err(|e| e.to_string())?;
        if status.success() {
            return Ok(());
        }
    }

    std::process::Command::new(exe)
        .envs(vars.iter().map(|var| (var, &portable_data)))
        .current_dir(dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn png_data_url(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(22 + bytes.len().div_ceil(3) * 4);
    out.push_str("data:image/png;base64,");
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = *chunk.get(1).unwrap_or(&0) as usize;
        let b2 = *chunk.get(2).unwrap_or(&0) as usize;
        out.push(TABLE[b0 >> 2] as char);
        out.push(TABLE[((b0 & 0x03) << 4) | (b1 >> 4)] as char);
        out.push(if chunk.len() > 1 { TABLE[((b1 & 0x0f) << 2) | (b2 >> 6)] as char } else { '=' });
        out.push(if chunk.len() > 2 { TABLE[b2 & 0x3f] as char } else { '=' });
    }
    out
}


/// Checks on disk whether a custom GitHub app is currently installed under `root`
/// (or the default `portableApplications` directory).
pub fn is_custom_app_installed(c: &crate::settings::CustomApp, root: &std::path::Path) -> bool {
    let preferred = Target::custom_in(&c.repo, &c.folder, c.own_format, c.private, root);
    if preferred.installed().is_some() {
        return true;
    }
    let default_root = crate::settings::app_dir().join(crate::updater::APPS_DIR);
    if root != default_root {
        let legacy = Target::custom_in(&c.repo, &c.folder, c.own_format, c.private, &default_root);
        if legacy.installed().is_some() {
            return true;
        }
    }
    false
}

/// Removes private custom apps that are not installed on disk once no signed-in GitHub
/// account can reach them: apps added with an account that is no longer in `signed_in`,
/// or, for apps that don't record an account, once nobody is signed in at all. Installed
/// private apps and all public apps are preserved. Returns the display names of the
/// removed apps.
pub fn prune_uninstalled_private_apps(settings: &mut crate::settings::Settings, signed_in: &[String]) -> Vec<String> {
    let root = settings.apps_root();
    prune_uninstalled_private_apps_with(settings, signed_in, |c| is_custom_app_installed(c, &root))
}

fn prune_uninstalled_private_apps_with<F>(
    settings: &mut crate::settings::Settings,
    signed_in: &[String],
    mut is_installed: F,
) -> Vec<String>
where
    F: FnMut(&crate::settings::CustomApp) -> bool,
{
    let reachable = |c: &crate::settings::CustomApp| match c.account.as_deref() {
        Some(login) => signed_in.iter().any(|l| l.eq_ignore_ascii_case(login)),
        None => !signed_in.is_empty(),
    };
    let mut removed_ids = std::collections::HashSet::new();
    let mut removed_names = Vec::new();
    settings.custom_apps.retain(|c| {
        if c.private && !reachable(c) && !is_installed(c) {
            removed_ids.insert(c.id.clone());
            removed_names.push(c.name.clone());
            false
        } else {
            true
        }
    });
    if !removed_ids.is_empty() {
        settings.pinned.retain(|p| !removed_ids.contains(p));
    }
    removed_names
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::updater::APPS_DIR;

    #[test]
    fn portside_keeps_the_folder_it_had_as_kittyconnect() {
        let a = CATALOG.iter().find(|a| a.id == "kittyconnect").unwrap();
        assert_eq!(a.name, "Portside");
        assert_eq!(a.folder(), "KittyConnect");
        // Installed copies are found, and updated, where they already are.
        let t = Target::app(a.repo, a.folder());
        assert!(t.dir().ends_with(std::path::Path::new(APPS_DIR).join("KittyConnect")), "{}", t.dir().display());
    }

    #[test]
    fn ink_cognito_is_a_game_installed_from_its_own_releases() {
        let a = CATALOG.iter().find(|a| a.id == "inkcognito").unwrap();
        assert_eq!(a.category.id(), "games");
        assert!(matches!(a.source, Source::Own));
        assert!(a.icon.is_some());
        assert_eq!(a.platforms, &["Windows", "MacOS"]);
        let t = Target::app(a.repo, a.folder());
        assert!(t.dir().ends_with(std::path::Path::new(APPS_DIR).join("Ink-cognito")), "{}", t.dir().display());
    }

    #[test]
    fn every_other_app_is_in_a_folder_named_after_it_and_no_two_share_one() {
        for a in CATALOG.iter().filter(|a| a.folder.is_none()) {
            assert_eq!(a.folder(), a.name);
        }
        let mut folders: Vec<String> = CATALOG.iter().map(|a| a.folder().to_ascii_lowercase()).collect();
        folders.sort();
        folders.dedup();
        assert_eq!(folders.len(), CATALOG.len());
    }

    #[test]
    fn javacrafter_apps_have_embedded_png_icons() {
        for id in ["lighthouse", "screenshotapp", "rustwinutil", "kittyconnect", "inkcognito"] {
            let app = CATALOG.iter().find(|a| a.id == id).unwrap();
            let bytes = app.icon.unwrap_or_else(|| panic!("{id} should have an embedded icon"));
            assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"), "{id} icon should be a valid PNG");
        }
    }

    #[test]
    fn prune_uninstalled_private_apps_keeps_installed_and_public() {
        use crate::settings::{CustomApp, Settings};

        let make = |id: &str, name: &str, private: bool| CustomApp {
            account: None,
            id: id.into(),
            name: name.into(),
            folder: name.into(),
            repo: format!("me/{name}"),
            tagline: String::new(),
            publisher: "me".into(),
            category: "apps".into(),
            platforms: vec!["Windows".into()],
            private,
            own_format: false,
        };

        let mut settings = Settings::default();
        settings.custom_apps = vec![
            make("priv-uninstalled", "SecretNotInstalled", true),
            make("priv-installed", "SecretInstalled", true),
            make("pub-uninstalled", "PublicNotInstalled", false),
            make("pub-installed", "PublicInstalled", false),
        ];
        settings.pinned = vec![
            "priv-uninstalled".into(),
            "priv-installed".into(),
            "pub-uninstalled".into(),
        ];

        let removed = prune_uninstalled_private_apps_with(&mut settings, &[], |c| {
            c.id == "priv-installed" || c.id == "pub-installed"
        });

        assert_eq!(removed, vec!["SecretNotInstalled".to_string()]);
        let remaining: Vec<&str> = settings.custom_apps.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(remaining, vec!["priv-installed", "pub-uninstalled", "pub-installed"]);
        assert_eq!(settings.pinned, vec!["priv-installed", "pub-uninstalled"]);
    }

    #[test]
    fn prune_only_drops_private_apps_of_the_removed_account() {
        use crate::settings::{CustomApp, Settings};

        let make = |id: &str, account: Option<&str>| CustomApp {
            id: id.into(),
            name: id.into(),
            folder: id.into(),
            repo: format!("someone/{id}"),
            tagline: String::new(),
            publisher: "someone".into(),
            category: "apps".into(),
            platforms: vec!["Windows".into()],
            private: true,
            own_format: false,
            account: account.map(String::from),
        };
        let mut settings = Settings::default();
        settings.custom_apps = vec![make("mine", Some("me")), make("friends", Some("Friend")), make("legacy", None)];

        // The friend's token was removed; mine is still signed in.
        let removed = prune_uninstalled_private_apps_with(&mut settings, &["me".into()], |_| false);
        assert_eq!(removed, vec!["friends".to_string()]);
        let left: Vec<&str> = settings.custom_apps.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(left, vec!["mine", "legacy"]);

        // Account names match case-insensitively.
        let removed = prune_uninstalled_private_apps_with(&mut settings, &["ME".into()], |_| false);
        assert!(removed.is_empty());

        // Last account gone: everything private and uninstalled goes.
        let removed = prune_uninstalled_private_apps_with(&mut settings, &[], |_| false);
        assert_eq!(removed, vec!["mine".to_string(), "legacy".to_string()]);
    }
}
