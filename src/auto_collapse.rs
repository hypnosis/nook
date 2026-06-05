//! Глобальный монитор мыши для автосворачивания.
//!
//! Делает ровно одно: пока включён, на каждое движение мыши дёргает у контроллера
//! метод `onMouseMoved:` — а тот сам решает (мышь в полосе menu bar или ушла →
//! сбросить или взвести таймер коллапса). Вся логика таймера живёт в контроллере;
//! здесь только подписка на события мыши и её снятие.
//!
//! Монитор пассивный (NSEvent global monitor) — наблюдает позицию, не перехватывает
//! события, не требует Accessibility. Ховер НЕ разворачивает: следим лишь за уходом.

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{msg_send, sel};
use objc2::MainThreadMarker;
use objc2_app_kit::{NSEvent, NSEventMask, NSScreen};
use objc2_foundation::NSTimer;

/// Задержка перед сворачиванием после ухода мыши из полосы menu bar.
// HARDCODE: задержка автосворачивания; вынести в конфиг позже.
const COLLAPSE_DELAY: f64 = 3.0;

/// Создаёт одноразовый таймер коллапса. Через COLLAPSE_DELAY вызовет
/// `onAutoCollapse:` на target (контроллере).
pub fn make_collapse_timer(target: &AnyObject) -> Retained<NSTimer> {
    unsafe {
        NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
            COLLAPSE_DELAY,
            target,
            sel!(onAutoCollapse:),
            None,
            false,
        )
    }
}

/// Высота полосы menu bar для детекта «мышь ушла». ~40px = реальная строка + запас,
/// чтобы не сворачивать при лёгком уводе мыши вниз.
// HARDCODE: высота полосы menu bar; вынести в конфиг позже.
const MENU_BAR_STRIP: f64 = 40.0;

/// Мышь сейчас в полосе menu bar активного (верхнего) экрана?
pub fn mouse_in_menu_bar_strip(mtm: MainThreadMarker) -> bool {
    let mouse = NSEvent::mouseLocation();
    let Some(screen) = NSScreen::mainScreen(mtm) else {
        return false;
    };
    let frame = screen.frame();
    // Координаты экрана: Y растёт вверх, top — верхняя кромка строки меню.
    let top = frame.origin.y + frame.size.height;
    mouse.y >= top - MENU_BAR_STRIP && mouse.y <= top
}

/// Токен глобального монитора. Хранить обязательно — иначе монитор снимается.
pub struct MouseMonitor {
    token: Option<Retained<AnyObject>>,
}

impl MouseMonitor {
    /// Подписывается на движения мыши. На каждое — зовёт `onMouseMoved:` у target.
    /// `target` (контроллер) живёт всё время работы приложения.
    pub fn start(target: Retained<AnyObject>) -> Self {
        let handler = RcBlock::new(move |_event: core::ptr::NonNull<NSEvent>| {
            unsafe {
                let _: () = msg_send![&*target, onMouseMoved];
            }
        });
        let token = NSEvent::addGlobalMonitorForEventsMatchingMask_handler(
            NSEventMask::MouseMoved,
            &handler,
        );
        crate::log::append("auto-collapse: монитор мыши включён");
        Self { token }
    }

    /// Снимает монитор.
    pub fn stop(&mut self) {
        if let Some(token) = self.token.take() {
            unsafe { NSEvent::removeMonitor(&token) };
            crate::log::append("auto-collapse: монитор мыши выключен");
        }
    }
}
