//! Erasing files and folders: overwrite every byte in place, scramble the name, then delete.
//!
//! For every file:
//! 1. Clear the read-only flag and open it for writing.
//! 2. For each pass of the method: overwrite the whole file (rounded up to the next 4 KiB, so
//!    the slack after the last byte is covered too), then flush it to the disk (`fsync`).
//! 3. Optionally read the last pass back and compare.
//! 4. Truncate to 0 bytes, rename it a few times to random names of the same length (so the old
//!    name doesn't survive in the directory entry), then delete it.
//!
//! Folders are walked without following symbolic links or junctions: a link is deleted, never
//! what it points to. Several files are erased at once (one thread each), which keeps SSDs busy
//! when there are many small files.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::method::{Method, Pattern};
use super::progress::Progress;
use super::rng::Rng;

pub const BUF_SIZE: usize = 4 * 1024 * 1024;
const SLACK_ALIGN: u64 = 4096;
const RENAMES: usize = 3;

#[derive(Debug, Clone)]
pub struct EraseOptions {
    pub method: Method,
    pub verify: bool,
    pub rename: bool,
    pub threads: usize,
}

impl Default for EraseOptions {
    fn default() -> Self {
        Self { method: Method::Random, verify: false, rename: true, threads: 4 }
    }
}

#[derive(Debug, Clone)]
pub struct Failure {
    pub path: PathBuf,
    pub error: String,
}

#[derive(Debug, Clone, Default)]
pub struct Outcome {
    pub files: u64,
    pub folders: u64,
    pub links: u64,
    /// Bytes of file content destroyed (not counting passes).
    pub bytes: u64,
    pub failures: Vec<Failure>,
    pub cancelled: bool,
    pub elapsed: Duration,
}

/// Everything found under the selected paths.
#[derive(Debug, Default)]
pub struct Scan {
    pub files: Vec<(PathBuf, u64)>,
    /// Folders, deepest first, so each is empty by the time it is removed.
    pub folders: Vec<PathBuf>,
    pub links: Vec<PathBuf>,
    pub bytes: u64,
    pub failures: Vec<Failure>,
}

/// Symlinks and junctions (and other reparse points on Windows) are never followed.
pub fn is_link(meta: &std::fs::Metadata) -> bool {
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    false
}

/// Bytes overwritten per pass for a file of `len` bytes. Small files are overwritten exactly
/// (on NTFS they can live inside the file table, and growing them would move them out of it
/// without touching the old copy); larger ones up to the next 4 KiB.
pub fn span(len: u64) -> u64 {
    if len < SLACK_ALIGN {
        len
    } else {
        len.div_ceil(SLACK_ALIGN) * SLACK_ALIGN
    }
}

fn walk(path: &Path, scan: &mut Scan) {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) => {
            scan.failures.push(Failure { path: path.to_path_buf(), error: e.to_string() });
            return;
        }
    };
    if is_link(&meta) {
        scan.links.push(path.to_path_buf());
    } else if meta.is_dir() {
        match std::fs::read_dir(path) {
            Ok(rd) => {
                for entry in rd {
                    match entry {
                        Ok(e) => walk(&e.path(), scan),
                        Err(e) => scan.failures.push(Failure { path: path.to_path_buf(), error: e.to_string() }),
                    }
                }
            }
            Err(e) => scan.failures.push(Failure { path: path.to_path_buf(), error: e.to_string() }),
        }
        scan.folders.push(path.to_path_buf());
    } else {
        scan.bytes += meta.len();
        scan.files.push((path.to_path_buf(), meta.len()));
    }
}

/// Collect every file, folder and link under `paths`.
pub fn scan(paths: &[PathBuf]) -> Scan {
    let mut scan = Scan::default();
    for p in paths {
        walk(p, &mut scan);
    }
    scan
}

/// Size and file count of one path (for the queue list).
pub fn measure(path: &Path) -> (u64, u64) {
    let s = scan(std::slice::from_ref(&path.to_path_buf()));
    (s.bytes, s.files.len() as u64)
}

