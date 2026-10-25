//! Сворачивание и раскрытие строки, автосворачивание, ⚠ при нарушенном порядке.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{sel, DefinedClass, MainThreadOnly, Message};
use objc2_app_kit::NSScreen;

use super::Controller;
use crate::auto_collapse::{self, MouseMonitor};
use crate::bar::Order;
use crate::panel::Panel;
use crate::status_bar::{self, StatusItems, ANCHOR_SYMBOL_BLOCKED, ANCHOR_SYMBOL_HIDDEN, ANCHOR_SYMBOL_SHOWN};
use crate::timer::Timer;
use crate::tuning::{
    COLLAPSE_DELAY, HIDDEN_WIDTH_MARGIN, HIDDEN_WIDTH_MAX, HIDING_WIDTH_MIN, NARROW_ITEM_WIDTH, PANEL_REFRESH_INTERVAL,
    SCREEN_WIDTH_FALLBACK,
};

impl Controller {
    /// Движение мыши (от монитора). Работает только когда иконки показаны.
    /// Мышь в полосе menu bar или над панелью → гасим таймер коллапса.
    /// Мышь ушла → взводим.
    pub(super) fn mouse_moved(&self) {
        if self.ivars().hidden.get() {
            return; // показывать нечего — следить незачем
        }
        let in_strip = auto_collapse::mouse_in_menu_bar_strip(self.mtm());
        let in_panel = self.mouse_in_panel();
        if in_strip || in_panel {
            self.cancel_collapse_timer();
        } else if self.ivars().collapse_timer.borrow().is_none() {
            self.arm_collapse_timer();
        }
    }

    /// Таймер коллапса дожил: мышь была вне полосы `COLLAPSE_DELAY`. Пока открыто меню копии,
    /// сворачивание откладывается на тот же срок; мышь снова в строке или над панелью — нет.
    pub(super) fn auto_collapse(&self) {
        self.ivars().collapse_timer.borrow_mut().take();
        if self.ivars().hidden.get() || self.ivars().moving.get() {
            return;
        }
        if crate::press::menu_open() {
            self.arm_collapse_timer();
            return;
        }
        if auto_collapse::mouse_in_menu_bar_strip(self.mtm()) || self.mouse_in_panel() {
            return;
        }
        self.toggle();
    }

    /// Переключает видимость зоны слева от якоря — с проверкой порядка.
    pub(super) fn toggle(&self) {
        let Some(items) = self.items() else {
            log::debug!("toggle: айтемы ещё не созданы — игнор");
            return;
        };

        let going_to_hide = !self.ivars().hidden.get();

        // Прячем только при целом порядке. `<` спрятан macOS или ещё не размещён — ждём.
        if going_to_hide {
            let (order, coords) = crate::bar::nook_order(self.notch_edge());
            if matches!(order, Order::Hidden | Order::Unknown) {
                log::debug!("строка: не сворачиваю, {order:?} — {coords}");
                return;
            }
            if order != Order::Ok || self.ivars().blocked.get() {
                self.show_blocked(order, &coords);
                return;
            }
        }

        self.ivars().hidden.set(going_to_hide);
        let (spacer_width, anchor_symbol) = if going_to_hide {
            (self.hidden_width(), ANCHOR_SYMBOL_HIDDEN)
        } else {
            (NARROW_ITEM_WIDTH, ANCHOR_SYMBOL_SHOWN)
        };

        log::debug!("строка: {}", if going_to_hide { "сворачиваю" } else { "раскрываю" });
        if !going_to_hide {
            self.cover_before_expand();
        }
        items.spacer.setLength(spacer_width);
        status_bar::set_anchor_symbol(&items, self.mtm(), anchor_symbol);
        self.bump_epoch();
        if going_to_hide {
            self.uncover_after_collapse();
        }

        // Смена состояния гасит взведённый таймер коллапса.
        self.cancel_collapse_timer();
        // Показали → следим за мышью (для автосворачивания). Скрыли → перестаём.
        if going_to_hide {
            self.stop_mouse_watch();
        } else {
            self.start_mouse_watch();
        }
        self.sync_panel(&items, going_to_hide);
    }

