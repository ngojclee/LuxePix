# LuxePix 1.2.0

This release completes the self-update flow for installed copies and gives the
interface a consistency and spacing pass.

## Highlights

- Installed copies now update end-to-end: LuxePix downloads the verified
  installer, closes itself, lets LuxePixUpdater silently remove the previous
  installation, and reruns the full setup wizard into the same folder.
- Settings, thumbnail cache, product index, and similarity data in AppData are
  preserved across updates; previously selected setup tasks (desktop icon) are
  carried over automatically.
- If an update is cancelled or fails, the existing installation is restarted
  untouched whenever possible, and the downloaded installer is kept for a
  manual retry.
- Settings > Advanced now shows the install folder, data root, config, session
  state, thumbnail cache, and update staging paths with quick open buttons.
- The header and first-run screen use the new paper-sketch LuxePix icon, and
  list rows, toolbars, side panels, and dialogs received a breathing-room
  spacing pass in both themes.

LuxePix does not upload product images, send telemetry, or write persistent
diagnostic logs.
