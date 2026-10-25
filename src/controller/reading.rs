//! Чтение строки: эпоха, приём снимка, длина разделителя, вид панели и редактора.

use std::time::Duration;

use objc2::runtime::AnyObject;
use objc2::{sel, DefinedClass, MainThreadOnly};
use objc2_app_kit::NSImage;
use objc2_foundation::{NSArray, NSNumber};

use super::Controller;
use crate::bar::{Order, Snapshot};
use crate::layout::Change;
use crate::panel::Panel;
use crate::settings::Settings;
use crate::tuning::{NARROW_ITEM_WIDTH, POSITION_TOLERANCE};

impl Controller {
    /// Открыт раздел «Расположение»: раскрываем строку и читаем её.
    /// Взведённый таймер сворачивания гасим, иначе он спрячет иконки посреди снимка.
    /// При автоматическом расположении редактор скрыт, и раскрывать строку незачем.
    pub(super) fn editor_needs_icons(&self) {
        if crate::settings::automatic_layout() {
            return;
        }
        self.cancel_collapse_timer();
        if self.ivars().hidden.get() {
            self.toggle();
        } else {
            self.request_read();
        }
    }

    /// Картинки для редактора готовы — он выстраивает их по модели.
    /// Пока иконку тянут или переносят, редактор не трогаем.
    pub(crate) fn show_editor_icons(&self, images: &NSArray<NSImage>, ids: &NSArray<NSNumber>) {
        if self.editor_busy() {
            return;
        }
        let layout = self.ivars().layout.borrow();
        if let Some(settings) = self.ivars().settings.borrow().as_ref() {
            settings.editor.set_icons(images, ids, layout.panel(), layout.main());
        }
    }

    /// Строка прочитана. Сначала разделитель получает нужную длину, шторка встаёт над
    /// иконками панели, потом модель принимает чтение, потом панель и редактор показывают её вид.
    pub(crate) fn bar_read(&self, snapshot: Option<Snapshot>) {
        let current = |snapshot: &Snapshot| {
            snapshot.epoch == self.ivars().epoch.get() && !self.ivars().moving.get() && !self.ivars().hidden.get()
        };
        let Some(snapshot) = snapshot else {
            self.finish_startup_if_stuck();
            return;
        };
        if !current(&snapshot) {
            return;
        }
        if let Some(divider) = &snapshot.divider {
            self.ivars().divider_window.set(Some(divider.id));
        }
        let automatic = crate::settings::automatic_layout();
        if self.fit_divider(&snapshot, automatic) {
            return;
        }
        self.follow_shroud(&snapshot, automatic);
        let (panel, main) = if automatic {
            snapshot.automatic()
        } else {
            let Some(reading) = snapshot.reading() else {
                log::debug!("строка: разделителя нет — чтение пропущено");
                self.finish_startup_if_stuck();
                return;
            };
            let change = self.ivars().layout.borrow_mut().observe(reading);
            let dropped = self.ivars().pending_drop.borrow_mut().take();
            let mut changed = change != Change::Noise;
            if let Some((id, order)) = dropped {
                if self.ivars().layout.borrow().panel().contains(&id) {
                    changed |= self.ivars().layout.borrow_mut().reorder_panel(&order);
                }
            }
            let layout = self.ivars().layout.borrow();
            if changed {
                log::info!("раскладка: {change:?}, панель {:?}, основной ряд {:?}", layout.panel(), layout.main());
            }
            (layout.panel().to_vec(), layout.main().to_vec())
        };
        self.persist_panel();
        self.show_view(&panel, &main);
    }

    /// Снимки иконок панели готовы. На старте после них строка сворачивается.
    pub(crate) fn show_panel_icons(&self, images: &NSArray<NSImage>, ids: &NSArray<NSNumber>) {
        let target: &AnyObject = self.as_ref();
        self.ivars()
            .panel
            .borrow_mut()
            .get_or_insert_with(|| Panel::new(self.mtm()))
            .set_icons(self.mtm(), target, images, ids);
        self.finish_startup();
    }

    /// Nook сам меняет строку: чтения, начатые раньше, не принимаются.
    pub(super) fn bump_epoch(&self) {
        self.ivars().epoch.set(self.ivars().epoch.get() + 1);
    }

