//! Контекстное меню статус-айтема (правый клик по якорю).
//!
//! Меню НЕ присваивается через `statusItem.setMenu` — иначе ЛЕВЫЙ клик тоже
//! открывал бы его и ломал toggle. Вместо этого контроллер ловит правый клик и
//! показывает меню вручную через `popUpMenuPositioningItem_atLocation_inView`.
//!
//! Здесь только ПОСТРОЕНИЕ меню. Действия пунктов (`onAbout:`, `onQuit:`)
//! определены в контроллере — он target этих пунктов.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{sel, MainThreadMarker};
use objc2_app_kit::{NSMenu, NSMenuItem};
use objc2_foundation::NSString;

use crate::strings::{self, Lang};

/// Строит контекстное меню: «О программе» · разделитель · «Выход».
/// Строки локализованы по `lang`.
///
/// `target` — контроллер с методами `onAbout:` и `onQuit:`.
///
/// # Safety
/// `target` должен жить, пока показывается меню, и реализовывать оба селектора.
pub unsafe fn build(mtm: MainThreadMarker, target: &AnyObject, lang: Lang) -> Retained<NSMenu> {
    let menu = NSMenu::new(mtm);

    let about = item(mtm, strings::menu_about(lang), sel!(onAbout:), "", target);
    menu.addItem(&about);

    menu.addItem(&NSMenuItem::separatorItem(mtm));

    // key_equivalent "q" → пункт ловит Cmd+Q (модификатор Command по умолчанию).
    let quit = item(mtm, strings::menu_quit(lang), sel!(onQuit:), "q", target);
    menu.addItem(&quit);

    menu
}

/// Создаёт пункт меню с заголовком, действием, горячей клавишей и target'ом.
unsafe fn item(
    mtm: MainThreadMarker,
    title: &str,
    action: objc2::runtime::Sel,
    key_equivalent: &str,
    target: &AnyObject,
) -> Retained<NSMenuItem> {
    let menu_item = NSMenuItem::initWithTitle_action_keyEquivalent(
        mtm.alloc(),
        &NSString::from_str(title),
        Some(action),
        &NSString::from_str(key_equivalent),
    );
    menu_item.setTarget(Some(target));
    menu_item
}
