# Nook — overview

Nook is a menu bar manager for macOS. It hides extra status icons and shows them
in a panel below the menu bar, where they stay clickable. The goal is a tool that
is fast, small and native: Rust on top of AppKit, nothing else.

## How it works

- **Anchor `‹` and the global divider.** The anchor toggles the hidden icons: when
  collapsed, an invisible spacer pushes everything to its left off screen. The
  menu bar collapses on its own after a few seconds of inactivity.
- **Panel divider.** A second invisible item splits the expanded menu bar: icons
  to its left live only in the panel and are never visible in the menu bar. It is
  12 pt while there is room next to the notch, otherwise 500 pt, which hides
  everything to its left.
- **Shroud.** With the narrow divider, panel icons sit next to the notch under a
  shroud: a window above the menu bar with a picture of the empty bar. It catches
  clicks. When the active app's menu reaches that strip, the shroud goes away:
  macOS hides the icons under the menu by itself.
- **Panel.** Shows images of those icons below the menu bar. A click on an image
  presses the real icon where it stands; its menu opens under it, next to the
  notch.
- **Layout.** In automatic mode, whatever doesn't fit goes to the panel and the
  panel divider leaves the menu bar. In manual mode, the user drags icons between
  two rows in Settings, and Nook moves the real icon. Turning automatic mode off
  restores the saved manual layout.
- **Sources of truth.** The menu bar tells what icons exist and which side of the
  panel divider they are on; Nook's settings keep the panel order. The panel
  changes only on real events: a drop in the editor, a Cmd+drag, an app adding or
  removing an icon. The full behavior is in ADR 022.

## Permissions

| Permission | Used for |
|------------|----------|
| Screen Recording | Images of hidden icons in the panel, the shroud picture |
| Accessibility | Pressing hidden icons, moving icons in manual layout, the active app's menu edge |

Without them, hiding and showing still works. Without Screen Recording, panel icons stay
hidden in the menu bar: there is no shroud picture to cover them at the notch.

## Scope

- Built for macOS 26 Tahoe. May also run on macOS 15 Sequoia, not tested.
- A personal tool, not a commercial product.

Decisions and their reasons live in [`docs/decisions/`](../decisions/), plans in
[`sprints/`](../../sprints/).
