---
title: LuxePix Explorer Extended Decisions
created: 2026-07-16
updated: 2026-07-16
type: query
tags: [luxepix, questionnaire, roadmap, file-management, safety, ux]
sources:
  - raw/articles/filebrowser-quantum-2026-07-16.md
  - raw/articles/filebrowser-original-2026-07-16.md
confidence: high
---

# LuxePix Explorer Extended Decisions

This register continues the decisions from
[[luxepix-file-explorer-roadmap-and-questionnaire]] and applies the feature
priorities defined in [[luxepix-vs-filebrowser]].

## Locked Decisions

| ID | Locked choice | Recorded |
|---|---|---|
| LPX-EXP-028 | B - Use a non-blocking Task Center with progress, speed, remaining time, pause, cancel, retry, and error details. | 2026-07-16 |
| LPX-EXP-029 | C - Provide Ctrl+Z file-operation history for tracked rename, move, paste, new-folder, and Recycle Bin operations. | 2026-07-16 |
| LPX-EXP-030 | B - Show favorites, recent paths, configured sources, and source availability in the compact sidebar. | 2026-07-16 |
| LPX-EXP-031 | B - Support Open, Open With, and Reveal in Windows Explorer. | 2026-07-16 |
| LPX-EXP-032 | B - Add a hidden-file toggle while keeping protected system files hidden by default. | 2026-07-16 |
| LPX-EXP-033 | B - Require confirmation for permanent delete, overwrite, and protected operations; make lesser warnings configurable. | 2026-07-16 |
| LPX-EXP-034 | C - Default to product cards grouped by folder, with the strongest matched image, expandable images, and an Images view. | 2026-07-16 |
| LPX-EXP-035 | B - Run strict starts-with matching first, then clearly labeled contains and fuzzy suggestions. | 2026-07-16 |
| LPX-EXP-036 | B - Show 10 similarity results by default with an adjustable range from 5 to 50. | 2026-07-16 |
| LPX-EXP-037 | B - Show ranked scores and a low-confidence warning without claiming an exact match. | 2026-07-16 |
| LPX-EXP-038 | B - Support configurable prefix or regex product-code rules per source with folder-name fallback. | 2026-07-16 |
| LPX-EXP-039 | B - Process query images and embeddings entirely on the local machine without uploads. | 2026-07-16 |
| LPX-EXP-040 | B - Keep pHash in the core portable app and provide CLIP as an optional local AI Model Pack. | 2026-07-16 |
| LPX-EXP-041 | C - Publish both a Windows installer and a self-contained portable ZIP. | 2026-07-16 |
| LPX-EXP-042 | C - Installed mode uses AppData; portable mode uses a data directory beside the executable. | 2026-07-16 |
| LPX-EXP-043 | B - Check GitHub Releases at startup or on demand, show the available release, and automate download and installation after approval. | 2026-07-16 |
| LPX-EXP-044 | B - Auto-detect a separate AI Model Pack and allow a custom local model path. | 2026-07-16 |
| LPX-EXP-045 | B - Ship the interface in English only. | 2026-07-16 |
| LPX-EXP-046 | C - Use a short first-run flow for sources, cache location, and optional indexing. | 2026-07-16 |
| LPX-EXP-047 | C - Do not create persistent diagnostic logs or send telemetry. | 2026-07-16 |

## Locked GitHub Update Contract

- Provide automatic update checks when the app opens and a manual
  `Check for Updates` command in Settings.
- Read version and release assets from GitHub Releases.
- Show the new version and release notes before beginning the update.
- After user approval, download the correct portable or installer asset and
  verify its published checksum before applying it.
- Portable mode: close LuxePix, run a separate updater helper, replace the old
  application files while preserving the `data` directory, and reopen LuxePix.
- Installed mode: close LuxePix, launch the downloaded installer, ask for
  approval to remove the old installed application, preserve AppData, install
  the new version, and reopen LuxePix.
- If replacement or installation fails, keep the previous version recoverable
  and show the error in the current session without writing a persistent log.

## Reference Questions - Batch 5

