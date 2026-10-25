//! Переносы иконок: броски из редактора, восстановление раскладки, сторож переноса.

use objc2::{sel, DefinedClass};

use super::Controller;
use crate::timer::Timer;
use crate::tuning::{MOVE_WATCHDOG, MOVE_WATCHDOG_POLL, NARROW_ITEM_WIDTH};

impl Controller {
    /// Плитка `id` брошена в редакторе. Внутри панели строку не трогаем: меняется только
    /// порядок панели. Иначе настоящая иконка переносится одним переносом: в панель —
    /// к разделителю, в основной ряд — к соседям по редактору.
    pub(super) fn tile_dropped(&self, id: u32) {
        if self.ivars().moving.get() {
            return;
        }
        let Some((panel, main)) = self.ivars().settings.borrow().as_ref().map(|settings| settings.editor.order())
        else {
            return;
        };
        let was_in_panel = self.ivars().layout.borrow().panel().contains(&id);
        if panel.contains(&id) && was_in_panel {
            if self.ivars().layout.borrow_mut().reorder_panel(&panel) {
                self.persist_panel();
            }
            self.show_layout();
            return;
        }
        let Some(divider) = self.ivars().divider_window.get() else {
            log::warn!("бросок: окна разделителя нет — плитка возвращается");
            self.show_layout();
            return;
        };
        let (before, after) = if panel.contains(&id) {
            *self.ivars().pending_drop.borrow_mut() = Some((id, panel));
            (Some(divider), None)
        } else {
            let Some(place) = main.iter().position(|&tile| tile == id) else { return };
            let after = place.checked_sub(1).map(|left| main[left]).or(Some(divider));
            (main.get(place + 1).copied(), after)
        };
        self.start_move(id, before, after);
    }

    /// Перенос закончен: строка читается заново, модель видит перестановку.
    pub(super) fn tile_moved(&self) {
        self.give_back_mouse();
        self.set_moving(false);
        self.bump_epoch();
        self.request_read();
    }

    /// Пока перенос делает шаги, мышь остаётся отвязанной. Завис — возвращаем её,
    /// чтобы она не осталась отвязанной навсегда.
    pub(super) fn check_move(&self) {
        if !self.ivars().mouse_taken.get() {
            return;
        }
        if crate::mover::is_stalled(MOVE_WATCHDOG) {
            log::warn!("бросок: перенос завис — возвращаю мышь");
            crate::mover::cancel();
            self.give_back_mouse();
            self.set_moving(false);
            self.ivars().pending_drop.borrow_mut().take();
            self.bump_epoch();
            self.show_layout();
            self.request_read();
            return;
        }
        self.arm_move_watchdog();
    }

    /// Раскладка восстановлена — строка читается заново, разделитель получит нужную длину.
    pub(super) fn layout_restored(&self) {
        self.give_back_mouse();
        self.set_moving(false);
        self.bump_epoch();
        self.request_read();
    }

    /// Возвращает мышь, если она забрана; повторный вызов ничего не делает.
    fn give_back_mouse(&self) {
        if self.ivars().mouse_taken.replace(false) {
            crate::mover::release_mouse();
        }
    }

    /// Пока брошенная иконка едет, строка не читается и плитки не берутся.
    pub(super) fn set_moving(&self, moving: bool) {
        self.ivars().moving.set(moving);
        if let Some(settings) = self.ivars().settings.borrow().as_ref() {
            settings.editor.set_locked(moving);
        }
    }

    /// Разделитель узкий, пока раскладка восстанавливается: иконки по обе стороны видны
    /// и переносятся надёжно.
    pub(super) fn restore_layout(&self) {
        self.cancel_collapse_timer();
        if self.ivars().hidden.get() {
            self.toggle();
        }
        if let Some(divider) = self.ivars().divider.borrow().as_ref() {
            divider.setLength(NARROW_ITEM_WIDTH);
        }
        self.start_restore();
    }

    /// Иконки запомненной панели встают левее разделителя панели, остальные — правее.
    /// Пока идут переносы, строка не читается, курсор спрятан и мышь отвязана.
    pub(super) fn start_restore(&self) {
        let (panel, keys) = {
            let layout = self.ivars().layout.borrow();
            (layout.panel().to_vec(), layout.saved().to_vec())
        };
        log::info!("раскладка: восстанавливаю панель {keys:?}");
        self.set_moving(true);
        self.bump_epoch();
        self.ivars().mouse_taken.set(true);
        crate::mover::take_mouse();
        crate::mover::restore(panel, keys, self.notch_right());
        self.arm_move_watchdog();
    }

    /// Один перенос иконки `id` левее `before` и правее `after`; мышь на это время отвязана.
    pub(super) fn start_move(&self, id: u32, before: Option<u32>, after: Option<u32>) {
        self.set_moving(true);
        self.bump_epoch();
        self.ivars().mouse_taken.set(true);
        crate::mover::take_mouse();
        crate::mover::move_one(id, before, after, self.notch_right());
        self.arm_move_watchdog();
    }

    /// Сторож переноса проверит его через `MOVE_WATCHDOG_POLL`; прошлая проверка отменяется.
    fn arm_move_watchdog(&self) {
        let timer = Timer::once(self.as_ref(), MOVE_WATCHDOG_POLL, sel!(onMoveWatchdog:));
        *self.ivars().move_watchdog.borrow_mut() = Some(timer);
    }
}
