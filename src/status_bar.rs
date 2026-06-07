//! Создание и настройка двух статус-айтемов: видимый якорь `<` + спейсер.
//!
//! Архитектура скрытия (ШАГ 1, проверена по работающему HiddenBar):
//!
//! ```text
//! ПОКАЗАНО:  [ иконки ]  [спейсер]  [ < ]   ← спейсер узкий
//!
//! СКРЫТО:    [иконки уехали]←[ спейсер раздут ][ < ]  ← спейсер ~ширина экрана,
//!                          вытолкнул за край            < на месте, виден
//! ```
//!
//! Роли:
//! - ЯКОРЬ `<` — видимый, `variableLength`, ширину НЕ меняем НИКОГДА.
//! - СПЕЙСЕР — раздуваем ЕГО: правый край прибит к левому краю якоря, растёт
//!   влево, толкает только иконки левее себя.
//!
//! НАДЁЖНОСТЬ РАЗМЕЩЕНИЯ (Tahoe, journal 006): размещение NSStatusItem асинхронно
//! и иногда «залипает» на x=0 (айтем не получил слот). Лечится тремя слоями:
//! 1. спейсер создаётся ПЕРВЫМ (стабильнее получает правый слот);
//! 2. контроллер по таймеру пересоздаёт застрявший на x=0 айтем (retry);
//! 3. GUARD перед раздуванием требует оба x>0 и спейсер строго левее якоря —
//!    иначе безопасный отказ (якорь не уезжает). Пользователь правит Cmd+drag.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{sel, MainThreadMarker};
use objc2_app_kit::{
    NSEventMask, NSImage, NSStatusBar, NSStatusItem, NSVariableStatusItemLength,
};
use objc2_foundation::NSString;

use crate::strings::{self, Lang};

/// Стабильные имена автосохранения позиций (Cmd+drag переживает перезапуск).
const ANCHOR_AUTOSAVE: &str = "nook-anchor";
const SPACER_AUTOSAVE: &str = "nook-spacer";

/// Ширина спейсера в показанном состоянии. 12pt — иконка cutter остаётся видимой
/// и кликабельной (при 6pt пропадала).
// HARDCODE: ширина спейсера показанного; вынести в конфиг позже.
pub const SPACER_WIDTH_SHOWN: f64 = 12.0;

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

    // ИТЕРАЦИЯ 1: спейсер создаётся ПЕРВЫМ. macOS добавляет новые статус-айтемы
    // слева от существующих; спейсер (fixed width) первым стабильно получает
    // правый слот, якорь встаёт рядом. При обратном порядке (якорь первым)
    // спейсер в ~50% запусков не получал слот и падал на x=0 (journal 004, T6).
    let spacer = make_spacer(&bar, mtm, lang);
    let anchor = make_anchor(&bar, mtm, target, lang);

    StatusItems { anchor, spacer }
}

/// Создаёт спейсер (fixed width, символ cutter). Вынесено для переиспользования.
unsafe fn make_spacer(bar: &NSStatusBar, mtm: MainThreadMarker, lang: Lang) -> Retained<NSStatusItem> {
    let spacer = bar.statusItemWithLength(SPACER_WIDTH_SHOWN);
    spacer.setAutosaveName(Some(&NSString::from_str(SPACER_AUTOSAVE)));
    if let Some(button) = spacer.button(mtm) {
        button.setToolTip(Some(&NSString::from_str(strings::cutter_tooltip(lang))));
        set_button_symbol(&button, SPACER_SYMBOL);
    }
    crate::log::append(&format!(
        "created spacer: autosave='{SPACER_AUTOSAVE}' length={SPACER_WIDTH_SHOWN} symbol='{SPACER_SYMBOL}'"
    ));
    spacer
}

/// Создаёт якорь `<` (variableLength, клик-обработчик). Вынесено для retry.
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
        // Ловим И левый, И правый клик — разделяем их в обработчике (левый =
        // toggle, правый = меню). sendActionOn возвращает старую маску — игнор.
        let _ = button.sendActionOn(NSEventMask::LeftMouseUp | NSEventMask::RightMouseUp);
        crate::log::append(&format!(
            "created anchor: autosave='{ANCHOR_AUTOSAVE}' symbol='{ANCHOR_SYMBOL_SHOWN}' variableLength"
        ));
    } else {
        crate::log::append("WARNING: anchor button() returned nil — клик работать не будет");
    }
    anchor
}

/// ИТЕРАЦИЯ 4-retry: пересоздаёт якорь, если он застрял на x=0. Старый айтем
/// удаляется из бара, создаётся новый. Вызывается асинхронно из таймера контроллера
/// (после возврата из didFinishLaunching — иначе координаты не размещаются).
pub unsafe fn recreate_anchor(
    items: &mut StatusItems,
    mtm: MainThreadMarker,
    target: &AnyObject,
    lang: Lang,
) {
    let bar = NSStatusBar::systemStatusBar();
    bar.removeStatusItem(&items.anchor);
    crate::log::append("retry: удалил застрявший якорь, создаю заново");
    items.anchor = make_anchor(&bar, mtm, target, lang);
}

/// ИТЕРАЦИЯ 4b-retry: пересоздаёт спейсер, если застрял на x=0.
pub unsafe fn recreate_spacer(items: &mut StatusItems, mtm: MainThreadMarker, lang: Lang) {
    let bar = NSStatusBar::systemStatusBar();
    bar.removeStatusItem(&items.spacer);
    crate::log::append("retry: удалил застрявший спейсер, создаю заново");
    items.spacer = make_spacer(&bar, mtm, lang);
}

/// ИТЕРАЦИЯ 7-эскалация: полный сброс композиции. Когда повторное пересоздание
/// одного айтема не помогает (новый садится на тот же x=0), удаляем ОБА и создаём
/// заново в правильном порядке — система перераскладывает слоты с нуля.
pub unsafe fn recreate_both(
    items: &mut StatusItems,
    mtm: MainThreadMarker,
    target: &AnyObject,
    lang: Lang,
) {
    let bar = NSStatusBar::systemStatusBar();
    bar.removeStatusItem(&items.spacer);
    bar.removeStatusItem(&items.anchor);
    crate::log::append("retry-эскалация: удалил ОБА айтема, пересоздаю композицию");
    items.spacer = make_spacer(&bar, mtm, lang);
    items.anchor = make_anchor(&bar, mtm, target, lang);
}

/// Ставит на кнопку якоря SF Symbol по имени. Используется контроллером при
/// смене состояния (показано/скрыто/заблокировано).
pub fn set_anchor_symbol(items: &StatusItems, mtm: MainThreadMarker, symbol_name: &str) {
    if let Some(button) = items.anchor.button(mtm) {
        set_button_symbol(&button, symbol_name);
    }
}

/// Загружает системный SF Symbol и ставит его картинкой на кнопку. Если символа
/// нет в системе (опечатка/старая macOS) — логируем и оставляем кнопку как есть.
fn set_button_symbol(button: &objc2_app_kit::NSStatusBarButton, symbol_name: &str) {
    let name = NSString::from_str(symbol_name);
    match NSImage::imageWithSystemSymbolName_accessibilityDescription(&name, None) {
        Some(image) => button.setImage(Some(&image)),
        None => crate::log::append(&format!(
            "WARNING: SF Symbol '{symbol_name}' не найден в системе"
        )),
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
