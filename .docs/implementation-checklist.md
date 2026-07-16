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
- [x] Make `cargo test` pass with meaningful tests.
- [x] Confirm a clean source checkout can build.

## E1 - Core Architecture And Visual Shell

- [x] Add mode-aware storage root and atomic state writes.
- [x] Migrate config to typed Serde structures with schema versioning.
- [x] Support Light, Dark, and System theme modes.
- [x] Add a unified `CommandId` registry and enabled-state model.
- [x] Route toolbar, context menu, shortcuts, and action bar through commands.
- [x] Add shortcut capture, validation, conflict detection, reset, import, and export.
- [x] Add path-based stable item IDs.
- [x] Add background task/event infrastructure.
- [x] Move scans and long operations off the egui frame thread.
- [x] Split large UI sections out of `src/app.rs`.
- [x] Rebuild the approved `design/` shell in reusable egui components.
- [x] Add first-run source/cache/index setup.
- [x] Add tabbed Settings.
- [x] Add compact source/favorites/recent sidebar.
- [x] Add collapsible details panel.
- [x] Fix inline rename so row/tile geometry never moves.

## E2 - Explorer Interaction

- [x] Add path-based multi-selection.
- [x] Add Ctrl toggle, Shift range, Ctrl+A, Escape, and keyboard focus movement.
- [x] Add optional selection checkboxes.
- [x] Add rectangle selection in grid/gallery.
- [x] Add Back, Forward, Up, breadcrumbs, and refresh.
- [x] Restore prior selection and scroll state per folder.
- [x] Add Gallery view.
- [x] Add Grid view.
- [x] Add Compact List view.
- [x] Add Details view.
- [x] Add natural sorting by name, date, size, and type with folders first.
- [x] Add selection-driven side preview and Space full viewer.
- [x] Add Open, Open With, and Reveal in Windows Explorer.
- [x] Add hidden-file toggle while keeping protected system files hidden.

## E3 - Professional File Operations

- [x] Add New Folder.
- [~] Add Windows Shell copy/cut clipboard interoperability.
- [x] Keep an internal clipboard fallback.
- [x] Add Replace, Keep Both, Skip, and Apply to All collisions.
- [x] Add streamed copy and move workers.
- [x] Add Windows-style internal drag/drop with modifier indicator.
- [!] Prove and implement outbound OLE drag/drop to Windows Explorer.
- [~] Send normal Delete to Windows Recycle Bin.
- [x] Require confirmation for Shift+Delete permanent deletion.
- [!] Prove exact Ctrl+Z restore for tracked Recycle Bin deletion.
- [x] Add Task Center progress, speed, ETA, pause, cancel, retry, and errors.
- [~] Add bounded Undo history for rename, move, paste, new folder, and delete.
- [x] Add risk-based confirmation settings.

## E4 - Index And Product-Code Search

- [x] Add configured sources and source availability state.
- [x] Add per-source prefix/regex product-code extraction.
- [x] Add local SQLite schema and migrations.
- [x] Build the initial index asynchronously in bounded batches.
- [x] Add incremental updates for local filesystems.
- [!] Validate network-share watching and define polling reconciliation.
- [x] Add pause, resume, rebuild, progress, and corruption recovery.
- [x] Add Quick Find `starts with` matching.
- [x] Select and scroll to result one while typing without opening it.
- [x] Show top 10 product-folder matches.
- [x] Add clearly separated Contains and Fuzzy fallbacks.
- [x] Add Current, Current Source, and All Sources scopes.
- [x] Reveal the selected result in its parent folder.
- [x] Group multiple images into one product result card.
- [x] Meet the 100,000-file p95 search target.

## E5 - Find Product By Image

- [x] Accept query image from file picker.
- [x] Accept query image from clipboard paste.
- [x] Accept query image by drag/drop.
- [x] Compute and persist real perceptual hashes.
- [x] Rank the best image per product folder.
- [x] Show top 10 by default with adjustable 5-50 result count.
- [x] Show scores, matched image, source, path, and low-confidence warning.
- [x] Add local Model Pack manifest and discovery.
- [x] Support a custom local Model Pack path.
- [!] Benchmark and select the ONNX CLIP vision model/runtime.
- [~] Add a local `LuxePixAI.exe` inference bridge; the validated client
  protocol, timeout, and failure handling are complete, but the helper binary
  remains coupled to the ONNX feasibility spike.
- [~] Rerank a bounded pHash shortlist with CLIP when a validated helper is
  present; final runtime proof awaits the selected Model Pack.
