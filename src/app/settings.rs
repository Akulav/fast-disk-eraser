//! Preferences (`settings.json`) and the job history (`history.json`), both in the app folder.

use serde::{Deserialize, Serialize};

use super::paths::{write_atomic, Paths};
use crate::core::method::Method;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// Method for erasing files and folders.
    pub method: Method,
    /// Method for wiping free space.
    pub free_method: Method,
    pub verify: bool,
    pub rename: bool,
    /// Files erased at the same time.
    pub threads: usize,
    pub clean_file_table: bool,
    pub keep_history: bool,
    pub check_updates: bool,
    /// Require holding the confirm button instead of a single click.
    pub hold_to_confirm: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            method: Method::Random,
            free_method: Method::Random,
            verify: false,
            rename: true,
            threads: std::thread::available_parallelism().map(|n| n.get().min(4)).unwrap_or(2),
            clean_file_table: true,
            keep_history: true,
            check_updates: true,
            hold_to_confirm: true,
        }
    }
}

impl Settings {
    pub fn load(paths: &Paths) -> Self {
        std::fs::read_to_string(paths.settings_file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self, paths: &Paths) -> anyhow::Result<()> {
        write_atomic(&paths.settings_file(), serde_json::to_string_pretty(self)?.as_bytes())?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum JobKind {
    Files,
    FreeSpace,
}

/// One finished job. Deliberately holds no file names: a list of what you shredded would defeat
/// the point of shredding it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub when: chrono::DateTime<chrono::Local>,
    pub kind: JobKind,
    pub method: Method,
    /// Files erased (Files) or 0 (FreeSpace).
    pub items: u64,
    pub bytes: u64,
    pub seconds: f64,
    pub failures: u64,
    pub cancelled: bool,
    /// The drive, for free-space wipes (e.g. "C:\").
    #[serde(default)]
    pub drive: Option<String>,
}

const MAX_HISTORY: usize = 500;

pub fn load_history(paths: &Paths) -> Vec<HistoryEntry> {
    std::fs::read_to_string(paths.history_file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn save_history(paths: &Paths, history: &[HistoryEntry]) -> anyhow::Result<()> {
    let start = history.len().saturating_sub(MAX_HISTORY);
    write_atomic(&paths.history_file(), serde_json::to_string_pretty(&history[start..])?.as_bytes())?;
    Ok(())
}

pub fn clear_history(paths: &Paths) {
    let _ = std::fs::remove_file(paths.history_file());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_in_the_app_folder() {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths { root: d.path().to_path_buf() };
        let s = Settings { method: Method::Dod7, verify: true, ..Default::default() };
        s.save(&paths).unwrap();
        assert_eq!(Settings::load(&paths), s);
        let h = vec![HistoryEntry {
            when: chrono::Local::now(),
            kind: JobKind::Files,
            method: Method::Zero,
            items: 3,
            bytes: 10,
            seconds: 0.5,
            failures: 0,
            cancelled: false,
            drive: None,
        }];
        save_history(&paths, &h).unwrap();
        assert_eq!(load_history(&paths).len(), 1);
        // Unknown or missing fields fall back to defaults.
        std::fs::write(paths.settings_file(), r#"{"verify":true,"future":1}"#).unwrap();
        assert!(Settings::load(&paths).verify);
    }
}
