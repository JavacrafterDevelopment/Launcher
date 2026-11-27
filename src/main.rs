// No console window in release builds; debug builds keep one for logs.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod apps;
mod github_auth;
mod instance;
mod procs;
mod settings;
mod updater;

use serde::Serialize;
use serde_json::{json, Value};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tao::dpi::LogicalSize;
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
#[cfg(target_os = "macos")]
use tao::platform::macos::WindowBuilderExtMacOS;
#[cfg(windows)]
use tao::platform::windows::WindowBuilderExtWindows;
use tao::window::{Icon, ResizeDirection, Window, WindowBuilder};
use wry::{WebContext, WebView, WebViewBuilder};
#[cfg(windows)]
use wry::WebViewBuilderExtWindows;

use apps::{AppEvent, Library};
use github_auth::{AuthStatus, GitHubUser, RepoPreview, RepoSummary, StoredCredential};
use settings::{app_dir, data_dir, CustomApp, Settings};
use updater::{Release, Target};

const INDEX_HTML: &str = include_str!("../ui/index.html");
const APP_CSS: &str = include_str!("../ui/app.css");
const APP_JS: &str = include_str!("../ui/app.js");
const ICON_RGBA: &[u8] = include_bytes!("../assets/icon-64.rgba");

/// How often background checks look for launcher and app updates while open.
const AUTO_CHECK_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// Why a self-update check was triggered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
enum CheckMode {
    /// Launch-time Discord-style loading splash: auto-downloads & restarts if an update is found.
    Startup,
    /// Periodic 5-minute background check: never auto-restarts; shows a bottom-right popup.
    Background,
    /// User clicked Check for updates manually.
    Manual,
}

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

pub(crate) enum AuthEvent {
    Connected(GitHubUser),
    Error(String),
    CredentialsListed(Result<Vec<StoredCredential>, String>),
    CredentialsCleared { scope: &'static str, result: Result<usize, String>, signed_out: bool },
    PrivateRepos(Result<Vec<RepoSummary>, String>),
}

pub(crate) enum UserEvent {
    Ipc(String),
    /// From the self-update worker.
    Update { update: Update, mode: CheckMode },
    /// From an app's check or install worker.
    App(String, AppEvent),
    /// From a GitHub auth, OS credential store, or private-repo worker.
    Auth(AuthEvent),
    /// From inspecting a pasted GitHub link or chosen private repo.
    RepoInspected(Result<RepoPreview, String>),
    /// Result from the off-thread native folder picker.
    PickedInstallDir(Option<String>),
    /// Every couple of seconds: time to look at which apps are running and whether
    /// the 5-minute background update check is due.
    Tick,
    /// A few seconds after start: shows the window if the page never said it was
    /// ready, so a page that fails to boot doesn't leave an invisible process.
    Reveal,
    /// A menu bar item was picked (macOS); carries the item's id.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Menu(String),
}

const MENU_CHECK_LAUNCHER: &str = "check-launcher";
const MENU_CHECK_APPS: &str = "check-apps";
const MENU_QUIT: &str = "quit";

struct App {
    settings: Settings,
    update: Update,
    /// Why the current update state began.
    check_mode: CheckMode,
    /// True while the Discord-style startup loading splash is still waiting on the initial check.
    splash_active: bool,
    /// When the last automatic check (startup or 5-minute periodic) ran.
    last_bg_check: Option<Instant>,
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
    #[cfg(windows)]
    {
        let fixed = settings::app_dir().join("WebView2");
        if fixed.join("msedgewebview2.exe").exists() {
            std::env::set_var("WEBVIEW2_BROWSER_EXECUTABLE_FOLDER", &fixed);
        }
    }