- [x] Fall back to pHash when the Model Pack is unavailable or fails.

## E6 - Update, Installer, Portable, And Releases

- [x] Set GitHub owner/repository and public visibility.
- [x] Add non-blocking startup update check.
- [x] Add manual `Check for Updates`.
- [x] Show version, notes, size, and approval before download.
- [x] Add release manifest and semantic-version validation.
- [x] Add SHA-256 and byte-length verification.
- [x] Add cancellable update download in Task Center.
- [x] Build `LuxePixUpdater.exe`.
- [x] Prove portable replacement while the application executable is locked.
- [x] Preserve portable `data` and `.portable`.
- [x] Add startup handshake and portable rollback.
- [x] Add Inno Setup installer with stable AppId.
- [x] Confirm removal/replacement of the prior installed version.
- [x] Preserve AppData during update and default uninstall.
- [x] Reopen LuxePix after installed update.
- [x] Add tag-driven GitHub Release workflow.
- [x] Produce portable ZIP, installer, manifest, checksums, and notes.

## E7 - Power Tools

- [x] Batch Rename with preview.
- [x] Duplicate Finder.
- [x] Archive and extract.
- [x] Folder Size analysis.

## Quality And Acceptance

- [x] Add unit tests for commands, shortcuts, selection, sorting, extraction, collisions, versions, updates, power tools, and byte-bounded thumbnail caching.
- [x] Add temp-directory integration tests for file operations and Undo.
- [x] Add SQLite migration, rebuild, and search tests.
- [x] Add pHash grouping and ranking tests.
- [x] Add portable updater transaction and rollback tests.
- [x] Capture Light/Dark/System screenshots.
- [x] Capture stable inline-rename screenshots.
- [x] Smoke-test installer and portable build with isolated Windows host fixtures.
- [!] Repeat the installer and portable smoke in Windows Sandbox; Sandbox was unavailable in this release session.
- [x] Verify first window appears within 1.5 seconds on the reference machine.
- [x] Verify 2,000-item folder metadata is usable within 300 ms.
- [x] Verify 100,000-entry prefix search is below 100 ms at p95.
- [x] Verify thumbnail memory stays inside the configurable byte budget.
- [x] Confirm no image upload, telemetry, or persistent diagnostic logs.

## Evidence Log

- 2026-07-16: Planning wiki lint passed for 7 pages with 47 locked decisions.
- 2026-07-16: Pre-implementation `cargo test` passed with zero tests and three
  existing dead-code warnings.
- 2026-07-16: Baseline committed as `bc86613`; default branch renamed to
  `main`; implementation branch created as `codex/luxepix-explorer`.
- 2026-07-16: Rust `1.93.1` format and strict Clippy checks passed after
  baseline formatting and dead-code cleanup; `cargo test` still contains zero
  test cases.
- 2026-07-16: Clean clone of commit `3307014` completed a locked release build.
- 2026-07-16: Added baseline tests for image-extension detection and byte-size
  formatting.
- 2026-07-16: Completed the first E1 core slice: installed/portable storage,
  transactional config writes, typed schema migration, System theme support,
  unified command enablement/dispatch, tabbed Settings, and captured shortcut
  profiles with validation, conflict detection, reset, import, and export.
- 2026-07-16: E1 core verification passed with format, strict Clippy, 12 unit
  tests, locked release build, and a release executable startup smoke test.
- 2026-07-16: Replaced index-based folder/file selection with path-based
  `ItemId`, added browser-style Back/Forward/Up history, and moved root/folder
  scans onto typed background tasks with stale-result rejection.
- 2026-07-16: Background-scan verification passed with format, strict Clippy,
  16 unit tests, locked release build, and an asynchronous startup smoke test.
- 2026-07-16: Completed the E1 visual shell with reusable sidebar, details,
  and first-run modules; source/favorite persistence; responsive compact
  layout; and a fixed-geometry inline rename editor.
- 2026-07-16: E1 visual-shell verification passed with format, strict Clippy,
  18 unit tests, locked release build, and a four-second release startup smoke
  test.
- 2026-07-16: Completed E2 explorer interaction with stable multi-selection,
  keyboard and rectangle selection, Gallery/Grid/Compact/Details views,
  natural sorting, clickable breadcrumbs with configurable Ctrl+L address
  focus, per-folder selection/scroll restoration, preview/open/reveal
  commands, and hidden-file filtering.
- 2026-07-16: E2 verification passed with format, strict Clippy, 27 unit tests,
  a locked release build, and a four-second release startup smoke test.