    /// Читает строку, когда она встанет. Пока идёт перенос, не читает: прочитает его конец.
    pub(super) fn request_read(&self) {
        if !self.ivars().moving.get() {
            crate::bar::read_when_still(self.ivars().epoch.get());
        }
    }

    /// Читает строку через `delay`, когда macOS разложит её после действия Nook.
    pub(super) fn read_after(&self, delay: Duration) {
        crate::timer::after(self.as_ref(), delay, sel!(onBarTick:));
    }

    /// Держит длину разделителя в ручном режиме по правилу `Snapshot::divider_length`. Перед
    /// сужением шторка закрывает место у чёлки; пока нажатая иконка подсвечена, сужения нет. Возвращает `true`, если длина поменялась и строка
    /// будет прочитана заново.
    pub(super) fn fit_divider(&self, snapshot: &Snapshot, automatic: bool) -> bool {
        if automatic {
            return false;
        }
        let Some(wanted) = snapshot.divider_length(self.notch_edge()) else { return false };
        let current = self.ivars().divider.borrow().as_ref().map(|divider| divider.length());
        if current.is_none_or(|current| (current - wanted).abs() <= POSITION_TOLERANCE) {
            return false;
        }
        if self.ivars().blocked.get() || crate::bar::nook_order(self.notch_edge()).0 != Order::Ok {
            return false;
        }
        if wanted <= NARROW_ITEM_WIDTH && crate::press::icon_highlighted() {
            log::debug!("разделитель: нажатая иконка подсвечена — сужу позже");
            return false;
        }
        log::info!("разделитель: длина {current:?} → {wanted}");
        if wanted <= NARROW_ITEM_WIDTH {
            if let Some(right) = snapshot.main_left() {
                self.cover_now(right);
            }
        }
        if let Some(divider) = self.ivars().divider.borrow().as_ref() {
            divider.setLength(wanted);
        }
        self.bump_epoch();
        self.request_read();
        true
    }

    /// Разделитель в строке есть только в ручном режиме. Невидимый, он не занимает места,
    /// а macOS помнит, где он стоял.
    pub(super) fn show_divider(&self, visible: bool) {
        if let Some(divider) = self.ivars().divider.borrow().as_ref() {
            divider.setVisible(visible);
            log::info!("разделитель: {}", if visible { "в строке" } else { "убран из строки" });
        }
    }

    /// Панель и редактор показывают вид модели и переснимают картинки.
    pub(super) fn show_view(&self, panel: &[u32], main: &[u32]) {
        let target: &AnyObject = self.as_ref();
        self.ivars()
            .panel
            .borrow_mut()
            .get_or_insert_with(|| Panel::new(self.mtm()))
            .set_order(self.mtm(), target, panel);
        if self.ivars().starting.get() || crate::settings::show_panel() {
            crate::capture::capture_for_panel(panel);
        }
        if self.layout_shown() && !crate::settings::automatic_layout() && !self.editor_busy() {
            self.show_layout();
            let ids: Vec<u32> = panel.iter().chain(main).copied().collect();
            crate::capture::capture_for_editor(&ids);
        }
    }

    /// Порядок панели на диске следует за моделью.
    pub(super) fn persist_panel(&self) {
        let layout = self.ivars().layout.borrow();
        if layout.saved() != crate::settings::panel_order().as_slice() {
            crate::settings::set_panel_order(layout.saved());
        }
    }

    pub(super) fn layout_shown(&self) -> bool {
        self.ivars().settings.borrow().as_ref().is_some_and(Settings::is_layout_shown)
    }

    /// Плитку тянут или иконку переносят: редактор не перестраивается.
    pub(super) fn editor_busy(&self) -> bool {
        let dragging = self.ivars().settings.borrow().as_ref().is_some_and(|settings| settings.editor.is_dragging());
        dragging || self.ivars().moving.get()
    }

    /// Редактор показывает модель: брошенная, но не перенесённая плитка возвращается.
    pub(super) fn show_layout(&self) {
        let layout = self.ivars().layout.borrow();
        if let Some(settings) = self.ivars().settings.borrow().as_ref() {
            settings.editor.show(layout.panel(), layout.main());
        }
    }
}
