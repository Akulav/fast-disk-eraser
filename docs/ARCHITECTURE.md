# Fast Disk Eraser: architecture

How the eraser works and where each piece lives. What it does for users is in the [README](../README.md).

## Overview

One Rust program in three layers:

| Layer | Folder | Rule |
|---|---|---|
| **Engine** | `src/core/` | Erasing only. No UI, no network. Used by the window and by `--cli`. |
| **App** | `src/app/` | Install / update, settings and history, the app folder, update checks |
| **Window** | `src/ui/` | egui pages; the engine runs on worker threads and reports through shared counters |

## The engine (`src/core/`)

| File | Role |
|---|---|
| `method.rs` | Overwrite methods: which patterns (zeros, ones, random, fixed bytes) in which order |
| `erase.rs` | Erasing files and folders |
| `freespace.rs` | Wiping a drive's free space |
| `drives.rs` | The mounted volumes the user can wipe |
| `rng.rs` | xoshiro256++ seeded through SplitMix64: fast overwrite data, reproducible per pass for verification |
| `progress.rs` | Lock-free counters and a cancel flag shared between the worker and the UI |

### Erasing a file

1. Clear the read-only flag and open the file for writing.
2. For each pass of the method, overwrite the whole file in place (rounded up to the next 4 KiB, so the file's
   last block is covered too) and flush it to the disk.
3. With **verify** on, read the last pass back and compare it with what was written (random passes regenerate
   their data from the pass's seed).
4. Truncate it to 0 bytes and rename it a few times to random names of the same length, so the original name
   doesn't survive in the directory entry.
5. Delete it.

Folders are walked **without following symbolic links or junctions** (a link is deleted, never what it points
to), and several files are erased at once, one thread each, which keeps SSDs busy when there are many small files.
Any failed pass, flush or delete is reported: a partial wipe is never shown as a success.

### Wiping free space

A hidden work folder `.fde-wipe-<random>` is created on the drive and filled with files of overwrite data until
the drive is full; later passes rewrite those same files in place. Then the folder is deleted. Anything deleted
earlier, by any program, is overwritten along the way.

### Limits worth knowing

Overwriting in place is reliable on hard drives. On **SSDs and flash** the controller may write to new cells
(wear levelling) and keep old ones until it erases them, and on **copy-on-write or snapshotting file systems**
(Btrfs, ZFS, APFS snapshots, Windows shadow copies) old copies can survive. For those, wipe free space as well, and
for a whole SSD use the drive's own secure-erase or full-disk encryption.

## The app (`src/app/`)

| File | Role |
|---|---|
| `paths.rs` | Everything lives in the install folder next to the program |
| `settings.rs` | `settings.json` (preferences) and `history.json` (finished jobs) |
| `update.rs`, `net.rs` | Update check and download from GitHub releases, verified against `SHA256SUMS.txt` |
| `install/` | Inno Setup on Windows (`installer/fast-disk-eraser.iss`); self-install on Linux / macOS |

## The window (`src/ui/`)

`app.rs` holds the state, title bar, sidebar, running jobs, dialogs and toasts; `pages.rs` the pages and the
running-job and result screens; `theme.rs` and `widgets.rs` the look shared with Akulav's other apps.

## Command line

`FastDiskEraser --cli drives` lists volumes; `FastDiskEraser --cli erase [--method …] [--verify] [--no-rename]
[--yes] <path>…` erases without the window (see `src/cli.rs` for every option).