    /// Панель открыта ровно тогда, когда иконки раскрыты; пока они раскрыты, строка
    /// читается по таймеру.
    pub(super) fn sync_panel(&self, items: &StatusItems, hidden: bool) {
        self.watch_order(!hidden && self.ivars().placement_done.get());
        if hidden {
            self.ivars().bar_timer.borrow_mut().take();
        } else {
            self.request_read();
            let target: &AnyObject = self.as_ref();
            let mut timer = self.ivars().bar_timer.borrow_mut();
            if timer.is_none() {
                *timer = Some(Timer::repeating(target, PANEL_REFRESH_INTERVAL, sel!(onBarTick:)));
            }
        }
        let mut panel = self.ivars().panel.borrow_mut();
        if hidden || !crate::settings::show_panel() {
            if let Some(panel) = panel.as_mut() {
                panel.hide();
            }
            return;
        }
        let Some(anchor_window) = items.anchor.button(self.mtm()).and_then(|b| b.window()) else {
            return;
        };
        panel
            .get_or_insert_with(|| Panel::new(self.mtm()))
            .show_below(&anchor_window);
    }

    /// Запускает монитор мыши, если ещё не запущен.
    fn start_mouse_watch(&self) {
        if self.ivars().monitor.borrow().is_some() {
            return;
        }
        let target: Retained<AnyObject> = self.retain().into_super().into();
        *self.ivars().monitor.borrow_mut() = Some(MouseMonitor::start(target));
    }

    /// Останавливает монитор мыши.
    fn stop_mouse_watch(&self) {
        if let Some(mut monitor) = self.ivars().monitor.borrow_mut().take() {
            monitor.stop();
        }
    }

    /// Гасит взведённый таймер коллапса, если он есть.
    pub(super) fn cancel_collapse_timer(&self) {
        self.ivars().collapse_timer.borrow_mut().take();
    }

    /// Взводит таймер коллапса (мышь ушла из полосы). Через COLLAPSE_DELAY
    /// сработает onAutoCollapse: и свернёт, если мышь не вернулась.
    fn arm_collapse_timer(&self) {
        let timer = Timer::once(self.as_ref(), COLLAPSE_DELAY, sel!(onAutoCollapse:));
        *self.ivars().collapse_timer.borrow_mut() = Some(timer);
    }

    /// Показывает знак блокировки на якоре и оставляет всё как есть.
    /// Порядок нарушен: спейсер и разделитель узкие, иконки раскрыты, на якоре ⚠.
    pub(super) fn show_blocked(&self, order: Order, coords: &str) {
        if self.ivars().hidden.get() {
            self.toggle();
        }
        let Some(items) = self.items() else { return };
        items.spacer.setLength(NARROW_ITEM_WIDTH);
        if let Some(divider) = self.divider() {
            divider.setLength(NARROW_ITEM_WIDTH);
        }
        status_bar::set_anchor_symbol(&items, self.mtm(), ANCHOR_SYMBOL_BLOCKED);
        if !self.ivars().blocked.replace(true) {
            self.bump_epoch();
            log::error!("СТОП: {order:?} — скрытие заблокировано, показываю ⚠ ({coords})");
        }
    }

    /// Ширина спейсера в скрытом состоянии: ширина экрана + запас, ограниченная.
    fn hidden_width(&self) -> f64 {
        let screen_width = NSScreen::mainScreen(self.mtm())
            .map(|screen| screen.frame().size.width)
            .unwrap_or(SCREEN_WIDTH_FALLBACK);
        (screen_width + HIDDEN_WIDTH_MARGIN).clamp(HIDING_WIDTH_MIN, HIDDEN_WIDTH_MAX)
    }

    pub(super) fn mouse_in_panel(&self) -> bool {
        self.ivars().panel.borrow().as_ref().is_some_and(Panel::contains_mouse)
    }
}
