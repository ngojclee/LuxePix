---
title: LuxePix Implementation Plan
created: 2026-07-16
updated: 2026-07-16
type: query
tags: [luxepix, roadmap, desktop-app, windows, file-management, search, indexing, image-similarity, shortcuts, portable, safety, performance]
sources:
  - raw/articles/filebrowser-quantum-2026-07-16.md
  - raw/articles/filebrowser-original-2026-07-16.md
confidence: high
---

# LuxePix Implementation Plan

This plan turns all locked choices in
[[luxepix-file-explorer-roadmap-and-questionnaire]] and
[[luxepix-explorer-extended-decisions]] into an executable Rust delivery path.
It keeps the image/product-first direction from [[luxepix-vs-filebrowser]] and
does not embed either File Browser web application.

## Delivery Mode

- Work shape: multi-epic, high-risk Windows desktop upgrade.
- Preserve the current `eframe/egui 0.29` app and supplied `design/` language.
- Refactor first because `src/app.rs` owns UI, state, shortcuts, and file work.
- Keep all source images, query images, indexes, and embeddings local.
- Ship English UI, three theme modes, no telemetry, and no persistent log.

## Target Module Map

| Path | Responsibility |
|---|---|
| `src/app.rs` | Thin application shell, frame update, modal routing, and top-level composition |
| `src/model.rs` | Stable item/source IDs, file metadata, view state, and shared result types |
| `src/commands.rs` | Command IDs, labels, icons, default shortcuts, enabled state, and dispatch |
| `src/selection.rs` | Focus, anchor, Ctrl/Shift ranges, rectangle selection, and select-all |
| `src/navigation.rs` | Sources, favorites, breadcrumbs, Back/Forward/Up, scroll and selection restore |
| `src/views/` | Sidebar, bars, panes, four view modes, details panel, Settings, dialogs, and viewer |
| `src/tasks.rs` | Background task registry, progress, speed, ETA, pause, cancel, retry, and transient errors |
| `src/file_ops/` | Operation planning, collision decisions, streamed copy/move, rename, and new folder |
| `src/windows_shell.rs` | Shell clipboard, Recycle Bin, Open With, Reveal, and Windows drag/drop adapter |
| `src/undo.rs` | Bounded inverse-operation history and Ctrl+Z execution |
| `src/storage.rs` | Installed/portable data roots, directory creation, config migration, and atomic writes |
| `src/index/` | SQLite schema, scanner, incremental updates, watcher/poll fallback, and rebuild |
| `src/search.rs` | Quick filter, product-code rules, prefix/contains/fuzzy ranking, and reveal |
| `src/similarity/` | pHash indexing, shortlist/ranking, product grouping, and optional CLIP bridge |
| `src/updater.rs` | Release checks, manifest parsing, asset selection, download, and checksum verification |
| `src/bin/luxepix_updater.rs` | Out-of-process portable replacement, restart handshake, and rollback |
| `installer/LuxePix.iss` | Inno Setup installer, upgrade confirmation, AppData preservation, and restart |
| `.github/workflows/` | CI plus tag-driven Windows build and GitHub Release publication |

## Foundation Contracts

- Use path-based stable IDs so sorting, refresh, rename, and scans cannot silently move selection.
- Every button, menu item, shortcut, and selection action invokes one `CommandId` dispatcher.
- A command owns one enabled-state predicate and one execution path.
- Shortcut profiles store command IDs plus a schema version and reject conflicts before save/import.
- Typed task events keep scans, operations, indexing, hashing, downloads, and inference off the egui frame thread.
- State files use temp-write plus atomic replace; SQLite uses WAL and explicit migrations.
- Rename places its editor inside a preallocated label rectangle, so tile/list geometry never changes.

## Data Layout

Installed mode root: `%APPDATA%\LuxePix`.

Portable mode root: `<LuxePix directory>\data`, detected by `.portable`.

- Both roots contain `config.json`, session-only `state.json`,
  `index/luxepix.db`, `thumbnails/`, `updates/`, and `similarity/`.
- `updates/` contains temporary verified downloads and recovery state and is
  cleaned after success.
- Source folders, including network shares, never receive a database or sidecar.

## Epic Map

### E0 - Repository And Reproducible Baseline

- Preserve the current untracked workspace, add an appropriate `.gitignore`,
  create the first reviewed baseline commit, and rename `master` to `main`.
- Add `cargo fmt --check`, `cargo clippy -- -D warnings`, tests, and Windows
  release build to CI.
- Set version from `Cargo.toml`; tags use `vMAJOR.MINOR.PATCH`.
- Exit: a clean clone builds the same executable and CI passes on `main`.

### E1 - Application Core And Visual Shell

- Introduce storage mode, typed config with migration, command registry,
  selection model, navigation history, and task/event infrastructure.
- Rebuild the supplied design in egui with reusable palette and spacing tokens.
- Implement Light, Dark, and System themes without changing layout between themes.
- Add the compact source/favorites/recent sidebar, contextual action bar,
  details panel, stable inline rename, first-run setup, and tabbed Settings.
