---
title: LuxePix vs File Browser
created: 2026-07-16
updated: 2026-07-16
type: comparison
tags: [luxepix, filebrowser, filebrowser-quantum, comparison, ux, safety]
sources:
  - raw/articles/filebrowser-quantum-2026-07-16.md
  - raw/articles/filebrowser-original-2026-07-16.md
confidence: high
---

# LuxePix vs File Browser

Based on [[filebrowser-quantum]] and [[filebrowser-original]], LuxePix already
has a good image-first foundation, but it does not yet have a complete
selection, navigation, command, safety, and search model.

## Current LuxePix Baseline

The current Rust app already provides:

- Dual-pane folder overview and file grid.
- Async image thumbnails and full-screen image preview.
- Light/dark themes and portable configuration.
- Path input, recent path history, refresh, and folder picker.
- Single selection, arrow navigation, open, copy, cut, paste, rename, delete,
  and configurable shortcuts.
- File/folder context menus and a selection action bar.

Important current limits:

- Only one item can be selected.
- File operations use an internal LuxePix clipboard.
- Delete is permanent instead of using the Windows Recycle Bin.
- There is no collision decision UI, undo, new-folder command, drag/drop,
  native clipboard interoperability, sorting control, list view, breadcrumb
  navigation, back/forward history, indexed search, favorites, or details
  inspector.
- The scanner is non-recursive for each opened folder and rescans synchronously
  when navigating.

## Feature Priority Matrix

| Feature | LuxePix now | User value | Effort | Recommendation |
|---|---|---:|---:|---|
| Unified command registry | Commands are duplicated across UI surfaces | Very high | Medium | P0 foundation |
| Multi-select with Ctrl/Shift | Missing | Very high | Medium | P0 |
| Rectangle selection | Missing | High | Medium | P1 after multi-select |
| Windows clipboard interoperability | Internal clipboard only | Very high | High | P0 |
| Drag/drop files and folders | Missing | High | High | P1 |
| New folder | Missing | High | Low | P0 |
| Recycle Bin and Shift+Delete | Permanent delete | Critical safety | Medium | P0 |
| Conflict dialog: replace/rename/skip | Auto-generates copy names | Very high | Medium | P0 |
| Breadcrumb, Back, Forward, Up | Path text only | Very high | Medium | P0 |
| Restore selection and scroll | Missing | High | Medium | P1 |
| Grid, gallery, and list views | Grid only | High | Medium | P1 |
| Sort by name/date/size/type | Fixed images-first/name | High | Low | P0 |
| Quick filter in current folder | Missing | High | Low | P0 |
| Jump by product-code prefix | Missing | Very high | Medium | P0 product feature |
| Indexed search across roots | Missing | Very high | High | P1 |
| Reveal result in parent folder | Missing | Very high | Medium | P0 with search |
| Image-similarity top 10 | Missing | Very high | High | P2 product feature |
| Favorites/pinned folders | Missing | Medium-high | Low | P1 |
| Details/EXIF inspector | Preview only | High for images | Medium | P1 |
| Duplicate finder | Missing | Medium | Medium-high | P2 |
| Batch rename | Missing | Medium-high | Medium | P2 |
| Archive/extract | Missing | Medium | Medium | P2 |
| Text/Office editor | External open only | Low for core workflow | Very high | Defer |
| Users/shares/auth/WebDAV | Missing | Low for desktop app | Very high | Do not add now |

## Recommended Product Shape

LuxePix should become an **image/product-first explorer**, not a complete
Windows Explorer replacement.

Keep the current visual advantage:

- Folder contact sheet on the left.
- Contents and product images on the right.
- Strong preview and future similarity search.

Add professional explorer behavior underneath:

- A single selection model.
- A single command registry.
- Native Windows file operation adapters.
- Persistent navigation and search state.
- Safe file operations with clear conflict and recovery paths.

## Architecture Direction

Reimplement behavior in Rust rather than embedding either web project.
Apache-2.0 permits reuse with notices, but the frameworks and runtime model do
not fit the current `eframe/egui` application.

Suggested modules:

| Module | Responsibility |
|---|---|
| `commands.rs` | Command IDs, labels, icons, shortcuts, enabled state, execution |
| `selection.rs` | Focused item, anchor, Ctrl/Shift range, rectangle selection |
| `navigation.rs` | Sources, breadcrumbs, back/forward/up, per-folder scroll state |
| `file_ops.rs` | Copy/move/new folder/rename/conflicts/progress/cancel |
| `windows_shell.rs` | Recycle Bin and Windows clipboard integration |
| `indexer.rs` | Local SQLite metadata index per configured source |
| `search.rs` | Prefix jump, current-folder filter, global search, reveal result |
| `similarity.rs` | pHash shortlist and optional CLIP ranking bridge |

The index database should live in `%APPDATA%\LuxePix` or beside the executable
in portable mode. Do not put the SQLite database on `N:`.

The implementation sequence is captured in
[[luxepix-file-explorer-roadmap-and-questionnaire]].

