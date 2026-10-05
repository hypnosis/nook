//! Разделитель панели: невидимый элемент строки меню. Иконки левее него наверху
//! не видны никогда — они живут только в панели.
//!
//! Широкий разделитель сам не помещается в строку, и macOS прячет его вместе с
//! иконками левее, оставляя их на экране (x ≥ 0) — там панель их снимает и нажимает.

use objc2::rc::Retained;
use objc2::{AnyThread, MainThreadMarker};
use objc2_app_kit::{NSImage, NSStatusBar, NSStatusItem};
use objc2_foundation::{NSSize, NSString, NSUserDefaults};

use crate::tuning::NARROW_ITEM_WIDTH;

pub const AUTOSAVE: &str = "nook-divider";

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

/// macOS помнит место разделителя в строке. Пока разделитель невидим, места она не помнит.
pub fn has_saved_place() -> bool {
    let key = NSString::from_str(&format!("NSStatusItem Preferred Position {AUTOSAVE}"));
    NSUserDefaults::standardUserDefaults().objectForKey(&key).is_some()
}
