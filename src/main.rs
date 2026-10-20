// No console window in release builds; debug builds keep one for logs.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod apps;
mod instance;
mod procs;
mod settings;
mod updater;

use serde::Serialize;
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};
use tao::dpi::LogicalSize;
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::platform::windows::WindowBuilderExtWindows;
use tao::window::{Icon, ResizeDirection, Window, WindowBuilder};
use wry::{WebContext, WebView, WebViewBuilder, WebViewBuilderExtWindows};

use apps::{AppEvent, Library};
use settings::{app_dir, data_dir, Settings};
use updater::{Release, Target};

const INDEX_HTML: &str = include_str!("../ui/index.html");
const APP_CSS: &str = include_str!("../ui/app.css");
const APP_JS: &str = include_str!("../ui/app.js");
const ICON_RGBA: &[u8] = include_bytes!("../assets/icon-64.rgba");


/// Where the self-update stands. Sent to the page whole on every change.
#[derive(Clone, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
enum Update {
    Idle,
    Checking,
    UpToDate,
    Available { release: Release },
    Downloading { release: Release, done: u64, total: u64 },
    Ready { release: Release },
    Error { message: String },
}

impl Update {
    fn busy(&self) -> bool {
        matches!(self, Update::Checking | Update::Downloading { .. })
    }
}

