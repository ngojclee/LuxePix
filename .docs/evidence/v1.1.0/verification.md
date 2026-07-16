# LuxePix 1.1.0 Verification

Date: 2026-07-16

## Automated Gates

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | Passed |
| `cargo clippy --all-targets --locked -- -D warnings` | Passed |
| `cargo test --all-targets --locked` | 4 updater tests and 72 app tests passed; 2 release performance tests ignored by default |
| 2,000-item metadata p95 | 37.6846 ms; target below 300 ms |
| 100,000-product prefix-search p95 | 9.9372 ms; target below 100 ms |
| Thumbnail cache | Byte-bounded LRU tests passed; default 256 MB, configurable 64-2,048 MB |

## Windows Package Smoke

- Portable update was exercised while the old `LuxePix.exe` was locked.
- The helper waited for the old PID, replaced the executable, launched the new
  build, received the startup handshake, and removed transaction files.
- Portable `data`, `.portable`, and an unrelated adjacent file were retained.
- Installer install, update, and uninstall runs completed with exit code 0
  using `/CURRENTUSER`.
- The update replaced a deliberately damaged `LuxePixUpdater.exe` and reopened
  LuxePix.
- Uninstall registration was removed and AppData was retained by default.
- First window appeared in 533 ms on the reference machine.

These package checks used isolated fixtures on the Windows host. A repeat inside
Windows Sandbox remains documented because Sandbox was not available in this
session.

## UI Evidence

- [Light explorer](light-explorer.png)
- [Dark explorer](dark-explorer.png)
- [System explorer](system-explorer.png)
- [Stable inline rename](inline-rename.png)

## Data Boundary Audit

The source tree was searched for network and persistence behavior. Outbound
network access is limited to HTTPS GitHub update checks/downloads and opening an
explicit release link. No image-upload path, telemetry, analytics, or persistent
diagnostic log writer was found.

## Known Follow-Ups

- Outbound OLE drag into Windows Explorer is not implemented.
- Exact Ctrl+Z restoration for a Recycle Bin delete is not proven.
- Network shares use bounded reconciliation polling; native watcher behavior is
  not proven.
- The optional ONNX Model Pack/helper remains a separate feasibility item.
  Local pHash matching is the shipping fallback.
