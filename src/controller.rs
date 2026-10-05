//! Контроллер-делегат: владеет айтемами и логикой скрытия с GUARD'ом.
//!
//! Это `NSApplicationDelegate`. Айтемы создаются в `applicationDidFinishLaunching:`.
//! Контроллер держит оба `Retained<NSStatusItem>` живыми, пока жив сам.
//!
//! GUARD (бронебойность): перед раздуванием спейсера сверяем реальные X обоих
//! айтемов. Раздуваем ТОЛЬКО если спейсер строго левее якоря. Иначе клик не
//! прячет ничего (якорь не улетит), показывает `⚠` и пишет в лог — пользователь
//! поправляет порядок Cmd+drag. Это решает баг «< улетел за край», т.к. в опасной
//! конфигурации мы просто ничего не делаем.
//!
//! Раскладка идёт в одну сторону: строка читается одним чтением (`bar`), модель
//! (`layout`) решает состав и порядок, панель и редактор показывают её вид.

use std::cell::{Cell, RefCell};
use std::time::Duration;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
use objc2::MainThreadMarker;
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly, Message};
use objc2::sel;
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate, NSButton,
    NSControlStateValueOn, NSEventType, NSImage, NSScreen, NSStatusItem, NSSwitch, NSWindow,
    NSWindowDidBecomeKeyNotification, NSWindowDidMoveNotification, NSWindowWillCloseNotification,
};
use objc2_foundation::{
    NSArray, NSNotification, NSNotificationCenter, NSNumber, NSPoint, NSTimer,
};

use crate::auto_collapse::{self, MouseMonitor};
use crate::bar::Snapshot;
use crate::layout::{Change, Layout};
use crate::onboarding::Onboarding;
use crate::panel::Panel;
use crate::settings::Settings;
use crate::status_bar::{
    self, StatusItems, ANCHOR_SYMBOL_BLOCKED, ANCHOR_SYMBOL_HIDDEN, ANCHOR_SYMBOL_SHOWN,
};
use crate::strings::{self, Lang};
use crate::tuning::{
    ANCHOR_MAX_RETRIES, DIVIDER_SETTLE_DELAY, HIDDEN_WIDTH_MARGIN, HIDDEN_WIDTH_MAX, HIDING_WIDTH_MIN,
    MOVE_WATCHDOG, MOVE_WATCHDOG_POLL, NARROW_ITEM_WIDTH, PANEL_REFRESH_INTERVAL, PLACEMENT_CHECK_INTERVAL,
    PLACEMENT_ESCALATE_AFTER, PLACEMENT_MAX_ATTEMPTS, POSITION_TOLERANCE, SCREEN_WIDTH_FALLBACK,
};

/// x айтема, которого macOS ещё не разместила в строке.
const UNPLACED_X: f64 = 0.0;

