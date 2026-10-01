//! Разделитель панели: невидимый элемент строки меню. Иконки левее него наверху
//! не видны никогда — они живут только в панели.
//!
//! Широкий разделитель сам не помещается в строку, и macOS прячет его вместе с
//! иконками левее, оставляя их на экране (x ≥ 0) — там панель их снимает и нажимает.

use objc2::rc::Retained;
use objc2::{AnyThread, MainThreadMarker};
use objc2_app_kit::{NSImage, NSStatusBar, NSStatusItem};
use objc2_foundation::{NSSize, NSString, NSUserDefaults};

const AUTOSAVE: &str = "nook-divider";
const ENABLED_KEY: &str = "panelDividerEnabled";

// HARDCODE: геометрия разделителя; вынести в конфиг позже.
pub const NARROW_WIDTH: f64 = 12.0;
/// Окно элемента строки меню шире его длины на эти поля.
const WINDOW_CHROME: f64 = 16.0;
/// Запас от левого края экрана для самой левой иконки панели.
const SCREEN_MARGIN: f64 = 20.0;
/// Меньше этого macOS перемешивает строку и может спрятать сам `<`.
const MIN_HIDING_WIDTH: f64 = 500.0;

/// Есть ли иконки в панели — тогда разделитель создаётся при запуске.
pub fn is_enabled() -> bool {
    NSUserDefaults::standardUserDefaults().boolForKey(&NSString::from_str(ENABLED_KEY))
}

pub fn set_enabled(on: bool) {
    NSUserDefaults::standardUserDefaults().setBool_forKey(on, &NSString::from_str(ENABLED_KEY));
}

/// Узкий разделитель; место в строке macOS восстанавливает по `AUTOSAVE`.
/// Прозрачная картинка нужна, чтобы macOS сразу разложила элемент: совсем пустой
/// долго стоит с нулевой шириной.
pub fn create(mtm: MainThreadMarker) -> Retained<NSStatusItem> {
    let item = NSStatusBar::systemStatusBar().statusItemWithLength(NARROW_WIDTH);
    item.setAutosaveName(Some(&NSString::from_str(AUTOSAVE)));
    if let Some(button) = item.button(mtm) {
        let blank = NSImage::initWithSize(NSImage::alloc(), NSSize::new(1.0, 1.0));
        button.setImage(Some(&blank));
    }
    crate::log::append(&format!("divider: создан, autosave='{AUTOSAVE}'"));
    item
}

pub fn remove(item: &NSStatusItem) {
    NSStatusBar::systemStatusBar().removeStatusItem(item);
    crate::log::append("divider: удалён");
}

/// Ширина, при которой разделитель (окно `divider_id`) прячет иконки левее себя,
/// а самая левая из них остаётся на экране.
pub fn hiding_width(divider_id: u32) -> f64 {
    let layout = crate::capture::icon_layout();
    let Some(divider_x) = layout.iter().find(|window| window.id == divider_id).map(|window| window.x) else {
        return MIN_HIDING_WIDTH;
    };
    let panel_width: f64 = layout
        .iter()
        .filter(|window| window.x < divider_x - 1.0)
        .map(|window| window.width)
        .sum();
    let Some(next_x) = layout.iter().find(|window| window.x > divider_x + 1.0).map(|window| window.x) else {
        return MIN_HIDING_WIDTH;
    };
    let width = next_x - WINDOW_CHROME - panel_width - SCREEN_MARGIN;
    if width < MIN_HIDING_WIDTH {
        crate::log::append(&format!(
            "divider: иконок панели слишком много ({panel_width} pt) — часть уйдёт за край"
        ));
    }
    let width = width.max(MIN_HIDING_WIDTH);
    crate::log::append(&format!("divider: ширина {width} (панель {panel_width} pt, справа x={next_x})"));
    width
}
