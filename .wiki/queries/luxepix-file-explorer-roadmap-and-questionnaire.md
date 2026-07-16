---
title: LuxePix File Explorer Roadmap and Questionnaire
created: 2026-07-16
updated: 2026-07-16
type: query
tags: [luxepix, roadmap, questionnaire, file-management, search, image-similarity]
sources:
  - raw/articles/filebrowser-quantum-2026-07-16.md
  - raw/articles/filebrowser-original-2026-07-16.md
confidence: high
---

# LuxePix File Explorer Roadmap and Questionnaire

This proposal follows the gap analysis in [[luxepix-vs-filebrowser]] and the
interaction patterns observed in [[filebrowser-quantum]].

## Locked Decisions

| ID | Locked choice | Recorded |
|---|---|---|
| LPX-EXP-001 | B - LuxePix is an image/product explorer with professional file operations. | 2026-07-16 |
| LPX-EXP-002 | B - Keep the two-pane layout by default, allow panels to collapse, and add a compact navigation sidebar. | 2026-07-16 |
| LPX-EXP-003 | A - Single-click selects, double-click opens, and F2 or an explicit command renames. | 2026-07-16 |
| LPX-EXP-004 | A - Use Windows-style Ctrl/Shift multi-selection with optional checkboxes. | 2026-07-16 |
| LPX-EXP-005 | A - Delete moves items to the Recycle Bin; Shift+Delete permanently deletes. | 2026-07-16 |
| LPX-EXP-006 | A - Use the Windows Shell clipboard with an internal LuxePix fallback. | 2026-07-16 |
| LPX-EXP-007 | A - Resolve collisions with Replace, Keep Both, Skip, and Apply to All. | 2026-07-16 |
| LPX-EXP-008 | C - Provide Gallery, Grid, Compact List, and Details view modes. | 2026-07-16 |
| LPX-EXP-009 | C - Sort by natural name, modified date, size, and type, with folders first. | 2026-07-16 |
| LPX-EXP-010 | C - Search the current root by default with Current and All Sources scope controls. | 2026-07-16 |
| LPX-EXP-011 | C - Show the top 10 prefix matches and immediately scroll to and select the first result while typing. | 2026-07-16 |
| LPX-EXP-012 | B - Single-click selects or previews; Enter or double-click reveals the result in its folder. | 2026-07-16 |
| LPX-EXP-013 | C - Store the index in AppData or beside the executable in portable mode, never on a network source. | 2026-07-16 |
| LPX-EXP-014 | C - Use pHash for the shortlist and optional CLIP reranking with automatic pHash fallback. | 2026-07-16 |
| LPX-EXP-015 | C - Accept query images through a file picker, clipboard paste, and drag-and-drop. | 2026-07-16 |
| LPX-EXP-016 | C - Use a collapsible details panel with file information, image dimensions, EXIF, and full path. | 2026-07-16 |
| LPX-EXP-017 | B - Build Batch Rename, Duplicate Finder, Archive tools, and Folder Size analysis in that order. | 2026-07-16 |
| LPX-EXP-018 | B - Keep browsing and indexed search smooth for collections of about 100,000 files. | 2026-07-16 |
| LPX-EXP-019 | C - Support editable shortcuts with conflict detection, reset defaults, and profile import/export. | 2026-07-16 |
| LPX-EXP-020 | A - Rename inside a fixed-size tile editor without changing row or tile geometry. | 2026-07-16 |
| LPX-EXP-021 | A - Follow Windows drag-and-drop copy/move rules with modifiers and a visible action indicator. | 2026-07-16 |
| LPX-EXP-022 | B - Use a structured professional context menu with primary actions and a More submenu. | 2026-07-16 |
| LPX-EXP-023 | B - Show a contextual selection action bar and expose the same commands in the context menu. | 2026-07-16 |
| LPX-EXP-024 | B - Organize Settings into General, Appearance, Shortcuts, Search and Index, File Operations, and Advanced tabs. | 2026-07-16 |
| LPX-EXP-025 | B - Provide complete Light and Dark themes plus a System switching mode. | 2026-07-16 |
| LPX-EXP-026 | C - Update the side panel on selection and open the large viewer with Space or an explicit Open command. | 2026-07-16 |
| LPX-EXP-027 | C - Maintain the index incrementally in the background with pause, resume, rebuild, and progress controls. | 2026-07-16 |

