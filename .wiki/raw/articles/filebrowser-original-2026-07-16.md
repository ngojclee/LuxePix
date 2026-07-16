---
source_url: https://github.com/filebrowser/filebrowser
ingested: 2026-07-16
sha256: 999ef9395c6720e88ae17111298c54c5690a9c952aac565b54e03568a00c947d
---

# Original File Browser source snapshot

Official sources checked on 2026-07-16:

- Repository and README:
  `https://github.com/filebrowser/filebrowser`
- Documentation:
  `https://filebrowser.org/`
- File listing source:
  `frontend/src/views/files/FileListing.vue`
- Context menu source:
  `frontend/src/components/ContextMenu.vue`
- Clipboard store:
  `frontend/src/stores/clipboard.ts`

Extracted facts:

- The latest release checked was `v2.63.18`, published 2026-07-04.
- The project is Apache-2.0 licensed.
- Its README describes the project as maintenance-only, with no new features
  planned.
- The listing implements grid/list modes, sorting by name/size/modified,
  multi-selection, drag-and-drop upload, context actions, keyboard shortcuts,
  copy/cut/paste, conflict resolution, download, upload, preview, and editing.
- Its clipboard for file operations is application state rather than a native
  Windows Shell file clipboard.