    let mut settings = Settings::load();
    let pat_removed_externally = github_auth::init_from_store(&mut settings);
    if pat_removed_externally || github_auth::current_token().is_none() {
        let pruned = apps::prune_uninstalled_private_apps(&mut settings);
        if !pruned.is_empty() || pat_removed_externally {
            let _ = settings.save();
        }
    }
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let mut window_builder = WindowBuilder::new()
        .with_title("Launcher")
        .with_inner_size(LogicalSize::new(1120.0, 720.0))
        .with_min_inner_size(LogicalSize::new(720.0, 480.0))
        .with_decorations(false)
        .with_visible(false)
        .with_window_icon(Icon::from_rgba(ICON_RGBA.to_vec(), 64, 64).ok());
    #[cfg(windows)]
    {
        window_builder = window_builder
            .with_window_classname(instance::WINDOW_CLASS)
            .with_undecorated_shadow(true);
    }
    // A real titled window under a see-through title bar: AppKit draws the traffic
    // lights (hover glyphs, the zoom button's tiling menu), the rounded corners,
    // the shadow and the edge resizing, and the page runs up underneath.
    #[cfg(target_os = "macos")]
    {
        window_builder = window_builder
            .with_decorations(true)
            .with_titlebar_transparent(true)
            .with_title_hidden(true)
            .with_fullsize_content_view(true)
            .with_traffic_light_inset(tao::dpi::LogicalPosition::new(18.0, 20.0));
    }
    let window = window_builder.build(&event_loop).expect("create window");

    let mut web_context = WebContext::new(Some(data_dir().join("webview")));
    let ipc_proxy = proxy.clone();
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut webview_builder = WebViewBuilder::new_with_web_context(&mut web_context)
        .with_html(page_html(&settings))
        .with_background_color(settings.paper())
        .with_devtools(cfg!(debug_assertions))
        .with_ipc_handler(move |req| {
            let _ = ipc_proxy.send_event(UserEvent::Ipc(req.into_body()));
        });
    #[cfg(windows)]
    {
        webview_builder = webview_builder.with_browser_accelerator_keys(false);
    }
    let webview = webview_builder.build(&window);

    let webview = match webview {
        Ok(w) => w,
        Err(e) => {
            missing_webview2(&e.to_string());
            return;
        }
    };

