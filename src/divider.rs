//! Разделитель панели: невидимый элемент строки меню. Иконки левее него наверху
//! не видны никогда — они живут только в панели.
//!
//! Широкий разделитель сам не помещается в строку, и macOS прячет его вместе с
//! иконками левее, оставляя их на экране (x ≥ 0) — там панель их снимает и нажимает.

use objc2::rc::Retained;
use objc2::{AnyThread, MainThreadMarker};
use objc2_app_kit::{NSImage, NSStatusBar, NSStatusItem};
use objc2_foundation::{NSSize, NSString};

use crate::capture::IconWindow;
use crate::tuning::{HIDING_WIDTH_MIN, NARROW_ITEM_WIDTH, NARROW_WINDOW_MAX, POSITION_TOLERANCE, SCREEN_MARGIN, WINDOW_CHROME};

pub const AUTOSAVE: &str = "nook-divider";

/// Разделитель с окном `window` узкий: иконки левее него стоят в строке по порядку.
pub fn is_narrow(window: &IconWindow) -> bool {
    window.width <= NARROW_WINDOW_MAX
}

/// Узкий разделитель; место в строке macOS восстанавливает по `AUTOSAVE`.
/// Прозрачная картинка нужна, чтобы macOS сразу разложила элемент: совсем пустой
/// долго стоит с нулевой шириной.
pub fn create(mtm: MainThreadMarker) -> Retained<NSStatusItem> {
    let item = NSStatusBar::systemStatusBar().statusItemWithLength(NARROW_ITEM_WIDTH);
    item.setAutosaveName(Some(&NSString::from_str(AUTOSAVE)));
    if let Some(button) = item.button(mtm) {
        let blank = NSImage::initWithSize(NSImage::alloc(), NSSize::new(1.0, 1.0));
        button.setImage(Some(&blank));
    }
    item
}

pub fn remove(item: &NSStatusItem) {
    NSStatusBar::systemStatusBar().removeStatusItem(item);
}

/// Ширина, при которой разделитель (окно `divider_id`) прячет иконки левее себя,
/// а самая левая из них остаётся на экране.
pub fn hiding_width(divider_id: u32) -> f64 {
    let layout = crate::capture::icon_layout();
    let Some(divider_x) = layout
        .iter()
        .find(|window| window.id == divider_id)
        .map(|window| window.x)
    else {
        return HIDING_WIDTH_MIN;
    };
    let panel_width: f64 = layout
        .iter()
        .filter(|window| crate::capture::is_panel_icon(window, divider_x))
        .map(|window| window.width)
        .sum();
    let Some(next_x) = layout
        .iter()
        .find(|window| window.x > divider_x + POSITION_TOLERANCE)
        .map(|window| window.x)
    else {
        return HIDING_WIDTH_MIN;
    };
    let width = next_x - WINDOW_CHROME - panel_width - SCREEN_MARGIN;
    if width < HIDING_WIDTH_MIN {
        log::warn!(
            "иконок панели слишком много ({panel_width} pt) — часть уйдёт за край"
        );
    }
    width.max(HIDING_WIDTH_MIN)
}
