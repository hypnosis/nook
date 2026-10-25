//! Сторож порядка элементов Nook и смена экранов.

use std::time::Instant;

use objc2::{sel, DefinedClass, MainThreadOnly};
use objc2_app_kit::NSEvent;

use super::Controller;
use crate::bar::Order;
use crate::status_bar::{self, ANCHOR_SYMBOL_HIDDEN, ANCHOR_SYMBOL_SHOWN};
use crate::timer::Timer;
use crate::tuning::{ORDER_BAD_READS, ORDER_CHECK_INTERVAL, SCREEN_SETTLE};

impl Controller {
    /// Сторож: нарушенный порядок, державшийся несколько проверок подряд, блокирует скрытие и
    /// ставит ⚠. Спрятанный `<` — не нарушение: его прячет macOS, и место `<` Nook не меняет.
    pub(super) fn check_order(&self) {
        let settling = self
            .ivars()
            .screens_changed
            .get()
            .is_some_and(|at| at.elapsed() < SCREEN_SETTLE);
        if self.ivars().moving.get() || NSEvent::pressedMouseButtons() != 0 || settling {
            return;
        }
        let (order, coords) = crate::bar::nook_order(self.notch_edge());
        match order {
            Order::Unknown | Order::Hidden => {}
            Order::Ok => {
                self.ivars().bad_reads.set(0);
                if self.ivars().blocked.replace(false) {
                    if let Some(items) = self.items() {
                        let symbol = if self.ivars().hidden.get() { ANCHOR_SYMBOL_HIDDEN } else { ANCHOR_SYMBOL_SHOWN };
                        status_bar::set_anchor_symbol(&items, self.mtm(), symbol);
                    }
                    log::info!("сторож: порядок восстановлен — {coords}");
                    self.request_read();
                }
            }
            Order::Misordered => {
                let bad = self.ivars().bad_reads.get() + 1;
                self.ivars().bad_reads.set(bad);
                if bad >= ORDER_BAD_READS {
                    self.show_blocked(order, &coords);
                }
            }
        }
    }

    /// Экраны сменились: строка раскладывается заново, сторож переждёт.
    pub(super) fn screens_changed(&self) {
        self.ivars().screens_changed.set(Some(Instant::now()));
        self.ivars().bad_reads.set(0);
    }

    /// Сторож сверяет порядок, пока строка раскрыта. В свёрнутой строке он не нужен: перед
    /// сворачиванием порядок проверяет `toggle`.
    pub(super) fn watch_order(&self, on: bool) {
        let mut timer = self.ivars().order_timer.borrow_mut();
        if !on {
            if timer.take().is_some() {
                self.ivars().bad_reads.set(0);
            }
            return;
        }
        if timer.is_none() {
            *timer = Some(Timer::repeating(self.as_ref(), ORDER_CHECK_INTERVAL, sel!(onOrderCheck:)));
        }
    }
}