## Proposed Delivery Phases

### Phase 0 - Stabilize The Current File Manager

Goal: make existing operations safe and predictable before expanding scope.

- Replace duplicated toolbar/context-menu/shortcut logic with one command
  registry.
- Fix folder and file grid geometry so inline rename never reflows a row.
- Add New Folder.
- Add sort controls: natural name, modified date, size, type; ascending and
  descending; folders first.
- Move normal Delete to Windows Recycle Bin.
- Reserve `Shift+Delete` for permanent deletion with a stronger warning.
- Add a collision dialog for Replace, Keep Both, Skip, and Apply to All.
- Add progress and cancel state for long copy/move operations.

Exit criteria:

- Every destructive operation has a recovery or explicit permanent-delete
  path.
- The same command has identical enabled state from shortcut, toolbar, and
  context menu.
- Rename cannot shift neighboring tiles.

### Phase 1 - Professional Explorer Interaction

Goal: make daily browsing feel familiar to Windows users.

- Multi-select with `Ctrl`, range select with `Shift`, `Ctrl+A`, and Escape to
  clear.
- Rectangle selection in empty grid space.
- Windows Explorer clipboard interoperability with internal fallback.
- Drag/drop between folders and from Explorer into LuxePix.
- Breadcrumbs and Back, Forward, Up navigation.
- Restore previous selection and scroll position when returning to a folder.
- Add grid, gallery, compact list, and detailed list modes.
- Add optional details/EXIF inspector.
- Add favorites/pinned roots and improve Recent Paths into a navigation
  sidebar.

Exit criteria:

- Copy/cut in Explorer can paste into LuxePix, and LuxePix can paste into
  Explorer.
- All selection paths work consistently in every view mode.
- Back returns to the previous folder and reveals the previously opened item.

### Phase 2 - Product Search

Goal: solve the high-value LuxePix-specific lookup workflow.

- Add a command/search box opened with `/` or `Ctrl+F`.
- Immediate current-folder filtering while typing.
- Quick Find immediately scrolls to and selects the first result while keeping
  the top 10 result list visible; it does not open the folder automatically.
- Product-code mode matches folder names with case-insensitive `starts with`.
- Return the best 10 matches with thumbnail, product code/name, full path, and
  source badge.
- Single-click previews/selects; Enter or "Reveal in folder" navigates to the
  parent and selects the result.
- Add search scopes: Current folder, Current source, All configured sources.
- Persist a local SQLite index and update it in the background.

Exit criteria:

- A known code prefix can reveal its folder in no more than two interactions.
- Searches do not block the UI.
- Index corruption can be recovered by rebuilding without touching source
  files.

### Phase 3 - Find Product By Image

Goal: accept a query image and show the ten closest product candidates.

- Choose, paste, or drop a query image.
- Stage A: perceptual-hash shortlist for speed and offline portability.
- Stage B: optional CLIP embedding rerank, borrowing the proven mapping flow
  from
  `D:\Python\projects\LuxeClaw\0_Automation\product-mapper`.
- Show top 10 with score, matched image, folder/product code, and path.
- Reuse the same "Reveal in folder" action as text search.
- Cache embeddings and update only changed files.

Exit criteria:

- Results explain which image matched and where it lives.
- The feature remains usable without network access.
- A missing Python/CLIP helper degrades to pHash instead of breaking browsing.

### Phase 4 - Power Tools

- Duplicate finder.
- Batch rename with preview.
- Archive/extract.
- Folder size analysis.
- Slideshow and richer EXIF.
- Optional file watcher for external changes.

Text editing, Office integration, account management, web sharing, OIDC, LDAP,
and WebDAV remain deferred because they do not strengthen the core LuxePix
workflow.

## Product Decision Questionnaire

The "Recommended" column is the proposed default. Decisions should be locked
one at a time before implementation.

