//! Everything the launcher keeps lives in `data\` beside the executable, so the
//! folder can be carried around on a stick and nothing is written elsewhere.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::{fs, io};

/// The folder the running executable (or `.app` bundle on macOS) sits in.
///
/// On macOS, opening a quarantined `.app` extracted from a `.zip` runs it from a
/// read-only Gatekeeper translocation mount (`/private/var/folders/.../AppTranslocation/<UUID>/d/Launcher.app`).
/// We resolve that mount back to the real `.app` on disk and strip `com.apple.quarantine`
/// so portable folders can self-update and store `data/` beside `Launcher.app`.
pub fn install_dir() -> PathBuf {
    static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    DIR.get_or_init(resolve_install_dir).clone()
}

fn resolve_install_dir() -> PathBuf {
    let Some(exe) = std::env::current_exe().ok() else {
        return PathBuf::from(".");
    };
    #[cfg(target_os = "macos")]
    {
        for ancestor in exe.ancestors() {
            if ancestor.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("app")) {
                let bundle = untranslocate_bundle(ancestor);
                if let Some(parent) = bundle.parent() {
                    return parent.to_path_buf();
                }
            }
        }
    }
    exe.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(target_os = "macos")]
fn untranslocate_bundle(bundle: &Path) -> PathBuf {
    let s = bundle.to_string_lossy();
    if !s.contains("/AppTranslocation/") {
        return bundle.to_path_buf();
    }
    // Mountpoint is the parent of `d/<App>.app`: `.../AppTranslocation/<UUID>`.
    let Some(mnt) = bundle.parent().and_then(Path::parent) else {
        return bundle.to_path_buf();
    };
    let needle = format!(" on {} (", mnt.to_string_lossy());
    let Ok(out) = std::process::Command::new("mount").output() else {
        return bundle.to_path_buf();
    };
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        if let Some((from, _)) = line.split_once(&needle) {
            let orig = PathBuf::from(from);
            if orig.exists() {
                let target = orig.parent().unwrap_or(&orig);
                let _ = std::process::Command::new("xattr")
                    .args(["-dr", "com.apple.quarantine"])
                    .arg(target)
                    .status();
                return orig;
            }
        }
    }
    bundle.to_path_buf()
}

/// True when `dir` is on a writable filesystem (not a read-only DMG or translocation mount).
pub fn is_dir_writable(dir: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        let s = dir.to_string_lossy();
        if s.starts_with("/private/var/folders/") || s.starts_with("/Volumes/") {
            return false;
        }
    }
    fs::metadata(dir).map(|m| !m.permissions().readonly()).unwrap_or(false)
}

/// Where `data/` and `portableApplications/` live. On Windows and in a portable
/// macOS folder this is right beside `Launcher.exe` / `Launcher.app`; if someone
/// dragged `Launcher.app` into `/Applications` or opened it from a read-only DMG,
/// falls back to `~/Library/Application Support/Launcher`.
pub fn app_dir() -> PathBuf {
    let dir = install_dir();
    #[cfg(target_os = "macos")]
    {
        let s = dir.to_string_lossy();
        let in_apps = s == "/Applications" || s.ends_with("/Applications");
        if in_apps || !is_dir_writable(&dir) {
            if let Ok(home) = std::env::var("HOME") {
                return PathBuf::from(home).join("Library/Application Support/Launcher");
            }
        }
    }
    dir
}

pub fn data_dir() -> PathBuf {
    app_dir().join("data")
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// "light", "dark", or "system".
    pub theme: String,
    /// "modern" (ArtCraft-inspired) or "legacy" (original reading-room look).
    pub look: String,
    /// Interface language: "auto" (follow system), "en", "es", or "ru".
    pub language: String,
    /// "auto" (check on launch and every 5 minutes) or "manual" (only when asked).
    pub check_every: String,
    /// Check GitHub on launch (Discord-style loading splash), and if a newer launcher
    /// release is out, download and restart into it.
    pub auto_update: bool,
    /// Maximum concurrent app downloads (1–4, or 0 for no limit).
    pub simultaneous_downloads: u8,
    /// Show a bottom-right popup when a background check finds an installed app update.
    pub notify_app_updates: bool,
    /// Consider pre-releases when looking for a newer version.
    pub prereleases: bool,
    /// Custom folder where apps are installed; None uses `portableApplications/` beside Launcher.
    pub install_dir: Option<String>,
    /// Unix seconds of the last successful check, shown in settings and footer.
    pub last_check: Option<u64>,
    /// Colour scheme used in Legacy light mode; one of [`LIGHT_SCHEMES`].
    pub light_scheme: String,
    /// Colour scheme used in Legacy dark mode; one of [`DARK_SCHEMES`].
    pub dark_scheme: String,
    /// "grid" or "table".
    pub view: String,
    /// "name-asc", "name-desc", "status", or "updated".
    pub sort: String,
    /// App IDs pinned to the top of their tab.
    pub pinned: Vec<String>,
}

/// The colour schemes ui/app.css defines for Legacy look, the default first. Kept in data\
/// beside the exe like every other setting, so a choice survives updates.
pub const LIGHT_SCHEMES: &[&str] = &["walnut", "sage", "slate", "rose"];
pub const DARK_SCHEMES: &[&str] = &["gold", "ocean", "forest", "plum"];
pub const LOOKS: &[&str] = &["modern", "legacy"];
pub const LANGUAGES: &[&str] = &["auto", "en", "es", "ru"];
pub const VIEWS: &[&str] = &["grid", "table"];
pub const SORTS: &[&str] = &["name-asc", "name-desc", "status", "updated"];

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "light".into(),
            look: "modern".into(),
            language: "auto".into(),
            check_every: "auto".into(),
            auto_update: true,
            simultaneous_downloads: 1,
            notify_app_updates: true,
            prereleases: false,
            install_dir: None,
            last_check: None,
            light_scheme: LIGHT_SCHEMES[0].into(),
            dark_scheme: DARK_SCHEMES[0].into(),
            view: "grid".into(),
            sort: "name-asc".into(),
            pinned: Vec::new(),
        }
    }
}

