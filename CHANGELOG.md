# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [0.1.2] - 2026-06-08

### Added
- "Open at Login" toggle in the context menu — registers/unregisters the app as a login item via the native `SMAppService.mainApp` API (no login helpers, no LaunchAgent plists). The checkmark reflects the current state. See ADR 009.

### Changed
- The context menu now shows the version (`Nook X.Y.Z`) as the top item, replacing the old "About" entry.
- Removed the `⌘Q` shortcut from the Quit item — just "Quit" now.

### Fixed
- Context menu no longer overlaps the menu bar on first open (no more scroll-arrow `^` hiding the first item). The menu is anchored to the button's bottom edge so it drops downward. See ADR 010.

## [0.1.1] - 2026-06-05

### Fixed
- App name is now capitalized as **Nook** (`CFBundleName` / `CFBundleDisplayName`), per Apple's Human Interface Guidelines — the bundle and Finder name were lowercase `nook` in 0.1.0. The binary, bundle id and download file names stay lowercase.

## [0.1.0] - 2026-06-05

First release. Hiding extra menu bar icons — the foundation is in place and
verified live on Tahoe.

### Added
- Icon hiding via a spacer: two status items — a visible anchor `<` (click to hide/show icons) and a spacer-cutter `|` that expands to roughly the screen width and pushes the icons left of it off the edge.
- Order guard: the spacer only expands when it is actually left of the anchor (X-coordinate check). Otherwise hiding is blocked and the anchor shows `⚠` with a hint to fix the order via Cmd+drag. Prevents the "`<` flew off the edge" bug.
- Auto-hide: 1 second after launch and after 3 seconds of inactivity (once the mouse leaves the menu bar).
- Right-click context menu on the anchor: About and Quit.
- Item positions are remembered via `setAutosaveName` (persist across restarts).
- Agent app with no Dock icon (`LSUIElement`); builds `.app` + `.dmg` through `make-dmg.sh` (ad-hoc signed).

### Changed
- Sprints `01_CORE_status-item-ffi` and `02_CORE_hide-show` completed and moved to `sprints/archive/`.