| ID | Decision question | Options | Recommended |
|---|---|---|---|
| LPX-EXP-001 | What is LuxePix's primary identity? | General file explorer / Image-product explorer / Pure image viewer | Image-product explorer with professional file operations |
| LPX-EXP-002 | Keep the current two-pane layout? | Always two-pane / Optional one- or two-pane / Replace with Explorer tree | Keep two-pane and add optional compact navigation sidebar |
| LPX-EXP-003 | What should a normal single click do? | Select only / Open immediately / Rename name text | Select only; double-click opens; F2 or explicit command renames |
| LPX-EXP-004 | Which multi-select convention? | Windows Ctrl/Shift / Checkbox mode only / Both | Windows Ctrl/Shift plus optional checkbox mode |
| LPX-EXP-005 | What should Delete do? | Permanent / Recycle Bin / Ask every time | Recycle Bin; `Shift+Delete` is permanent |
| LPX-EXP-006 | Which clipboard model? | LuxePix internal / Windows Shell / Hybrid | Windows Shell with internal fallback |
| LPX-EXP-007 | How should name collisions work? | Auto rename / Always ask / Configurable default | Ask with Replace, Keep Both, Skip, and Apply to All |
| LPX-EXP-008 | Which view modes are required? | Grid only / Grid+list / Gallery+grid+compact+details | Gallery, grid, compact list, and details |
| LPX-EXP-009 | Which sort fields are required? | Name only / Name+date / Name+date+size+type | Name, modified, size, type; natural numeric; folders first |
| LPX-EXP-010 | Where should quick code search look? | Current folder / Current root / All saved roots | Default current root, with Current/All scope toggle |
| LPX-EXP-011 | How should prefix matches behave? | Jump to first / Show all / Show top 10 | Show top 10 and preselect the strongest exact-prefix match |
| LPX-EXP-012 | What should clicking a search result do? | Open folder / Reveal and select / Preview only | Single-click preview/select; Enter or double-click reveal |
| LPX-EXP-013 | Where should the search index live? | Beside source files / Network drive / Local config or portable folder | Local config or portable folder, never on the network source |
| LPX-EXP-014 | Which image-similarity engine? | pHash / CLIP / Hybrid | Hybrid: pHash shortlist, optional CLIP rerank |
| LPX-EXP-015 | How are query images provided? | File picker / Paste / Drag-drop / All | All three |
| LPX-EXP-016 | Which metadata panel is needed? | None / Basic file info / File info+EXIF+dimensions | Collapsible file info, EXIF, dimensions, and full path |
| LPX-EXP-017 | Which advanced tool comes first? | Duplicates / Batch rename / Archive / Folder sizes | Batch rename, then duplicates, then archive/folder sizes |
| LPX-EXP-018 | What collection scale must remain smooth? | Under 10k / 10k-100k / Over 100k files | Design for at least 100k indexed files; confirm real maximum |
| LPX-EXP-019 | How configurable should shortcuts be? | Fixed / Configurable / Configurable with conflict detection, reset, import, and export | Configurable with conflict detection, reset defaults, import, and export |
| LPX-EXP-020 | How should inline rename behave? | Fixed-size inline editor / Rename dialog / Details-panel editor | Fixed-size inline editor that never changes tile or row geometry |
| LPX-EXP-021 | What should drag-and-drop do by default? | Windows-like copy/move rules / Always copy / Always ask | Windows-like copy/move rules with a visible action indicator and modifier override |
| LPX-EXP-022 | How should the item context menu be organized? | Minimal menu / Structured professional menu / Native Windows menu | Structured professional menu with primary actions and a More submenu |
| LPX-EXP-023 | Where should commands appear after selection? | Permanent toolbar / Contextual selection bar / Context menu only | Contextual selection bar plus the same commands in the context menu |
| LPX-EXP-024 | How should Settings be organized? | One long page / Category tabs / Separate windows | Tabs for General, Appearance, Shortcuts, Search and Index, File Operations, and Advanced |
| LPX-EXP-025 | How should theme selection work? | Manual Light or Dark / Light, Dark, or System / System only | Two complete Light and Dark themes with a System switching option |
| LPX-EXP-026 | How should image preview work? | Full-screen only / Side preview only / Side preview plus full-screen | Selection updates the side panel; Space or explicit Open enters the full-screen viewer |
| LPX-EXP-027 | How should the search index stay current? | Rescan on every search / Manual rebuild / Incremental background updates | Incremental background indexing and watching with pause, resume, and rebuild controls |

## Decision Status

- Locked: `LPX-EXP-001` through `LPX-EXP-027`.
- Extended decisions continue in [[luxepix-explorer-extended-decisions]].
