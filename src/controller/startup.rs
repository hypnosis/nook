//! Запуск: размещение элементов Nook, окно разрешений, стартовое чтение и сворачивание.

use objc2::runtime::AnyObject;
use objc2::{sel, DefinedClass, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationDidChangeScreenParametersNotification, NSWindowDidMoveNotification, NSWorkspace,
    NSWorkspaceDidActivateApplicationNotification,
};
use objc2_foundation::NSNotificationCenter;

use super::Controller;
use crate::onboarding::Onboarding;
use crate::status_bar;
use crate::timer::Timer;
use crate::tuning::{DIVIDER_SETTLE_DELAY, PLACEMENT_CHECK_INTERVAL, PLACEMENT_ESCALATE_AFTER, PLACEMENT_MAX_ATTEMPTS};

/// x айтема, которого macOS ещё не разместила в строке.
const UNPLACED_X: f64 = 0.0;

impl Controller {
    /// Создаёт элементы Nook, подписывается на события строки, экранов и приложений, ставит меню.
    pub(super) fn launch(&self) {
        let mtm = self.mtm();
        let target: &AnyObject = self.as_ref();
        let items = unsafe { status_bar::create(mtm, target, self.ivars().lang) };

        *self.ivars().items.borrow_mut() = Some(items);
        self.ivars().hidden.set(false);

        // ОСНОВНОЙ триггер: подписка на NSWindowDidMoveNotification.
        // Система постит её, когда айтем получает реальную координату (x:0→1700).
        // Ловим точное СОБЫТИЕ размещения вместо гадания по таймеру.
        let target: &AnyObject = self.as_ref();
        unsafe {
            NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                target,
                sel!(onWindowMoved:),
                Some(NSWindowDidMoveNotification),
                None,
            );
        }
        crate::theme::observe(target);
        unsafe {
            NSWorkspace::sharedWorkspace().notificationCenter().addObserver_selector_name_object(
                target,
                sel!(onAppActivated:),
                Some(NSWorkspaceDidActivateApplicationNotification),
                None,
            );
        }
        unsafe {
            NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                target,
                sel!(onScreensChanged:),
                Some(NSApplicationDidChangeScreenParametersNotification),
                None,
            );
        }

        // Запасная проверка: элемент, застрявший на x=0 без события, пересоздаётся.
        // Размещение асинхронно: синхронное ожидание его ломает.
        let target: &AnyObject = self.as_ref();
        *self.ivars().placement_timer.borrow_mut() =
            Some(Timer::repeating(target, PLACEMENT_CHECK_INTERVAL, sel!(onPlacementCheck:)));
        let main_menu = unsafe { crate::menu::main_menu(mtm, target, self.ivars().lang) };
        NSApplication::sharedApplication(mtm).setMainMenu(Some(&main_menu));
        self.show_onboarding_if_needed();
    }

    /// Стартовый разделитель встал — читаем строку: чтение задаст длину разделителя и снимет
    /// иконки панели. Если macOS забыла место разделителя, сначала восстанавливаем раскладку.
    pub(super) fn divider_placed(&self) {
        self.ivars().starting.set(true);
        if self.ivars().restore_on_start.get() {
            self.start_restore();
        } else {
            self.request_read();
        }
    }

    /// Проверка размещения айтемов (периодическая, после старта).
    /// Если айтем застрял на x=0 — пересоздаёт его (до лимита попыток).
    /// Когда ОБА размещены — запускает стартовое авто-скрытие и останавливается.
    /// Так первый toggle гарантированно идёт по валидным координатам.
    pub(super) fn check_placement(&self) {
        let mtm = self.mtm();
        let (sx, ax) = {
            let items_ref = self.ivars().items.borrow();
            let Some(items) = items_ref.as_ref() else { return };
            (
                status_bar::item_origin_x(&items.spacer, mtm),
                status_bar::item_origin_x(&items.anchor, mtm),
            )
        };

        let lang = self.ivars().lang;
        let target: &AnyObject = self.as_ref();
        let both_ok = is_placed(sx) && is_placed(ax);

        // Оба размещены → общий финиш (событие могло уже его сделать) + стоп таймера.
        if both_ok {
            self.ivars().placement_timer.borrow_mut().take();
            self.finish_placement_if_ready();
            return;
        }

        // Не размещены. Считаем попытки; защита от вечного таймера.
        let attempts = self.ivars().placement_attempts.get() + 1;
        self.ivars().placement_attempts.set(attempts);
        if attempts >= PLACEMENT_MAX_ATTEMPTS {
            self.ivars().placement_timer.borrow_mut().take();
            log::warn!("placement: лимит попыток исчерпан — сдаюсь (guard защитит)");
            return;
        }

        // Эскалация: после нескольких безуспешных попыток одиночное пересоздание
        // не помогает (новый айтем садится на тот же x=0) → пересоздать ОБА.
        if attempts.is_multiple_of(PLACEMENT_ESCALATE_AFTER) {
            let mut items_ref = self.ivars().items.borrow_mut();
            if let Some(items) = items_ref.as_mut() {
                unsafe { status_bar::recreate_both(items, mtm, target, lang) };
            }
            return;
        }

        // Обычный retry: пересоздать конкретный застрявший айтем.
        if sx == Some(UNPLACED_X) && self.ivars().spacer_retries.get() > 0 {
            self.ivars()
                .spacer_retries
                .set(self.ivars().spacer_retries.get() - 1);
            let mut items_ref = self.ivars().items.borrow_mut();
            if let Some(items) = items_ref.as_mut() {
                unsafe { status_bar::recreate_spacer(items, mtm, lang) };
            }
            return;
        }
        if ax == Some(UNPLACED_X) && self.ivars().anchor_retries.get() > 0 {
            self.ivars()
                .anchor_retries
                .set(self.ivars().anchor_retries.get() - 1);
            let mut items_ref = self.ivars().items.borrow_mut();
            if let Some(items) = items_ref.as_mut() {
                unsafe { status_bar::recreate_anchor(items, mtm, target, lang) };
            }
        }
    }

    /// Показывает окно разрешений, если какого-то разрешения нет.
    pub(super) fn show_onboarding_if_needed(&self) {
        if crate::permissions::all_granted() {
            return;
        }
        let target: &AnyObject = self.as_ref();
        let onboarding = Onboarding::new(self.mtm(), target, self.ivars().lang);
        self.observe_window(onboarding.window());
        self.set_in_dock(true);
        onboarding.show(self.mtm());
        *self.ivars().onboarding.borrow_mut() = Some(onboarding);
    }

    /// Старт закончен: иконки панели сняты — строка сворачивается.
    pub(super) fn finish_startup(&self) {
        if self.ivars().starting.replace(false) && !self.ivars().hidden.get() {
            self.toggle();
        }
    }

    /// На старте разделитель так и не прочитался — сворачиваемся без снимков.
    pub(super) fn finish_startup_if_stuck(&self) {
        if self.ivars().starting.get() {
            log::warn!("старт: разделитель не прочитан — сворачиваю без снимков панели");
            self.finish_startup();
        }
    }

    /// Завершает стартовую настройку, КОГДА оба айтема размещены (x>0).
    /// Вызывается и из события NSWindowDidMove (основной триггер), и из
    /// fallback-таймера. Идемпотентно: срабатывает один раз (флаг placement_done),
    /// отписывается от нотификации и создаёт разделитель панели.
    pub(super) fn finish_placement_if_ready(&self) {
        if self.ivars().placement_done.get() {
            return;
        }
        let mtm = self.mtm();
        let (sx, ax) = {
            let items_ref = self.ivars().items.borrow();
            let Some(items) = items_ref.as_ref() else { return };
            (
                status_bar::item_origin_x(&items.spacer, mtm),
                status_bar::item_origin_x(&items.anchor, mtm),
            )
        };
        let both_ok = is_placed(sx) && is_placed(ax);
        if !both_ok {
            return;
        }

        self.ivars().placement_done.set(true);
        self.watch_order(!self.ivars().hidden.get());
        // Отписываемся от событий перемещения — стартовая настройка завершена.
        let observer: &AnyObject = self.as_ref();
        unsafe {
            NSNotificationCenter::defaultCenter().removeObserver_name_object(
                observer,
                Some(NSWindowDidMoveNotification),
                None,
            );
        }
        if self.ivars().hidden.get() {
            return;
        }
        let automatic = crate::settings::automatic_layout();
        self.ivars().restore_on_start.set(!automatic && !crate::divider::has_saved_place());
        *self.ivars().divider.borrow_mut() = Some(crate::divider::create(self.mtm()));
        self.show_divider(!automatic);
        crate::timer::after(self.as_ref(), DIVIDER_SETTLE_DELAY, sel!(onDividerPlaced:));
    }
}

/// Айтем стоит в строке: его x известен и это не место неразмещённого.
fn is_placed(x: Option<f64>) -> bool {
    x.is_some_and(|x| x != UNPLACED_X)
}
