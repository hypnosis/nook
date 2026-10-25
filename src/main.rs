//! Nook — пряталка лишних иконок в строке меню macOS 26 Tahoe.
//!
//! Свёрнутая строка прячет всё левее `<`. Раскрытая показывает основной ряд, а иконки панели —
//! в панели под строкой меню. Места своих элементов строки помнит macOS.

mod auto_collapse;
mod bar;
mod capture;
mod click;
mod controller;
mod divider;
mod editor;
mod layout;
mod logging;
mod login;
mod menu;
mod mover;
mod onboarding;
mod panel;
mod permissions;
mod press;
mod settings;
mod shroud;
mod status_bar;
mod strings;
mod theme;
mod timer;
mod tuning;
mod ui_style;

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate};

use crate::controller::Controller;

fn main() {
    logging::init();

    let mtm = MainThreadMarker::new().expect("main должен идти на главном потоке");
    let app = NSApplication::sharedApplication(mtm);

    // Accessory: приложение-агент без иконки в Dock, живёт только в строке меню.
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    mover::allow_cursor_hiding_in_background();

    // Контроллер удерживается до конца программы — он владелец айтемов и делегат.
    let controller: Retained<Controller> = Controller::new(mtm);
    let delegate = ProtocolObject::<dyn NSApplicationDelegate>::from_ref(&*controller);
    app.setDelegate(Some(delegate));

    app.run();
}