- Settings tabs: General, Appearance, Shortcuts, Search and Index, File
  Operations, and Advanced.
- Exit: existing behavior remains available, rename never reflows, and command
  availability is identical from every UI surface.

### E2 - Explorer Interaction

- Add Ctrl/Shift multi-select, optional checkboxes, Ctrl+A, Escape, keyboard
  focus movement, and rectangle selection in every view.
- Add breadcrumbs, Back, Forward, Up, refresh, per-folder scroll restoration,
  and previous-item reveal.
- Add Gallery, Grid, Compact List, and Details views with natural sorting by
  name, modified date, size, and type; folders remain first.
- Selection updates the side preview; Space opens the large viewer.
- Add Open, Open With, Reveal in Windows Explorer, hidden-file toggle, and
  protected-system-file filtering.
- Exit: mouse and keyboard selection behave consistently in all four views and
  returning to a folder restores the prior location.

### E3 - Professional File Operations

- Implement Shell copy/cut interoperability using file-drop clipboard data and
  preferred drop effect, with an internal fallback.
- Implement New Folder, rename, streamed copy/move, paste, Windows-style
  internal drag/drop, Explorer inbound/outbound drag/drop, and collision
  decisions: Replace, Keep Both, Skip, and Apply to All.
- Normal Delete goes to the Windows Recycle Bin; Shift+Delete is permanent and
  always requires explicit confirmation.
- Show long operations in Task Center with progress, speed, ETA, pause, cancel,
  retry, and per-item transient errors.
- Record inverse actions for rename, move, paste, new folder, and Recycle Bin
  delete; Ctrl+Z executes the latest still-valid inverse.
- Exit: Explorer-to-LuxePix and LuxePix-to-Explorer clipboard tests pass;
  cancellation leaves valid files; destructive paths have recovery or an
  explicit permanent warning.

### E4 - Local Index And Product-Code Search

- Add configured sources with availability status and per-source product-code
  extraction using prefix or regex rules with folder-name fallback.
- Build the SQLite index in batches and update it incrementally. Use filesystem
  notifications where reliable and a low-cost periodic reconciliation for
  network shares.
- Quick Find first applies case-insensitive `starts with`, shows up to ten
  product folders, and immediately selects/scrolls to result one without
  opening it.
- When strict prefix results are absent, display separate Contains and Fuzzy
  suggestion groups. Enter or double-click reveals the chosen folder.
- Advanced search supports Current, Current Source, and All Sources scopes and
  groups multiple images under one product card.
- Add pause, resume, rebuild, progress, and corruption recovery.
- Exit: a known product prefix is revealed in at most two actions and search
  never blocks browsing.

### E5 - Find Product By Image

- Accept query images from file picker, clipboard paste, and drag/drop.
- Compute and store a real perceptual hash for each indexed image.
- Group images by extracted product ID and keep the best image score per product.
- Return ten products by default, adjustable from 5 to 50, with score, matched
  image, source, path, and a low-confidence warning where applicable.
- Add an optional Model Pack containing `LuxePixAI.exe`, a CLIP vision ONNX
  model, runtime dependencies, and a checksummed `model-pack.json`.
- Auto-detect the pack beside LuxePix and support a custom local path. The
  helper stays loaded, communicates locally, and never receives network URLs.
- Use pHash for the core/offline result and CLIP to rerank a bounded shortlist;
  helper failure immediately falls back to pHash.
- Exit: missing/corrupt Model Pack cannot break browsing, and every result can
  reveal the matched product folder.

### E6 - Updates, Installer, And Portable Release

- Implement startup/manual GitHub checks, checksummed downloads, Inno Setup,
  portable replacement, restart handshakes, and rollback.
- The full packaging transaction, CI design, artifact contract, and quality
  gates are specified in [[luxepix-release-update-and-quality-plan]].
- Exit: upgrade and forced-failure rollback pass in Windows Sandbox for both
  package modes.

### E7 - Power Tools After Core Acceptance

- Deliver Batch Rename with preview, Duplicate Finder, archive/extract, and
  folder-size analysis in that order.
- Reuse commands, Task Center, collision handling, and Undo rather than adding
  separate execution paths.

## Quality Gate

All unit, integration, Shell, UI, performance, packaging, and rollback proof in
[[luxepix-release-update-and-quality-plan]] is mandatory.

## Mandatory Feasibility Spikes

1. Prove exact Recycle Bin restore for a tracked deletion through Windows Shell.
2. Prove outbound OLE drag/drop from egui to Windows Explorer with copy/move modifiers.
3. Prove portable updater replacement and rollback while the original executable is locked.
4. Benchmark ONNX CLIP CPU inference and package size before choosing the final model.
5. Verify network-share change detection and define the reconciliation interval.

## Remaining External Input

- Set GitHub owner/repository and visibility before activating update CI; the plan assumes public Releases.

## Definition Of Done

LuxePix is done when a clean Windows machine passes all 47 decisions without
losing state or depending on Python, cloud services, telemetry, or saved logs.
