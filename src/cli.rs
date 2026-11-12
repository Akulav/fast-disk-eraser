//! `FastDiskEraser --cli …`: scripting and troubleshooting without the window.
//!
//! ```text
//! FastDiskEraser --cli drives
//! FastDiskEraser --cli erase [--method random] [--verify] [--no-rename] [--yes] <path>...
//! FastDiskEraser --cli wipe-free [--method random] [--no-file-table] [--yes] <drive or folder>
//! ```

use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;
use std::time::Duration;

use crate::app::paths::Paths;
use crate::app::settings::Settings;
use crate::core::erase::{self, EraseOptions};
use crate::core::freespace::{self, FreeSpaceOptions};
use crate::core::method::Method;
use crate::core::progress::Progress;
use crate::core::{drives, format_bytes, format_duration};

const USAGE: &str = "Fast Disk Eraser - made by Akulav

usage:
  FastDiskEraser --cli drives
  FastDiskEraser --cli erase [--method M] [--verify] [--no-rename] [--yes] <path>...
  FastDiskEraser --cli wipe-free [--method M] [--no-file-table] [--yes] <drive or folder>

methods: random (default), zero, dod3, dod7, schneier";

struct Opts {
    method: Option<Method>,
    verify: bool,
    no_rename: bool,
    no_file_table: bool,
    yes: bool,
    rest: Vec<String>,
}

fn parse(args: &[String]) -> Result<Opts, String> {
    let mut o = Opts { method: None, verify: false, no_rename: false, no_file_table: false, yes: false, rest: vec![] };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--method" | "-m" => {
                let v = it.next().ok_or("--method needs a value")?;
                o.method = Some(Method::parse(v).ok_or_else(|| format!("unknown method {v}"))?);
            }
            "--verify" => o.verify = true,
            "--no-rename" => o.no_rename = true,
            "--no-file-table" => o.no_file_table = true,
            "--yes" | "-y" => o.yes = true,
            _ => o.rest.push(a.clone()),
        }
    }
    Ok(o)
}

fn confirm(question: &str, yes: bool) -> bool {
    if yes {
        return true;
    }
    if !std::io::stdin().is_terminal() {
        eprintln!("Refusing to erase without --yes when not run from a terminal.");
        return false;
    }
    print!("{question} Type YES to continue: ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).is_ok() && line.trim() == "YES"
}

/// Print progress until the worker finishes.
fn watch<T: Send + 'static>(prog: Arc<Progress>, work: impl FnOnce() -> T + Send + 'static) -> T {
    let p = prog.clone();
    let handle = std::thread::spawn(work);
    while !handle.is_finished() {
        std::thread::sleep(Duration::from_millis(500));
        let pct = p.fraction().map(|f| f * 100.0).unwrap_or(0.0);
        eprint!("\r  {:>5.1}%  {}  {}            ", pct, format_bytes(p.bytes_done.load(Relaxed)), p.phase());
    }
    eprintln!();
    let _ = prog;
    handle.join().expect("worker panicked")
}

pub fn run(args: &[String]) -> i32 {
    let Some(cmd) = args.first() else {
        println!("{USAGE}");
        return 2;
    };
    let opts = match parse(&args[1..]) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}\n\n{USAGE}");
            return 2;
        }
    };
    let settings = Paths::detect().map(|p| Settings::load(&p)).unwrap_or_default();
    match cmd.as_str() {
        "drives" => {
            for d in drives::list() {
                println!(
                    "{:<28} {:<24} {:>9} free of {:>9}  {} {:?}{}",
                    d.label,
                    d.mount.display(),
                    format_bytes(d.available),
                    format_bytes(d.total),
                    d.fs,
                    d.kind,
                    if d.removable { " removable" } else { "" }
                );
            }
            0
        }
        "erase" => {
            if opts.rest.is_empty() {
                eprintln!("erase: no paths given\n\n{USAGE}");
                return 2;
            }
            let paths: Vec<PathBuf> = opts.rest.iter().map(PathBuf::from).collect();
            let protected: Vec<PathBuf> = crate::app::install::program_root().into_iter().collect();
            for p in &paths {
                if !p.exists() && std::fs::symlink_metadata(p).is_err() {
                    eprintln!("{}: not found", p.display());
                    return 1;
                }
                if let Some(why) = erase::protected_reason(p, &protected) {
                    eprintln!("{}: {why}", p.display());
                    return 1;
                }
            }
            let eo = EraseOptions {
                method: opts.method.unwrap_or(settings.method),
                verify: opts.verify || settings.verify,
                rename: !opts.no_rename && settings.rename,
                threads: settings.threads,
            };
            let scan = erase::scan(&paths);
            println!("{} files, {} folders, {} with {} ({} passes)", scan.files.len(), scan.folders.len(), format_bytes(scan.bytes), eo.method.name(), eo.method.pass_count());
            if !confirm("This permanently destroys them.", opts.yes) {
                return 1;
            }
            let prog = Progress::new();
            let p = prog.clone();
            let out = watch(prog, move || erase::erase(scan, &eo, &p));
            println!("erased {} files ({}) and {} folders in {}", out.files, format_bytes(out.bytes), out.folders, format_duration(out.elapsed));
            for f in &out.failures {
                eprintln!("  failed: {}: {}", f.path.display(), f.error);
            }
            i32::from(!out.failures.is_empty())
        }
        "wipe-free" => {
            let Some(target) = opts.rest.first().map(PathBuf::from) else {
                eprintln!("wipe-free: no drive given\n\n{USAGE}");
                return 2;
            };
            let drive = drives::list().into_iter().filter(|d| target.starts_with(&d.mount) || drives::same_volume(&target, &d.mount)).max_by_key(|d| d.mount.as_os_str().len());
            let mut locations = drive.as_ref().map(drives::fill_locations).unwrap_or_default();
            if target.is_dir() && !locations.contains(&target) {
                locations.insert(0, target.clone());
            }
            let free = drives::available_for(&target).unwrap_or(0);
            let fo = FreeSpaceOptions { method: opts.method.unwrap_or(settings.free_method), clean_file_table: !opts.no_file_table && settings.clean_file_table };
            println!("wiping {} of free space on {} with {} ({} passes)", format_bytes(free), target.display(), fo.method.name(), fo.method.pass_count());
            if !confirm("The drive will be completely full for a while.", opts.yes) {
                return 1;
            }
            let prog = Progress::new();
            let p = prog.clone();
            match watch(prog, move || freespace::wipe(&locations, free, &fo, &p)) {
                Ok(out) => {
                    println!("wiped {} of free space in {}", format_bytes(out.bytes), format_duration(out.elapsed));
                    0
                }
                Err(e) => {
                    eprintln!("wipe failed: {e:#}");
                    1
                }
            }
        }
        _ => {
            eprintln!("unknown command {cmd}\n\n{USAGE}");
            2
        }
    }
}
