# Changelog

All notable changes to Fast Disk Eraser are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses [Semantic Versioning](https://semver.org/).
Older versions are described on the [releases page](https://github.com/Akulav/fast-disk-eraser/releases).

## [1.2.0] - 2026-11-21

### Changed
- **New look:** Windows 11's dark style, like Akulav's other apps: a flat #202020 window, #2b2b2b cards with hairline
  borders, the Windows accent blue (black text on the main button), a flat progress ring, 4 px controls and 8 px
  cards, Windows-style switches and navigation, and Segoe UI on Windows (Inter elsewhere). Erasing stays red. No
  more glows or gradients.

### Security
- The Windows, Program Files and ProgramData folders are found from the system, so they are protected on any drive
  (not only `C:`), and nothing inside the Windows folder can be picked for erasing.

## [1.1.1] - 2026-11-20

### Changed
- Docs: a new `docs/ARCHITECTURE.md` (how files and free space are erased, and the limits on SSDs and
  copy-on-write file systems), linked from the README.