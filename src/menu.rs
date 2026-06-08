//! Контекстное меню статус-айтема (правый клик по якорю).
//!
//! Меню НЕ присваивается через `statusItem.setMenu` — иначе ЛЕВЫЙ клик тоже
//! открывал бы его и ломал toggle. Вместо этого контроллер ловит правый клик и
//! показывает меню вручную через `popUpMenuPositioningItem_atLocation_inView`.
//!
//! Здесь только ПОСТРОЕНИЕ меню. Действия пунктов (`onToggleLogin:`, `onQuit:`)
//! определены в контроллере — он target этих пунктов.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{sel, MainThreadMarker};
use objc2_app_kit::{NSControlStateValueOff, NSControlStateValueOn, NSMenu, NSMenuItem};
use objc2_foundation::NSString;

use crate::strings::{self, Lang};

/// Строит контекстное меню: версия · разделитель ·
/// «Запускать при входе» (тумблер) · разделитель · «Выход».
/// Строки локализованы по `lang`.
///
/// `target` — контроллер с методами `onToggleLogin:` и `onQuit:`.
///
/// # Safety
/// `target` должен жить, пока показывается меню, и реализовывать оба селектора.
pub unsafe fn build(mtm: MainThreadMarker, target: &AnyObject, lang: Lang) -> Retained<NSMenu> {
    let menu = NSMenu::new(mtm);

    // Версия первым пунктом (на месте бывшего «О Nook»). Обычный включённый пункт
    // без action — клик ничего не делает. Не disabled: disabled первый пункт macOS
    // рисует как scroll-arrow при показе через popUpMenuPositioningItem.
    let version = format!("Nook {}", env!("CARGO_PKG_VERSION"));
    let version_item = NSMenuItem::new(mtm);
    version_item.setTitle(&NSString::from_str(&version));
    menu.addItem(&version_item);

    menu.addItem(&NSMenuItem::separatorItem(mtm));

    // Тумблер автозапуска: галочка отражает текущее состояние SMAppService.
    let login = item(mtm, strings::menu_login(lang), sel!(onToggleLogin:), "", target);
    let state = if crate::login::is_enabled() {
        NSControlStateValueOn
    } else {
        NSControlStateValueOff
    };
    login.setState(state);
    menu.addItem(&login);

    menu.addItem(&NSMenuItem::separatorItem(mtm));

    let quit = item(mtm, strings::menu_quit(lang), sel!(onQuit:), "", target);
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
