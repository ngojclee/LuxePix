# LuxePix Implementation Checklist

Last updated: 2026-07-16

Status legend:

- `[x]` implemented and verified
- `[~]` implementation in progress
- `[ ]` not started
- `[!]` blocked by external input or an unresolved feasibility spike

This checklist follows:

- `.wiki/queries/luxepix-implementation-plan.md`
- `.wiki/queries/luxepix-release-update-and-quality-plan.md`

## E0 - Repository And Reproducible Baseline

- [x] Review ignore rules and exclude build/runtime artifacts.
- [x] Record the current source, design, docs, and wiki in a baseline commit.
- [x] Rename the default branch from `master` to `main`.
- [x] Create the `codex/luxepix-explorer` implementation branch.
- [x] Add Windows CI for format, Clippy, tests, and release build.
- [x] Make `cargo fmt --check` pass.
- [x] Make `cargo clippy --all-targets -- -D warnings` pass.
- [ ] Make `cargo test` pass with meaningful tests.
- [ ] Confirm a clean source checkout can build.

## E1 - Core Architecture And Visual Shell

- [ ] Add mode-aware storage root and atomic state writes.
- [ ] Migrate config to typed Serde structures with schema versioning.
- [ ] Support Light, Dark, and System theme modes.
- [ ] Add a unified `CommandId` registry and enabled-state model.
- [ ] Route toolbar, context menu, shortcuts, and action bar through commands.
- [ ] Add shortcut capture, validation, conflict detection, reset, import, and export.
- [ ] Add path-based stable item IDs.
- [ ] Add background task/event infrastructure.
- [ ] Move scans and long operations off the egui frame thread.
- [ ] Split large UI sections out of `src/app.rs`.
- [ ] Rebuild the approved `design/` shell in reusable egui components.
- [ ] Add first-run source/cache/index setup.
- [ ] Add tabbed Settings.
- [ ] Add compact source/favorites/recent sidebar.
- [ ] Add collapsible details panel.
- [ ] Fix inline rename so row/tile geometry never moves.

## E2 - Explorer Interaction

- [ ] Add path-based multi-selection.
- [ ] Add Ctrl toggle, Shift range, Ctrl+A, Escape, and keyboard focus movement.
- [ ] Add optional selection checkboxes.
- [ ] Add rectangle selection in grid/gallery.
- [ ] Add Back, Forward, Up, breadcrumbs, and refresh.
- [ ] Restore prior selection and scroll state per folder.
- [ ] Add Gallery view.
- [ ] Add Grid view.
- [ ] Add Compact List view.
- [ ] Add Details view.
- [ ] Add natural sorting by name, date, size, and type with folders first.
- [ ] Add selection-driven side preview and Space full viewer.
- [ ] Add Open, Open With, and Reveal in Windows Explorer.
- [ ] Add hidden-file toggle while keeping protected system files hidden.

## E3 - Professional File Operations

- [ ] Add New Folder.
- [ ] Add Windows Shell copy/cut clipboard interoperability.
- [ ] Keep an internal clipboard fallback.
- [ ] Add Replace, Keep Both, Skip, and Apply to All collisions.
- [ ] Add streamed copy and move workers.
- [ ] Add Windows-style internal drag/drop with modifier indicator.
- [!] Prove and implement outbound OLE drag/drop to Windows Explorer.
- [ ] Send normal Delete to Windows Recycle Bin.
- [ ] Require confirmation for Shift+Delete permanent deletion.
- [!] Prove exact Ctrl+Z restore for tracked Recycle Bin deletion.
- [ ] Add Task Center progress, speed, ETA, pause, cancel, retry, and errors.
- [ ] Add bounded Undo history for rename, move, paste, new folder, and delete.
- [ ] Add risk-based confirmation settings.

## E4 - Index And Product-Code Search

