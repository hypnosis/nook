# Nook

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Latest release](https://img.shields.io/github/v/release/hypnosis/nook?sort=semver)](https://github.com/hypnosis/nook/releases/latest)
![Platform: macOS 26 Tahoe](https://img.shields.io/badge/platform-macOS%2026%20Tahoe-blue)
![Rust](https://img.shields.io/badge/Rust-1.95+-orange?logo=rust)
![Built with objc2](https://img.shields.io/badge/built%20with-objc2-blueviolet)

A fast, lightweight menu bar manager for macOS. Native, written in Rust.

## Why

Menu bar icons pile up. On a MacBook with a notch, the ones that don't fit are
simply not drawn, and there is no way to reach them. Nook hides the extras and
keeps them one click away.

Nook is a single small binary built on AppKit through
[objc2](https://github.com/madsmtm/objc2). No Electron, no web views, no
background services. It uses system menu bar items and panels, so it looks and
behaves like part of macOS.

<img src="docs/screenshots/screenshot-shown.png" width="720" alt="Nook showing icons">

<img src="docs/screenshots/screenshot-hidden.png" width="720" alt="Nook hiding icons">

## Usage

Click the `‹` anchor to hide or show the extra icons. Hidden icons appear in a
panel below the menu bar; click one to open its menu. The menu bar collapses
again after a few seconds of inactivity.

### Automatic layout

Nothing to set up. Icons that don't fit in the menu bar go to the panel, and
come back when there is room again.

### Manual layout

**Settings → Layout.** Drag icons between the **Menu bar** and **Panel** rows.
The real icon moves in the menu bar right away, without moving your cursor. The
panel order is kept across restarts.

## Permissions

Hiding icons works without any permissions. The panel needs two:

- **Screen Recording** — to show hidden icons in the panel. Only the menu bar is
  captured.
- **Accessibility** — to open an icon's menu when you click it in the panel, and
  to move icons in manual layout.

## Requirements

- macOS 26 Tahoe
- Apple Silicon (arm64)

Nook may also run on macOS 15 Sequoia, but it is not tested there.

## Install

1. Download the `.dmg` from [Releases](../../releases) and drag **Nook** into
   Applications.
2. The app is ad-hoc signed, not notarized, so macOS blocks the first launch.
   Right-click the app → **Open** → **Open**, or run:

   ```sh
   xattr -dr com.apple.quarantine /Applications/Nook.app
   ```

To quit, right-click the `‹` anchor and choose **Quit**.

## Build from source

Requires Rust 1.95+.

```sh
cargo build --release   # build the binary
./make-dmg.sh           # bundle into Nook.app and a .dmg
```

## License

[MIT](LICENSE)