| ID | Decision question | Options | Recommended |
|---|---|---|---|
| LPX-EXP-028 | How should long file operations be presented? | Blocking dialog / Non-blocking task center / Silent background work | Non-blocking task center with progress, speed, remaining time, pause, cancel, retry, and error details |
| LPX-EXP-029 | How much undo history should LuxePix provide? | No undo / Last action only / File-operation history | Ctrl+Z history for tracked rename, move, paste, new-folder, and Recycle Bin operations |
| LPX-EXP-030 | What should the compact navigation sidebar contain? | Recent paths only / Favorites and sources / Full Explorer tree | Favorites, recent paths, configured sources, and source availability status |
| LPX-EXP-031 | How should external opening work? | Default app only / Default app plus Open With / Internal preview only | Default app, Open With submenu, and Reveal in Windows Explorer |
| LPX-EXP-032 | How should hidden and system files be handled? | Never show / Configurable visibility / Always show | Toggle hidden files; keep protected system files off by default |
| LPX-EXP-033 | When should confirmation dialogs appear? | Every operation / Risky operations only / Never | Confirm permanent delete, overwrite, and protected operations; make lesser warnings configurable |

## Reference Questions - Batch 6

| ID | Decision question | Options | Recommended |
|---|---|---|---|
| LPX-EXP-034 | What should one search result represent? | Individual image / Product folder / Grouped product card with expandable images | Default to product cards grouped by folder, with expandable matched images and an Images view toggle |
| LPX-EXP-035 | What happens when prefix matching finds no result? | Show no result / Fallback to contains and fuzzy matching / Always use fuzzy matching | Try strict starts-with first, then clearly labeled contains and fuzzy suggestions |
| LPX-EXP-036 | How many similarity results should be shown? | Fixed top 10 / Adjustable result count / Every result above threshold | Default to 10 with a user-adjustable range from 5 to 50 |
| LPX-EXP-037 | How should weak similarity matches be presented? | Show top results without warning / Show scores and confidence warning / Hide all weak matches | Show ranked scores and a low-confidence warning without claiming an exact match |
| LPX-EXP-038 | How should product codes be extracted from folders? | Entire folder name / Configurable rule per source / Manual mapping only | Configurable prefix or regex rules per source with folder-name fallback |
| LPX-EXP-039 | Where may query images and embeddings be processed? | Cloud service / Local machine only / User-selectable local or cloud | Local machine only; never upload product images |
| LPX-EXP-040 | How should the optional CLIP capability be packaged? | Bundle everything / Optional AI model pack / Require external Python | Keep the core portable app small, offer an optional local AI model pack, and retain pHash fallback |

## Reference Questions - Batch 7

The recommendations below are the original proposals. The locked choices above
are authoritative where the user's final answer differs.

| ID | Decision question | Options | Initial recommendation |
|---|---|---|---|
| LPX-EXP-041 | Which Windows release artifacts are required? | Installer only / Portable only / Installer and portable ZIP | Produce both an installer and a self-contained portable ZIP |
| LPX-EXP-042 | Where should settings and indexes be stored? | AppData only / Beside executable only / Mode-aware storage | Installed mode uses AppData; portable mode uses a data directory beside the executable |
| LPX-EXP-043 | How should application updates work? | No checks / Notify and ask / Automatic download and install | Check for releases, show release notes, and update only after user approval |
| LPX-EXP-044 | How should the AI Model Pack be located? | Fixed bundled path / Separate auto-detected package / External path only | Auto-detect a separate package and also allow a custom local model path |
| LPX-EXP-045 | Which interface languages are required? | Vietnamese only / English only / Vietnamese and English | Vietnamese by default with an English language option |
| LPX-EXP-046 | What should first launch show? | Empty app / Manual settings / Short source-setup flow | A short setup flow for sources, cache location, and optional indexing |
| LPX-EXP-047 | What diagnostics and telemetry are allowed? | Online telemetry / Local diagnostics only / No diagnostics | Rotating local logs with export and clear controls; no telemetry or automatic upload |

## Decision Status

- Locked: `LPX-EXP-028` through `LPX-EXP-047`.
- Pending: none.
- Detailed delivery plan: [[luxepix-implementation-plan]].
