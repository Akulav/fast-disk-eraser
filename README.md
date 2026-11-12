<div align="center">

<img src="assets/icon.png" alt="Fast Disk Eraser" width="128" height="128">

# Fast Disk Eraser

**Securely erase files, folders and free disk space. Made by Akulav.**

No telemetry. No ads. No bloat. One file per system that installs and updates itself.

[![Latest release](https://img.shields.io/github/v/release/Akulav/fast-disk-eraser?style=for-the-badge&label=release&color=e5383b)](https://github.com/Akulav/fast-disk-eraser/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/Akulav/fast-disk-eraser/total?style=for-the-badge&color=f4b740)](https://github.com/Akulav/fast-disk-eraser/releases)
[![Build](https://img.shields.io/github/actions/workflow/status/Akulav/fast-disk-eraser/build.yml?branch=main&style=for-the-badge&label=build)](https://github.com/Akulav/fast-disk-eraser/actions/workflows/build.yml)

![Windows](https://img.shields.io/badge/Windows-10%20%7C%2011-0078D6?style=flat-square&logo=windows&logoColor=white)
![Linux](https://img.shields.io/badge/Linux-x64-FCC624?style=flat-square&logo=linux&logoColor=black)
![macOS](https://img.shields.io/badge/macOS-Apple%20Silicon-000000?style=flat-square&logo=apple&logoColor=white)
![Rust](https://img.shields.io/badge/made%20with-Rust-B7410E?style=flat-square&logo=rust&logoColor=white)

### [⬇ Download the latest release](https://github.com/Akulav/fast-disk-eraser/releases/latest)

</div>

---

## ⬇️ Download

One file per system. The same file installs the app the first time and updates it later.

| Platform | File | Notes |
| :-- | :-- | :-- |
| 🪟 **Windows 10 / 11** (x64) | [`FastDiskEraser-setup.exe`](https://github.com/Akulav/fast-disk-eraser/releases/latest/download/FastDiskEraser-setup.exe) | Asks where to install. No admin rights needed. |
| 🐧 **Linux** (x64) | [`FastDiskEraser-linux-x64`](https://github.com/Akulav/fast-disk-eraser/releases/latest/download/FastDiskEraser-linux-x64) | glibc 2.28+, OpenGL, X11 or Wayland. `chmod +x` and run. |
| 🍎 **macOS** (Apple Silicon) | [`FastDiskEraser-macos-arm64`](https://github.com/Akulav/fast-disk-eraser/releases/latest/download/FastDiskEraser-macos-arm64) | `chmod +x`, then right-click → Open the first time. |

Every release also ships `SHA256SUMS.txt` so you can verify your download.

## ✨ Features

- **Erase files and folders.** Drag them onto the window or pick them. Every byte is overwritten in place, flushed to disk, the name is scrambled several times, then the file is deleted.
- **Wipe free space** on any drive, so files deleted earlier by any program can't be recovered. Optionally cleans the file table too, where NTFS keeps tiny files that a normal fill never reaches.
- **Fast.** Random data comes from xoshiro256++ at several GB/s per core, and many files are erased in parallel, so the disk is the only limit.
- **Safe by default.** Drive roots, system folders, your home folder itself and the app's own folder are refused. Symbolic links and junctions are deleted, never followed. Optional hold-to-confirm.
- **Verify** reads the last pass back and compares it.
- **History** of past erases: counts, sizes and times only, never file names. Can be turned off.
- **Command line** for scripting (see below).
- **One-click updates** from GitHub releases, verified with SHA-256 before anything runs.

### Overwrite methods

| Method | Passes | Pattern |
| :-- | :-: | :-- |
| **Random** (default) | 1 | random data |
| Quick zero | 1 | `0x00` |
| DoD 5220.22-M | 3 | `0x00`, `0xFF`, random |
| DoD 5220.22-M ECE | 7 | the DoD passes, random, the DoD passes again |
| Schneier | 7 | `0xFF`, `0x00`, five random passes |

One random pass is enough for any modern drive. Extra passes only take longer (and wear SSDs).

> [!NOTE]
> **SSDs and USB sticks** decide for themselves where data lands, and keep spare blocks no program can reach. An overwrite can't be guaranteed to hit every old copy there. Erasing still removes the file, and TRIM clears the freed blocks. For an entire SSD, full-disk encryption (BitLocker, FileVault, LUKS) is the safest choice.

## 📁 Your files stay in one folder

Fast Disk Eraser keeps **everything it writes in the folder you install it to**, and nowhere else: not `%APPDATA%`, not `~/.local/share`, not the temp folder.

```text
FastDiskEraser/
  FastDiskEraser.exe   the app
  settings.json        preferences
  history.json         past erases (counts and sizes only)
  window.ron           window size and position
  cache/update/        a downloaded update, until it is installed
  .fde-install         marks the install folder
```

Outside that folder there are only the OS's own entries for an installed app (Start-menu/desktop shortcuts, the uninstall entry), which uninstalling removes. The uninstaller asks whether to keep your settings and history.

**Portable mode:** put an empty `portable.txt` next to the program (or start it with `--portable`) and it never installs, just keeps its files next to itself.

## 🔄 Updates

The app checks GitHub for a newer release (can be turned off in Settings). Updating downloads this system's file into `cache/update`, checks its SHA-256 against the release's `SHA256SUMS.txt`, then:

- **Windows:** runs the setup silently in update mode. It installs into the same folder and restarts the app.
- **Linux / macOS:** swaps the binary in place and restarts.

Running a newer setup or binary by hand updates the existing install too.

## ⌨️ Command line

```text
FastDiskEraser --cli drives
FastDiskEraser --cli erase [--method M] [--verify] [--no-rename] [--yes] <path>...
FastDiskEraser --cli wipe-free [--method M] [--no-file-table] [--yes] <drive or folder>
FastDiskEraser --version
```

Methods: `random` (default), `zero`, `dod3`, `dod7`, `schneier`. Without `--yes` it asks before erasing.

## 🛠️ Building

Needs a recent stable [Rust](https://rustup.rs).

```bash
cargo test --release
cargo build --release
```

The program is `target/release/FastDiskEraser(.exe)`. Debug builds never offer to install themselves.

Windows setup (needs [Inno Setup 6](https://jrsoftware.org/isinfo.php)):

```bash
iscc /DAppVersion=1.0.0 installer\fast-disk-eraser.iss
```

The setup lands in `dist/`. `assets/make_icons.py` regenerates the icon, banner and installer images.

### Releasing

Bump `version` in `Cargo.toml` and push to `main`. The [build workflow](.github/workflows/build.yml) builds and tests all three systems and, when no release with that version exists yet, tags the commit and publishes the release with the three files and `SHA256SUMS.txt`.

## 📜 License

See [LICENSE](LICENSE). Bundled fonts: Inter (SIL Open Font License, see `assets/fonts/Inter-OFL-1.1.txt`) and DejaVu Sans Mono.

<div align="center">

Made by **Akulav**

</div>
