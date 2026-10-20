//! Everything the launcher keeps lives in `data\` beside the executable, so the
//! folder can be carried around on a stick and nothing is written elsewhere.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::{fs, io};

/// The folder the running executable (or `.app` bundle on macOS) sits in.
pub fn install_dir() -> PathBuf {
    let Some(exe) = std::env::current_exe().ok() else {
        return PathBuf::from(".");
    };
    #[cfg(target_os = "macos")]
    {
        for ancestor in exe.ancestors() {
            if ancestor.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("app")) {
                if let Some(parent) = ancestor.parent() {
                    return parent.to_path_buf();
                }
            }
        }
    }
    exe.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."))
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
        let translocated = s.starts_with("/private/var/folders/");
        let writable = fs::metadata(&dir).map(|m| !m.permissions().readonly()).unwrap_or(false);
        if in_apps || translocated || !writable {
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
    /// "light" or "dark".
    pub theme: String,
    /// Check GitHub on launch, and if a newer release is out, download and apply it.
    pub auto_update: bool,
    /// Consider pre-releases when looking for a newer version.
    pub prereleases: bool,
    /// Unix seconds of the last successful check, shown in settings.
    pub last_check: Option<u64>,
    /// Colour scheme used in light mode; one of [`LIGHT_SCHEMES`].
    pub light_scheme: String,
    /// Colour scheme used in dark mode; one of [`DARK_SCHEMES`].
    pub dark_scheme: String,
}

/// The colour schemes ui/app.css defines, the default first. Kept in data\
/// beside the exe like every other setting, so a choice survives updates.
pub const LIGHT_SCHEMES: &[&str] = &["walnut", "sage", "slate", "rose"];
pub const DARK_SCHEMES: &[&str] = &["gold", "ocean", "forest", "plum"];

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "light".into(),
            auto_update: true,
            prereleases: false,
            last_check: None,
            light_scheme: LIGHT_SCHEMES[0].into(),
            dark_scheme: DARK_SCHEMES[0].into(),
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

    /// The page background for the scheme in use, so the window doesn't flash
    /// a different colour before the page paints. Dark schemes share one.
    pub fn paper(&self) -> (u8, u8, u8, u8) {
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

    /// A settings file written before colour schemes existed keeps its other
    /// choices and opens in the original look.
    #[test]
    fn older_file_gets_default_schemes() {
        let old = r#"{"theme":"dark","autoUpdate":false,"prereleases":true,"lastCheck":1700000000}"#;
        let s: Settings = serde_json::from_str(old).unwrap();
        assert_eq!((s.theme.as_str(), s.auto_update, s.prereleases), ("dark", false, true));
        assert_eq!(s.light_scheme, "walnut");
        assert_eq!(s.dark_scheme, "gold");
        assert_eq!(s.palette(), "gold");
    }

    /// A name this version doesn't know (say, from a newer launcher) falls back
    /// to the default instead of leaving the page unstyled.
    #[test]
    fn unknown_scheme_falls_back() {
        let s = Settings { light_scheme: "neon".into(), ..Settings::default() };
        assert_eq!(s.palette(), "walnut");
        assert_eq!(s.paper(), (0xF4, 0xEE, 0xE2, 0xFF));
    }

    #[test]
    fn schemes_round_trip() {
        let s = Settings { theme: "dark".into(), light_scheme: "rose".into(), dark_scheme: "plum".into(), ..Settings::default() };
        let back: Settings = serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
        assert_eq!((back.light_scheme.as_str(), back.dark_scheme.as_str()), ("rose", "plum"));
        assert_eq!(back.palette(), "plum");
        assert_eq!(back.paper(), (0x12, 0x12, 0x12, 0xFF));
    }
}
