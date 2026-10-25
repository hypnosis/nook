//! Шторка над иконками панели: перед раскрытием, по чтению, по картинке, при смене приложения.

use std::time::Instant;

use objc2::{DefinedClass, MainThreadOnly};
use objc2_app_kit::NSWorkspace;

use super::Controller;
use crate::bar::Snapshot;
use crate::shroud::Shroud;
use crate::status_bar;
use crate::tuning::NARROW_ITEM_WIDTH;

impl Controller {
    /// Готова картинка пустой полосы: шторка встаёт на неё, если строка с тех пор не менялась
    /// и нажатая иконка не подсвечена.
    pub(super) fn place_shroud(&self) {
        let Some((image, rect, epoch)) = crate::shroud::take_picture() else { return };
        if epoch != self.ivars().epoch.get()
            || self.ivars().hidden.get()
            || crate::settings::automatic_layout()
            || crate::press::icon_highlighted()
        {
            return;
        }
        if let Some(shroud) = self.ivars().shroud.borrow_mut().as_mut() {
            shroud.show(self.mtm(), &image, rect);
            log::debug!("шторка: встала {:.0}..{:.0}", rect.origin.x, rect.origin.x + rect.size.width);
        }
    }

    /// Строка свернулась и встала: иконки панели ушли за край, шторка больше не нужна.
    pub(super) fn shroud_after_collapse(&self) {
        if self.ivars().hidden.get() {
            self.hide_shroud();
            log::debug!("шторка: ушла после сворачивания");
        }
    }

    /// Сменилось активное приложение: его меню заходит в полосу — шторка уходит сразу, не
    /// дожидаясь чтения; строка читается заново.
    pub(super) fn app_activated(&self) {
        if self.ivars().hidden.get() {
            return;
        }
        let shown = self.ivars().shroud.borrow().as_ref().filter(|shroud| shroud.is_shown()).and_then(Shroud::rect);
        if let Some(rect) = shown {
            let pid = NSWorkspace::sharedWorkspace().frontmostApplication().map(|app| app.processIdentifier());
            let menu_right = pid.and_then(crate::click::menu_right_of);
            if crate::bar::menu_covers(menu_right, rect.origin.x) {
                self.hide_shroud();
                log::debug!("шторка: меню приложения до {menu_right:?} заходит в полосу — ушла");
            }
        }
        self.bump_epoch();
        self.request_read();
    }

    /// До раскрытия шторка встаёт на прошлое место, чтобы иконки панели не мелькнули. В первый
    /// раз места нет: шторка закрывает всё от чёлки до `<`, первое чтение её уточнит.
    pub(super) fn cover_before_expand(&self) {
        if crate::settings::automatic_layout() || !self.divider_narrow() {
            return;
        }
        let last = self.ivars().shroud.borrow().as_ref().and_then(Shroud::rect);
        if let Some(rect) = last {
            if crate::bar::menu_covers(crate::click::app_menu_right(), rect.origin.x) {
                log::debug!("шторка: меню приложения заходит в полосу — раскрываю без шторки");
            } else if let Some(shroud) = self.ivars().shroud.borrow_mut().as_mut() {
                shroud.show_again();
            }
            return;
        }
        let anchor_x = crate::capture::icon_layout()
            .into_iter()
            .find(|window| window.name == status_bar::ANCHOR_AUTOSAVE)
            .map(|window| window.x);
        match anchor_x {
            Some(right) => self.cover_now(right),
            None => log::debug!("шторка: окна `<` нет — первое раскрытие без шторки"),
        }
    }

    /// Шторка сразу закрывает всё от чёлки до `right`: туда сейчас встанут иконки панели.
    pub(super) fn cover_now(&self, right: f64) {
        let left = self.notch_edge().unwrap_or(0.0);
        if crate::bar::menu_covers(crate::click::app_menu_right(), left) {
            log::debug!("шторка: меню приложения заходит в полосу {left:.0}..{right:.0} — без шторки");
            return;
        }
        let mut shroud = self.ivars().shroud.borrow_mut();
        let shroud = shroud.get_or_insert_with(|| Shroud::new(self.mtm()));
        let started = Instant::now();
        match crate::shroud::shoot_now(left, right, shroud.window_id()) {
            Some((image, rect)) => {
                shroud.show(self.mtm(), &image, rect);
                log::debug!("шторка: закрыла {left:.0}..{right:.0}, снимок {} мс", started.elapsed().as_millis());
            }
            None => log::debug!("шторка: снимка полосы {left:.0}..{right:.0} нет"),
        }
    }

    /// Разделитель панели сейчас узкий.
    fn divider_narrow(&self) -> bool {
        self.ivars().divider.borrow().as_ref().is_some_and(|divider| divider.length() <= NARROW_ITEM_WIDTH)
    }

    /// Шторка уходит, когда свёрнутая строка встанет.
    pub(super) fn uncover_after_collapse(&self) {
        if self.ivars().shroud.borrow().is_some() {
            crate::shroud::hide_when_still();
        }
    }

    pub(super) fn hide_shroud(&self) {
        if let Some(shroud) = self.ivars().shroud.borrow().as_ref() {
            shroud.hide();
        }
    }

    /// Шторка закрывает нарисованные иконки панели по свежему чтению. Картинка пустой строки
    /// переснимается, только когда сменилась полоса или картинка устарела, и никогда — пока
    /// открыто меню нажатой иконки: её подсветку macOS рисует в самой строке. В авторежиме
    /// шторки нет.
    pub(super) fn follow_shroud(&self, snapshot: &Snapshot, automatic: bool) {
        let strip = if automatic { None } else { snapshot.shroud_strip() };
        if !automatic {
            let shown = self
                .ivars()
                .shroud
                .borrow()
                .as_ref()
                .filter(|shroud| shroud.is_shown())
                .and_then(Shroud::rect)
                .map(|rect| (rect.origin.x, rect.origin.x + rect.size.width));
            let (outside, main_hidden) = snapshot.shroud_report(shown);
            log::debug!(
                "шторка: показана {shown:?}, полоса {strip:?}, иконки панели вне шторки: {outside}, основной ряд не нарисован: {main_hidden}"
            );
        }
        let Some((left, right)) = strip else {
            self.hide_shroud();
            return;
        };
        if crate::press::icon_highlighted() {
            return;
        }
        let mut shroud = self.ivars().shroud.borrow_mut();
        let shroud = shroud.get_or_insert_with(|| Shroud::new(self.mtm()));
        if shroud.needs_shot(left, right) {
            crate::shroud::shoot_later(left, right, shroud.window_id(), self.ivars().epoch.get());
        }
    }
}