/// Why `path` must never be erased, if it is a drive root, a system folder, the home folder
/// itself or holds the app (`protected` lists extra folders, e.g. the install folder).
pub fn protected_reason(path: &Path, protected: &[PathBuf]) -> Option<String> {
    let canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let norm = |p: &Path| -> String {
        let s = p.to_string_lossy().replace('\\', "/");
        let s = s.strip_prefix("//?/").unwrap_or(&s).trim_end_matches('/').to_string();
        if cfg!(any(windows, target_os = "macos")) {
            s.to_lowercase()
        } else {
            s
        }
    };
    let c = norm(&canon);
    if canon.parent().is_none() || c.is_empty() || (cfg!(windows) && c.len() <= 2) {
        return Some("This is the root of a drive. Use “Wipe free space” to clean a whole drive.".into());
    }
    if let Some(home) = dirs::home_dir() {
        if c == norm(&std::fs::canonicalize(&home).unwrap_or(home)) {
            return Some("This is your home folder. Pick the files or folders inside it instead.".into());
        }
    }
    let system: &[&str] = if cfg!(windows) {
        &["c:/windows", "c:/program files", "c:/program files (x86)", "c:/programdata", "c:/users"]
    } else if cfg!(target_os = "macos") {
        &["/system", "/library", "/applications", "/users", "/bin", "/sbin", "/usr", "/private", "/etc", "/var", "/opt"]
    } else {
        &["/bin", "/boot", "/dev", "/etc", "/home", "/lib", "/lib64", "/opt", "/proc", "/root", "/run", "/sbin", "/srv", "/sys", "/usr", "/var"]
    };
    if system.iter().any(|s| c == *s) {
        return Some("This is a system folder; erasing it would break your computer.".into());
    }
    for p in protected {
        let pn = norm(&std::fs::canonicalize(p).unwrap_or_else(|_| p.clone()));
        if !pn.is_empty() && (pn == c || pn.starts_with(&format!("{c}/"))) {
            return Some("This folder contains Fast Disk Eraser itself.".into());
        }
    }
    None
}

fn make_writable(path: &Path) {
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        let mut perms = meta.permissions();
        if perms.readonly() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                perms.set_mode(perms.mode() | 0o200);
            }
            #[cfg(not(unix))]
            #[allow(clippy::permissions_set_readonly_false)]
            perms.set_readonly(false);
            let _ = std::fs::set_permissions(path, perms);
        }
    }
}

/// Write `len` bytes of `pattern` from the start of `f`, reporting progress.
pub(crate) fn write_pattern(f: &mut File, len: u64, pattern: Pattern, seed: u64, buf: &mut [u8], prog: &Progress) -> std::io::Result<()> {
    f.seek(SeekFrom::Start(0))?;
    let mut rng = Rng::seeded(seed);
    if let Pattern::Byte(b) = pattern {
        buf.fill(b);
    }
    let mut left = len;
    while left > 0 {
        if prog.cancelled() {
            return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "cancelled"));
        }
        let n = left.min(buf.len() as u64) as usize;
        if pattern == Pattern::Random {
            rng.fill(&mut buf[..n]);
        }
        f.write_all(&buf[..n])?;
        prog.add_bytes(n as u64);
        left -= n as u64;
    }
    f.flush()?;
    f.sync_data()
}

fn verify_pattern(f: &mut File, len: u64, pattern: Pattern, seed: u64, buf: &mut [u8], expect: &mut [u8], prog: &Progress) -> std::io::Result<()> {
    f.seek(SeekFrom::Start(0))?;
    let mut rng = Rng::seeded(seed);
    if let Pattern::Byte(b) = pattern {
        expect.fill(b);
    }
    let mut left = len;
    while left > 0 {
        if prog.cancelled() {
            return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "cancelled"));
        }
        let n = left.min(buf.len() as u64) as usize;
        f.read_exact(&mut buf[..n])?;
        if pattern == Pattern::Random {
            rng.fill(&mut expect[..n]);
        }
        if buf[..n] != expect[..n] {
            return Err(std::io::Error::other("verification failed: the disk did not keep the overwrite data"));
        }
        prog.add_bytes(n as u64);
        left -= n as u64;
    }
    Ok(())
}

