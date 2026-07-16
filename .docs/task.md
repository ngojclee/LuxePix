# LuxePix — Task Tracking

## Completed
- [x] Plan architecture and technology stack (egui/eframe, pure Rust)
- [x] Set up Rust project with dependencies  
- [x] Implement dual-pane layout (left: folder overview, right: folder contents)
- [x] Implement address bar with path input and Browse button
- [x] Implement thumbnail grid with async loading (rayon thread pool)
- [x] Implement image zoom/preview modal (scroll zoom, drag pan, arrow nav)
- [x] Add zoom sliders on both panes (80-500px thumbnail size)
- [x] Show all file types (images + non-image files with type icons)
- [x] Arrow key navigation — all 4 directions, row-aware up/down for grid layout
- [x] **Inline rename** — click on file/folder name → in-place TextEdit, Enter to confirm, Esc/click-away to cancel
- [x] Click non-image file → open with system default app (video, docs, etc.)
- [x] Path history (last 50 paths, dropdown with ✕ delete, persisted to %APPDATA%)
- [x] UI settings persistence (thumb sizes, panel width, last path auto-restore)
- [x] Aligned headers (folder name + zoom slider on same row in both panes)
- [x] Custom app icon (window titlebar + exe icon via winresource)
- [x] Start Menu shortcut (install.bat + shortcut with custom icon)
- [x] Build release (6.55 MB single .exe)

## Pending
- [x] Initialize the local Git repository and build E0-E4 checkpoints.
- [x] Add local product-code Quick Find with SQLite indexing.
- [x] Add pHash-based Find Product by Image with local-only inputs and fallback.
- [x] Complete updater, installer, and portable packaging.
- [x] Complete Batch Rename, Duplicate Finder, ZIP, and Folder Size tools.
- [x] Capture final Windows host acceptance and UI evidence.
- [ ] Publish the verified `v1.1.0` tag and assets to GitHub Releases.

## Documented Follow-Ups
- [ ] Prove outbound OLE drag from LuxePix into Windows Explorer.
- [ ] Prove exact Ctrl+Z restoration for Recycle Bin deletes.
- [ ] Validate native network-share notifications; bounded polling is active.
- [ ] Select and distribute an ONNX Model Pack/helper; pHash fallback is active.
- [ ] Repeat package smoke tests in Windows Sandbox.
