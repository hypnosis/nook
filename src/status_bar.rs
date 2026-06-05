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
//! ВАЖНО (урок из тестов на Tahoe): порядок создания НЕ гарантирует, что спейсер
//! окажется левее якоря — система может поставить наоборот. Нет API двигать
//! айтемы программно. Поэтому надёжность даёт НЕ порядок, а GUARD в контроллере:
//! перед раздуванием сверяем реальные X-координаты, и если спейсер не левее —
//! НЕ раздуваем (иначе вылетел бы сам якорь). Пользователь правит Cmd+drag.
//!
//! Спейсер сейчас ВИДИМЫЙ (полоска) — для отладки, чтобы глазами видеть порядок.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{sel, MainThreadMarker};
use objc2_app_kit::{
    NSEventMask, NSImage, NSStatusBar, NSStatusItem, NSVariableStatusItemLength,
};
use objc2_foundation::NSString;

use crate::strings::{self, Lang};

/// Стабильные имена автосохранения позиций (Cmd+drag переживает перезапуск).
const ANCHOR_AUTOSAVE: &str = "clearbar-anchor";
const SPACER_AUTOSAVE: &str = "clearbar-spacer";

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

    let spacer = bar.statusItemWithLength(SPACER_WIDTH_SHOWN);
    spacer.setAutosaveName(Some(&NSString::from_str(SPACER_AUTOSAVE)));
    if let Some(button) = spacer.button(mtm) {
        button.setToolTip(Some(&NSString::from_str(strings::cutter_tooltip(lang))));
        set_button_symbol(&button, SPACER_SYMBOL);
    }
    crate::log::append(&format!(
        "created spacer: autosave='{SPACER_AUTOSAVE}' length={SPACER_WIDTH_SHOWN} symbol='{SPACER_SYMBOL}'"
    ));

    StatusItems { anchor, spacer }
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