    let library = Library::new(
        proxy.clone(),
        settings.apps_root(),
        settings.simultaneous_downloads,
        &settings.custom_apps,
    );
    let revealer = proxy.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(3));
        let _ = revealer.send_event(UserEvent::Reveal);
    });
    let ticker = proxy.clone();
    std::thread::spawn(move || {
        while ticker.send_event(UserEvent::Tick).is_ok() {
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
    });
    #[cfg(target_os = "macos")]
    let menu = menu_bar(proxy.clone());
    let mut app = App {
        settings,
        update: Update::Idle,
        check_mode: CheckMode::Manual,
        splash_active: false,
        last_bg_check: None,
        library,
        proxy,
    };

    event_loop.run(move |event, _, flow| {
        *flow = ControlFlow::Wait;
        // Keeps the context (and so the WebView2 profile folder) alive for the loop.
        let _ = &web_context;

        match event {
            Event::NewEvents(StartCause::Init) => {
                // NSApp is only finished once the loop is running.
                #[cfg(target_os = "macos")]
                menu.init_for_nsapp();
            }
            Event::UserEvent(UserEvent::Menu(id)) => {
                window.set_visible(true);
                window.set_minimized(false);
                window.set_focus();
                match id.as_str() {
                    MENU_CHECK_LAUNCHER => {
                        send(&webview, json!({ "type": "menuCheck", "check": "launcher", "page": "settings" }));
                        app.start_check(CheckMode::Manual);
                        // A check already under way, or an update already down,
                        // still gets reported to the page that's now waiting on it.
                        app.push_update(&webview);
                    }
                    MENU_CHECK_APPS => {
                        send(&webview, json!({ "type": "menuCheck", "check": "apps", "page": "library" }));
                        app.library.check_all(app.settings.prereleases);
                        send(&webview, app.library.snapshot());
                    }
                    MENU_QUIT => {
                        app.finish_on_exit();
                        *flow = ControlFlow::Exit;
                    }
                    _ => {}
                }
            }
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
            Event::UserEvent(UserEvent::Update { update, mode }) => {
                app.on_update(update, mode, &webview);
            }
            Event::UserEvent(UserEvent::App(id, event)) => {
                app.library.on_event(&id, event);
                send(&webview, app.library.snapshot());
            }
            Event::UserEvent(UserEvent::Auth(event)) => {
                app.on_auth_event(event, &webview);
            }
            Event::UserEvent(UserEvent::RepoInspected(result)) => {
                match result {
                    Ok(preview) => {
                        if app.library.has_repo(&preview.repo) {
                            send(
                                &webview,
                                json!({
                                    "type": "repoPreviewError",
                                    "message": format!("{} is already in your launcher.", preview.repo),
                                }),
                            );
                        } else {
                            send(&webview, json!({ "type": "repoPreview", "preview": preview }));
                        }
                    }
                    Err(message) => {
                        send(&webview, json!({ "type": "repoPreviewError", "message": message }));
                    }
                }
            }
            Event::UserEvent(UserEvent::PickedInstallDir(picked)) => {
                if let Some(dir) = picked.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
                    app.settings.install_dir = Some(dir);
                    let _ = app.settings.save();
                    app.library.set_install_root(app.settings.apps_root());
                    send(&webview, json!({ "type": "settings", "settings": app.settings }));
                    send(&webview, app.library.snapshot());
                }
            }
            Event::UserEvent(UserEvent::Reveal) => {
                if !window.is_visible() {
                    window.set_visible(true);
                    window.set_focus();
                }
            }
            Event::UserEvent(UserEvent::Tick) => {
                if app.library.refresh_running() {
                    send(&webview, app.library.snapshot());
                }
                if app.settings.checks_automatically()
                    && app.last_bg_check.is_some_and(|t| t.elapsed() >= AUTO_CHECK_INTERVAL)
                {
                    app.last_bg_check = Some(Instant::now());
                    app.start_check(CheckMode::Background);
                    app.library.check_all(app.settings.prereleases);
                    send(&webview, json!({ "type": "bgCheck" }));
                    send(&webview, app.library.snapshot());
                }
            }
            _ => {}
        }
    });
}

/// The menu bar. The first submenu is the app menu, shown under the app's name;
/// Edit is there because WebKit only gets Cmd+C/V/X/A/Z through menu items.
#[cfg(target_os = "macos")]
fn menu_bar(proxy: EventLoopProxy<UserEvent>) -> muda::Menu {
    use muda::accelerator::{Accelerator, Code, Modifiers};
    use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};

    let quit = Accelerator::new(Modifiers::META, Code::KeyQ);
    let app_menu = Submenu::with_items(
        "Launcher",
        true,
        &[
            &MenuItem::with_id(MENU_CHECK_LAUNCHER, "Check for Updates…", true, None),
            &MenuItem::with_id(MENU_CHECK_APPS, "Check for Software Updates…", true, None),
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id(MENU_QUIT, "Quit Launcher", true, Some(quit)),
        ],
    )
    .expect("app menu");
    let edit_menu = Submenu::with_items(
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(None),
            &PredefinedMenuItem::redo(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::cut(None),
            &PredefinedMenuItem::copy(None),
            &PredefinedMenuItem::paste(None),
            &PredefinedMenuItem::select_all(None),
        ],
    )
    .expect("edit menu");
    let menu = Menu::with_items(&[&app_menu, &edit_menu]).expect("menu bar");

    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        let _ = proxy.send_event(UserEvent::Menu(e.id.0));
    }));
    menu
}

