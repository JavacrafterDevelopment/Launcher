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

pub struct AppInfo {
    pub id: &'static str,
    /// Also the folder name under portableApplications.
    pub name: &'static str,
    pub repo: &'static str,
    pub tagline: &'static str,
    pub icon: Option<&'static [u8]>,
    pub stop: Stop,
    pub source: Source,
}

#[derive(Clone, Copy)]
pub enum Source {
    /// Built and packaged with `tools/package.ps1` (`<name>-windows-x64.zip` + `update.json`).
    Own,
    /// An open-source app from another author's GitHub releases. `package` is the
    /// release zip name (`{version}` is replaced with the release's version) and
    /// `exe` is the program inside it.
    ThirdParty { package: &'static str, exe: &'static str },
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
        repo: "JavacrafterDevelopment/Lighthouse",
        tagline: "Instant search across every file and folder on every drive, with Explorer's own icons.",
        icon: None,
        // Runs as administrator, and its close button only hides it to the tray.
        stop: Stop::QuitArg { arg: "--quit", since: "0.0.2" },
        source: Source::Own,
    },
    AppInfo {
        id: "screenshotapp",
        name: "ScreenshotApp",
        repo: "JavacrafterDevelopment/ScreenshotApp",
        tagline: "Timed and hotkey screenshots, numbered and saved wherever you like.",
        icon: None,
        stop: Stop::CloseWindows,
        source: Source::Own,
    },
    AppInfo {
        id: "rustwinutil",
        name: "RustWinUtil",
        repo: "JavacrafterDevelopment/RustWinUtil",
        tagline: "Chris Titus Tech's WinUtil, rebuilt in Rust: install apps, apply tweaks, fix Windows.",
        icon: None,
        // Runs as administrator, so the launcher can't close it itself.
        stop: Stop::QuitArg { arg: "--quit", since: "0.0.1" },
        source: Source::Own,
    },
    AppInfo {
        id: "photocraft",
        name: "PhotoCraft",
        repo: "storytold/photocraft",
        tagline: "Open-source photo editor written from scratch in pure Rust.",
        icon: Some(include_bytes!("../assets/photocraft.png")),
        stop: Stop::CloseWindows,
        source: Source::ThirdParty {
            package: "photocraft-{version}-windows-x64-portable.zip",
            exe: "photocraft.exe",
        },
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

struct AppState {
    info: &'static AppInfo,
    icon: Option<String>,
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

impl AppState {
    fn target(&self) -> Target {
        match self.info.source {
            Source::Own => Target::app(self.info.repo, self.info.name),
            Source::ThirdParty { package, exe } => {
                Target::third_party(self.info.repo, self.info.name, package, exe)
            }
        }
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
                | Task::Stopping
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
                    icon: info.icon.map(png_data_url),
                    installed: None,
                    latest: None,
                    checked: false,
                    task: Task::Idle,
                    log: Vec::new(),
                    damaged: Vec::new(),
                    running: false,
                    starting: None,
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
                    "icon": a.icon,
                    "thirdParty": matches!(a.info.source, Source::ThirdParty { .. }),
                    "installed": a.installed,
                    "latest": a.latest,
                    "checked": a.checked,
                    "updateAvailable": a.update_available(),
                    "task": a.task,
                    "log": a.log,
                    "damaged": a.damaged,
                    "running": a.running,
                    "starting": a.starting.is_some(),
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
            a.refresh_installed();
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
        // A verify that finds damage goes straight on to repairing it.
        let mut repair_next = false;
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
                a.running = a.target().exe_in_use();
                a.log("info", message);
                a.task = Task::Idle;
            }
            AppEvent::Stopped(result) => {
                a.running = a.target().exe_in_use();
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

    /// Notices apps starting and stopping, whether through this launcher or not.
    /// Returns true when anything changed and the page needs a new snapshot.
    pub fn refresh_running(&mut self) -> bool {
        let mut changed = false;
        for a in self.apps.iter_mut().filter(|a| a.installed.is_some()) {
            let running = a.target().exe_in_use();
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
        let Some(a) = self.get(id) else { return };
        let target = a.target();
        let Some(installed) = target.installed() else { return };
        // The button turns into Stop while it runs; this covers a click that
        // arrived just before that, or an app started some other way.
        if a.running || a.starting.is_some() || target.exe_in_use() {
            a.running = target.exe_in_use();
            a.log("info", format!("{} is already running", a.info.name));
            return;
        }
        let exe = target.dir().join(&installed.exe);
        match shell_open(&exe, target.dir()) {
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
        let Some(a) = self.get(id) else { return };
        if a.busy() || !a.running {
            return;
        }
        let (id, name, how, target) = (a.info.id, a.info.name, a.info.stop, a.target());
        let Some(installed) = target.installed() else { return };
        a.task = Task::Stopping;
        a.starting = None;
        std::thread::spawn(move || {
            let log = |line: String| {
                let _ = proxy.send_event(UserEvent::App(id, AppEvent::Log(line)));
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
                            let _ = proxy.send_event(UserEvent::App(id, AppEvent::StopDeferred(message)));
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

