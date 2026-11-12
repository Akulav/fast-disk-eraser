//! Filesystem layout of the app. Everything lives in one folder: the install folder the user
//! chose (see [`super::install::data_root`]), next to the program itself:
//!
//! ```text
//! FastDiskEraser/
//!   settings.json     preferences
//!   history.json      past erases (counts and sizes only, never file names)
//!   window.ron        window size and position
//!   cache/update/     a downloaded update, until it is installed
//!   .fde-install      marks the install folder (holds the installed version)
//! ```

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Paths {
    pub root: PathBuf,
}

impl Paths {
    /// `FDE_DATA_DIR` overrides the location (testing).
    pub fn detect() -> anyhow::Result<Self> {
        if let Some(dir) = std::env::var_os("FDE_DATA_DIR").filter(|d| !d.is_empty()) {
            return Ok(Self { root: dir.into() });
        }
        let root = super::install::data_root().ok_or_else(|| anyhow::anyhow!("could not locate the app folder"))?;
        Ok(Self { root })
    }

    pub fn settings_file(&self) -> PathBuf {
        self.root.join("settings.json")
    }
    pub fn history_file(&self) -> PathBuf {
        self.root.join("history.json")
    }
    pub fn window_file(&self) -> PathBuf {
        self.root.join(super::install::WINDOW_STATE_FILE)
    }
}

/// Write a file atomically (write to a temp file next to it, then rename over the target).
pub fn write_atomic(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, path)
}
