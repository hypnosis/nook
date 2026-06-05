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
use objc2_app_kit::{NSStatusBar, NSStatusItem, NSVariableStatusItemLength};
use objc2_foundation::NSString;

/// Стабильные имена автосохранения позиций (Cmd+drag переживает перезапуск).
const ANCHOR_AUTOSAVE: &str = "clearbar-anchor";
const SPACER_AUTOSAVE: &str = "clearbar-spacer";

/// Ширина спейсера в показанном состоянии. 12pt — полоска `▏` остаётся видимой
/// и кликабельной как cutter (при 6pt пропадала).
// HARDCODE: ширина спейсера показанного; вынести в конфиг + сделать невидимым позже.
pub const SPACER_WIDTH_SHOWN: f64 = 12.0;

/// Символ-полоска видимого спейсера (отладка — видеть, где он стоит).
// DEBUG: убрать заголовок спейсера, когда порядок/guard подтверждены.
const SPACER_TITLE_DEBUG: &str = "▏";

/// Символ якоря: иконки видны (клик спрячет).
pub const ANCHOR_TITLE_SHOWN: &str = "<";
/// Символ якоря: иконки спрятаны (клик вернёт).
pub const ANCHOR_TITLE_HIDDEN: &str = "v";
/// Символ якоря: guard заблокировал скрытие. `⚠` + `<` — видно, что это якорь
/// с ошибкой порядка, а не просто значок.
pub const ANCHOR_TITLE_BLOCKED: &str = "⚠<";

/// Подсказка на якоре — что делать. `<` должен идти ЗА cutter `▏` (правее него).
const ANCHOR_TOOLTIP: &str = "< должен идти за cutter ▏ (полоска слева от <). Поправь Cmd+drag.";

/// Пара айтемов. Контроллер держит оба `Retained`, пока жив.
pub struct StatusItems {
    pub anchor: Retained<NSStatusItem>,
    pub spacer: Retained<NSStatusItem>,
}

/// Создаёт якорь `<` и спейсер, вешает обработчик клика на `<`.
///
/// # Safety
/// `target` должен жить не меньше айтемов и реализовывать `action`.
pub unsafe fn create(mtm: MainThreadMarker, target: &AnyObject) -> StatusItems {
    let bar = NSStatusBar::systemStatusBar();

    let anchor = bar.statusItemWithLength(NSVariableStatusItemLength);
    anchor.setAutosaveName(Some(&NSString::from_str(ANCHOR_AUTOSAVE)));
    if let Some(button) = anchor.button(mtm) {
        button.setTitle(&NSString::from_str(ANCHOR_TITLE_SHOWN));
        button.setToolTip(Some(&NSString::from_str(ANCHOR_TOOLTIP)));
        button.setTarget(Some(target));
        button.setAction(Some(action_for_click()));
        crate::log::append(&format!(
            "created anchor: autosave='{ANCHOR_AUTOSAVE}' title='{ANCHOR_TITLE_SHOWN}' variableLength"
        ));
    } else {
        crate::log::append("WARNING: anchor button() returned nil — клик работать не будет");
    }

    let spacer = bar.statusItemWithLength(SPACER_WIDTH_SHOWN);
    spacer.setAutosaveName(Some(&NSString::from_str(SPACER_AUTOSAVE)));
    if let Some(button) = spacer.button(mtm) {
        button.setToolTip(Some(&NSString::from_str("спейсер (полоска) — отладка")));
        button.setTitle(&NSString::from_str(SPACER_TITLE_DEBUG));
    }
    crate::log::append(&format!(
        "created spacer: autosave='{SPACER_AUTOSAVE}' length={SPACER_WIDTH_SHOWN} title='{SPACER_TITLE_DEBUG}'(debug)"
    ));

    StatusItems { anchor, spacer }
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
