//! Wiping free space: overwrite every unused block of a drive, so files deleted earlier (by any
//! program) can't be recovered.
//!
//! A hidden work folder `.fde-wipe-<random>` is created on the drive and filled with files of
//! overwrite data until the drive is full. Further passes rewrite those same files in place.
//! Optionally, thousands of tiny files are created too: they take over the file-table records of
//! deleted small files (NTFS keeps files under ~700 bytes inside the file table itself, where a
//! normal fill never reaches). Everything is flushed to the disk and then deleted, also when the
//! wipe is cancelled or fails. The drive is completely full for a short time while this runs.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

use super::erase::{write_pattern, BUF_SIZE};
use super::method::{Method, Pattern};
use super::progress::Progress;
use super::rng::Rng;

/// Max size of one fill file (FAT32 can't hold 4 GiB files).
const FILE_CAP: u64 = 1024 * 1024 * 1024;
/// Smallest write tried once the drive reports it is full.
const MIN_CHUNK: usize = 4096;
/// Freed again after the fill to make room for the file-table cleanup.
const RESERVE: u64 = 64 * 1024 * 1024;
const TINY_FILES_MAX: u64 = 100_000;
const TINY_FILE_SIZE: usize = 640;
pub const WORK_PREFIX: &str = ".fde-wipe-";

#[derive(Debug, Clone)]
pub struct FreeSpaceOptions {
    pub method: Method,
    pub clean_file_table: bool,
}

#[derive(Debug, Clone, Default)]
pub struct FreeOutcome {
    /// Free space overwritten (one pass).
    pub bytes: u64,
    pub tiny_files: u64,
    pub cancelled: bool,
    pub elapsed: Duration,
}

/// True for "the disk is full" (and "this file can't grow any more") errors on every OS.
pub fn is_full(e: &std::io::Error) -> bool {
    use std::io::ErrorKind::*;
    if matches!(e.kind(), StorageFull | QuotaExceeded | FileTooLarge) {
        return true;
    }
    match e.raw_os_error() {
        // Windows: ERROR_HANDLE_DISK_FULL, ERROR_DISK_FULL, ERROR_DISK_QUOTA_EXCEEDED, ERROR_FILE_TOO_LARGE
        Some(39 | 112 | 1295 | 223) if cfg!(windows) => true,
        // Unix: EFBIG, ENOSPC, EDQUOT (Linux 122, macOS 69)
        Some(27 | 28) if cfg!(unix) => true,
        Some(122) if cfg!(target_os = "linux") => true,
        Some(69) if cfg!(target_os = "macos") => true,
        _ => false,
    }
}

/// Deletes the work folder whatever happens, so a cancelled or failed wipe never leaves the
/// drive full.
struct WorkDir {
    path: PathBuf,
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        if let Ok(rd) = std::fs::read_dir(&self.path) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    let _ = std::fs::remove_dir_all(&p);
                } else {
                    let _ = std::fs::remove_file(&p);
                }
            }
        }
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Create the work folder in the first of `locations` that accepts it.
fn create_work_dir(locations: &[PathBuf], rng: &mut Rng) -> Result<WorkDir> {
    let mut last_err = None;
    for loc in locations {
        let path = loc.join(format!("{WORK_PREFIX}{}", rng.name(8)));
        match std::fs::create_dir(&path) {
            Ok(()) => {
                #[cfg(windows)]
                hide(&path);
                return Ok(WorkDir { path });
            }
            Err(e) => last_err = Some((loc.clone(), e)),
        }
    }
    match last_err {
        Some((loc, e)) => Err(e).with_context(|| format!("no permission to write on this drive (tried {})", loc.display())),
        None => bail!("no folder to write to on this drive"),
    }
}

#[cfg(windows)]
fn hide(path: &Path) {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SetFileAttributesW(name: *const u16, attrs: u32) -> i32;
    }
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    // SAFETY: NUL-terminated UTF-16 path that outlives the call. FILE_ATTRIBUTE_HIDDEN = 2.
    unsafe {
        SetFileAttributesW(wide.as_ptr(), 2);
    }
}

