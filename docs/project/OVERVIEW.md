# Nook — overview

Nook is a menu bar manager for macOS. It hides extra status icons and shows them
in a panel below the menu bar, where they stay clickable. The goal is a tool that
is fast, small and native: Rust on top of AppKit, nothing else.

## How it works

- **Divider.** An invisible status item of Nook's own. Everything to its left is
  hidden; widening it pushes those icons out of view.
- **Anchor `‹`.** Toggles the hidden icons. The menu bar collapses on its own
  after a few seconds of inactivity.
- **Panel.** Shows images of the hidden icons below the menu bar. A click on an
  image presses the real icon, so its menu opens as usual.
- **Layout.** In automatic mode, whatever doesn't fit goes to the panel. In
  manual mode, the user drags icons between two rows in Settings, and Nook moves
  the real icon in the menu bar. The menu bar is the source of truth: the editor
  always shows the order macOS actually has.

## Permissions

| Permission | Used for |
|------------|----------|
| Screen Recording | Images of hidden icons in the panel |
| Accessibility | Pressing hidden icons, moving icons in manual layout |

Without them, hiding and showing still works.

## Scope

- Built for macOS 26 Tahoe. May also run on macOS 15 Sequoia, not tested.
- A personal tool, not a commercial product.

Decisions and their reasons live in [`docs/decisions/`](../decisions/), plans in
[`sprints/`](../../sprints/).