pub(crate) enum UserEvent {
    Ipc(String),
    /// From the self-update worker. `auto` means this run was started on launch
    /// and should carry straight through to downloading.
    Update { update: Update, auto: bool },
    /// From an app's check or install worker.
    App(&'static str, AppEvent),
    /// Every couple of seconds: time to look at which apps are running.
    Tick,
}

struct App {
    settings: Settings,
    update: Update,
    /// True when the current update run began automatically on launch.
    auto_run: bool,
    library: Library,
    proxy: EventLoopProxy<UserEvent>,
}

fn main() {
    // Started by an update: the old copy is still exiting, so wait for it.
    let just_updated = std::env::args().any(|a| a == "--updated");
    let Some(_instance) = instance::claim(just_updated) else {
        return;
    };

    // An update the user deferred with "Later" and never got applied on exit
    // (a crash, a power cut) goes in now, before anything else opens.
    let me = Target::launcher();
    if me.has_staged() && me.apply(&|_| {}).is_ok() {
        if let Ok(exe) = std::env::current_exe() {
            if updater::relaunch(&exe).is_ok() {
                return;
            }
        }
    }
    me.sweep_leftovers();

    // A fixed-version WebView2 runtime dropped in a `WebView2` folder beside the exe
    // is used in preference to the system one, for machines without Edge.
    let fixed = app_dir().join("WebView2");
    if fixed.join("msedgewebview2.exe").exists() {
        std::env::set_var("WEBVIEW2_BROWSER_EXECUTABLE_FOLDER", &fixed);
    }

    let settings = Settings::load();
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let window = WindowBuilder::new()
        .with_title("Launcher")
        .with_window_classname(instance::WINDOW_CLASS)
        .with_inner_size(LogicalSize::new(1040.0, 680.0))
        .with_min_inner_size(LogicalSize::new(720.0, 480.0))
        .with_decorations(false)
        .with_undecorated_shadow(true)
        .with_visible(false)
        .with_window_icon(Icon::from_rgba(ICON_RGBA.to_vec(), 64, 64).ok())
        .build(&event_loop)
        .expect("create window");

    let mut web_context = WebContext::new(Some(data_dir().join("webview")));
    let ipc_proxy = proxy.clone();
    let webview = WebViewBuilder::new_with_web_context(&mut web_context)
        .with_html(page_html(&settings))
        .with_background_color(settings.paper())
        .with_devtools(cfg!(debug_assertions))
        .with_browser_accelerator_keys(false)
        .with_ipc_handler(move |req| {
            let _ = ipc_proxy.send_event(UserEvent::Ipc(req.into_body()));
        })
        .build(&window);

    let webview = match webview {
        Ok(w) => w,
        Err(e) => {
            missing_webview2(&e.to_string());
            return;
        }
    };

    let library = Library::new(proxy.clone());
    let ticker = proxy.clone();
    std::thread::spawn(move || {
        while ticker.send_event(UserEvent::Tick).is_ok() {
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
    });
    let mut app = App { settings, update: Update::Idle, auto_run: false, library, proxy };

    event_loop.run(move |event, _, flow| {
        *flow = ControlFlow::Wait;
        // Keeps the context (and so the WebView2 profile folder) alive for the loop.
        let _ = &web_context;

        match event {
            Event::NewEvents(StartCause::Init) => {}
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                app.finish_on_exit();
                *flow = ControlFlow::Exit;
            }
            Event::WindowEvent { event: WindowEvent::Resized(_), .. } => {
                send(&webview, json!({ "type": "window", "maximized": window.is_maximized() }));
            }
            Event::UserEvent(UserEvent::Ipc(body)) => {
                if let Ok(msg) = serde_json::from_str::<Value>(&body) {
                    if app.on_message(&msg, &window, &webview) {
                        app.finish_on_exit();
                        *flow = ControlFlow::Exit;
                    }
                }
            }
            Event::UserEvent(UserEvent::Update { update, auto }) => {
                app.on_update(update, auto, &webview);
            }
            Event::UserEvent(UserEvent::App(id, event)) => {
                app.library.on_event(id, event);
                send(&webview, app.library.snapshot());
            }
            Event::UserEvent(UserEvent::Tick) => {
                if app.library.refresh_running() {
                    send(&webview, app.library.snapshot());
                }
            }
            _ => {}
        }
    });
}

/// The page is inlined into one document with the theme already stamped on, so a
/// dark install never flashes light on the way in.
fn page_html(settings: &Settings) -> String {
    let boot = json!({
        "version": updater::VERSION,
        "repo": updater::REPO,
        "settings": settings,
        "dataDir": data_dir().display().to_string(),
        "justUpdated": std::env::args().any(|a| a == "--updated"),
        "icons": apps::icons(),
    });
    INDEX_HTML
        .replace("{{THEME}}", if settings.is_dark() { "dark" } else { "light" })
        .replace("{{PALETTE}}", settings.palette())
        .replace("/*{{CSS}}*/", APP_CSS)
        .replace("/*{{BOOT}}*/", &format!("window.BOOT = {boot};"))
        .replace("/*{{JS}}*/", APP_JS)
}

fn send(webview: &WebView, msg: Value) {
    let _ = webview.evaluate_script(&format!("window.onHost && window.onHost({msg})"));
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl App {
    /// Handles one message from the page. Returns true when the launcher should exit.
    fn on_message(&mut self, msg: &Value, window: &Window, webview: &WebView) -> bool {
        let cmd = msg["cmd"].as_str().unwrap_or_default();
        match cmd {
            "ready" => {
                window.set_visible(true);
                window.set_focus();
                self.push_update(webview);
                if self.settings.auto_update {
                    self.start_check(true);
                }
                // App updates are always looked for, never installed unasked.
                self.library.sweep();
                self.library.check_all(self.settings.prereleases);
                send(webview, self.library.snapshot());
            }
            "appCheck" | "appInstall" | "appLaunch" | "appFolder" | "appUninstall" | "appVerify" | "appRepair" | "appStop" => {
                let id = msg["id"].as_str().unwrap_or_default();
                match cmd {
                    "appCheck" => self.library.check_all(self.settings.prereleases),
                    "appInstall" => self.library.install(id),
                    "appLaunch" => self.library.launch(id),
                    "appFolder" => self.library.open_folder(id),
                    "appVerify" => self.library.verify(id),
                    "appStop" => self.library.stop(id),
                    "appRepair" => self.library.repair(id),
                    _ => self.library.uninstall(id),
                }
                send(webview, self.library.snapshot());
            }
            "window" => match msg["action"].as_str().unwrap_or_default() {
                "drag" => {
                    let _ = window.drag_window();
                }
                "minimize" => window.set_minimized(true),
                "maximize" => window.set_maximized(!window.is_maximized()),
                "close" => return true,
                _ => {}
            },
            "resize" => {
                let dir = match msg["dir"].as_str().unwrap_or_default() {
                    "n" => ResizeDirection::North,
                    "s" => ResizeDirection::South,
                    "e" => ResizeDirection::East,
                    "w" => ResizeDirection::West,
                    "ne" => ResizeDirection::NorthEast,
                    "nw" => ResizeDirection::NorthWest,
                    "se" => ResizeDirection::SouthEast,
                    "sw" => ResizeDirection::SouthWest,
                    _ => return false,
                };
                if !window.is_maximized() {
                    let _ = window.drag_resize_window(dir);
                }
            }
            "settings" => {
                if let Some(t) = msg["theme"].as_str() {
                    self.settings.theme = if t == "dark" { "dark".into() } else { "light".into() };
                }
                if let Some(b) = msg["autoUpdate"].as_bool() {
                    self.settings.auto_update = b;
                }
                if let Some(b) = msg["prereleases"].as_bool() {
                    self.settings.prereleases = b;
                }
                // Only scheme names this version defines are kept.
                if let Some(s) = msg["lightScheme"].as_str().filter(|s| settings::LIGHT_SCHEMES.contains(s)) {
                    self.settings.light_scheme = s.into();
                }
                if let Some(s) = msg["darkScheme"].as_str().filter(|s| settings::DARK_SCHEMES.contains(s)) {
                    self.settings.dark_scheme = s.into();
                }
                let _ = self.settings.save();
                send(webview, json!({ "type": "settings", "settings": self.settings }));
            }
            "checkUpdate" => self.start_check(false),
            "downloadUpdate" => {
                if let Update::Available { release } = &self.update {
                    self.start_download(release.clone(), false);
                }
            }
            "restartNow" => {
                if let Update::Ready { .. } = self.update {
                    return self.install_and_restart(webview);
                }
            }
            "openUrl" => {
                // Only ever hand GitHub links to the shell, never arbitrary strings.
                if let Some(url) = msg["url"].as_str() {
                    if url.starts_with("https://github.com/") {
                        let _ = std::process::Command::new("explorer").arg(url).spawn();
                    }
                }
            }
            "openDataDir" => {
                let _ = std::fs::create_dir_all(data_dir());
                let _ = std::process::Command::new("explorer").arg(data_dir()).spawn();
            }
            _ => {}
        }
        false
    }

    fn on_update(&mut self, update: Update, auto: bool, webview: &WebView) {
        if matches!(update, Update::UpToDate | Update::Available { .. }) {
            self.settings.last_check = Some(now_secs());
            let _ = self.settings.save();
            send(webview, json!({ "type": "settings", "settings": self.settings }));
        }
        // On launch, a found update downloads without being asked.
        let carry_on = match (&update, auto) {
            (Update::Available { release }, true) => Some(release.clone()),
            _ => None,
        };
        self.update = update;
        self.auto_run = auto;
        self.push_update(webview);
        if let Some(release) = carry_on {
            self.start_download(release, true);
        }
    }

    fn push_update(&self, webview: &WebView) {
        send(webview, json!({ "type": "update", "update": self.update, "auto": self.auto_run }));
    }

    fn start_check(&mut self, auto: bool) {
        if self.update.busy() || matches!(self.update, Update::Ready { .. }) {
            return;
        }
        let prereleases = self.settings.prereleases;
        let proxy = self.proxy.clone();
        self.update = Update::Checking;
        let _ = proxy.send_event(UserEvent::Update { update: Update::Checking, auto });
        std::thread::spawn(move || {
            let current = updater::current_version();
            let update = match Target::launcher().latest(prereleases) {
                Ok(Some(release)) if release.semver().is_some_and(|v| v > current) => {
                    Update::Available { release }
                }
                Ok(_) => Update::UpToDate,
                Err(message) => Update::Error { message },
            };
            let _ = proxy.send_event(UserEvent::Update { update, auto });
        });
    }

    fn start_download(&mut self, release: Release, auto: bool) {
        if self.update.busy() {
            return;
        }
        let proxy = self.proxy.clone();
        let total = release.size;
        self.update = Update::Downloading { release: release.clone(), done: 0, total };
        std::thread::spawn(move || {
            let mut last_pct = u64::MAX;
            let progress = |done, total| {
                // One message per percent is plenty for a progress bar.
                let pct = if total > 0 { done * 100 / total } else { done / 1_000_000 };
                if pct != last_pct {
                    last_pct = pct;
                    let _ = proxy.send_event(UserEvent::Update {
                        update: Update::Downloading { release: release.clone(), done, total },
                        auto,
                    });
                }
            };
            let result = Target::launcher().download(&release, progress, &|_| {});
            let update = match result {
                Ok(_) => Update::Ready { release },
                Err(message) => Update::Error { message },
            };
            let _ = proxy.send_event(UserEvent::Update { update, auto });
        });
    }

    /// Returns true when the new copy is running and this one should exit.
    fn install_and_restart(&mut self, webview: &WebView) -> bool {
        let me = Target::launcher();
        let result = if me.has_staged() {
            me.apply(&|_| {}).and_then(|_| {
                let exe = std::env::current_exe().map_err(|e| e.to_string())?;
                updater::relaunch(&exe)
            })
        } else {
            Err("The downloaded update has gone missing. Check again.".to_string())
        };
        match result {
            Ok(()) => true,
            Err(message) => {
                self.update = Update::Error { message };
                self.push_update(webview);
                false
            }
        }
    }

    /// A downloaded update the user put off is swapped in as the launcher closes,
    /// so the next open is already the new version.
    fn finish_on_exit(&mut self) {
        if let Update::Ready { .. } = self.update {
            let me = Target::launcher();
            if me.has_staged() {
                let _ = me.apply(&|_| {});
            }
        }
    }
}

fn missing_webview2(detail: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, IDOK, MB_ICONWARNING, MB_OKCANCEL};
    let text = format!(
        "Launcher needs the Microsoft Edge WebView2 Runtime, which comes with Windows 11 \
         and most Windows 10 installs, but couldn't start it here.\n\n\
         Press OK to open Microsoft's download page.\n\n({detail})"
    );
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (text, title) = (wide(&text), wide("Launcher"));
    let choice = unsafe {
        MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), MB_OKCANCEL | MB_ICONWARNING)
    };
    if choice == IDOK {
        let _ = std::process::Command::new("explorer")
            .arg("https://developer.microsoft.com/microsoft-edge/webview2/")
            .spawn();
    }
}
