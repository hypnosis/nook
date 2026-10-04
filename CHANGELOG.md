# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [0.4.3] - 2026-10-04

### Changed
- Logging goes to the macOS unified log (subsystem `com.hypnosis.nook`): failures and key events always, details only after `defaults write com.hypnosis.nook debugLog -bool YES`. The file log in `/tmp` and the `debug-log` build feature are gone.
- Behaviour tuning values (menu bar widths, timings, tolerances) are kept in one place.

### Removed
- `scripts/test-stand.sh`, the placement test stand that relied on the file log.

## [0.4.2] - 2026-10-04

### Changed
- README and project overview rewritten in English.
- Minimum macOS version lowered to 15 Sequoia; tested on 26 Tahoe only.
- `scripts/install-local.sh` picks the signing identity from `.private/signing.env`; `--adhoc` signs like a release build, `--reset-permissions` resets Nook's permissions.

### Removed
- Temporary menu bar probe from the debug log.

## [0.4.1] - 2026-10-04

### Fixed
- The panel order survives a Nook restart: it is saved by app name and restored once the panel icons are recognised.

## [0.4.0] - 2026-10-04

### Changed
- No more Apply: dropping a tile in the layout editor moves that one icon in the menu bar right away. See ADR 020.
- The editor mirrors the menu bar: the main row follows the real order, including icons hidden behind the notch; a drop that lands elsewhere shows where the icon really went.

### Fixed
- A second drop no longer fails while the layout editor is open: the divider no longer narrows under it.
- With the camera indicator on, hidden icons no longer scramble the editor order.
- Drops next to the notch no longer release under the notch.
- Nook no longer crashes if a background thread fails while holding a shared lock.

## [0.3.2] - 2026-10-04

### Fixed
- One icon that refuses a targeted move no longer switches all later moves to visible dragging until Nook restarts.
- If a stuck Apply is cancelled, the divider widens again and panel icons stay hidden.
- Turning off automatic layout no longer triggers an unexpected Apply later when the divider is disabled.
- No more false "some icons are too close to the edge" warning after turning off automatic layout.

## [0.3.1] - 2026-10-04

### Added
- Settings → Permissions: a button that resets Nook's permissions, and a hint when the granted permissions no longer apply.
- Turning off automatic layout puts the manual order back into the menu bar right away.

### Changed
- Apply is much faster and invisible: icons are moved by events addressed to their windows, like Ice on Tahoe, and the cursor stays hidden. A run takes a fraction of a second instead of several seconds. Dragging is used only as a fallback. See ADR 019.
- The panel and the layout editor refresh right after Apply.
- "Not everything landed" now checks the whole menu bar: the main row order and which side of the divider each icon is on.

### Fixed
- Apply no longer needs a second press to get the order right: the divider is not recreated when it is already in place.
- Panel icons no longer show up in the menu bar after Apply while screen recording is active: the system recording indicator is no longer treated as an icon.
- Moving the mouse during Apply no longer breaks the layout: an icon whose snapshot failed keeps its place in the editor.
- Apply no longer drags Nook's own divider or pushes icons under the notch when everything fits.

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