- 2026-07-16: Implemented the E3 professional-operation core with New Folder,
  hybrid Windows/internal file clipboard handling, explicit collision
  decisions, streamed copy/move, internal and Explorer-inbound drag/drop,
  Recycle Bin and permanent-delete paths, bounded Undo, and a non-blocking
  Task Center with pause, cancel, retry, progress, speed, ETA, and errors.
- 2026-07-16: E3 automated verification passed with format, strict Clippy,
  42 tests, temp-directory copy/Undo round trips, collision and retry tests,
  a locked release build, and a four-second release startup smoke test.
  Manual Explorer clipboard round-trip and Recycle Bin smoke verification
  remain open; outbound OLE drag and exact Recycle Bin restore remain
  feasibility spikes.
- 2026-07-16: Completed E4 with mode-aware SQLite WAL storage, schema
  migration and corrupt-database recovery, asynchronous 100-product batches,
  operation-triggered and scheduled reconciliation, source availability,
  per-source folder/prefix/regex extraction, and pause/resume/rebuild controls.
- 2026-07-16: Quick Find verification passed for strict prefix results,
  separate Contains/Fuzzy fallbacks, Current/Current Source/All Sources
  scopes, ten-result limits, grouped product images, and automatic
  select/scroll/reveal of result one without opening the product folder.
- 2026-07-16: E4 automated gates passed with format, strict Clippy, 52 regular
  tests, SQLite migration/rebuild/reconciliation/search tests, a locked
  release build, and the ignored release-mode 100,000-product p95 performance
  gate. A portable light-theme smoke indexed three product folders and six
  images, opened Quick Find with Ctrl+F, matched `104`, and selected
  `SKU-1042 Blue Chrome` in place. Native network-share watcher validation
  remains a feasibility item; bounded per-source polling is implemented.
- 2026-07-16: Completed the E5 local image-search core with a real 32x32 DCT
  perceptual hash, SQLite schema v2 migration and hash persistence, best-image
  product grouping, adjustable top 5-50 results, confidence labeling, matched
  image/source/path details, reveal navigation, and picker/bitmap
  clipboard/file-drop input.
- 2026-07-16: Added checksummed Model Pack discovery, a custom local path,
  bounded `LuxePixAI.exe` JSON bridge protocol with a 15-second timeout, and
  automatic pHash fallback. ONNX model/runtime selection and the distributable
  helper remain a feasibility spike rather than being claimed as complete.
- 2026-07-16: E5 automated gates passed with format, strict Clippy, 57 regular
  tests including database migration, real hash persistence, product grouping,
  ranking and Model Pack validation, a locked release build, and a five-second
  release executable startup smoke test.
- 2026-07-16: Completed E6 with non-blocking startup/manual GitHub checks,
  semantic manifest validation, verified background downloads, a separate
  updater helper, portable replacement/handshake/rollback, Inno Setup packaging,
  and a tag-driven release workflow for `ngojclee/LuxePix`.
- 2026-07-16: Portable update smoke held the old executable open, waited for
  its process to exit, replaced it, completed the startup handshake, retained
  `data`, `.portable`, and unrelated adjacent files, and left no transaction
  debris.
- 2026-07-16: Installer smoke with `/CURRENTUSER` completed install, update,
  and uninstall with exit code 0; replaced a deliberately damaged updater,
  reopened LuxePix after update, removed uninstall registration, and retained
  AppData by default.
- 2026-07-16: Completed E7 Batch Rename, Duplicate Finder, ZIP create/extract,
  and Folder Size analysis with preview, safety validation, background work,
  Task Center integration, and temp-directory tests.
- 2026-07-16: Captured Light, Dark, System, and stable inline-rename evidence
  under `.docs/evidence/v1.1.0/`; first-window timing was 533 ms.
- 2026-07-16: Release-mode performance gates measured 37.6846 ms p95 for
  metadata from a 2,000-item folder and 9.9372 ms p95 for prefix search across
  100,000 products.
- 2026-07-16: Replaced count-based thumbnail eviction with configurable
  byte-bounded LRU caching (256 MB default, 64-2,048 MB range), bounded the
  decode result queue, and added focused budget/eviction tests.
- 2026-07-16: Source audit found outbound network access only in the GitHub
  updater and explicit release-link opening. No image upload, telemetry,
  analytics, or persistent diagnostic logging was found.
- 2026-07-16: Final automated candidate gates passed with 4 updater tests,
  72 application tests, and 2 explicit release-only performance tests.
  Remaining feasibility items are documented in E3-E5 and the Windows Sandbox
  repeat remains an environment follow-up.
