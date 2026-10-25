//! Настройки, меню `<`, переключатели, разрешения и Dock.

use objc2::runtime::AnyObject;
use objc2::{msg_send, sel, DefinedClass, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSControlStateValueOn, NSEventType, NSSwitch, NSWindow,
    NSWindowDidBecomeKeyNotification, NSWindowWillCloseNotification,
};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSPoint, NSProcessInfo};

use super::Controller;
use crate::settings::Settings;
use crate::tuning::DIVIDER_SETTLE_DELAY;

impl Controller {
    /// Клик по якорю. Левый → toggle (прятать/показывать), пока не идёт перенос. Правый → меню.
    /// Тип события берём из currentEvent — так разделяем левый/правый без
    /// присвоения statusItem.menu (иначе левый клик тоже открывал бы меню).
    pub(super) fn anchor_clicked(&self, sender: *mut AnyObject) {
        let event = NSApplication::sharedApplication(self.mtm()).currentEvent();
        let event_type = event.as_ref().map(|e| e.r#type());
        if let Some(event) = &event {
            let waited = NSProcessInfo::processInfo().systemUptime() - event.timestamp();
            log::debug!("клик по `<`: ждал главного потока {:.0} мс", waited * 1000.0);
        }

        if event_type == Some(NSEventType::RightMouseUp) {
            self.show_menu(sender);
        } else if !self.ivars().moving.get() {
            self.toggle();
        }
    }

    /// Пункт меню «Запускать при входе» — переключает автозапуск.
    /// Галочка обновится при следующем открытии меню (build читает статус).
    pub(super) fn toggle_login(&self) {
        crate::login::toggle();
        if let Some(settings) = self.ivars().settings.borrow().as_ref() {
            settings.refresh();
        }
    }

    /// Пункт меню «Настройки…» — открывает окно настроек.
    pub(super) fn open_settings(&self) {
        if self.ivars().settings.borrow().is_none() {
            let target: &AnyObject = self.as_ref();
            let settings = Settings::new(self.mtm(), target, self.ivars().lang);
            self.observe_window(settings.window());
            *self.ivars().settings.borrow_mut() = Some(settings);
        }
        self.set_in_dock(true);
        // Показ делает окно ключевым, и onWindowFocus: читает настройки внутри этого вызова.
        if let Some(settings) = self.ivars().settings.borrow().as_ref() {
            settings.show(self.mtm());
        }
        // Окно открыли снова на разделе «Расположение» — выбор раздела не меняется.
        if self.layout_shown() {
            let _: () = unsafe { msg_send![self, onEditorNeedsIcons] };
        }
    }

    /// Окно настроек или разрешений закрывается: других открытых нет — Nook уходит из Dock.
    pub(super) fn window_closing(&self, notification: &NSNotification) {
        let closing = notification.object();
        let is_other_open = |window: &NSWindow| {
            window.isVisible()
                && closing
                    .as_deref()
                    .is_none_or(|closing| !std::ptr::eq(closing, window.as_ref()))
        };
        let settings_open = self.ivars().settings.borrow().as_ref().is_some_and(|settings| is_other_open(settings.window()));
        let onboarding_open = self.ivars().onboarding.borrow().as_ref().is_some_and(|onboarding| is_other_open(onboarding.window()));
        if !settings_open && !onboarding_open {
            self.set_in_dock(false);
        }
    }

    /// Окно настроек или разрешений стало активным — перечитываем разрешения и переключатели.
    pub(super) fn refresh_windows(&self) {
        if let Some(settings) = self.ivars().settings.borrow().as_ref() {
            settings.refresh();
        }
        if let Some(onboarding) = self.ivars().onboarding.borrow().as_ref() {
            onboarding.refresh();
        }
    }

    /// Переключатель «Показывать панель».
    pub(super) fn show_panel_switched(&self, sender: &NSSwitch) {
        let on = sender.state() == NSControlStateValueOn;
        crate::settings::set_show_panel(on);
        if let Some(settings) = self.ivars().settings.borrow().as_ref() {
            settings.refresh();
        }
        if let Some(items) = self.items() {
            self.sync_panel(&items, self.ivars().hidden.get());
        }
    }

    /// Переключатель «Автоматическое расположение». В авто разделителя в строке нет,
    /// в ручном режиме он возвращается на своё место.
    pub(super) fn automatic_layout_switched(&self, sender: &NSSwitch) {
        if self.ivars().moving.get() {
            return;
        }
        let automatic = sender.state() == NSControlStateValueOn;
        crate::settings::set_automatic_layout(automatic);
        if let Some(settings) = self.ivars().settings.borrow().as_ref() {
            settings.refresh();
        }
        self.show_divider(!automatic);
        self.bump_epoch();
        if automatic {
            self.ivars().own_move.borrow_mut().take();
            self.read_after(DIVIDER_SETTLE_DELAY);
        } else {
            self.restore_layout();
        }
    }

    /// «Сбросить разрешения»: записи Nook убираются из Системных настроек, Nook
    /// перезапускается и спрашивает разрешения заново.
    pub(super) fn reset_permissions(&self) {
        crate::permissions::reset_all();
        crate::permissions::relaunch_later();
        NSApplication::sharedApplication(self.mtm()).terminate(None);
    }

    /// Тема macOS сменилась: панель сразу берёт картинки из кэша новой темы,
    /// а раскрытая строка ещё и переснимается.
    pub(super) fn theme_changed(&self) {
        let theme = crate::theme::current();
        if let Some(shroud) = self.ivars().shroud.borrow_mut().as_mut() {
            shroud.mark_stale();
        }
        let target: &AnyObject = self.as_ref();
        if let Some(panel) = self.ivars().panel.borrow_mut().as_mut() {
            panel.set_theme(self.mtm(), target, theme);
        }
        if !self.ivars().hidden.get() {
            self.read_after(DIVIDER_SETTLE_DELAY);
        }
    }

    /// Окно настроек или разрешений сообщает контроллеру, что стало активным и что закрывается.
    pub(super) fn observe_window(&self, window: &NSWindow) {
        let target: &AnyObject = self.as_ref();
        let center = NSNotificationCenter::defaultCenter();
        unsafe {
            center.addObserver_selector_name_object(
                target,
                sel!(onWindowFocus:),
                Some(NSWindowDidBecomeKeyNotification),
                Some(window),
            );
            center.addObserver_selector_name_object(
                target,
                sel!(onWindowClose:),
                Some(NSWindowWillCloseNotification),
                Some(window),
            );
        }
    }

    /// Пока открыто окно настроек или разрешений, Nook — обычное приложение:
    /// иконка в Dock, Cmd+Tab и своё меню. Иначе живёт только в строке меню.
    pub(super) fn set_in_dock(&self, on: bool) {
        let policy = if on {
            NSApplicationActivationPolicy::Regular
        } else {
            NSApplicationActivationPolicy::Accessory
        };
        NSApplication::sharedApplication(self.mtm()).setActivationPolicy(policy);
    }

    /// Показывает контекстное меню у кнопки якоря (правый клик).
    /// Меню строится здесь и показывается вручную — НЕ через statusItem.menu,
    /// чтобы не перехватывать левый клик.
    pub(super) fn show_menu(&self, _sender: *mut AnyObject) {
        let Some(items) = self.items() else {
            return;
        };
        let Some(button) = items.anchor.button(self.mtm()) else {
            return;
        };

        let target: &AnyObject = self.as_ref();
        let menu = unsafe { crate::menu::build(self.mtm(), target, self.ivars().lang) };

        // Якорим меню к НИЖНЕЙ кромке кнопки, чтобы оно падало вниз, а не налезало
        // на menu bar. NSStatusBarButton не flipped → y=0 это низ. При y>0 (над
        // кнопкой) верх меню уходил под строку меню и первый пункт прятался под
        // scroll-arrow. См. perplexity/AppKit: at = (midX, 0) в координатах кнопки.
        let origin = NSPoint::new(0.0, 0.0);
        menu.popUpMenuPositioningItem_atLocation_inView(None, origin, Some(&button));
    }
}