/// Внутреннее состояние.
/// - `items` появляются после запуска. `hidden`: текущий режим.
/// - `monitor` следит за мышью, пока иконки показаны (для автосворачивания).
/// - `collapse_timer` — взведённый таймер коллапса (мышь ушла из полосы).
/// - `lang` — язык интерфейса, определён один раз на старте.
pub struct ControllerIvars {
    items: RefCell<Option<StatusItems>>,
    hidden: Cell<bool>,
    monitor: RefCell<Option<MouseMonitor>>,
    collapse_timer: RefCell<Option<Retained<NSTimer>>>,
    lang: Lang,
    /// Сколько раз ещё можно пересоздать застрявший якорь.
    anchor_retries: Cell<u32>,
    /// Сколько раз ещё можно пересоздать застрявший спейсер.
    spacer_retries: Cell<u32>,
    /// Сколько проверок размещения прошло без успеха.
    placement_attempts: Cell<u32>,
    /// Стартовое авто-скрытие уже запущено: его зовут и событие NSWindowDidMove, и таймер.
    placement_done: Cell<bool>,
    /// Идёт старт: строка раскрыта, пока не сняты иконки панели.
    starting: Cell<bool>,
    /// На старте macOS не помнила место разделителя: раскладка восстанавливается переносами.
    restore_on_start: Cell<bool>,
    panel: RefCell<Option<Panel>>,
    /// Таймер чтения строки: живёт, пока строка раскрыта.
    bar_timer: RefCell<Option<Retained<NSTimer>>>,
    settings: RefCell<Option<Settings>>,
    /// Окно разрешений: создаётся при запуске, если какого-то разрешения нет.
    onboarding: RefCell<Option<Onboarding>>,
    /// Разделитель панели: создаётся на старте и живёт всё время работы.
    divider: RefCell<Option<Retained<NSStatusItem>>>,
    /// Модель раскладки: из неё строятся панель и редактор.
    layout: RefCell<Layout>,
    /// Номер окна разделителя у WindowServer.
    divider_window: Cell<Option<u32>>,
    /// Поколение строки: растёт, когда Nook сам её меняет. Чтение старого поколения не принимается.
    epoch: Cell<u64>,
    /// Идёт перенос иконки.
    moving: Cell<bool>,
    /// Иконка, брошенная в панель, и порядок панели в редакторе: применяется после переноса.
    pending_drop: RefCell<Option<(u32, Vec<u32>)>>,
    /// Мышь отвязана от курсора на время переноса.
    mouse_taken: Cell<bool>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = ControllerIvars]
    pub struct Controller;

    unsafe impl NSObjectProtocol for Controller {}

    unsafe impl NSApplicationDelegate for Controller {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _notification: &NSNotification) {

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

            // FALLBACK-таймер: если айтем ЗАЛИП на x=0 без события — пересоздаёт его
            // (retry + эскалация). Размещение асинхронно, поэтому через таймер, а не
            // синхронно: синхронная блокировка ломает размещение.
            let target: &AnyObject = self.as_ref();
            unsafe {
                NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                    PLACEMENT_CHECK_INTERVAL,
                    target,
                    sel!(onPlacementCheck:),
                    None,
                    true,
                );
            }
            let main_menu = unsafe { crate::menu::main_menu(mtm, target, self.ivars().lang) };
            NSApplication::sharedApplication(mtm).setMainMenu(Some(&main_menu));
            self.show_onboarding_if_needed();
        }
    }

    impl Controller {
        /// Клик по якорю. Левый → toggle (прятать/показывать), пока не идёт перенос. Правый → меню.
        /// Тип события берём из currentEvent — так разделяем левый/правый без
        /// присвоения statusItem.menu (иначе левый клик тоже открывал бы меню).
        #[unsafe(method(onAnchorClick:))]
        fn on_anchor_click(&self, sender: *mut AnyObject) {
            let event_type = NSApplication::sharedApplication(self.mtm())
                .currentEvent()
                .map(|e| e.r#type());

            if event_type == Some(NSEventType::RightMouseUp) {
                self.show_menu(sender);
            } else if !self.ivars().moving.get() {
                self.toggle();
            }
        }

        /// Пункт меню «Запускать при входе» — переключает автозапуск.
        /// Галочка обновится при следующем открытии меню (build читает статус).
        #[unsafe(method(onToggleLogin:))]
        fn on_toggle_login(&self, _sender: *mut AnyObject) {
            crate::login::toggle();
            if let Some(settings) = self.ivars().settings.borrow().as_ref() {
                settings.refresh();
            }
        }

        /// Пункт меню «Настройки…» — открывает окно настроек.
        #[unsafe(method(onOpenSettings:))]
        fn on_open_settings(&self, _sender: *mut AnyObject) {
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
        #[unsafe(method(onWindowClose:))]
        fn on_window_close(&self, notification: &NSNotification) {
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
        #[unsafe(method(onWindowFocus:))]
        fn on_window_focus(&self, _notification: &NSNotification) {
            if let Some(settings) = self.ivars().settings.borrow().as_ref() {
                settings.refresh();
            }
            if let Some(onboarding) = self.ivars().onboarding.borrow().as_ref() {
                onboarding.refresh();
            }
        }

        /// Переключатель «Показывать панель».
        #[unsafe(method(onToggleShowPanel:))]
        fn on_toggle_show_panel(&self, sender: &NSSwitch) {
            let on = sender.state() == NSControlStateValueOn;
            crate::settings::set_show_panel(on);
            if let Some(settings) = self.ivars().settings.borrow().as_ref() {
                settings.refresh();
            }
            if let Some(items) = self.ivars().items.borrow().as_ref() {
                self.sync_panel(items, self.ivars().hidden.get());
            }
        }

        /// Переключатель «Автоматическое расположение». В авто разделителя в строке нет,
        /// в ручном режиме он возвращается на своё место.
        #[unsafe(method(onToggleAutomaticLayout:))]
        fn on_toggle_automatic_layout(&self, sender: &NSSwitch) {
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
                self.read_after(DIVIDER_SETTLE_DELAY);
            } else {
                self.restore_layout();
            }
        }

        /// Открыт раздел «Расположение»: раскрываем строку и читаем её.
        /// Взведённый таймер сворачивания гасим, иначе он спрячет иконки посреди снимка.
        /// При автоматическом расположении редактор скрыт, и раскрывать строку незачем.
        #[unsafe(method(onEditorNeedsIcons))]
        fn on_editor_needs_icons(&self) {
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
        #[unsafe(method(setEditorIcons:ids:))]
        fn set_editor_icons(&self, images: &NSArray<NSImage>, ids: &NSArray<NSNumber>) {
            if self.editor_busy() {
                return;
            }
            let layout = self.ivars().layout.borrow();
            if let Some(settings) = self.ivars().settings.borrow().as_ref() {
                settings.editor.set_icons(images, ids, layout.panel(), layout.main());
            }
        }

        /// Плитка `id` брошена в редакторе. Внутри панели строку не трогаем: меняется только
        /// порядок панели. Иначе настоящая иконка переносится одним переносом: в панель —
        /// к разделителю, в основной ряд — к соседям по редактору.
        #[unsafe(method(onTileDropped:))]
        fn on_tile_dropped(&self, id: u32) {
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
        #[unsafe(method(onTileMoved))]
        fn on_tile_moved(&self) {
            self.give_back_mouse();
            self.set_moving(false);
            self.bump_epoch();
            self.request_read();
        }

        /// Пока перенос делает шаги, мышь остаётся отвязанной. Завис — возвращаем её,
        /// чтобы она не осталась отвязанной навсегда.
        #[unsafe(method(onMoveWatchdog:))]
        fn on_move_watchdog(&self, _timer: *mut AnyObject) {
            if !self.ivars().mouse_taken.get() {
                return;
            }
            if crate::mover::is_stalled(Duration::from_secs_f64(MOVE_WATCHDOG)) {
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

        /// Стартовый разделитель встал — читаем строку: чтение расширит разделитель и снимет
        /// иконки панели. Если macOS забыла место разделителя, сначала восстанавливаем раскладку.
        #[unsafe(method(onDividerPlaced:))]
        fn on_divider_placed(&self, _timer: *mut AnyObject) {
            self.ivars().starting.set(true);
            if self.ivars().restore_on_start.get() {
                self.start_restore();
            } else {
                self.request_read();
            }
        }

        /// Раскладка восстановлена — строка читается заново, разделитель снова широкий.
        #[unsafe(method(onLayoutRestored))]
        fn on_layout_restored(&self) {
            self.give_back_mouse();
            self.set_moving(false);
            self.bump_epoch();
            self.request_read();
        }

        /// Чтение строки по таймеру.
        #[unsafe(method(onBarTick:))]
        fn on_bar_tick(&self, _timer: *mut AnyObject) {
            if !self.ivars().hidden.get() {
                self.request_read();
            }
        }

        /// Строка прочитана. Сначала разделитель получает нужную ширину, потом модель
        /// принимает чтение, потом панель и редактор показывают её вид.
        #[unsafe(method(onBarRead))]
        fn on_bar_read(&self) {
            let current = |snapshot: &Snapshot| {
                snapshot.epoch == self.ivars().epoch.get() && !self.ivars().moving.get() && !self.ivars().hidden.get()
            };
            let Some(snapshot) = crate::bar::take() else {
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

        #[unsafe(method(onOpenAccessibility:))]
        fn on_open_accessibility(&self, _sender: *mut AnyObject) {
            crate::permissions::open_accessibility_pane();
        }

        #[unsafe(method(onOpenScreenRecording:))]
        fn on_open_screen_recording(&self, _sender: *mut AnyObject) {
            crate::permissions::open_screen_recording_pane();
        }

        /// «Сбросить разрешения»: записи Nook убираются из Системных настроек, Nook
        /// перезапускается и спрашивает разрешения заново.
        #[unsafe(method(onResetPermissions:))]
        fn on_reset_permissions(&self, _sender: *mut AnyObject) {
            crate::permissions::reset_all();
            crate::permissions::relaunch_later();
            NSApplication::sharedApplication(self.mtm()).terminate(None);
        }

        /// Пункт меню «Выход» — завершаем приложение.
        #[unsafe(method(onQuit:))]
        fn on_quit(&self, _sender: *mut AnyObject) {
            NSApplication::sharedApplication(self.mtm()).terminate(None);
        }

        /// ОСНОВНОЙ триггер: айтем получил/сменил позицию. Если оба размещены —
        /// завершаем стартовую настройку (авто-скрытие). Это точное событие,
        /// в отличие от поллинга по таймеру.
        #[unsafe(method(onWindowMoved:))]
        fn on_window_moved(&self, _notification: &NSNotification) {
            self.finish_placement_if_ready();
        }

        /// Проверка размещения айтемов (периодическая, после старта).
        /// Если айтем застрял на x=0 — пересоздаёт его (до лимита попыток).
        /// Когда ОБА размещены — запускает стартовое авто-скрытие и останавливается.
        /// Так первый toggle гарантированно идёт по валидным координатам.
        #[unsafe(method(onPlacementCheck:))]
        fn on_placement_check(&self, timer: *mut AnyObject) {
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
                self.stop_placement_timer(timer);
                self.finish_placement_if_ready();
                return;
            }

            // Не размещены. Считаем попытки; защита от вечного таймера.
            let attempts = self.ivars().placement_attempts.get() + 1;
            self.ivars().placement_attempts.set(attempts);
            if attempts >= PLACEMENT_MAX_ATTEMPTS {
                self.stop_placement_timer(timer);
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

        /// Движение мыши (от монитора). Работает только когда иконки показаны.
        /// Мышь в полосе menu bar или над панелью → гасим таймер коллапса.
        /// Мышь ушла → взводим.
        #[unsafe(method(onMouseMoved))]
        fn on_mouse_moved(&self) {
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

        /// Снимки иконок панели готовы. На старте после них строка сворачивается.
        #[unsafe(method(setPanelIcons:ids:))]
        fn set_panel_icons(&self, images: &NSArray<NSImage>, ids: &NSArray<NSNumber>) {
            let target: &AnyObject = self.as_ref();
            self.ivars()
                .panel
                .borrow_mut()
                .get_or_insert_with(|| Panel::new(self.mtm()))
                .set_icons(self.mtm(), target, images, ids);
            self.finish_startup();
        }

        /// Тема macOS сменилась: панель сразу берёт картинки из кэша новой темы,
        /// а раскрытая строка ещё и переснимается.
        #[unsafe(method(onThemeChanged:))]
        fn on_theme_changed(&self, _notification: &NSNotification) {
            let theme = crate::theme::current();
            let target: &AnyObject = self.as_ref();
            if let Some(panel) = self.ivars().panel.borrow_mut().as_mut() {
                panel.set_theme(self.mtm(), target, theme);
            }
            if !self.ivars().hidden.get() {
                self.read_after(DIVIDER_SETTLE_DELAY);
            }
        }

        /// Клик по копии в панели — нажимаем настоящую иконку там, где она спрятана.
        #[unsafe(method(onCloneClick:))]
        fn on_clone_click(&self, sender: &NSButton) {
            crate::press::press(sender.tag() as u32);
        }

        /// Таймер коллапса дожил: мышь была вне полоса COLLAPSE_DELAY секунд.
        /// Сворачиваем, если всё ещё показано.
        #[unsafe(method(onAutoCollapse:))]
        fn on_auto_collapse(&self, _timer: *mut AnyObject) {
            self.ivars().collapse_timer.borrow_mut().take();
            if self.ivars().hidden.get() || self.ivars().moving.get() || crate::press::menu_open() {
                return;
            }
            self.toggle();
        }
    }
);

impl Controller {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc().set_ivars(ControllerIvars {
            items: RefCell::new(None),
            hidden: Cell::new(false),
            monitor: RefCell::new(None),
            collapse_timer: RefCell::new(None),
            lang: strings::detect(),
            anchor_retries: Cell::new(ANCHOR_MAX_RETRIES),
            spacer_retries: Cell::new(ANCHOR_MAX_RETRIES),
            placement_attempts: Cell::new(0),
            placement_done: Cell::new(false),
            starting: Cell::new(false),
            restore_on_start: Cell::new(false),
            panel: RefCell::new(None),
            bar_timer: RefCell::new(None),
            settings: RefCell::new(None),
            onboarding: RefCell::new(None),
            divider: RefCell::new(None),
            layout: RefCell::new(Layout::new(crate::settings::panel_order())),
            divider_window: Cell::new(None),
            epoch: Cell::new(0),
            moving: Cell::new(false),
            pending_drop: RefCell::new(None),
            mouse_taken: Cell::new(false),
        });
        unsafe { msg_send![super(this), init] }
    }

    /// Показывает окно разрешений, если какого-то разрешения нет.
    fn show_onboarding_if_needed(&self) {
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

    /// Окно настроек или разрешений сообщает контроллеру, что стало активным и что закрывается.
    fn observe_window(&self, window: &NSWindow) {
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
    fn set_in_dock(&self, on: bool) {
        let policy = if on {
            NSApplicationActivationPolicy::Regular
        } else {
            NSApplicationActivationPolicy::Accessory
        };
        NSApplication::sharedApplication(self.mtm()).setActivationPolicy(policy);
    }

    /// Переключает видимость зоны слева от якоря — с проверкой порядка.
    fn toggle(&self) {
        let items_ref = self.ivars().items.borrow();
        let Some(items) = items_ref.as_ref() else {
            log::debug!("toggle: айтемы ещё не созданы — игнор");
            return;
        };

        let going_to_hide = !self.ivars().hidden.get();

        // GUARD: прячем только если спейсер реально левее якоря.
        if going_to_hide && !self.spacer_is_left_of_anchor(items) {
            self.show_blocked(items);
            return;
        }

        self.ivars().hidden.set(going_to_hide);
        let (spacer_width, anchor_symbol) = if going_to_hide {
            (self.hidden_width(), ANCHOR_SYMBOL_HIDDEN)
        } else {
            (NARROW_ITEM_WIDTH, ANCHOR_SYMBOL_SHOWN)
        };

        items.spacer.setLength(spacer_width);
        status_bar::set_anchor_symbol(items, self.mtm(), anchor_symbol);
        self.bump_epoch();

        // Смена состояния гасит взведённый таймер коллапса.
        self.cancel_collapse_timer();
        // Показали → следим за мышью (для автосворачивания). Скрыли → перестаём.
        if going_to_hide {
            self.stop_mouse_watch();
        } else {
            self.start_mouse_watch();
        }
        self.sync_panel(items, going_to_hide);
    }

    /// Панель открыта ровно тогда, когда иконки раскрыты; пока они раскрыты, строка
    /// читается по таймеру.
    fn sync_panel(&self, items: &StatusItems, hidden: bool) {
        if hidden {
            if let Some(timer) = self.ivars().bar_timer.borrow_mut().take() {
                timer.invalidate();
            }
        } else {
            self.request_read();
            let target: &AnyObject = self.as_ref();
            let mut timer = self.ivars().bar_timer.borrow_mut();
            if timer.is_none() {
                *timer = Some(unsafe {
                    NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                        PANEL_REFRESH_INTERVAL,
                        target,
                        sel!(onBarTick:),
                        None,
                        true,
                    )
                });
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

    /// Nook сам меняет строку: чтения, начатые раньше, не принимаются.
    fn bump_epoch(&self) {
        self.ivars().epoch.set(self.ivars().epoch.get() + 1);
    }

    /// Читает строку, когда она встанет. Пока идёт перенос, не читает: прочитает его конец.
    fn request_read(&self) {
        if !self.ivars().moving.get() {
            crate::bar::read_when_still(self.ivars().epoch.get());
        }
    }

    /// Читает строку через `delay` секунд, когда macOS разложит её после действия Nook.
    fn read_after(&self, delay: f64) {
        let target: &AnyObject = self.as_ref();
        unsafe {
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                delay,
                target,
                sel!(onBarTick:),
                None,
                false,
            );
        }
    }

    /// Держит ширину разделителя в ручном режиме: иконки панели спрятаны, крайняя левая
    /// на экране. Возвращает `true`, если ширина поменялась и строка будет прочитана заново.
    fn fit_divider(&self, snapshot: &Snapshot, automatic: bool) -> bool {
        if automatic {
            return false;
        }
        let Some(wanted) = snapshot.hiding_width() else { return false };
        let current = self.ivars().divider.borrow().as_ref().map(|divider| divider.length());
        if current.is_none_or(|current| (current - wanted).abs() <= POSITION_TOLERANCE) {
            return false;
        }
        log::debug!("разделитель: длина {current:?} → {wanted}");
        if let Some(divider) = self.ivars().divider.borrow().as_ref() {
            divider.setLength(wanted);
        }
        self.bump_epoch();
        self.request_read();
        true
    }

    /// Разделитель в строке есть только в ручном режиме. Невидимый, он не занимает места,
    /// а macOS помнит, где он стоял.
    fn show_divider(&self, visible: bool) {
        if let Some(divider) = self.ivars().divider.borrow().as_ref() {
            divider.setVisible(visible);
            log::info!("разделитель: {}", if visible { "в строке" } else { "убран из строки" });
        }
    }

    /// Панель и редактор показывают вид модели и переснимают картинки.
    fn show_view(&self, panel: &[u32], main: &[u32]) {
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
    fn persist_panel(&self) {
        let layout = self.ivars().layout.borrow();
        if layout.saved() != crate::settings::panel_order().as_slice() {
            crate::settings::set_panel_order(layout.saved());
        }
    }

    /// Старт закончен: иконки панели сняты — строка сворачивается.
    fn finish_startup(&self) {
        if self.ivars().starting.replace(false) && !self.ivars().hidden.get() {
            self.toggle();
        }
    }

    /// На старте разделитель так и не прочитался — сворачиваемся без снимков.
    fn finish_startup_if_stuck(&self) {
        if self.ivars().starting.get() {
            log::warn!("старт: разделитель не прочитан — сворачиваю без снимков панели");
            self.finish_startup();
        }
    }

    fn mouse_in_panel(&self) -> bool {
        self.ivars().panel.borrow().as_ref().is_some_and(Panel::contains_mouse)
    }

    fn layout_shown(&self) -> bool {
        self.ivars().settings.borrow().as_ref().is_some_and(Settings::is_layout_shown)
    }

    /// Плитку тянут или иконку переносят: редактор не перестраивается.
    fn editor_busy(&self) -> bool {
        let dragging = self.ivars().settings.borrow().as_ref().is_some_and(|settings| settings.editor.is_dragging());
        dragging || self.ivars().moving.get()
    }

    /// Останавливает таймер проверки размещения (по сырому указателю из колбэка).
    fn stop_placement_timer(&self, timer: *mut AnyObject) {
        if let Some(t) = unsafe { (timer as *const NSTimer).as_ref() } {
            t.invalidate();
        }
    }

    /// Завершает стартовую настройку, КОГДА оба айтема размещены (x>0).
    /// Вызывается и из события NSWindowDidMove (основной триггер), и из
    /// fallback-таймера. Идемпотентно: срабатывает один раз (флаг placement_done),
    /// отписывается от нотификации и создаёт разделитель панели.
    fn finish_placement_if_ready(&self) {
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
        let target: &AnyObject = self.as_ref();
        unsafe {
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                DIVIDER_SETTLE_DELAY,
                target,
                sel!(onDividerPlaced:),
                None,
                false,
            );
        }
    }

    /// Возвращает мышь, если она забрана; повторный вызов ничего не делает.
    fn give_back_mouse(&self) {
        if self.ivars().mouse_taken.replace(false) {
            crate::mover::release_mouse();
        }
    }

    /// Пока брошенная иконка едет, строка не читается и плитки не берутся.
    fn set_moving(&self, moving: bool) {
        self.ivars().moving.set(moving);
        if let Some(settings) = self.ivars().settings.borrow().as_ref() {
            settings.editor.set_locked(moving);
        }
    }

    /// Редактор показывает модель: брошенная, но не перенесённая плитка возвращается.
    fn show_layout(&self) {
        let layout = self.ivars().layout.borrow();
        if let Some(settings) = self.ivars().settings.borrow().as_ref() {
            settings.editor.show(layout.panel(), layout.main());
        }
    }

    /// Разделитель узкий, пока раскладка восстанавливается: иконки по обе стороны видны
    /// и переносятся надёжно.
    fn restore_layout(&self) {
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
    fn start_restore(&self) {
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

    fn notch_right(&self) -> f64 {
        NSScreen::mainScreen(self.mtm()).map_or(0.0, |screen| screen.auxiliaryTopRightArea().origin.x)
    }

    /// Один перенос иконки `id` левее `before` и правее `after`; мышь на это время отвязана.
    fn start_move(&self, id: u32, before: Option<u32>, after: Option<u32>) {
        self.set_moving(true);
        self.bump_epoch();
        self.ivars().mouse_taken.set(true);
        crate::mover::take_mouse();
        crate::mover::move_one(id, before, after, self.notch_right());
        self.arm_move_watchdog();
    }

    fn arm_move_watchdog(&self) {
        let target: &AnyObject = self.as_ref();
        unsafe {
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                MOVE_WATCHDOG_POLL,
                target,
                sel!(onMoveWatchdog:),
                None,
                false,
            );
        }
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
    fn cancel_collapse_timer(&self) {
        if let Some(timer) = self.ivars().collapse_timer.borrow_mut().take() {
            timer.invalidate();
        }
    }

    /// Взводит таймер коллапса (мышь ушла из полосы). Через COLLAPSE_DELAY
    /// сработает onAutoCollapse: и свернёт, если мышь не вернулась.
    fn arm_collapse_timer(&self) {
        let target: &AnyObject = self.as_ref();
        let timer = auto_collapse::make_collapse_timer(target);
        *self.ivars().collapse_timer.borrow_mut() = Some(timer);
    }

    /// Проверка порядка: оба айтема размещены (x>0) И спейсер строго левее якоря.
    /// Ужесточено: x==0 значит «не размещён», а НЕ «левее» — иначе раздувание от
    /// нуля растягивало бы спейсер через весь экран и уносило якорь.
    /// Если координаты невалидны — безопасный отказ (лучше не спрятать, чем уронить).
    fn spacer_is_left_of_anchor(&self, items: &StatusItems) -> bool {
        let mtm = self.mtm();
        let spacer_x = status_bar::item_origin_x(&items.spacer, mtm);
        let anchor_x = status_bar::item_origin_x(&items.anchor, mtm);

        match (spacer_x, anchor_x) {
            (Some(sx), Some(ax)) => {
                // Оба размещены, и спейсер строго левее якоря.
                let placed = sx > UNPLACED_X && ax > UNPLACED_X;
                let ok = placed && sx < ax;
                if !ok {
                    let why = if placed { "правее якоря" } else { "не размещён" };
                    log::debug!("guard: spacer.x={sx} anchor.x={ax} — спейсер {why}");
                }
                ok
            }
            _ => {
                log::debug!("guard: координаты ещё недоступны → отказ (безопасно)");
                false
            }
        }
    }

    /// Показывает контекстное меню у кнопки якоря (правый клик).
    /// Меню строится здесь и показывается вручную — НЕ через statusItem.menu,
    /// чтобы не перехватывать левый клик.
    fn show_menu(&self, _sender: *mut AnyObject) {
        let items_ref = self.ivars().items.borrow();
        let Some(items) = items_ref.as_ref() else {
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

    /// Показывает знак блокировки на якоре и оставляет всё как есть.
    fn show_blocked(&self, items: &StatusItems) {
        status_bar::set_anchor_symbol(items, self.mtm(), ANCHOR_SYMBOL_BLOCKED);
        log::warn!("СТОП: спейсер не левее якоря — скрытие заблокировано, показываю ⚠");
    }

    /// Ширина спейсера в скрытом состоянии: ширина экрана + запас, ограниченная.
    fn hidden_width(&self) -> f64 {
        let screen_width = NSScreen::mainScreen(self.mtm())
            .map(|screen| screen.frame().size.width)
            .unwrap_or(SCREEN_WIDTH_FALLBACK);
        (screen_width + HIDDEN_WIDTH_MARGIN).clamp(HIDING_WIDTH_MIN, HIDDEN_WIDTH_MAX)
    }
}

/// Айтем стоит в строке: его x известен и это не место неразмещённого.
fn is_placed(x: Option<f64>) -> bool {
    x.is_some_and(|x| x != UNPLACED_X)
}