/// Write pattern data to `f` until it holds `limit` bytes or the drive is full. Returns the
/// bytes written and whether the drive is full.
fn fill_file(f: &mut File, limit: u64, pattern: Pattern, rng: &mut Rng, buf: &mut [u8], chunk: &mut usize, prog: &Progress) -> Result<(u64, bool)> {
    if let Pattern::Byte(b) = pattern {
        buf.fill(b);
    }
    let mut written = 0u64;
    while written < limit {
        if prog.cancelled() {
            return Ok((written, false));
        }
        let n = (*chunk as u64).min(limit - written) as usize;
        if pattern == Pattern::Random {
            rng.fill(&mut buf[..n]);
        }
        match f.write(&buf[..n]) {
            Ok(0) => return Ok((written, true)),
            Ok(k) => {
                written += k as u64;
                prog.add_bytes(k as u64);
            }
            Err(e) if is_full(&e) => {
                if *chunk > MIN_CHUNK {
                    *chunk = (*chunk / 16).max(MIN_CHUNK);
                } else {
                    return Ok((written, true));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Ok((written, false))
}

fn sync(f: &File) -> Result<()> {
    match f.sync_all() {
        Ok(()) => Ok(()),
        Err(e) if is_full(&e) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Fill the drive with new files until it is full. Returns the files and their sizes.
fn fill_drive(dir: &Path, first_index: usize, pattern: Pattern, rng: &mut Rng, buf: &mut [u8], prog: &Progress) -> Result<Vec<(PathBuf, u64)>> {
    let mut files = Vec::new();
    let mut chunk = BUF_SIZE;
    for i in first_index.. {
        if prog.cancelled() {
            break;
        }
        let path = dir.join(format!("fill-{i:05}.bin"));
        let mut f = match File::create(&path) {
            Ok(f) => f,
            Err(e) if is_full(&e) || !files.is_empty() => break,
            Err(e) => return Err(e).with_context(|| format!("could not create {}", path.display())),
        };
        let (written, full) = fill_file(&mut f, FILE_CAP, pattern, rng, buf, &mut chunk, prog)?;
        sync(&f)?;
        drop(f);
        if written == 0 {
            let _ = std::fs::remove_file(&path);
        } else {
            files.push((path, written));
        }
        if full {
            break;
        }
    }
    Ok(files)
}

/// Create tiny files until the file table can't take more (or a sane maximum).
fn fill_file_table(dir: &Path, rng: &mut Rng, prog: &Progress) -> u64 {
    let tiny = dir.join("t");
    if std::fs::create_dir(&tiny).is_err() {
        return 0;
    }
    prog.set_phase("Cleaning the file table");
    prog.items_total.store(TINY_FILES_MAX, Ordering::Relaxed);
    let mut data = vec![0u8; TINY_FILE_SIZE];
    let mut n = 0u64;
    while n < TINY_FILES_MAX && !prog.cancelled() {
        rng.fill(&mut data);
        // Spread over sub-folders: huge single folders get slow on every file system.
        let sub = tiny.join(format!("{:03}", n / 2000));
        if n.is_multiple_of(2000) &&std::fs::create_dir(&sub).is_err() {
            break;
        }
        let ok = OpenOptions::new().write(true).create_new(true).open(sub.join(format!("{n:06}"))).and_then(|mut f| f.write_all(&data));
        if ok.is_err() {
            break;
        }
        n += 1;
        prog.items_done.store(n, Ordering::Relaxed);
    }
    n
}

/// Wipe the free space of the drive holding `locations[0]` (see [`super::drives::fill_locations`]).
/// `free` is the drive's free space now, used for progress.
pub fn wipe(locations: &[PathBuf], free: u64, opts: &FreeSpaceOptions, prog: &Progress) -> Result<FreeOutcome> {
    let started = Instant::now();
    let mut rng = Rng::seeded(Rng::fresh_seed());
    let work = create_work_dir(locations, &mut rng)?;
    let mut out = FreeOutcome::default();
    let passes = opts.method.passes();
    prog.passes.store(passes.len() as u32, Ordering::Relaxed);
    prog.bytes_total.store(free.max(1) * passes.len() as u64, Ordering::Relaxed);
    let mut buf = vec![0u8; BUF_SIZE];

    // Pass 1: fill the drive. A reserve file comes first so that the file-table cleanup has room.
    prog.pass.store(1, Ordering::Relaxed);
    prog.set_phase(format!("Pass 1 of {}: filling free space ({})", passes.len(), passes[0].label()));
    prog.set_current(work.path.display().to_string());
    let mut files: Vec<(PathBuf, u64)> = Vec::new();
    let reserve = work.path.join("reserve.bin");
    if opts.clean_file_table {
        let mut f = File::create(&reserve).context("could not write on this drive")?;
        let mut chunk = BUF_SIZE;
        let (w, _) = fill_file(&mut f, RESERVE.min(free / 4), passes[0], &mut rng, &mut buf, &mut chunk, prog)?;
        sync(&f)?;
        files.push((reserve.clone(), w));
    }
    files.extend(fill_drive(&work.path, 0, passes[0], &mut rng, &mut buf, prog)?);

    if opts.clean_file_table && !prog.cancelled() {
        // Free the reserve, take over file-table records with tiny files, then fill what is left.
        let _ = std::fs::remove_file(&reserve);
        files.retain(|(p, _)| p != &reserve);
        out.tiny_files = fill_file_table(&work.path, &mut rng, prog);
        prog.set_phase(format!("Pass 1 of {}: filling the rest", passes.len()));
        files.extend(fill_drive(&work.path, files.len() + 1, passes[0], &mut rng, &mut buf, prog)?);
    }
    out.bytes = files.iter().map(|(_, l)| l).sum();
    // The drive's free space shrank while we filled it, if other programs wrote too: rescale.
    prog.bytes_total.store(out.bytes.max(1) * passes.len() as u64, Ordering::Relaxed);
    prog.bytes_done.store(out.bytes, Ordering::Relaxed);

    // Further passes rewrite the same files in place (same blocks on the disk).
    for (i, pattern) in passes.iter().enumerate().skip(1) {
        if prog.cancelled() {
            break;
        }
        prog.pass.store(i as u32 + 1, Ordering::Relaxed);
        prog.set_phase(format!("Pass {} of {}: {}", i + 1, passes.len(), pattern.label()));
        for (path, len) in &files {
            if prog.cancelled() {
                break;
            }
            let mut f = OpenOptions::new().write(true).open(path)?;
            write_pattern(&mut f, *len, *pattern, rng.next_u64(), &mut buf, prog).or_else(|e| if prog.cancelled() { Ok(()) } else { Err(e) })?;
        }
    }

    prog.set_phase("Releasing the space again");
    prog.set_current("");
    drop(work);
    out.cancelled = prog.cancelled();
    out.elapsed = started.elapsed();
    Ok(out)
}

/// Remove work folders left behind by a wipe that was killed (power loss, crash) in `dir`.
pub fn remove_leftovers(dir: &Path) -> usize {
    let mut n = 0;
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().starts_with(WORK_PREFIX) && e.path().is_dir() && std::fs::remove_dir_all(e.path()).is_ok() {
                n += 1;
            }
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_errors() {
        assert!(is_full(&std::io::Error::from(std::io::ErrorKind::StorageFull)));
        assert!(!is_full(&std::io::Error::from(std::io::ErrorKind::PermissionDenied)));
        let raw = if cfg!(windows) { 112 } else { 28 };
        assert!(is_full(&std::io::Error::from_raw_os_error(raw)));
    }

    #[test]
    fn fill_stops_at_limit() {
        let d = tempfile::tempdir().unwrap();
        let mut f = File::create(d.path().join("x")).unwrap();
        let prog = Progress::default();
        let mut buf = vec![0u8; 1 << 16];
        let mut chunk = buf.len();
        let (w, full) = fill_file(&mut f, 300_000, Pattern::Random, &mut Rng::seeded(3), &mut buf, &mut chunk, &prog).unwrap();
        assert_eq!((w, full), (300_000, false));
        assert_eq!(std::fs::metadata(d.path().join("x")).unwrap().len(), 300_000);
    }

    #[test]
    fn work_dir_is_always_removed() {
        let d = tempfile::tempdir().unwrap();
        let mut rng = Rng::seeded(9);
        let w = create_work_dir(&[d.path().to_path_buf()], &mut rng).unwrap();
        std::fs::write(w.path.join("fill-00000.bin"), b"x").unwrap();
        std::fs::create_dir_all(w.path.join("t/000")).unwrap();
        let p = w.path.clone();
        drop(w);
        assert!(!p.exists());

        std::fs::create_dir_all(d.path().join(format!("{WORK_PREFIX}old"))).unwrap();
        assert_eq!(remove_leftovers(d.path()), 1);
    }

    #[test]
    fn cancelled_wipe_cleans_up() {
        let d = tempfile::tempdir().unwrap();
        let prog = Progress::default();
        prog.cancel();
        let out = wipe(&[d.path().to_path_buf()], 1 << 20, &FreeSpaceOptions { method: Method::Dod3, clean_file_table: true }, &prog).unwrap();
        assert!(out.cancelled);
        assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 0, "nothing left behind");
    }
}
