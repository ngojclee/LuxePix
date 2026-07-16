---
title: FileBrowser Quantum
created: 2026-07-16
updated: 2026-07-16
type: entity
tags: [filebrowser-quantum, web-app, file-management, search, indexing, ux]
sources: [raw/articles/filebrowser-quantum-2026-07-16.md]
confidence: high
---

# FileBrowser Quantum

FileBrowser Quantum is the project actually deployed under
`N:\projects\nailart-pro-ai\filebrowser`. It is not the same project as
[[filebrowser-original]], although it is a large fork of it.

## What It Does Well

- A compact action model: global actions live in a main action panel, while
  item-specific actions appear in a context menu.
- Search is always close at hand, opens with `/`, searches names and paths,
  and can be narrowed by source, file/folder, category, size, and date.
- Search results preserve full path context and can reveal an item in its
  parent folder.
- Listing state includes natural sorting, pinned-first ordering, view modes,
  thumbnail size, selection state, and restored scroll position.
- Multi-selection supports modifiers, keyboard selection, select all, and a
  rectangular selection gesture.
- Preview support is broad, but the most relevant LuxePix behavior is image
  navigation, zoom, EXIF information, and preserving the selected item when
  returning to the listing.

## What LuxePix Should Learn

The strongest reusable idea is not its web stack. It is the command model:
selection determines which commands are enabled, and the same command can be
called from a shortcut, toolbar, context menu, or search result.

The second reusable idea is search-result navigation. LuxePix can use the same
interaction for product-code search and image-similarity results:

1. Show a ranked result with thumbnail, product/folder name, and full path.
2. Single-click selects and previews the result.
3. "Reveal in folder" navigates to the parent and selects the item.
4. Double-click opens the folder or preview.

See [[luxepix-vs-filebrowser]] for the feature gap and
[[luxepix-file-explorer-roadmap-and-questionnaire]] for the proposed rollout.

## What LuxePix Should Not Import

- Authentication, OIDC, LDAP, user groups, share links, WebDAV, and server
  administration are outside LuxePix's current desktop scope.
- The Go/Vue server architecture would duplicate the native Rust app and add
  lifecycle, port, database, and browser-state complexity.
- SQLite indexing is useful, but its database should remain local to the
  machine or portable folder, not on a network drive.

