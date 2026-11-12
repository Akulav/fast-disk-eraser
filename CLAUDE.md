# CLAUDE.md

Notes for working on **Fast Disk Eraser**, made by Akulav: securely erases files, folders and free disk space.
Rust + eframe/egui. One file per OS that installs and updates itself (same model as `akulav-mc-launcher`).

## Commands

```bash
cargo run
cargo test                                   # CI runs: cargo test --release
cargo clippy --all-targets -- -D warnings
cargo build --release
python assets/make_icons.py                  # regenerate icons
```

## Layout

- `src/core/` — the erasing engine: `erase.rs` (files/folders), `freespace.rs` (free-space wipe), `method.rs`
  (overwrite methods / passes), `rng.rs`, `drives.rs`, `progress.rs`
- `src/app/` — app plumbing: `paths.rs`, `settings.rs`, `net.rs`, `update.rs` (GitHub releases + `SHA256SUMS.txt`),
  `install/` (Windows / Unix self-install)
- `src/ui/` — egui app (`app.rs`, `pages.rs`, `theme.rs`, `widgets.rs`); `src/cli.rs` — CLI flags
- `installer/fast-disk-eraser.iss` — Inno Setup script. **Never change its AppId.**

## Rules

- **This program destroys data.** Never run an erase in tests or manual checks against anything but a scratch file
  or folder you created for it. Never point it at a real drive, the repo, or the user's folders.
- Erasure must fail loudly: if a pass, flush or delete fails, report it; never report success for a partial wipe.
- Don't follow symlinks/junctions out of the selected folder when erasing a tree.
- Release asset names are read by installed copies' updater: keep them. Version in `Cargo.toml`; `Cargo.lock` is
  committed.
- Linux/macOS-only code can't be built on the Windows dev machine: review by hand, let CI check it.

## graphify

This project has a knowledge graph at graphify-out/ with god nodes, community structure, and cross-file relationships. It is local only (`graphify-out/` is git-ignored).

Rules:
- When graphify-out/graph.json exists, run `graphify query "<question>"` before your first grep, search or source-file read for a codebase question, and put this rule in every subagent prompt that explores code. Read raw files only for lines the graph or an earlier read already located. Use `graphify path "<A>" "<B>"` for relationships and `graphify explain "<concept>"` for focused concepts.
- Read graphify-out/GRAPH_REPORT.md only for broad architecture review or when query/path/explain do not surface enough context.
- After modifying code, run `graphify update .` to keep the graph current (AST-only, no API cost). If graphify-out/ is missing, rebuild it the same way.
