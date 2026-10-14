//! Everything the launcher keeps lives in `data\` beside the executable, so the
//! folder can be carried around on a stick and nothing is written elsewhere.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::{fs, io};

/// The folder the running executable sits in.
pub fn app_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "light".into(),
            auto_update: true,
            prereleases: false,
            last_check: None,
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
}
