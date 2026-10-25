//! Переносы иконок: броски из редактора, восстановление раскладки, сторож переноса.

use objc2::{sel, DefinedClass};

use super::Controller;
use crate::timer::Timer;
use crate::tuning::{MOVE_WATCHDOG, MOVE_WATCHDOG_POLL, NARROW_ITEM_WIDTH};

/// Перенос Nook, итог которого сверит следующее чтение строки.
pub(super) struct OwnMove {
    kind: MoveKind,
    /// Перенос уже повторён: вторую осечку Nook не повторяет.
    retried: bool,
}

enum MoveKind {
    /// Бросок из редактора: иконка, её соседи по строке и порядок панели в редакторе, если
    /// иконку бросили в панель.
    Drop { id: u32, before: Option<u32>, after: Option<u32>, panel: Option<Vec<u32>> },
    /// Возврат запомненной панели.
    Restore,
}

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
        let kind = if panel.contains(&id) {
            MoveKind::Drop { id, before: Some(divider), after: None, panel: Some(panel) }
        } else {
            let Some(place) = main.iter().position(|&tile| tile == id) else { return };
            let after = place.checked_sub(1).map(|left| main[left]).or(Some(divider));
            MoveKind::Drop { id, before: main.get(place + 1).copied(), after, panel: None }
        };
        self.start_move(OwnMove { kind, retried: false });
    }

    /// Перенос закончен: строка читается заново, модель видит перестановку.
    pub(crate) fn tile_moved(&self) {
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
            self.ivars().own_move.borrow_mut().take();
            self.bump_epoch();
            self.show_layout();
            self.request_read();
            return;
        }
        self.arm_move_watchdog();
    }

    /// Раскладка восстановлена — строка читается заново, разделитель получит нужную длину.
    pub(crate) fn layout_restored(&self) {
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
        if let Some(divider) = self.divider() {
            divider.setLength(NARROW_ITEM_WIDTH);
        }
        self.start_restore();
    }

    /// Иконки запомненной панели встают левее разделителя панели, остальные — правее.
    pub(super) fn start_restore(&self) {
        self.start_move(OwnMove { kind: MoveKind::Restore, retried: false });
    }

    /// Запускает перенос Nook. Пока он идёт, строка не читается, курсор спрятан и мышь отвязана.
    /// Повтор возврата панели ищет иконки по ключам: окна в модели уже следуют строке.
    fn start_move(&self, own: OwnMove) {
        self.set_moving(true);
        self.bump_epoch();
        self.ivars().mouse_taken.set(true);
        crate::mover::take_mouse();
        match &own.kind {
            MoveKind::Drop { id, before, after, .. } => crate::mover::move_one(*id, *before, *after, self.notch_right()),
            MoveKind::Restore => {
                let (panel, keys) = {
                    let layout = self.ivars().layout.borrow();
                    let panel = if own.retried { Vec::new() } else { layout.panel().to_vec() };
                    (panel, layout.saved().to_vec())
                };
                log::info!("раскладка: восстанавливаю панель {keys:?}");
                crate::mover::restore(panel, keys, self.notch_right());
            }
        }
        *self.ivars().own_move.borrow_mut() = Some(own);
        self.arm_move_watchdog();
    }

    /// Сверяет строку после переноса Nook с намерением. Перенос дошёл — бросок становится
    /// намерением. Нет — перенос повторяется один раз, потом остаётся в журнале, намерение
    /// прежнее. `None` — пошёл повтор, а это чтение уже устарело.
    pub(super) fn settle_own_move(&self, own: OwnMove) -> Option<bool> {
        let reached = {
            let layout = self.ivars().layout.borrow();
            match &own.kind {
                MoveKind::Drop { id, panel, .. } => layout.panel().contains(id) == panel.is_some(),
                MoveKind::Restore => layout.bar_matches_saved(),
            }
        };
        if reached {
            if let MoveKind::Drop { id, panel, .. } = &own.kind {
                self.ivars().layout.borrow_mut().accept_drop(*id, panel.as_deref());
            }
            return Some(true);
        }
        if !own.retried {
            log::warn!("раскладка: перенос Nook не дошёл до намерения — повторяю");
            self.start_move(OwnMove { kind: own.kind, retried: true });
            return None;
        }
        log::warn!("раскладка: перенос Nook не удался — порядок на диске прежний {:?}", self.ivars().layout.borrow().saved());
        Some(false)
    }

    /// Сторож переноса проверит его через `MOVE_WATCHDOG_POLL`; прошлая проверка отменяется.
    fn arm_move_watchdog(&self) {
        let timer = Timer::once(self.as_ref(), MOVE_WATCHDOG_POLL, sel!(onMoveWatchdog:));
        *self.ivars().move_watchdog.borrow_mut() = Some(timer);
    }
}
