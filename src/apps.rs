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
use tao::event_loop::EventLoopProxy;

use crate::updater::{Release, Target};
use crate::UserEvent;

pub struct AppInfo {
    pub id: &'static str,
    /// Also the folder name under portableApplications and the package name
    /// (`<name>-windows-x64.zip`).
    pub name: &'static str,
    pub repo: &'static str,
    pub tagline: &'static str,
}

pub const CATALOG: &[AppInfo] = &[
    AppInfo {
        id: "lighthouse",
        name: "Lighthouse",
        repo: "JavacrafterDevelopment/Lighthouse",
        tagline: "Instant search across every file and folder on every drive, with Explorer's own icons.",
    },
    AppInfo {
        id: "screenshotapp",
        name: "ScreenshotApp",
        repo: "JavacrafterDevelopment/ScreenshotApp",
        tagline: "Timed and hotkey screenshots, numbered and saved wherever you like.",
    },
];

/// What an app is busy with, if anything.
#[derive(Clone, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum Task {
    Idle,
    Checking,
    Downloading { done: u64, total: u64 },
    Installing,
    Verifying { done: usize, total: usize },
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

struct AppState {
    info: &'static AppInfo,
    installed: Option<String>,
    latest: Option<Release>,
    /// True once a check has answered, so "no release yet" can be told apart
    /// from "not checked yet".
    checked: bool,
    task: Task,
    log: Vec<LogLine>,
    /// Files the last verify found missing or changed; Repair fetches these.
    damaged: Vec<String>,
}

impl AppState {
    fn target(&self) -> Target {
        Target::app(self.info.repo, self.info.name)
    }

    fn refresh_installed(&mut self) {
        self.installed = self.target().installed().map(|i| i.version);
    }

    fn update_available(&self) -> bool {
        let latest = self.latest.as_ref().and_then(|r| r.semver());
        let installed = self.installed.as_deref().and_then(|v| semver::Version::parse(v).ok());
        matches!((latest, installed), (Some(l), Some(i)) if l > i)
    }

    fn busy(&self) -> bool {
        matches!(
            self.task,
            Task::Checking
                | Task::Downloading { .. }
                | Task::Installing
                | Task::Verifying { .. }
                | Task::Repairing { .. }
        )
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
}

impl Library {
    pub fn new(proxy: EventLoopProxy<UserEvent>) -> Self {
        let apps = CATALOG
            .iter()
            .map(|info| {
                let mut a = AppState {
                    info,
                    installed: None,
                    latest: None,
                    checked: false,
                    task: Task::Idle,
                    log: Vec::new(),
                    damaged: Vec::new(),
                };
                a.refresh_installed();
                a
            })
            .collect();
        Self { apps, proxy }
    }

    /// Clears what earlier updates left behind in each installed app's folder.
    pub fn sweep(&self) {
        for a in self.apps.iter().filter(|a| a.installed.is_some()) {
            a.target().sweep_leftovers();
        }
    }

    pub fn snapshot(&self) -> Value {
        let apps: Vec<Value> = self
            .apps
            .iter()
            .map(|a| {
                json!({
                    "id": a.info.id,
                    "name": a.info.name,
                    "tagline": a.info.tagline,
                    "repo": a.info.repo,
                    "installed": a.installed,
                    "latest": a.latest,
                    "checked": a.checked,
                    "updateAvailable": a.update_available(),
                    "task": a.task,
                    "log": a.log,
                    "damaged": a.damaged,
                })
            })
            .collect();
        json!({ "type": "apps", "apps": apps })
    }

    fn get(&mut self, id: &str) -> Option<&mut AppState> {
        self.apps.iter_mut().find(|a| a.info.id == id)
    }

    /// Looks for new releases of every app. Finding one only shows the Update
    /// button; nothing is downloaded until it's pressed.
    pub fn check_all(&mut self, prereleases: bool) {
        for a in self.apps.iter_mut().filter(|a| !a.busy()) {
            a.task = Task::Checking;
            let (id, target, proxy) = (a.info.id, a.target(), self.proxy.clone());
            std::thread::spawn(move || {
                let result = target.latest(prereleases);
                let _ = proxy.send_event(UserEvent::App(id, AppEvent::Checked(result)));
            });
        }
    }

