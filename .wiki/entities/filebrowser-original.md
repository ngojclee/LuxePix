---
title: Original File Browser
created: 2026-07-16
updated: 2026-07-16
type: entity
tags: [filebrowser, web-app, file-management, navigation, shortcuts]
sources: [raw/articles/filebrowser-original-2026-07-16.md]
confidence: high
---

# Original File Browser

The original File Browser is a mature, single-binary web file manager. It is a
useful behavioral baseline for selection, sorting, upload, file operations,
conflict handling, previews, and shortcuts.

Its current maintenance-only status makes it a weaker source for future-facing
features than [[filebrowser-quantum]], but its simpler implementation is useful
for studying conventional explorer behavior.

## Relevant Patterns

- One command is available through toolbar, context menu, and shortcut.
- `Ctrl+A`, `Ctrl+C`, `Ctrl+X`, `Ctrl+V`, `Delete`, and `F2` follow familiar
  desktop conventions.
- Paste checks collisions and offers an explicit conflict-resolution flow.
- Listing modes and sorting are persistent user preferences.
- A file opened and then closed is revealed again in the list.

## Limits For LuxePix

- It is a browser client/server product, while LuxePix is a local native app.
- Its search is less capable than Quantum's indexed search.
- Its internal file clipboard does not by itself provide Windows Explorer
  interoperability.

See [[luxepix-vs-filebrowser]] and
[[luxepix-file-explorer-roadmap-and-questionnaire]].