/// The page is inlined into one document with the theme already stamped on, so a
/// dark install never flashes light on the way in.
fn page_html(settings: &Settings) -> String {
    let default_apps_dir = app_dir().join(updater::APPS_DIR);
    let boot = json!({
        "version": updater::VERSION,
        "repo": updater::REPO,
        "githubOrgUrl": "https://github.com/JavacrafterDevelopment",
        "os": apps::CURRENT_OS,
        "settings": settings,
        "auth": AuthStatus::from_settings(settings),
        "dataDir": data_dir().display().to_string(),
        "defaultAppsDir": default_apps_dir.display().to_string(),
        "startupCheck": settings.auto_update && settings.checks_automatically(),
        "justUpdated": std::env::args().any(|a| a == "--updated"),
        "icons": apps::icons(),
    });
    let html_lang = match settings.language_code() {
        "ru" => "ru",
        "es" => "es",
        _ => "en",
    };
    INDEX_HTML
        .replace("{{LANG}}", html_lang)
        .replace("{{LOOK}}", settings.look_name())
        .replace("{{THEME}}", if settings.is_dark() { "dark" } else { "light" })
        .replace("{{PALETTE}}", settings.palette())
        .replace("{{OS}}", apps::CURRENT_OS)
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
                self.last_bg_check = Some(Instant::now());
                self.push_update(webview);
                if self.settings.auto_update && self.settings.checks_automatically() {
                    self.splash_active = true;
                    self.start_check(CheckMode::Startup);
                } else {
                    self.splash_active = false;
                }
                // App updates are always looked for when automatic checks are on, never installed unasked.
                self.library.sweep();
                if self.settings.checks_automatically() {
                    self.library.check_all(self.settings.prereleases);
                }
                self.push_auth(webview);
                self.spawn_list_credentials();
                if github_auth::current_token().is_some() {
                    self.spawn_fetch_private_repos();
                }
                send(webview, self.library.snapshot());
            }
            "dismissSplash" => {
                self.splash_active = false;
            }
            "appCheck" | "appInstall" | "appUpdateAll" | "appCancelQueue" | "appLaunch" | "appLaunchBat" | "appFolder"
            | "appUninstall" | "appVerify" | "appRepair" | "appStop" => {
                let id = msg["id"].as_str().unwrap_or_default();
                match cmd {
                    "appCheck" => {
                        self.last_bg_check = Some(Instant::now());
                        self.library.check_all(self.settings.prereleases);
                        self.settings.last_check = Some(now_secs());
                        let _ = self.settings.save();
                        send(webview, json!({ "type": "settings", "settings": self.settings }));
                    }
                    "appInstall" => self.library.install(id),
                    "appUpdateAll" => self.library.update_all(),
                    "appCancelQueue" => self.library.cancel_queued(id),
                    "appLaunch" => self.library.launch(id),
                    "appLaunchBat" => self.library.launch_bat(id),
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
                    self.settings.theme = match t {
                        "dark" => "dark".into(),
                        "system" => "system".into(),
                        _ => "light".into(),
                    };
                }
                if let Some(l) = msg["look"].as_str().filter(|l| settings::LOOKS.contains(l)) {
                    self.settings.look = l.into();
                }
                if let Some(lang) = msg["language"].as_str().filter(|l| settings::LANGUAGES.contains(l)) {
                    self.settings.language = lang.into();
                }
                if let Some(c) = msg["checkEvery"].as_str() {
                    self.settings.check_every = if c == "manual" { "manual".into() } else { "auto".into() };
                }
                if let Some(b) = msg["autoUpdate"].as_bool() {
                    self.settings.auto_update = b;
                }
                if let Some(n) = msg["simultaneousDownloads"].as_u64() {
                    let clamped = if n == 0 { 0 } else { n.min(4) as u8 };
                    self.settings.simultaneous_downloads = clamped;
                    self.library.set_max_downloads(clamped);
                }
                if let Some(b) = msg["notifyAppUpdates"].as_bool() {
                    self.settings.notify_app_updates = b;
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
                if let Some(v) = msg["view"].as_str().filter(|v| settings::VIEWS.contains(v)) {
                    self.settings.view = v.into();
                }
                if let Some(s) = msg["sort"].as_str().filter(|s| settings::SORTS.contains(s)) {
                    self.settings.sort = s.into();
                }
                if let Some(arr) = msg["pinned"].as_array() {
                    self.settings.pinned = arr
                        .iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect();
                }
                let _ = self.settings.save();
                send(webview, json!({ "type": "settings", "settings": self.settings }));
                send(webview, self.library.snapshot());
            }
            "browseInstallDir" => {
                let proxy = self.proxy.clone();
                let initial = Some(self.settings.apps_root().display().to_string());
                std::thread::spawn(move || {
                    let picked = pick_folder(initial.as_deref());
                    let _ = proxy.send_event(UserEvent::PickedInstallDir(picked));
                });
            }
            "resetInstallDir" => {
                self.settings.install_dir = None;
                let _ = self.settings.save();
                self.library.set_install_root(self.settings.apps_root());
                send(webview, json!({ "type": "settings", "settings": self.settings }));
                send(webview, self.library.snapshot());
            }
            "checkUpdate" => self.start_check(CheckMode::Manual),
            "downloadUpdate" => {
                if let Update::Available { release } = &self.update {
                    self.start_download(release.clone(), self.check_mode);
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
                        open_in_shell(url);
                    }
                }
            }
            "openDataDir" => {
                let _ = std::fs::create_dir_all(data_dir());
                open_in_shell(data_dir());
            }
            "githubLoginPat" => {
                let token = msg["token"].as_str().unwrap_or_default().trim().to_string();
                let proxy = self.proxy.clone();
                std::thread::spawn(move || {
                    match github_auth::fetch_user(&token) {
                        Ok(user) => match github_auth::save_active_token(&token, &user.login) {
                            Ok(()) => {
                                let _ = proxy.send_event(UserEvent::Auth(AuthEvent::Connected(user)));
                            }
                            Err(e) => {
                                let _ = proxy.send_event(UserEvent::Auth(AuthEvent::Error(e)));
                            }
                        },
                        Err(e) => {
                            let _ = proxy.send_event(UserEvent::Auth(AuthEvent::Error(e)));
                        }
                    }
                });
            }
            "githubLogout" => {
                let account = self.settings.github_user.clone();
                let _ = github_auth::sign_out(account.as_deref());
                self.handle_pat_removed(webview);
                self.spawn_list_credentials();
            }
            "githubListCredentials" => {
                self.spawn_list_credentials();
            }
            "githubDeleteCredential" => {
                let key = msg["key"].as_str().unwrap_or_default().to_string();
                let account = msg["account"].as_str().map(String::from);
                let proxy = self.proxy.clone();
                std::thread::spawn(move || {
                    let res = github_auth::delete_credential(&key, account.as_deref());
                    let (result, signed_out) = match res {
                        Ok(was_current) => (Ok(1), was_current),
                        Err(e) => (Err(e), false),
                    };
                    let _ = proxy.send_event(UserEvent::Auth(AuthEvent::CredentialsCleared {
                        scope: "single",
                        result,
                        signed_out,
                    }));
                });
            }
            "githubClearOldCredentials" => {
                let proxy = self.proxy.clone();
                std::thread::spawn(move || {
                    let result = github_auth::clear_old_credentials();
                    let _ = proxy.send_event(UserEvent::Auth(AuthEvent::CredentialsCleared {
                        scope: "old",
                        result,
                        signed_out: false,
                    }));
                });
            }
            "githubClearAllCredentials" => {
                let proxy = self.proxy.clone();
                std::thread::spawn(move || {
                    let result = github_auth::clear_all_credentials();
                    let _ = proxy.send_event(UserEvent::Auth(AuthEvent::CredentialsCleared {
                        scope: "all",
                        result,
                        signed_out: true,
                    }));
                });
            }
            "githubFetchPrivateRepos" => {
                self.spawn_fetch_private_repos();
            }
            "customAppInspect" => {
                let input = msg["input"].as_str().unwrap_or_default().to_string();
                let prereleases = self.settings.prereleases;
                let proxy = self.proxy.clone();
                std::thread::spawn(move || {
                    let res = github_auth::inspect_repo(&input, prereleases);
                    let _ = proxy.send_event(UserEvent::RepoInspected(res));
                });
            }
            "customAppAdd" => {
                let repo = msg["repo"].as_str().unwrap_or_default().trim().to_string();
                if repo.is_empty() {
                    return false;
                }
                if self.library.has_repo(&repo) {
                    send(
                        webview,
                        json!({
                            "type": "repoPreviewError",
                            "message": format!("{repo} is already in your launcher."),
                        }),
                    );
                    return false;
                }
                let fallback_name = repo.rsplit('/').next().unwrap_or("App");
                let name = msg["name"]
                    .as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or(fallback_name)
                    .to_string();
                let owner = msg["publisher"]
                    .as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| repo.split('/').next().unwrap_or("GitHub"))
                    .to_string();
                let tagline = msg["tagline"].as_str().unwrap_or_default().trim().to_string();
                let category = CustomApp::normalize_category(msg["category"].as_str().unwrap_or("apps"));
                let platforms: Vec<String> = msg["platforms"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .filter(|v: &Vec<String>| !v.is_empty())
                    .unwrap_or_else(|| vec!["Windows".into(), "MacOS".into()]);
                let private = msg["private"].as_bool().unwrap_or(false);
                let own_format = msg["ownFormat"].as_bool().unwrap_or(false);

                let existing_ids = self.library.existing_ids();
                let existing_folders = self.library.existing_folders();
                let id = CustomApp::make_id(&repo, &existing_ids);
                let folder = CustomApp::make_folder(&name, &owner, &existing_folders);

                let custom = CustomApp {
                    id: id.clone(),
                    name: name.clone(),
                    folder,
                    repo,
                    tagline,
                    publisher: owner,
                    category: category.clone(),
                    platforms,
                    private,
                    own_format,
                };
                self.settings.custom_apps.push(custom);
                let _ = self.settings.save();
                self.library.sync_custom_apps(&self.settings.custom_apps, self.settings.prereleases);
                send(webview, json!({ "type": "settings", "settings": self.settings }));
                send(webview, self.library.snapshot());
                send(
                    webview,
                    json!({ "type": "customAppAdded", "id": id, "name": name, "category": category }),
                );
            }
            "customAppMoveCategory" => {
                let id = msg["id"].as_str().unwrap_or_default();
                let category = CustomApp::normalize_category(msg["category"].as_str().unwrap_or("apps"));
                if let Some(c) = self.settings.custom_apps.iter_mut().find(|c| c.id == id) {
                    c.category = category;
                    let _ = self.settings.save();
                    self.library.sync_custom_apps(&self.settings.custom_apps, self.settings.prereleases);
                    send(webview, json!({ "type": "settings", "settings": self.settings }));
                    send(webview, self.library.snapshot());
                }
            }
            "customAppRemove" => {
                let id = msg["id"].as_str().unwrap_or_default();
                if self.library.is_installed(id) {
                    let name = self.library.app_name(id).unwrap_or_else(|| id.to_string());
                    send(webview, self.library.snapshot());
                    send(
                        webview,
                        json!({
                            "type": "customAppRemoveBlocked",
                            "id": id,
                            "name": name,
                            "message": format!("{name} is still installed. Uninstall it first before removing it from Launcher."),
                        }),
                    );
                    return false;
                }
                let before = self.settings.custom_apps.len();
                self.settings.custom_apps.retain(|c| c.id != id);
                self.settings.pinned.retain(|p| p != id);
                if self.settings.custom_apps.len() != before {
                    let _ = self.settings.save();
                    self.library.sync_custom_apps(&self.settings.custom_apps, self.settings.prereleases);
                    send(webview, json!({ "type": "settings", "settings": self.settings }));
                    send(webview, self.library.snapshot());
                }
            }
            _ => {}
        }
        false
    }

    fn push_auth(&self, webview: &WebView) {
        send(
            webview,
            json!({
                "type": "githubAuth",
                "auth": AuthStatus::from_settings(&self.settings),
            }),
        );
    }

    fn handle_pat_removed(&mut self, webview: &WebView) {
        self.settings.github_user = None;
        self.settings.github_avatar = None;
        let pruned = apps::prune_uninstalled_private_apps(&mut self.settings);
        let _ = self.settings.save();
        if !pruned.is_empty() {
            self.library.sync_custom_apps(&self.settings.custom_apps, self.settings.prereleases);
        }
        send(webview, json!({ "type": "settings", "settings": self.settings }));
        self.push_auth(webview);
        send(webview, json!({ "type": "githubPrivateRepos", "repos": [] }));
        if !pruned.is_empty() {
            send(webview, self.library.snapshot());
            send(webview, json!({ "type": "privateAppsPruned", "names": pruned }));
        }
    }

    fn spawn_list_credentials(&self) {
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let res = github_auth::list_credentials();
            let _ = proxy.send_event(UserEvent::Auth(AuthEvent::CredentialsListed(res)));
        });
    }

    fn spawn_fetch_private_repos(&self) {
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let res = github_auth::fetch_private_repos();
            let _ = proxy.send_event(UserEvent::Auth(AuthEvent::PrivateRepos(res)));
        });
    }

    fn on_auth_event(&mut self, event: AuthEvent, webview: &WebView) {
        match event {
            AuthEvent::Connected(user) => {
                self.settings.github_user = Some(user.login);
                self.settings.github_avatar = user.avatar_url;
                let _ = self.settings.save();
                send(webview, json!({ "type": "settings", "settings": self.settings }));
                self.push_auth(webview);
                self.spawn_list_credentials();
                self.spawn_fetch_private_repos();
                // Immediately re-check any private repos or previously failed checks.
                self.library.recheck_private_or_failed(self.settings.prereleases);
                send(webview, self.library.snapshot());
            }
            AuthEvent::Error(message) => {
                send(webview, json!({ "type": "githubAuthError", "message": message }));
            }
            AuthEvent::CredentialsListed(res) => match res {
                Ok(credentials) => {
                    send(webview, json!({ "type": "githubCredentials", "credentials": credentials }));
                }
                Err(message) => {
                    send(webview, json!({ "type": "githubCredentialsError", "message": message }));
                }
            },
            AuthEvent::CredentialsCleared { scope, result, signed_out } => {
                if signed_out {
                    self.handle_pat_removed(webview);
                }
                match result {
                    Ok(removed) => {
                        send(
                            webview,
                            json!({
                                "type": "githubCredentialsCleared",
                                "scope": scope,
                                "removed": removed,
                            }),
                        );
                    }
                    Err(message) => {
                        send(webview, json!({ "type": "githubCredentialsError", "message": message }));
                    }
                }
                self.spawn_list_credentials();
            }
            AuthEvent::PrivateRepos(res) => match res {
                Ok(repos) => {
                    send(webview, json!({ "type": "githubPrivateRepos", "repos": repos }));
                }
                Err(message) => {
                    send(webview, json!({ "type": "githubPrivateReposError", "message": message }));
                }
            },
        }
    }

    fn on_update(&mut self, update: Update, mut mode: CheckMode, webview: &WebView) {
        if matches!(update, Update::UpToDate | Update::Available { .. }) {
            self.settings.last_check = Some(now_secs());
            let _ = self.settings.save();
            send(webview, json!({ "type": "settings", "settings": self.settings }));
        }
        // If the startup splash already closed (slow network / user skipped), downgrade a
        // late startup check to a non-intrusive background notification rather than rebooting.
        if mode == CheckMode::Startup && !self.splash_active && matches!(update, Update::Available { .. }) {
            mode = CheckMode::Background;
        }
        // Only a live startup splash automatically downloads & restarts; background 5-minute
        // checks notify via bottom-right popup instead.
        let carry_on = match (&update, mode, self.splash_active) {
            (Update::Available { release }, CheckMode::Startup, true) => Some(release.clone()),
            _ => None,
        };
        if matches!(update, Update::UpToDate | Update::Error { .. }) && mode == CheckMode::Startup {
            self.splash_active = false;
        }
        self.update = update;
        self.check_mode = mode;
        self.push_update(webview);
        if let Some(release) = carry_on {
            self.start_download(release, CheckMode::Startup);
        }
    }

    fn push_update(&self, webview: &WebView) {
        send(
            webview,
            json!({
                "type": "update",
                "update": self.update,
                "mode": self.check_mode,
                "auto": self.check_mode == CheckMode::Startup,
            }),
        );
    }

    fn start_check(&mut self, mode: CheckMode) {
        if self.update.busy() || matches!(self.update, Update::Ready { .. }) {
            return;
        }
        let prereleases = self.settings.prereleases;
        let proxy = self.proxy.clone();
        self.update = Update::Checking;
        self.check_mode = mode;
        let _ = proxy.send_event(UserEvent::Update { update: Update::Checking, mode });
        std::thread::spawn(move || {
            let current = updater::current_version();
            let update = match Target::launcher().latest(prereleases) {
                Ok(Some(release)) if release.semver().is_some_and(|v| v > current) => {
                    Update::Available { release }
                }
                Ok(_) => Update::UpToDate,
                Err(message) => Update::Error { message },
            };
            let _ = proxy.send_event(UserEvent::Update { update, mode });
        });
    }

    fn start_download(&mut self, release: Release, mode: CheckMode) {
        if self.update.busy() {
            return;
        }
        let proxy = self.proxy.clone();
        let total = release.size;
        self.update = Update::Downloading { release: release.clone(), done: 0, total };
        self.check_mode = mode;
        std::thread::spawn(move || {
            let mut last_pct = u64::MAX;
            let progress = |done, total| {
                // One message per percent is plenty for a progress bar.
                let pct = if total > 0 { done * 100 / total } else { done / 1_000_000 };
                if pct != last_pct {
                    last_pct = pct;
                    let _ = proxy.send_event(UserEvent::Update {
                        update: Update::Downloading { release: release.clone(), done, total },
                        mode,
                    });
                }
            };
            let result = Target::launcher().download(&release, progress, &|_| {});
            let update = match result {
                Ok(_) => Update::Ready { release },
                Err(message) => Update::Error { message },
            };
            let _ = proxy.send_event(UserEvent::Update { update, mode });
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

/// Opens the native OS folder picker without adding extra crate dependencies.
#[cfg(windows)]
fn pick_folder(initial: Option<&str>) -> Option<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let init = initial.unwrap_or("").replace('\'', "''");
    let script = format!(
        "Add-Type -AssemblyName System.Windows.Forms; \
         $d = New-Object System.Windows.Forms.FolderBrowserDialog; \
         $d.Description = 'Choose default install folder for apps'; \
         $d.UseDescriptionForTitle = $true; \
         if ('{init}' -and (Test-Path -LiteralPath '{init}')) {{ $d.SelectedPath = '{init}' }}; \
         if ($d.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {{ [Console]::Out.Write($d.SelectedPath) }}"
    );
    let out = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-STA", "-WindowStyle", "Hidden", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}

#[cfg(target_os = "macos")]
fn pick_folder(_initial: Option<&str>) -> Option<String> {
    let out = std::process::Command::new("osascript")
        .args(["-e", "POSIX path of (choose folder with prompt \"Choose default install folder\")"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().trim_end_matches('/').to_string();
    (!s.is_empty()).then_some(s)
}

#[cfg(all(not(windows), not(target_os = "macos")))]
fn pick_folder(_initial: Option<&str>) -> Option<String> {
    None
}

pub(crate) fn open_in_shell(target: impl AsRef<std::ffi::OsStr>) {
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(not(target_os = "macos"))]
    let program = "explorer";

    let _ = std::process::Command::new(program).arg(target).spawn();
}

#[cfg(windows)]
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
        open_in_shell("https://developer.microsoft.com/microsoft-edge/webview2/");
    }
}

#[cfg(not(windows))]
fn missing_webview2(detail: &str) {
    eprintln!("Launcher couldn't start the system WebView ({detail})");
}
