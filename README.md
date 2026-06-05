# nook

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Latest release](https://img.shields.io/github/v/release/hypnosis/nook?sort=semver)](https://github.com/hypnosis/nook/releases/latest)
![Platform: macOS 26 Tahoe](https://img.shields.io/badge/platform-macOS%2026%20Tahoe-blue)
![Rust](https://img.shields.io/badge/Rust-1.95+-orange?logo=rust)
![Built with objc2](https://img.shields.io/badge/built%20with-objc2-blueviolet)

Hide extra menu bar icons on macOS. Native, lightweight, no hacks.

## What it does

Your menu bar fills up with status icons you rarely look at. **nook** pushes the
extra ones off the edge of the screen, so they're out of sight. Click to bring
them back. That's it.

**Shown** — the chevron anchor `‹` and the icons to its left:

<img src="docs/screenshots/screenshot-shown.png" width="720" alt="nook showing icons">

**Hidden** — click the anchor and they slide past the edge:

<img src="docs/screenshots/screenshot-hidden.png" width="720" alt="nook hiding icons">

## How it works

nook adds two small items to your menu bar:

- **the chevron anchor** `‹` — click it to hide or show.
- **the cutter** `▏` — everything to the **left** of the cutter is what gets hidden.

When you hide, the cutter expands leftward and shoves every icon left of it past
the edge of the screen. The chevron flips to point down `⌄`. Click it again and
they slide back.

It also hides on its own: **1 second after launch**, and after **3 seconds of
inactivity** once your mouse leaves the menu bar.

### If the anchor shows `⚠`

The order of the two items matters: the cutter `▏` must sit to the **left** of
the anchor `‹`. macOS doesn't guarantee the order they end up in, so sometimes
they land swapped — the anchor ends up left of the cutter. When that happens nook
refuses to hide (it would push its own anchor off-screen) and the anchor shows
`⚠` instead.

To fix it, swap their positions: **hold Cmd and drag** the items in the menu bar
until the cutter is left of the anchor.

## Why no hacks

Tools like Bartender mirror other apps' icons by screen-recording the menu bar
and faking input events (Screen Recording, Accessibility, synthetic events).
That's fragile — it breaks on almost every macOS update. nook doesn't do any of
that. It only manages its own menu bar items and pushes the rest off-screen with
a plain spacer. Pure AppKit, nothing to break.

## Tested on

- macOS 26 Tahoe
- Apple Silicon (arm64)

## Install

1. Download the `.dmg` from [Releases](../../releases) and drag **nook** into Applications.
2. The app is ad-hoc signed (not notarized), so on first launch macOS will block
   it. Right-click the app → **Open** → **Open**. Or from Terminal:

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

## Note

Right-clicking the anchor opens a menu — using that menu as the place for
settings and controls is a direction worth exploring.

## License

[MIT](LICENSE) — free to use, modify, and build on.
