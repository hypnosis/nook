//! Свои элементы Nook в строке меню: видимый якорь `<` и спейсер.
//!
//! ```text
//! Показано:  [ иконки ]  [спейсер]  [ < ]            спейсер узкий
//! Скрыто:    [иконки за краем]←[ спейсер раздут ][ < ]  спейсер шире экрана, `<` на месте
//! ```
//!
//! Ширина якоря не меняется. Спейсер растёт влево от якоря и выталкивает за край иконки
//! левее себя.
//!
//! macOS размещает элементы асинхронно и иногда оставляет элемент на x=0 без места. Спейсер
//! создаётся первым, застрявший элемент контроллер пересоздаёт, а сворачивание идёт только
//! при спейсере строго левее якоря.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{sel, AnyThread, MainThreadMarker};
use objc2_app_kit::{NSEventMask, NSImage, NSStatusBar, NSStatusItem, NSVariableStatusItemLength};
use objc2_foundation::{NSData, NSSize, NSString, NSUserDefaults};

use crate::strings::{self, Lang};
use crate::tuning::{ANCHOR_DEFAULT_PLACE, NARROW_ITEM_WIDTH, SPACER_DEFAULT_PLACE};

/// Стабильные имена автосохранения позиций (Cmd+drag переживает перезапуск).
pub const ANCHOR_AUTOSAVE: &str = "nook-anchor";
pub const SPACER_AUTOSAVE: &str = "nook-spacer";

/// x элемента строки, которого macOS ещё не разместила.
pub const UNPLACED_X: f64 = 0.0;

/// SF Symbol спейсера-cutter (decrease.indent — нативный аналог list-indent-decrease).
/// Видимость постоянная: cutter явно показывает границу зоны скрытия.
const SPACER_SYMBOL: &str = "decrease.indent";

/// SF Symbol якоря: иконки видны (клик спрячет) — шеврон влево.
pub const ANCHOR_SYMBOL_SHOWN: &str = "chevron.left";
/// SF Symbol якоря: иконки спрятаны (клик вернёт) — шеврон вниз.
pub const ANCHOR_SYMBOL_HIDDEN: &str = "chevron.down";
/// SF Symbol якоря: guard заблокировал скрытие — треугольник предупреждения.
pub const ANCHOR_SYMBOL_BLOCKED: &str = "exclamationmark.triangle";

/// Пара айтемов. Контроллер держит оба `Retained`, пока жив.
#[derive(Clone)]
pub struct StatusItems {
    pub anchor: Retained<NSStatusItem>,
    pub spacer: Retained<NSStatusItem>,
}

/// Создаёт якорь `<` и спейсер, вешает обработчик клика на `<`.
///
/// # Safety
/// `target` должен жить не меньше айтемов и реализовывать `action`.
pub unsafe fn create(mtm: MainThreadMarker, target: &AnyObject, lang: Lang) -> StatusItems {
    let bar = NSStatusBar::systemStatusBar();
    if saved_place(SPACER_AUTOSAVE).is_none() {
        write_place(SPACER_AUTOSAVE, SPACER_DEFAULT_PLACE);
    }
    if saved_place(ANCHOR_AUTOSAVE).is_none() {
        write_place(ANCHOR_AUTOSAVE, ANCHOR_DEFAULT_PLACE);
    }

    // Спейсер создаётся первым: macOS добавляет новые статус-айтемы слева от
    // существующих, и иначе спейсер может не получить слот и упасть на x=0.
    let spacer = make_spacer(&bar, mtm, lang);
    let anchor = make_anchor(&bar, mtm, target, lang);

    StatusItems { anchor, spacer }
}

/// Создаёт узкий спейсер.
unsafe fn make_spacer(
    bar: &NSStatusBar,
    mtm: MainThreadMarker,
    lang: Lang,
) -> Retained<NSStatusItem> {
    let spacer = bar.statusItemWithLength(NARROW_ITEM_WIDTH);
    spacer.setAutosaveName(Some(&NSString::from_str(SPACER_AUTOSAVE)));
    if let Some(button) = spacer.button(mtm) {
        button.setToolTip(Some(&NSString::from_str(strings::cutter_tooltip(lang))));
        set_button_symbol(&button, SPACER_SYMBOL);
    }
    spacer
}

/// Создаёт якорь `<`: переменной длины, ловит левый и правый клик.
unsafe fn make_anchor(
    bar: &NSStatusBar,
    mtm: MainThreadMarker,
    target: &AnyObject,
    lang: Lang,
) -> Retained<NSStatusItem> {
    let anchor = bar.statusItemWithLength(NSVariableStatusItemLength);
    anchor.setAutosaveName(Some(&NSString::from_str(ANCHOR_AUTOSAVE)));
    if let Some(button) = anchor.button(mtm) {
        set_button_symbol(&button, ANCHOR_SYMBOL_SHOWN);
        button.setToolTip(Some(&NSString::from_str(strings::anchor_tooltip(lang))));
        button.setTarget(Some(target));
        button.setAction(Some(action_for_click()));
        // Левый и правый клик разделяет обработчик: левый сворачивает, правый открывает меню.
        // sendActionOn возвращает прежнюю маску, она не нужна.
        let _ = button.sendActionOn(NSEventMask::LeftMouseUp | NSEventMask::RightMouseUp);
    } else {
        log::warn!("anchor button() returned nil — клик работать не будет");
    }
    anchor
}

/// Пересоздаёт якорь, застрявший на x=0; место в строке сохраняется. Зовётся из таймера
/// контроллера: внутри didFinishLaunching элементы ещё не размещены.
pub unsafe fn recreate_anchor(
    items: &mut StatusItems,
    mtm: MainThreadMarker,
    target: &AnyObject,
    lang: Lang,
) {
    let bar = NSStatusBar::systemStatusBar();
    remove_keeping_place(&bar, &items.anchor, ANCHOR_AUTOSAVE);
    log::info!("строка: якорь не размещён — пересоздаю");
    items.anchor = make_anchor(&bar, mtm, target, lang);
}

