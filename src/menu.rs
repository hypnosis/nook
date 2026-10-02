//! Контекстное меню статус-айтема (правый клик по якорю) и меню приложения.
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

/// Строит контекстное меню: «Настройки…» · разделитель ·
/// «Запускать при входе» (тумблер) · разделитель · «Выход».
/// Строки локализованы по `lang`.
///
/// `target` — контроллер с методами `onOpenSettings:`, `onToggleLogin:` и `onQuit:`.
///
/// # Safety
/// `target` должен жить, пока показывается меню, и реализовывать оба селектора.
pub unsafe fn build(mtm: MainThreadMarker, target: &AnyObject, lang: Lang) -> Retained<NSMenu> {
    let menu = NSMenu::new(mtm);

    let settings = item(
        mtm,
        strings::menu_settings(lang),
        sel!(onOpenSettings:),
        ",",
        Some(target),
    );
    menu.addItem(&settings);

    menu.addItem(&NSMenuItem::separatorItem(mtm));

    // Тумблер автозапуска: галочка отражает текущее состояние SMAppService.
    let login = item(
        mtm,
        strings::menu_login(lang),
        sel!(onToggleLogin:),
        "",
        Some(target),
    );
    let state = if crate::login::is_enabled() {
        NSControlStateValueOn
    } else {
        NSControlStateValueOff
    };
    login.setState(state);
    menu.addItem(&login);

    menu.addItem(&NSMenuItem::separatorItem(mtm));

    let quit = item(mtm, strings::menu_quit(lang), sel!(onQuit:), "", Some(target));
    menu.addItem(&quit);

    menu
}

/// Меню приложения в строке меню, пока открыто окно настроек или разрешений:
/// «Nook» (Настройки…, Скрыть, Завершить) и «Файл» (Закрыть окно).
///
/// # Safety
/// `target` должен жить до конца программы и реализовывать `onOpenSettings:` и `onQuit:`.
pub unsafe fn main_menu(mtm: MainThreadMarker, target: &AnyObject, lang: Lang) -> Retained<NSMenu> {
    let app_menu = NSMenu::new(mtm);
    app_menu.addItem(&item(
        mtm,
        strings::menu_settings(lang),
        sel!(onOpenSettings:),
        ",",
        Some(target),
    ));
    app_menu.addItem(&NSMenuItem::separatorItem(mtm));
    app_menu.addItem(&item(mtm, strings::menu_hide_app(lang), sel!(hide:), "h", None));
    app_menu.addItem(&NSMenuItem::separatorItem(mtm));
    app_menu.addItem(&item(
        mtm,
        strings::menu_quit_app(lang),
        sel!(onQuit:),
        "q",
        Some(target),
    ));

    let file_menu = NSMenu::initWithTitle(mtm.alloc(), &NSString::from_str(strings::menu_file(lang)));
    file_menu.addItem(&item(
        mtm,
        strings::menu_close_window(lang),
        sel!(performClose:),
        "w",
        None,
    ));

    let bar = NSMenu::new(mtm);
    for submenu in [&app_menu, &file_menu] {
        let holder = NSMenuItem::new(mtm);
        holder.setSubmenu(Some(submenu));
        bar.addItem(&holder);
    }
    bar
}

/// Создаёт пункт меню с заголовком, действием и горячей клавишей. Без `target`
/// действие идёт по цепочке ответчиков: к ключевому окну и приложению.
unsafe fn item(
    mtm: MainThreadMarker,
    title: &str,
    action: objc2::runtime::Sel,
    key_equivalent: &str,
    target: Option<&AnyObject>,
) -> Retained<NSMenuItem> {
    let menu_item = NSMenuItem::initWithTitle_action_keyEquivalent(
        mtm.alloc(),
        &NSString::from_str(title),
        Some(action),
        &NSString::from_str(key_equivalent),
    );
    menu_item.setTarget(target);
    menu_item
}
