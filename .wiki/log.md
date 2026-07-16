# Wiki Log

> Chronological record of wiki actions.

## [2026-07-16] create | LuxePix product research wiki initialized

- Domain: LuxePix file explorer, product-code search, and image similarity.
- Created `SCHEMA.md`, `index.md`, and `log.md`.

## [2026-07-16] ingest | File Browser official projects and local deployment

- Added source notes for FileBrowser Quantum and original File Browser.
- Added entity pages for both projects.
- Added a LuxePix feature-gap comparison.
- Filed the roadmap and product-decision questionnaire.

## [2026-07-16] lint | 0 issues found

- Validated four wiki pages and two raw source notes.
- Confirmed frontmatter, tag taxonomy, index completeness, wikilinks, inbound
  links, page sizes, and body-only source checksums.

## [2026-07-16] update | LuxePix explorer decision LPX-EXP-001 locked

- Recorded choice B: LuxePix is an image/product explorer with professional
  file operations.
- Remaining product decisions will be collected in short batches.

## [2026-07-16] update | LuxePix explorer decisions LPX-EXP-002 to LPX-EXP-008 locked

- Kept the collapsible two-pane layout with a compact navigation sidebar.
- Locked familiar Windows selection, Recycle Bin, hybrid clipboard, collision
  handling, and four view modes.

## [2026-07-16] update | LuxePix explorer decisions LPX-EXP-009 to LPX-EXP-015 locked

- Locked sorting, search scope, top-10 prefix matching, result reveal behavior,
  local index storage, hybrid image similarity, and all query-image inputs.
- Refined Quick Find to scroll to and select the first result immediately
  without opening its folder.
- Added pending decisions for shortcut configuration, stable inline rename,
  and drag-and-drop behavior.

## [2026-07-16] update | LuxePix explorer decisions LPX-EXP-016 to LPX-EXP-021 locked

- Locked the details panel, power-tool order, 100,000-file target, portable
  shortcut profiles, fixed-geometry inline rename, and Windows-style drag/drop.
- Added pending decisions for context menus, command placement, Settings,
  themes, preview behavior, and background indexing.

## [2026-07-16] update | LuxePix explorer decisions LPX-EXP-022 to LPX-EXP-027 locked

- Locked the structured context menu, contextual action bar, tabbed Settings,
  Light/Dark/System theme switching, dual preview model, and incremental index.
- Created `queries/luxepix-explorer-extended-decisions.md` for the next decision
  batches and linked it from the roadmap and index.

## [2026-07-16] update | LuxePix explorer decisions LPX-EXP-028 to LPX-EXP-033 locked

- Locked the Task Center, multi-step Undo, navigation sidebar, external opening,
  hidden-file policy, and risk-based confirmations.
- Added pending product-search and local image-similarity decisions
  `LPX-EXP-034` through `LPX-EXP-040`.

## [2026-07-16] update | LuxePix explorer decisions LPX-EXP-034 to LPX-EXP-040 locked

- Locked grouped product results, prefix fallback, adjustable result count,
  confidence warnings, per-source product-code rules, local-only processing,
  and the optional AI Model Pack.
- Added the final release, portable-storage, update, language, onboarding, and
  diagnostics decisions `LPX-EXP-041` through `LPX-EXP-047`.

## [2026-07-16] update | LuxePix explorer decisions LPX-EXP-041 to LPX-EXP-047 locked

- Locked installer plus portable releases, mode-aware data storage, GitHub
  updates, AI Model Pack discovery, English-only UI, first-run setup, and no
  persistent diagnostics.
- Recorded separate portable replacement and installed uninstall/reinstall
  update workflows with checksum verification, restart, and rollback behavior.
- All product decisions `LPX-EXP-001` through `LPX-EXP-047` are now locked.

## [2026-07-16] create | LuxePix implementation and release plans

- Added `queries/luxepix-implementation-plan.md` and
  `queries/luxepix-release-update-and-quality-plan.md`.
- Defined the Rust module split, seven core delivery epics plus deferred power
  tools, acceptance criteria, tests, and 100,000-file performance gates.
- Specified Git initialization, CI, tag-driven GitHub Releases, Inno Setup,
  the portable updater helper, checksum validation, restart, and rollback.
- Linked both plans from the extended decision register, each other, and the
  wiki index.

## [2026-07-16] lint | 0 issues found

- Validated seven wiki pages, required frontmatter, taxonomy tags, page sizes,
  index completeness, inbound/outbound wikilinks, and raw source hashes.
- Confirmed all decisions `LPX-EXP-001` through `LPX-EXP-047` are present once
  in the locked registers with no missing IDs.
- `cargo test` passed with zero tests and the three pre-existing dead-code
  warnings in `scanner.rs` and `thumbs.rs`.