/// Rename `path` to random names of the same length a few times; returns where it ended up.
pub(crate) fn scramble_name(path: &Path, rng: &mut Rng) -> PathBuf {
    let mut cur = path.to_path_buf();
    let len = path.file_name().map(|n| n.to_string_lossy().chars().count()).unwrap_or(8).clamp(1, 64);
    for _ in 0..RENAMES {
        let next = cur.with_file_name(rng.name(len));
        if next.exists() {
            continue;
        }
        if std::fs::rename(&cur, &next).is_ok() {
            cur = next;
        }
    }
    cur
}

struct Worker {
    buf: Vec<u8>,
    expect: Vec<u8>,
    rng: Rng,
}

impl Worker {
    fn new(verify: bool) -> Self {
        Self { buf: vec![0; BUF_SIZE], expect: if verify { vec![0; BUF_SIZE] } else { Vec::new() }, rng: Rng::seeded(Rng::fresh_seed()) }
    }

    fn erase_file(&mut self, path: &Path, len: u64, opts: &EraseOptions, prog: &Progress) -> std::io::Result<()> {
        make_writable(path);
        let span = span(len);
        {
            let mut f = OpenOptions::new().read(opts.verify).write(true).open(path)?;
            let passes = opts.method.passes();
            let mut last = (Pattern::Random, 0u64);
            for pattern in passes {
                let seed = self.rng.next_u64();
                write_pattern(&mut f, span, pattern, seed, &mut self.buf, prog)?;
                last = (pattern, seed);
            }
            if opts.verify && span > 0 {
                verify_pattern(&mut f, span, last.0, last.1, &mut self.buf, &mut self.expect, prog)?;
            }
            f.set_len(0)?;
            f.sync_all()?;
        }
        let target = if opts.rename { scramble_name(path, &mut self.rng) } else { path.to_path_buf() };
        std::fs::remove_file(&target)
    }
}

/// Bytes of disk I/O an erase of `scan` performs (for progress).
pub fn work_bytes(scan: &Scan, opts: &EraseOptions) -> u64 {
    let per_pass: u64 = scan.files.iter().map(|(_, l)| span(*l)).sum();
    per_pass * (opts.method.pass_count() as u64 + opts.verify as u64)
}

