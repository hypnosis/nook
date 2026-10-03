# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [0.3.0] - 2026-10-03

### Added
- Panel for icons hidden under the notch: a strip under the menu bar shows snapshots of the icons the notch hides. Clicking a snapshot presses the real icon, and its menu opens next to the panel. See ADR 012, 016, 017.
- The panel follows the macOS light/dark theme and refreshes its icons every 2 s while open.
- Settings window with General, Layout and Permissions sections. The layout editor arranges icons between the main row and the panel and moves the real menu bar icons on Apply; icons that refuse to move stay put. See ADR 013.
- Automatic layout option in Settings → Layout: the panel shows every icon that doesn't fit in the menu bar, with no manual arrangement. See ADR 018.
- Permissions window on launch: explains both permissions and opens the matching System Settings pane.
- Dock icon while a Nook window is open.

### Changed
- The panel needs two macOS permissions: Screen Recording (snapshots of the menu bar only) and Accessibility (pressing the real icons). Hiding icons works without them.
- The first panel open is instant: icons and their Accessibility items are captured at startup.
- Builds are ad-hoc signed, so permissions must be granted again after each update. If Nook is already switched on in System Settings but still asks, remove it with − and grant again (ADR 011).

## [0.2.1] - 2026-07-12

### Added
- Blocked-state anchor icon: when hiding is blocked (spacer not left of the anchor, wrong order), the anchor now shows a composite `‹ ⚠` icon — a chevron with an overlapping warning triangle — instead of a bare `⚠`. Built from two real SF Symbols as a template image, so it follows the menu bar theme. Pipeline in `assets/blocked-icon/`.

## [0.2.0] - 2026-07-11

### Changed
- Minimum supported macOS lowered from 26 (Tahoe) to **13 (Ventura)**. The app
  now runs on macOS 13 and later. `LSMinimumSystemVersion` and the linker
  deployment target are set to 13.0; all APIs used (NSStatusItem, SF Symbols,
  `SMAppService.mainApp`) exist since macOS 13 or earlier.

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
