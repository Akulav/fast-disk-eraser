//! Mounted drives (volumes) the user can wipe free space on.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveKind {
    Ssd,
    Hdd,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct Drive {
    /// Display name, e.g. "Local Disk (C:)" or "Data".
    pub label: String,
    pub mount: PathBuf,
    pub fs: String,
    pub kind: DriveKind,
    pub removable: bool,
    pub read_only: bool,
    pub total: u64,
    pub available: u64,
}

impl Drive {
    pub fn used_fraction(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            (1.0 - self.available as f64 / self.total as f64).clamp(0.0, 1.0) as f32
        }
    }
}

const PSEUDO_FS: &[&str] = &[
    "tmpfs", "devtmpfs", "overlay", "squashfs", "proc", "sysfs", "efivarfs", "ramfs", "autofs", "devfs", "nullfs", "fuse.portal",
    "fuse.gvfsd-fuse", "cgroup", "cgroup2", "debugfs", "tracefs", "securityfs", "pstore", "bpf", "configfs", "mqueue", "hugetlbfs",
];

fn hidden(mount: &Path, fs: &str) -> bool {
    let m = mount.to_string_lossy();
    if PSEUDO_FS.iter().any(|p| fs.eq_ignore_ascii_case(p)) {
        return true;
    }
    if cfg!(target_os = "linux") {
        let skip = ["/proc", "/sys", "/dev", "/snap", "/boot", "/run/user", "/run/snapd", "/var/snap", "/var/lib/docker"];
        return skip.iter().any(|s| m == *s || m.starts_with(&format!("{s}/")));
    }
    if cfg!(target_os = "macos") {
        // The sealed system volume and the helper volumes are read-only or not for users.
        return m == "/" || (m.starts_with("/System/Volumes/") && m != "/System/Volumes/Data") || m.starts_with("/Library/Developer");
    }
    false
}

fn display_label(name: &str, mount: &Path) -> String {
    let m = mount.to_string_lossy().to_string();
    if cfg!(windows) {
        let letter = m.trim_end_matches('\\');
        let name = if name.trim().is_empty() { "Local Disk" } else { name.trim() };
        return format!("{name} ({letter})");
    }
    if cfg!(target_os = "macos") && m == "/System/Volumes/Data" {
        return "Macintosh HD".into();
    }
    if m == "/" {
        return "System (/)".into();
    }
    if name.trim().is_empty() || name.starts_with("/dev/") {
        mount.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or(m)
    } else {
        name.trim().to_string()
    }
}

/// Every mounted drive worth showing, sorted by mount point.
pub fn list() -> Vec<Drive> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let mut out: Vec<Drive> = Vec::new();
    for d in disks.list() {
        let fs = d.file_system().to_string_lossy().to_string();
        let mount = d.mount_point().to_path_buf();
        if d.total_space() == 0 || hidden(&mount, &fs) || out.iter().any(|o| o.mount == mount) {
            continue;
        }
        out.push(Drive {
            label: display_label(&d.name().to_string_lossy(), &mount),
            kind: match d.kind() {
                sysinfo::DiskKind::SSD => DriveKind::Ssd,
                sysinfo::DiskKind::HDD => DriveKind::Hdd,
                _ => DriveKind::Unknown,
            },
            removable: d.is_removable(),
            read_only: d.is_read_only(),
            total: d.total_space(),
            available: d.available_space(),
            fs,
            mount,
        });
    }
    out.sort_by(|a, b| a.mount.cmp(&b.mount));
    out
}

/// Free space of the volume holding `path` (refreshed now).
pub fn available_for(path: &Path) -> Option<u64> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let p = canon.to_string_lossy().replace("\\\\?\\", "").to_lowercase();
    disks
        .list()
        .iter()
        .filter(|d| p.starts_with(&d.mount_point().to_string_lossy().to_lowercase()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| d.available_space())
}

/// True when `a` and `b` are on the same volume.
pub fn same_volume(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match (std::fs::metadata(a), std::fs::metadata(b)) {
            (Ok(x), Ok(y)) => x.dev() == y.dev(),
            _ => false,
        }
    }
    #[cfg(not(unix))]
    {
        let letter = |p: &Path| {
            let c = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
            let s = c.to_string_lossy().replace("\\\\?\\", "").to_uppercase();
            s.chars().take(2).collect::<String>()
        };
        letter(a) == letter(b)
    }
}

/// Folders on `drive` where the temporary fill files can go, best first: the drive's own root,
/// then folders of the user's that live on the same drive (when the root isn't writable, e.g.
/// `/` on Linux without root rights).
pub fn fill_locations(drive: &Drive) -> Vec<PathBuf> {
    let mut out = vec![drive.mount.clone()];
    let user_dirs = [dirs::home_dir(), dirs::document_dir(), dirs::download_dir(), dirs::desktop_dir()];
    for d in user_dirs.into_iter().flatten() {
        // Compared by device, not by path: on macOS /Users lives on /System/Volumes/Data.
        if same_volume(&d, &drive.mount) && !out.contains(&d) {
            out.push(d);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_something() {
        // CI machines always have at least the drive the tests run from.
        let l = list();
        assert!(!l.is_empty());
        assert!(l.iter().all(|d| d.total > 0));
    }

    #[test]
    fn same_volume_for_siblings() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("a")).unwrap();
        assert!(same_volume(&d.path().join("a"), d.path()));
    }
}
