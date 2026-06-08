//! Nook — пряталка лишних иконок в строке меню macOS 26 Tahoe.
//!
//! ШАГ 1: видимый кликабельный `<` + невидимый спейсер слева. Клик прячет иконки
//! левее спейсера (символ → `v`), повторный клик возвращает (→ `<`). Якорь `<`
//! не двигается. Guard блокирует скрытие, если спейсер не левее якоря (→ `⚠`).
//!
//! Позиции айтемов запоминаются системой через setAutosaveName: разложил пару
//! Cmd+drag один раз — порядок переживает перезапуск.

mod auto_collapse;
mod controller;
mod log;
mod login;
mod menu;
mod status_bar;
mod strings;

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate};

use crate::controller::Controller;

fn main() {
    log::reset();
    log::append("=== ШАГ 1 запуск ===");

    let mtm = MainThreadMarker::new().expect("main должен идти на главном потоке");
    let app = NSApplication::sharedApplication(mtm);

    // Accessory: приложение-агент без иконки в Dock, живёт только в строке меню.
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    // Контроллер удерживается до конца программы — он владелец айтемов и делегат.
    let controller: Retained<Controller> = Controller::new(mtm);
    let delegate = ProtocolObject::<dyn NSApplicationDelegate>::from_ref(&*controller);
    app.setDelegate(Some(delegate));

    log::append("делегат установлен, запускаю RunLoop");
    app.run();
}
