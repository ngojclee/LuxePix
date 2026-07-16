# LuxePix

LuxePix is a Windows image and product-file explorer built for fast browsing,
professional file operations, product-code lookup, and local image matching.

## Highlights

- Light, Dark, and System themes.
- Gallery, Grid, Compact, and Details views.
- Multi-selection, configurable shortcuts, copy/cut/paste, rename, delete,
  collision handling, Task Center, and bounded Undo.
- Product-code Quick Find with starts-with matching and automatic first-result
  selection.
- Local Find Product by Image using perceptual hashes, with optional Model Pack
  reranking support.
- Batch Rename, Duplicate Finder, ZIP create/extract, and Folder Size analysis.
- Installed and portable storage modes with verified GitHub updates.

LuxePix does not upload product images, send telemetry, or write persistent
diagnostic logs.

## Install

Download either the Windows x64 installer or portable ZIP from the GitHub
Releases page. The portable package keeps settings, thumbnails, index data, and
update staging inside its adjacent `data` directory.

## Build

Requirements:

- Rust `1.93.1`
- Inno Setup 6 for the installer

```powershell
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all-targets --locked
.\packaging\build-release.ps1 -Version 1.1.0
```

Release assets are written to `dist\v1.1.0`.

## Verification

The implementation checklist is in
`.docs/implementation-checklist.md`. Release evidence is in
`.docs/evidence/v1.1.0/verification.md`.
