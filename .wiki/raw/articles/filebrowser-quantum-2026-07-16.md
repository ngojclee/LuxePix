---
source_url: https://github.com/gtsteffaniak/filebrowser
ingested: 2026-07-16
sha256: b3a69344de44b1ca0fbda419b536332dcf3bc631a414bac578da2e80e7fad1dc
---

# FileBrowser Quantum source snapshot

Official sources checked on 2026-07-16:

- Repository README and source tree:
  `https://github.com/gtsteffaniak/filebrowser`
- Official documentation:
  `https://filebrowserquantum.com/`
- Documentation source:
  `https://github.com/quantumx-apps/filebrowserDocs`
- Stable release notes:
  `https://github.com/gtsteffaniak/filebrowser/releases/tag/v1.4.0-stable`
- Context menu source:
  `frontend/src/components/ContextMenu.vue`
- Listing behavior:
  `frontend/src/views/files/ListingView.vue`
- Search behavior:
  `frontend/src/components/Search.vue`

Extracted facts:

- The local binary at
  `N:\projects\nailart-pro-ai\filebrowser\filebrowser.exe` reports
  `v1.2.3-stable`, commit `56cc547`.
- The latest stable release checked was `v1.4.0-stable`, published
  2026-06-19.
- Quantum is an Apache-2.0 fork of the original File Browser.
- It supports multiple filesystem sources, SQLite indexing, instant search,
  advanced filters, thumbnails, multiple view modes, multi-selection,
  context-sensitive actions, archives, previews, pinned items, duplicate
  finding, size analysis, and file watching.
- Its search result menu exposes actions equivalent to "open parent folder"
  and "go to item".
- Version 1.4.0 added pinned files/folders, advanced search, additional
  shortcuts, and security fixes.

Local deployment observations:

- `runtime-config.yaml` exposes seven sources and listens on
  `127.0.0.1:8080`.
- Authentication is disabled.
- Starting the binary on 2026-07-16 reproduced a SQLite
  `database is locked` failure against the database stored on `N:`.