    /// Installs the app, or updates it to the newest release found.
    pub fn install(&mut self, id: &str) {
        let proxy = self.proxy.clone();
        let Some(a) = self.get(id) else { return };
        if a.busy() {
            return;
        }
        let Some(release) = a.latest.clone() else {
            a.task = Task::Error { message: "No release has been found to install yet.".into() };
            return;
        };
        let target = a.target();
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
        let id = a.info.id;
        std::thread::spawn(move || {
            let send = |e: AppEvent| {
                let _ = proxy.send_event(UserEvent::App(id, e));
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
        let Some(a) = self.get(id) else { return };
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
                a.refresh_installed();
                a.damaged.clear();
                let done = format!("Done: {} v{} is installed", a.info.name, a.installed.as_deref().unwrap_or("?"));
                a.log("ok", done);
                a.task = Task::Idle;
            }
            AppEvent::Finished(Err(message)) => {
                a.refresh_installed();
                a.log("error", format!("Stopped: {message}"));
                a.task = Task::Error { message };
            }
            AppEvent::VerifyProgress(done, total) => a.task = Task::Verifying { done, total },
            AppEvent::Verified(Ok(damaged)) => {
                a.damaged = damaged;
                a.log(if a.damaged.is_empty() { "ok" } else { "error" }, match a.damaged.len() {
                    0 => "Verify finished: nothing to repair".to_string(),
                    n => format!("Verify finished: {n} file{} to repair. Press Repair to fetch just those.", if n == 1 { "" } else { "s" }),
                });
                a.task = Task::Idle;
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
        }
    }

    /// Checks every installed file against its SHA-256. Damaged files are only
    /// reported; Repair fixes them.
    pub fn verify(&mut self, id: &str) {
        let proxy = self.proxy.clone();
        let Some(a) = self.get(id) else { return };
        if a.busy() || a.installed.is_none() {
            return;
        }
        a.log.clear();
        a.log("info", format!("Verifying {} v{}", a.info.name, a.installed.as_deref().unwrap_or("?")));
        a.task = Task::Verifying { done: 0, total: 0 };
        let (id, target) = (a.info.id, a.target());
        std::thread::spawn(move || {
            let send = |e: AppEvent| {
                let _ = proxy.send_event(UserEvent::App(id, e));
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
        let Some(a) = self.get(id) else { return };
        if a.busy() || a.damaged.is_empty() {
            return;
        }
        let target = a.target();
        if target.exe_in_use() {
            let message = format!("{} is running. Close it, then repair.", a.info.name);
            a.log("error", message.clone());
            a.task = Task::Error { message };
            return;
        }
        a.log("info", format!("Repairing {} file{}", a.damaged.len(), if a.damaged.len() == 1 { "" } else { "s" }));
        a.task = Task::Repairing { done: 0, total: 0 };
        let (id, paths) = (a.info.id, a.damaged.clone());
        std::thread::spawn(move || {
            let send = |e: AppEvent| {
                let _ = proxy.send_event(UserEvent::App(id, e));
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

    pub fn launch(&mut self, id: &str) {
        let Some(a) = self.get(id) else { return };
        let target = a.target();
        let Some(installed) = target.installed() else { return };
        let exe = target.dir().join(&installed.exe);
        match shell_open(&exe, target.dir()) {
            Ok(()) => a.log("info", format!("Started {}", exe.display())),
            Err(message) => {
                let message = format!("Couldn't start {}: {message}", a.info.name);
                a.log("error", message.clone());
                a.task = Task::Error { message };
            }
        }
    }

    pub fn open_folder(&self, id: &str) {
        if let Some(a) = self.apps.iter().find(|a| a.info.id == id) {
            let target = a.target();
            if target.dir().exists() {
                let _ = std::process::Command::new("explorer").arg(target.dir()).spawn();
            }
        }
    }

    /// Deletes the app's folder, its settings included. The page asks first.
    pub fn uninstall(&mut self, id: &str) {
        let Some(a) = self.get(id) else { return };
        if a.busy() {
            return;
        }
        let target = a.target();
        if target.exe_in_use() {
            let message = format!("{} is running. Close it, then uninstall.", a.info.name);
            a.log("error", message.clone());
            a.task = Task::Error { message };
            return;
        }
        a.log("info", format!("Deleting {}", target.dir().display()));
        let result = std::fs::remove_dir_all(target.dir());
        a.refresh_installed();
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
fn shell_open(exe: &std::path::Path, dir: &std::path::Path) -> Result<(), String> {
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