impl Settings {
    fn path() -> PathBuf {
        data_dir().join("settings.json")
    }

    /// Missing or unreadable settings fall back to defaults rather than failing:
    /// a launcher that refuses to open over a bad JSON file is no launcher.
    pub fn load() -> Self {
        fs::read(Self::path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    /// Written to a temp file and renamed over, so a crash mid-write never
    /// leaves half a settings file behind.
    pub fn save(&self) -> io::Result<()> {
        let path = Self::path();
        fs::create_dir_all(data_dir())?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        fs::rename(tmp, path)
    }

    pub fn is_dark(&self) -> bool {
        self.theme == "dark"
    }

    pub fn look_name(&self) -> &str {
        if self.look == "legacy" { "legacy" } else { "modern" }
    }

    pub fn language_code(&self) -> &str {
        LANGUAGES.iter().find(|l| **l == self.language.as_str()).copied().unwrap_or("auto")
    }

    pub fn is_modern(&self) -> bool {
        self.look_name() == "modern"
    }

    pub fn checks_automatically(&self) -> bool {
        self.check_every != "manual"
    }

    /// Root folder where installed apps live: custom `install_dir` if set, otherwise
    /// `portableApplications/` beside the launcher.
    pub fn apps_root(&self) -> PathBuf {
        self.install_dir
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| app_dir().join(crate::updater::APPS_DIR))
    }

    /// The scheme for whichever mode is on, falling back to the default if the
    /// saved name isn't one this version knows.
    pub fn palette(&self) -> &str {
        let (saved, known) = if self.is_dark() {
            (&self.dark_scheme, DARK_SCHEMES)
        } else {
            (&self.light_scheme, LIGHT_SCHEMES)
        };
        known.iter().find(|s| **s == saved.as_str()).copied().unwrap_or(known[0])
    }

    /// The page background for the look and scheme in use, so the window doesn't flash
    /// a different colour before the page paints.
    pub fn paper(&self) -> (u8, u8, u8, u8) {
        if self.is_modern() {
            return if self.is_dark() {
                (0x12, 0x13, 0x16, 0xFF)
            } else {
                (0xF2, 0xF1, 0xEE, 0xFF)
            };
        }
        match self.palette() {
            "sage" => (0xEE, 0xF1, 0xE8, 0xFF),
            "slate" => (0xEE, 0xF1, 0xF5, 0xFF),
            "rose" => (0xF6, 0xEE, 0xEE, 0xFF),
            _ if self.is_dark() => (0x12, 0x12, 0x12, 0xFF),
            _ => (0xF4, 0xEE, 0xE2, 0xFF),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A settings file written before colour schemes or modern look existed keeps its other
    /// choices and picks up clean defaults.
    #[test]
    fn older_file_gets_default_schemes() {
        let old = r#"{"theme":"dark","autoUpdate":false,"prereleases":true,"lastCheck":1700000000}"#;
        let s: Settings = serde_json::from_str(old).unwrap();
        assert_eq!((s.theme.as_str(), s.auto_update, s.prereleases), ("dark", false, true));
        assert_eq!(s.light_scheme, "walnut");
        assert_eq!(s.dark_scheme, "gold");
        assert_eq!(s.palette(), "gold");
        assert_eq!(s.look_name(), "modern");
        assert_eq!(s.language, "auto");
        assert_eq!(s.language_code(), "auto");
        assert_eq!(s.simultaneous_downloads, 1);
        assert!(s.notify_app_updates);
        assert!(s.checks_automatically());
    }

    /// A name this version doesn't know (say, from a newer launcher) falls back
    /// to the default instead of leaving the page unstyled.
    #[test]
    fn unknown_scheme_falls_back() {
        let s = Settings {
            look: "legacy".into(),
            language: "klingon".into(),
            light_scheme: "neon".into(),
            ..Settings::default()
        };
        assert_eq!(s.palette(), "walnut");
        assert_eq!(s.language_code(), "auto");
        assert_eq!(s.paper(), (0xF4, 0xEE, 0xE2, 0xFF));
    }

    #[test]
    fn modern_paper_colours() {
        let light = Settings { look: "modern".into(), theme: "light".into(), ..Settings::default() };
        assert_eq!(light.paper(), (0xF2, 0xF1, 0xEE, 0xFF));
        let dark = Settings { look: "modern".into(), theme: "dark".into(), ..Settings::default() };
        assert_eq!(dark.paper(), (0x12, 0x13, 0x16, 0xFF));
    }

    #[test]
    fn schemes_round_trip() {
        let s = Settings {
            look: "legacy".into(),
            language: "ru".into(),
            theme: "dark".into(),
            light_scheme: "rose".into(),
            dark_scheme: "plum".into(),
            simultaneous_downloads: 3,
            notify_app_updates: false,
            check_every: "manual".into(),
            pinned: vec!["lighthouse".into()],
            ..Settings::default()
        };
        let back: Settings = serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
        assert_eq!((back.light_scheme.as_str(), back.dark_scheme.as_str()), ("rose", "plum"));
        assert_eq!(back.language_code(), "ru");
        assert_eq!(back.palette(), "plum");
        assert_eq!(back.paper(), (0x12, 0x12, 0x12, 0xFF));
        assert_eq!(back.simultaneous_downloads, 3);
        assert!(!back.notify_app_updates);
        assert!(!back.checks_automatically());
        assert_eq!(back.pinned, vec!["lighthouse"]);
    }
}
