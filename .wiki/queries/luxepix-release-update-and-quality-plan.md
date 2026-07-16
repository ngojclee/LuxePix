---
title: LuxePix Release Update and Quality Plan
created: 2026-07-16
updated: 2026-07-16
type: query
tags: [luxepix, roadmap, desktop-app, windows, portable, safety, security, performance]
sources:
  - raw/articles/filebrowser-quantum-2026-07-16.md
  - raw/articles/filebrowser-original-2026-07-16.md
confidence: high
---

# LuxePix Release Update and Quality Plan

This page expands the release epic in [[luxepix-implementation-plan]] and the
locked updater contract in [[luxepix-explorer-extended-decisions]]. It assumes
public GitHub Releases until an owner/repository is supplied.

## Git And CI Baseline

- Preserve the current workspace, add a reviewed `.gitignore`, make the initial
  baseline commit, and rename `master` to `main`.
- Pull requests and `main` run formatting, Clippy with warnings denied, unit
  and integration tests, and a Windows MSVC build.
- A `vMAJOR.MINOR.PATCH` tag builds release binaries once and feeds both
  portable and installer packaging jobs.
- Release jobs generate hashes and manifest from the exact uploaded bytes.
- The GitHub owner/repository is compiled into release builds through CI
  configuration rather than a user-editable URL.

## Release Assets

Each `vX.Y.Z` GitHub Release contains:

- `LuxePix-vX.Y.Z-windows-x64-portable.zip`
- `LuxePix-vX.Y.Z-windows-x64-setup.exe`
- `release-manifest.json`
- `SHA256SUMS.txt`
- release notes

The portable ZIP includes `LuxePix.exe`, `LuxePixUpdater.exe`, `.portable`,
licenses/notices, and an empty `data/` directory. The installer includes the
same app/updater binaries and stores mutable data in `%APPDATA%\LuxePix`.

## Release Manifest

`release-manifest.json` contains:

- schema version, app version, publication date, and minimum updater version;
- exact asset names, package mode, architecture, byte length, and SHA-256;
- executable entry point and files/directories that portable update preserves;
- optional Model Pack compatibility range;
- release-notes URL supplied by GitHub Releases.

Unknown schema versions, mismatched architecture/mode, invalid semantic
versions, wrong lengths, or checksum failures stop the update before exit.

## Update Check

- After the first UI frame, query the latest non-draft, non-prerelease GitHub
  Release with a short timeout. Network failure stays silent except in the
  Settings status row.
- `Check for Updates` always performs an immediate manual check and reports
  current, available, offline, rate-limited, or invalid-release state.
- When a newer version exists, show version, release notes, package size, and
  the actions `Download and Update` and `Later`.
- Download only after approval, show progress in Task Center, support cancel,
  save under the mode-aware data root, and verify before launching any helper.

## Portable Update Transaction

1. Download and verify the portable ZIP and manifest.
2. Copy `LuxePixUpdater.exe` to a unique local temporary directory so the
   packaged updater can itself be replaced.
3. Start the temporary updater with target directory, archive, expected hash,
   current PID, restart command, and one-time handshake token.
4. LuxePix closes; the updater waits for the process and file handles to exit.
5. Extract to a sibling staging directory and reject unsafe archive paths.
6. Back up replaceable application files while excluding `data` and `.portable`.
7. Move staged files into place with same-volume rename where possible.
8. Start the new LuxePix with the handshake token.
9. The app writes a success marker only after config migration, UI creation,
   and data-root access succeed.
10. On success, remove backup/staging data. On timeout or failed startup,
    terminate the failed process, restore the backup, and reopen the old app.

Recovery metadata is transactional state, not a diagnostic log, and is deleted
after success. User content and the portable `data` directory are never rolled
back or replaced.

## Installed Update Transaction

1. Download and verify the Inno Setup package.
2. Launch it with an update flag, then close LuxePix.
3. The installer detects the same AppId and shows explicit confirmation that
   the previous application version will be removed or replaced.
4. AppData is excluded from upgrade removal. Declining cancels the upgrade.
5. Inno validates writable destinations, stages files, performs its rollback-
   capable replacement, updates shortcuts/uninstall metadata, and opens LuxePix.
6. Manual uninstall separately offers `Remove LuxePix settings and cache`,
   unchecked by default; update mode never presents or selects that cleanup.

Installer failure must leave either the old application launchable or a clear
retry path while preserving AppData.

## Verification Matrix

| Layer | Required proof |
|---|---|
| Unit | Command enablement, shortcut conflicts, selection ranges, sorting, code extraction, collision naming, versions, manifest/checksum |
| Integration | Temp-directory file operations/undo, SQLite migration/rebuild, watcher reconciliation, pHash grouping, config import/export |
| Windows | Clipboard both directions, Recycle Bin restore, drag/drop modifiers, Open With/Reveal, hidden files, both storage modes |
| UI | Rename geometry, four views, three theme modes, Settings conflicts, Task Center states, first-result selection |
| Release | Clean install/uninstall, AppData retention, portable run, startup/manual update, forced rollback, restart handshake |

## Performance Gates

- First window appears within 1.5 seconds on a normal SSD; restore and scanning
  continue asynchronously.
- A 2,000-item folder reaches first usable metadata within 300 ms; thumbnails
  fill progressively.
- Prefix search across 100,000 indexed files is below 100 ms at p95 on the
  selected reference machine.
- Indexing, file operations, hashing, download, and inference stay cancellable
  and do not perform unbounded memory reads.
- Thumbnail memory uses byte-bounded LRU eviction with a configurable 256 MB
  default budget.
- SQLite writes use bounded batches without a UI-facing lock.

## Release Acceptance

- Test installer and portable artifacts in Windows Sandbox from a clean state.
- Test paths with spaces, Unicode names, long paths, read-only files, network
  disconnects, antivirus delay, low disk space, and update interruption.
- Confirm startup/manual checks make no source-image uploads and create no
  persistent diagnostic logs.
- Publish only when both package modes install/run, checksums match uploaded
  assets, and forced-failure rollback evidence is attached to the release run.