/// Пересоздаёт спейсер, застрявший на x=0.
pub unsafe fn recreate_spacer(items: &mut StatusItems, mtm: MainThreadMarker, lang: Lang) {
    let bar = NSStatusBar::systemStatusBar();
    remove_keeping_place(&bar, &items.spacer, SPACER_AUTOSAVE);
    log::info!("строка: спейсер не размещён — пересоздаю");
    items.spacer = make_spacer(&bar, mtm, lang);
}

/// Пересоздаёт оба элемента в нужном порядке, когда пересоздание одного не помогает:
/// macOS раскладывает их места заново.
pub unsafe fn recreate_both(
    items: &mut StatusItems,
    mtm: MainThreadMarker,
    target: &AnyObject,
    lang: Lang,
) {
    let bar = NSStatusBar::systemStatusBar();
    remove_keeping_place(&bar, &items.spacer, SPACER_AUTOSAVE);
    remove_keeping_place(&bar, &items.anchor, ANCHOR_AUTOSAVE);
    log::info!("строка: элементы не размещены — пересоздаю оба");
    items.spacer = make_spacer(&bar, mtm, lang);
    items.anchor = make_anchor(&bar, mtm, target, lang);
}

/// Составная template-иконка blocked-состояния (шеврон `<` + `⚠` внахлёст).
/// Вшита в бинарь; собирается пайплайном `assets/blocked-icon/build.sh`.
const BLOCKED_ICON_PNG: &[u8] = include_bytes!("blocked.png");
/// Логический размер blocked-иконки в pt (PNG отрендерен @2x: 42×30px → 21×15pt).
const BLOCKED_ICON_SIZE: NSSize = NSSize::new(21.0, 15.0);

/// Ставит на кнопку якоря SF Symbol по имени. Используется контроллером при
/// смене состояния (показано/скрыто/заблокировано). Для blocked-состояния
/// вместо одиночного символа ставит составную template-иконку `< ⚠`.
pub fn set_anchor_symbol(items: &StatusItems, mtm: MainThreadMarker, symbol_name: &str) {
    let Some(button) = items.anchor.button(mtm) else {
        return;
    };
    if symbol_name == ANCHOR_SYMBOL_BLOCKED && set_button_blocked_icon(&button) {
        return;
    }
    set_button_symbol(&button, symbol_name);
}

/// Загружает системный SF Symbol и ставит его картинкой на кнопку. Если символа
/// нет в системе (опечатка/старая macOS) — логируем и оставляем кнопку как есть.
fn set_button_symbol(button: &objc2_app_kit::NSStatusBarButton, symbol_name: &str) {
    let name = NSString::from_str(symbol_name);
    match NSImage::imageWithSystemSymbolName_accessibilityDescription(&name, None) {
        Some(image) => button.setImage(Some(&image)),
        None => log::warn!(
            "SF Symbol '{symbol_name}' не найден в системе"
        ),
    }
}

/// Ставит на кнопку составную blocked-иконку из вшитого PNG. Template — macOS
/// сама красит под тему. `true` при успехе, `false` если PNG не распарсился
/// (тогда вызывающий откатывается на системный `exclamationmark.triangle`).
fn set_button_blocked_icon(button: &objc2_app_kit::NSStatusBarButton) -> bool {
    // SAFETY: BLOCKED_ICON_PNG живёт всю программу (static); dataWithBytes_length
    // копирует байты внутрь NSData, так что переживать за время жизни не нужно.
    let data = unsafe {
        NSData::dataWithBytes_length(BLOCKED_ICON_PNG.as_ptr() as *mut _, BLOCKED_ICON_PNG.len())
    };
    match NSImage::initWithData(NSImage::alloc(), &data) {
        Some(image) => {
            image.setSize(BLOCKED_ICON_SIZE);
            image.setTemplate(true);
            button.setImage(Some(&image));
            true
        }
        None => {
            log::warn!("blocked-иконка (blocked.png) не распарсилась");
            false
        }
    }
}

/// Место элемента в строке, которое помнит macOS: расстояние от правого края экрана.
pub fn saved_place(autosave: &str) -> Option<f64> {
    let key = place_key(autosave);
    let defaults = NSUserDefaults::standardUserDefaults();
    defaults.objectForKey(&key)?;
    Some(defaults.doubleForKey(&key))
}

/// Место, записанное до создания элемента, macOS использует при его размещении.
fn write_place(autosave: &str, place: f64) {
    NSUserDefaults::standardUserDefaults().setDouble_forKey(place, &place_key(autosave));
}

fn place_key(autosave: &str) -> Retained<NSString> {
    NSString::from_str(&format!("NSStatusItem Preferred Position {autosave}"))
}

/// Удаление элемента стирает его место: оно возвращается сразу после удаления.
fn remove_keeping_place(bar: &NSStatusBar, item: &NSStatusItem, autosave: &str) {
    let place = saved_place(autosave);
    bar.removeStatusItem(item);
    if let Some(place) = place {
        write_place(autosave, place);
    }
}

/// X-координата айтема на экране (левый край его окна). `None`, если окна ещё нет
/// (до стабилизации RunLoop). Используется guard'ом для сверки порядка.
pub fn item_origin_x(item: &NSStatusItem, mtm: MainThreadMarker) -> Option<f64> {
    let button = item.button(mtm)?;
    let window = button.window()?;
    Some(window.frame().origin.x)
}

fn action_for_click() -> objc2::runtime::Sel {
    sel!(onAnchorClick:)
}