/// Erase everything in `scan`. Runs on the calling thread (plus `opts.threads` helpers).
pub fn erase(scan: Scan, opts: &EraseOptions, prog: &Progress) -> Outcome {
    let started = Instant::now();
    prog.bytes_total.store(work_bytes(&scan, opts), Ordering::Relaxed);
    prog.items_total.store((scan.files.len() + scan.folders.len() + scan.links.len()) as u64, Ordering::Relaxed);
    prog.passes.store(opts.method.pass_count(), Ordering::Relaxed);
    prog.set_phase(format!("Overwriting with {}", opts.method.name()));

    let failures = Mutex::new(scan.failures);
    let next = AtomicUsize::new(0);
    let erased = AtomicUsize::new(0);
    let erased_bytes = std::sync::atomic::AtomicU64::new(0);
    let files = Arc::new(scan.files);
    let threads = opts.threads.clamp(1, 16).min(files.len().max(1));

    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                let mut w = Worker::new(opts.verify);
                loop {
                    if prog.cancelled() {
                        break;
                    }
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some((path, len)) = files.get(i) else { break };
                    prog.set_current(path.display().to_string());
                    match w.erase_file(path, *len, opts, prog) {
                        Ok(()) => {
                            erased.fetch_add(1, Ordering::Relaxed);
                            erased_bytes.fetch_add(*len, Ordering::Relaxed);
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted && prog.cancelled() => break,
                        Err(e) => failures.lock().unwrap().push(Failure { path: path.clone(), error: e.to_string() }),
                    }
                    prog.item_done();
                }
            });
        }
    });

    let mut out = Outcome {
        files: erased.load(Ordering::Relaxed) as u64,
        bytes: erased_bytes.load(Ordering::Relaxed),
        failures: failures.into_inner().unwrap(),
        ..Default::default()
    };
    if prog.cancelled() {
        out.cancelled = true;
        out.elapsed = started.elapsed();
        return out;
    }

    prog.set_phase("Removing links and folders");
    let mut rng = Rng::seeded(Rng::fresh_seed());
    for link in &scan.links {
        // A link to a folder is removed like a folder on Windows; elsewhere it's a file.
        let r = std::fs::remove_file(link).or_else(|_| std::fs::remove_dir(link));
        match r {
            Ok(()) => out.links += 1,
            Err(e) => out.failures.push(Failure { path: link.clone(), error: e.to_string() }),
        }
        prog.item_done();
    }
    for dir in &scan.folders {
        make_writable(dir);
        let target = if opts.rename { scramble_name(dir, &mut rng) } else { dir.clone() };
        match std::fs::remove_dir(&target) {
            Ok(()) => out.folders += 1,
            Err(e) => {
                // Put the name back so the user can find what was left.
                if target != *dir {
                    let _ = std::fs::rename(&target, dir);
                }
                out.failures.push(Failure { path: dir.clone(), error: e.to_string() });
            }
        }
        prog.item_done();
    }
    prog.set_current("");
    out.elapsed = started.elapsed();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans() {
        assert_eq!(span(0), 0);
        assert_eq!(span(100), 100);
        assert_eq!(span(4096), 4096);
        assert_eq!(span(4097), 8192);
    }

    #[test]
    fn erases_tree_and_keeps_link_targets() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("secret");
        std::fs::create_dir_all(root.join("a/b")).unwrap();
        std::fs::write(root.join("a/b/f1.txt"), vec![7u8; 10_000]).unwrap();
        std::fs::write(root.join("a/f2.bin"), b"hello").unwrap();
        std::fs::write(root.join("empty"), b"").unwrap();
        let outside = d.path().join("keep.txt");
        std::fs::write(&outside, b"keep me").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();

        let scan = scan(std::slice::from_ref(&root));
        assert_eq!(scan.files.len(), 3);
        let prog = Progress::default();
        let out = erase(scan, &EraseOptions { method: Method::Dod3, verify: true, rename: true, threads: 2 }, &prog);
        assert!(out.failures.is_empty(), "{:?}", out.failures);
        assert_eq!(out.files, 3);
        assert_eq!(out.bytes, 10_005);
        assert!(!root.exists(), "the whole tree is gone");
        assert_eq!(std::fs::read(&outside).unwrap(), b"keep me", "link targets are never touched");
        assert_eq!(prog.bytes_done.load(Ordering::Relaxed), prog.bytes_total.load(Ordering::Relaxed));
    }

    #[test]
    fn overwrites_before_deleting() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("x.bin");
        std::fs::write(&f, vec![0xABu8; 5000]).unwrap();
        let mut file = OpenOptions::new().read(true).write(true).open(&f).unwrap();
        let prog = Progress::default();
        let mut buf = vec![0u8; 1024];
        write_pattern(&mut file, span(5000), Pattern::Byte(0x00), 0, &mut buf, &prog).unwrap();
        drop(file);
        let data = std::fs::read(&f).unwrap();
        assert_eq!(data.len(), 8192, "slack up to 4 KiB is covered");
        assert!(data.iter().all(|&b| b == 0));
    }

    #[test]
    fn refuses_dangerous_targets() {
        let root = if cfg!(windows) { PathBuf::from(r"C:\") } else { PathBuf::from("/") };
        assert!(protected_reason(&root, &[]).is_some());
        if let Some(home) = dirs::home_dir() {
            assert!(protected_reason(&home, &[]).is_some());
        }
        let d = tempfile::tempdir().unwrap();
        let app = d.path().join("apps/FastDiskEraser");
        std::fs::create_dir_all(&app).unwrap();
        assert!(protected_reason(&d.path().join("apps"), std::slice::from_ref(&app)).is_some());
        assert!(protected_reason(&app, std::slice::from_ref(&app)).is_some());
        let other = d.path().join("other");
        std::fs::create_dir_all(&other).unwrap();
        assert!(protected_reason(&other, &[app]).is_none());
    }
}
