//! Разделитель панели: невидимый элемент строки меню. Иконки левее него наверху
//! не видны никогда — они живут только в панели.
//!
//! Широкий разделитель сам не помещается в строку, и macOS прячет его вместе с
//! иконками левее, оставляя их на экране (x ≥ 0) — там панель их снимает и нажимает.

use objc2::rc::Retained;
use objc2::{AnyThread, MainThreadMarker};
use objc2_app_kit::{NSImage, NSStatusBar, NSStatusItem};
use objc2_foundation::{NSSize, NSString, NSUserDefaults};

use crate::capture::IconWindow;

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
/// Окно узкого разделителя не шире этого. Виден он может и не быть: при длинном
/// основном ряде он стоит под чёлкой.
const NARROW_WINDOW_MAX: f64 = 40.0;

/// Разделитель с окном `window` узкий: иконки левее него стоят в строке по порядку.
pub fn is_narrow(window: &IconWindow) -> bool {
    window.width <= NARROW_WINDOW_MAX
}

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
        return MIN_HIDING_WIDTH;
    };
    let panel_width: f64 = layout
        .iter()
        .filter(|window| crate::capture::is_panel_icon(window, divider_x))
        .map(|window| window.width)
        .sum();
    let Some(next_x) = layout
        .iter()
        .find(|window| window.x > divider_x + crate::capture::POSITION_TOLERANCE)
        .map(|window| window.x)
    else {
        return MIN_HIDING_WIDTH;
    };
    let width = next_x - WINDOW_CHROME - panel_width - SCREEN_MARGIN;
    if width < MIN_HIDING_WIDTH {
        crate::log::append(&format!(
            "divider: иконок панели слишком много ({panel_width} pt) — часть уйдёт за край"
        ));
    }
    let width = width.max(MIN_HIDING_WIDTH);
    width
}