- [ ] Add configured sources and source availability state.
- [ ] Add per-source prefix/regex product-code extraction.
- [ ] Add local SQLite schema and migrations.
- [ ] Build the initial index asynchronously in bounded batches.
- [ ] Add incremental updates for local filesystems.
- [!] Validate network-share watching and define polling reconciliation.
- [ ] Add pause, resume, rebuild, progress, and corruption recovery.
- [ ] Add Quick Find `starts with` matching.
- [ ] Select and scroll to result one while typing without opening it.
- [ ] Show top 10 product-folder matches.
- [ ] Add clearly separated Contains and Fuzzy fallbacks.
- [ ] Add Current, Current Source, and All Sources scopes.
- [ ] Reveal the selected result in its parent folder.
- [ ] Group multiple images into one product result card.
- [ ] Meet the 100,000-file p95 search target.

## E5 - Find Product By Image

- [ ] Accept query image from file picker.
- [ ] Accept query image from clipboard paste.
- [ ] Accept query image by drag/drop.
- [ ] Compute and persist real perceptual hashes.
- [ ] Rank the best image per product folder.
- [ ] Show top 10 by default with adjustable 5-50 result count.
- [ ] Show scores, matched image, source, path, and low-confidence warning.
- [ ] Add local Model Pack manifest and discovery.
- [ ] Support a custom local Model Pack path.
- [!] Benchmark and select the ONNX CLIP vision model/runtime.
- [ ] Add a local `LuxePixAI.exe` inference bridge.
- [ ] Rerank a pHash shortlist with CLIP.
- [ ] Fall back to pHash when the Model Pack is unavailable or fails.

## E6 - Update, Installer, Portable, And Releases

- [!] Set GitHub owner/repository and public/private visibility.
- [ ] Add non-blocking startup update check.
- [ ] Add manual `Check for Updates`.
- [ ] Show version, notes, size, and approval before download.
- [ ] Add release manifest and semantic-version validation.
- [ ] Add SHA-256 and byte-length verification.
- [ ] Add cancellable update download in Task Center.
- [ ] Build `LuxePixUpdater.exe`.
- [!] Prove portable replacement while the installed executable is locked.
- [ ] Preserve portable `data` and `.portable`.
- [ ] Add startup handshake and portable rollback.
- [ ] Add Inno Setup installer with stable AppId.
- [ ] Confirm removal/replacement of the prior installed version.
- [ ] Preserve AppData during update and default uninstall.
- [ ] Reopen LuxePix after installed update.
- [ ] Add tag-driven GitHub Release workflow.
- [ ] Produce portable ZIP, installer, manifest, checksums, and notes.

## E7 - Power Tools

- [ ] Batch Rename with preview.
- [ ] Duplicate Finder.
- [ ] Archive and extract.
- [ ] Folder Size analysis.

## Quality And Acceptance

- [ ] Add unit tests for commands, shortcuts, selection, sorting, extraction, collisions, and versions.
- [ ] Add temp-directory integration tests for file operations and Undo.
- [ ] Add SQLite migration, rebuild, and search tests.
- [ ] Add pHash grouping and ranking tests.
- [ ] Add portable updater transaction and rollback tests.
- [ ] Capture Light/Dark/System screenshots.
- [ ] Capture stable inline-rename screenshots.
- [ ] Smoke-test installer and portable build in Windows Sandbox.
- [ ] Verify first window appears within 1.5 seconds on the reference machine.
- [ ] Verify 2,000-item folder metadata is usable within 300 ms.
- [ ] Verify 100,000-entry prefix search is below 100 ms at p95.
- [ ] Verify thumbnail memory stays inside the configured byte budget.
- [ ] Confirm no image upload, telemetry, or persistent diagnostic logs.

## Evidence Log

- 2026-07-16: Planning wiki lint passed for 7 pages with 47 locked decisions.
- 2026-07-16: Pre-implementation `cargo test` passed with zero tests and three
  existing dead-code warnings.
- 2026-07-16: Baseline committed as `bc86613`; default branch renamed to
  `main`; implementation branch created as `codex/luxepix-explorer`.
- 2026-07-16: Rust `1.93.1` format and strict Clippy checks passed after
  baseline formatting and dead-code cleanup; `cargo test` still contains zero
  test cases.
