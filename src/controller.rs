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

use std::cell::{Cell, RefCell};
use std::thread;
use std::time::{Duration, Instant};

use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
use objc2::MainThreadMarker;
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly, Message};
use objc2::sel;
use objc2_app_kit::{
    NSApplication, NSApplicationDelegate, NSButton, NSControlStateValueOn, NSEventType, NSImage,
    NSScreen, NSStatusItem, NSSwitch, NSWindowDidMoveNotification, NSWorkspace,
    NSWorkspaceActiveSpaceDidChangeNotification,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{
    NSArray, NSNotification, NSNotificationCenter, NSNumber, NSPoint, NSTimer,
};

use crate::auto_collapse::{self, MouseMonitor};
use crate::panel::Panel;
use crate::settings::Settings;
use crate::status_bar::{
    self, StatusItems, ANCHOR_SYMBOL_BLOCKED, ANCHOR_SYMBOL_HIDDEN, ANCHOR_SYMBOL_SHOWN,
    SPACER_WIDTH_SHOWN,
};
use crate::strings::{self, Lang};

/// Запас сверх ширины экрана, чтобы гарантированно вытолкнуть крайние иконки.
// HARDCODE: параметры ширины скрытия; вынести в конфиг позже.
const HIDDEN_WIDTH_MARGIN: f64 = 200.0;
const HIDDEN_WIDTH_MIN: f64 = 500.0;
const HIDDEN_WIDTH_MAX: f64 = 4000.0;
const SCREEN_WIDTH_FALLBACK: f64 = 1728.0;

/// Интервал проверки размещения айтемов после старта. Размещение асинхронно;
/// проверяем каждые 0.3с, пересоздавая застрявшие на x=0, пока оба не встанут.
const PLACEMENT_CHECK_INTERVAL: f64 = 0.3;

/// Максимум пересозданий одного айтема, если он застрял на x=0 (retry).
const ANCHOR_MAX_RETRIES: u32 = 10;

/// После стольких безуспешных проверок размещения — эскалация: пересоздать ОБА
/// айтема (одиночное пересоздание иногда садится на тот же x=0, journal 006 итер.7).
const PLACEMENT_ESCALATE_AFTER: u32 = 4;

/// Полный потолок проверок размещения — затем сдаёмся и останавливаем таймер
/// (защита от вечного таймера, если размещение не удаётся вообще).
const PLACEMENT_MAX_ATTEMPTS: u32 = 30;

/// Пауза после раскрытия, чтобы окна иконок встали на места перед съёмкой.
// HARDCODE: задержка съёмки иконок; вынести в конфиг позже.
const PANEL_CAPTURE_DELAY: f64 = 0.3;
/// Шаг опроса, вышел ли разделитель на экран после раскрытия.
const PANEL_SETTLE_POLL: Duration = Duration::from_millis(5);
/// Пауза, чтобы macOS разложила окна строки меню после смены ширин.
// HARDCODE: пауза перед перестановкой иконок; вынести в конфиг позже.
const APPLY_SETTLE_DELAY: f64 = 0.4;
const APPLY_FIRST_CHECK: f64 = 0.05;
const APPLY_WAIT_INTERVAL: f64 = 0.05;
const APPLY_MAX_WAITS: u32 = 30;
/// Окно узкого разделителя не шире этого — значит, он уже сужен и нарисован.
const DIVIDER_NARROW_WINDOW: f64 = 40.0;
/// Дольше перенос идти не может — мышь возвращается в любом случае.
const APPLY_WATCHDOG: f64 = 5.0;

/// Как часто обновлять клоны, пока панель открыта.
// HARDCODE: период обновления клонов; вынести в конфиг позже.
const PANEL_REFRESH_INTERVAL: f64 = 2.0;

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
    /// Сколько раз ещё можно пересоздать застрявший якорь (retry, итер.4).
    anchor_retries: Cell<u32>,
    /// Сколько раз ещё можно пересоздать застрявший спейсер (retry, итер.4b).
    spacer_retries: Cell<u32>,
    /// Сколько проверок размещения прошло без успеха (для эскалации, итер.7).
    placement_attempts: Cell<u32>,
    /// Стартовое авто-скрытие уже запущено? (защита от двойного вызова из
    /// события NSWindowDidMove и fallback-таймера, итер.8).
    placement_done: Cell<bool>,
    /// Сколько стартовых шагов (снимок иконок, поиск их кнопок) ещё идёт — авто-скрытие ждёт их.
    startup_steps_pending: Cell<u8>,
    panel: RefCell<Option<Panel>>,
    /// Таймер обновления клонов: живёт только пока панель открыта.
    panel_refresh_timer: RefCell<Option<Retained<NSTimer>>>,
    settings: RefCell<Option<Settings>>,
    /// Разделитель панели: есть, пока в панели есть иконки.
    divider: RefCell<Option<Retained<NSStatusItem>>>,
    /// Идёт перестановка иконок — автосворачивание ждёт.
    applying: Cell<bool>,
    /// Сколько раз уже ждали, пока разделитель разложится.
    apply_waits: Cell<u32>,
    /// Номер окна разделителя у WindowServer.
    divider_window: Cell<Option<u32>>,
    /// Мышь отвязана от курсора на время переноса.
    mouse_taken: Cell<bool>,
    /// Окна иконок панели из последнего снимка в обычном режиме.
    panel_ids: RefCell<Vec<u32>>,
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

            // ОСНОВНОЙ триггер (journal 007): подписка на NSWindowDidMoveNotification.
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
                    sel!(onSpaceChanged:),
                    Some(NSWorkspaceActiveSpaceDidChangeNotification),
                    None,
                );
            }

            // FALLBACK-таймер: если айтем ЗАЛИП на x=0 без события — пересоздаёт его
            // (retry + эскалация). Размещение асинхронно, поэтому через таймер, а не
            // синхронно (синхронная блокировка ломает размещение, journal 006).
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
        }
    }

    impl Controller {
        /// Клик по якорю. Левый → toggle (прятать/показывать). Правый → меню.
        /// Тип события берём из currentEvent — так разделяем левый/правый без
        /// присвоения statusItem.menu (иначе левый клик тоже открывал бы меню).
        #[unsafe(method(onAnchorClick:))]
        fn on_anchor_click(&self, sender: *mut AnyObject) {
            let event_type = NSApplication::sharedApplication(self.mtm())
                .currentEvent()
                .map(|e| e.r#type());

            if event_type == Some(NSEventType::RightMouseUp) {
                self.show_menu(sender);
            } else {
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
            let target: &AnyObject = self.as_ref();
            self.ivars()
                .settings
                .borrow_mut()
                .get_or_insert_with(|| Settings::new(self.mtm(), target, self.ivars().lang))
                .show(self.mtm());
        }

        /// Окно настроек стало активным — перечитываем статус разрешений и переключателей.
        #[unsafe(method(onSettingsFocus:))]
        fn on_settings_focus(&self, _notification: &NSNotification) {
            if let Some(settings) = self.ivars().settings.borrow().as_ref() {
                settings.refresh();
            }
        }

        /// Переключатель «Показывать панель».
        #[unsafe(method(onToggleShowPanel:))]
        fn on_toggle_show_panel(&self, sender: &NSSwitch) {
            let on = sender.state() == NSControlStateValueOn;
            crate::settings::set_show_panel(on);
            if let Some(items) = self.ivars().items.borrow().as_ref() {
                self.sync_panel(items, self.ivars().hidden.get());
            }
        }

        /// Открыт раздел «Расположение»: раскрываем строку и снимаем иконки левее ≡◂.
        /// Взведённый таймер сворачивания гасим, иначе он спрячет иконки посреди снимка.
        #[unsafe(method(onEditorNeedsIcons))]
        fn on_editor_needs_icons(&self) {
            self.cancel_collapse_timer();
            if self.ivars().hidden.get() {
                self.toggle();
            }
            let target: &AnyObject = self.as_ref();
            unsafe {
                NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                    PANEL_CAPTURE_DELAY,
                    target,
                    sel!(onEditorCapture:),
                    None,
                    false,
                );
            }
        }

        #[unsafe(method(onEditorCapture:))]
        fn on_editor_capture(&self, _timer: *mut AnyObject) {
            let spacer_x = {
                let items_ref = self.ivars().items.borrow();
                let Some(items) = items_ref.as_ref() else { return };
                status_bar::item_origin_x(&items.spacer, self.mtm())
            };
            if let Some(spacer_x) = spacer_x {
                crate::capture::capture_left_of(spacer_x, self.ivars().divider_window.get());
            }
        }

        /// Снимки иконок для редактора готовы: левее разделителя — панель, правее — основной ряд.
        #[unsafe(method(setEditorIcons:ids:))]
        fn set_editor_icons(&self, images: &NSArray<NSImage>, ids: &NSArray<NSNumber>) {
            let ids_vec: Vec<u32> = ids.iter().map(|id| id.unsignedIntValue()).collect();
            let panel_ids: Vec<u32> = match self.divider_window_x() {
                Some(divider_x) => crate::capture::window_centers(&ids_vec)
                    .into_iter()
                    .filter(|(_, center)| *center < divider_x)
                    .map(|(id, _)| id)
                    .collect(),
                None => Vec::new(),
            };
            if let Some(settings) = self.ivars().settings.borrow().as_ref() {
                settings.editor.set_icons(images, ids, &panel_ids);
            }
        }

        /// «Применить»: раскрываем строку, сужаем разделитель и после раскладки окон
        /// переставляем настоящие иконки.
        #[unsafe(method(onApplyLayout:))]
        fn on_apply_layout(&self, _sender: *mut AnyObject) {
            if self.ivars().applying.get() {
                return;
            }
            self.set_applying(true);
            crate::reveal::exit(None);
            self.ivars().apply_waits.set(0);
            if self.ivars().hidden.get() {
                self.toggle();
            }
            let mtm = self.mtm();
            self.ivars()
                .divider
                .borrow_mut()
                .get_or_insert_with(|| crate::divider::create(mtm))
                .setLength(crate::divider::NARROW_WIDTH);
            let target: &AnyObject = self.as_ref();
            unsafe {
                NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                    APPLY_FIRST_CHECK,
                    target,
                    sel!(onApplyStart:),
                    None,
                    false,
                );
            }
        }

        /// Разделитель разложен — переставляем; ещё нет — ждём ещё, но недолго.
        #[unsafe(method(onApplyStart:))]
        fn on_apply_start(&self, _timer: *mut AnyObject) {
            let (panel_ids, main_ids) = match self.ivars().settings.borrow().as_ref() {
                Some(settings) => settings.editor.order(),
                None => (Vec::new(), Vec::new()),
            };
            let known: Vec<u32> = panel_ids.iter().chain(&main_ids).copied().collect();
            let divider_id = self.find_divider_window(&known).filter(|&id| {
                crate::capture::icon_layout()
                    .iter()
                    .any(|window| window.id == id && window.onscreen && window.width <= DIVIDER_NARROW_WINDOW)
            });
            let Some(divider_id) = divider_id else {
                let waits = self.ivars().apply_waits.get() + 1;
                self.ivars().apply_waits.set(waits);
                if waits > APPLY_MAX_WAITS {
                    crate::log::append("применить: разделитель не встал на место — отмена");
                    self.set_applying(false);
                    return;
                }
                let target: &AnyObject = self.as_ref();
                unsafe {
                    NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                        APPLY_WAIT_INTERVAL,
                        target,
                        sel!(onApplyStart:),
                        None,
                        false,
                    );
                }
                return;
            };
            let order: Vec<u32> =
                panel_ids.iter().copied().chain([divider_id]).chain(main_ids.iter().copied()).collect();
            self.ivars().mouse_taken.set(true);
            crate::mover::take_mouse();
            crate::mover::arrange(order);
            let target: &AnyObject = self.as_ref();
            unsafe {
                NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                    APPLY_WATCHDOG,
                    target,
                    sel!(onApplyWatchdog:),
                    None,
                    false,
                );
            }
        }

        /// Перестановка не закончилась вовремя — возвращаем мышь, чтобы она не осталась отвязанной.
        #[unsafe(method(onApplyWatchdog:))]
        fn on_apply_watchdog(&self, _timer: *mut AnyObject) {
            if self.ivars().mouse_taken.get() {
                crate::log::append("применить: перенос завис — возвращаю мышь");
                self.give_back_mouse();
                self.set_applying(false);
            }
        }

        /// Перестановка закончена: прячем панель разделителем или убираем его, если панель пуста.
        #[unsafe(method(onLayoutApplied))]
        fn on_layout_applied(&self) {
            self.give_back_mouse();
            self.set_applying(false);
            let panel_empty = self
                .ivars()
                .settings
                .borrow()
                .as_ref()
                .is_none_or(|settings| settings.editor.order().0.is_empty());
            if panel_empty {
                if let Some(divider) = self.ivars().divider.borrow_mut().take() {
                    crate::divider::remove(&divider);
                }
                self.ivars().divider_window.set(None);
            } else {
                self.widen_divider();
            }
            crate::divider::set_enabled(!panel_empty);
            let target: &AnyObject = self.as_ref();
            unsafe {
                NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                    PANEL_CAPTURE_DELAY,
                    target,
                    sel!(onEditorCapture:),
                    None,
                    false,
                );
            }
        }

        /// Стартовый разделитель встал на своё место — включаем его и снимаем иконки.
        #[unsafe(method(onDividerPlaced:))]
        fn on_divider_placed(&self, _timer: *mut AnyObject) {
            self.find_divider_window(&[]);
            self.widen_divider();
            let target: &AnyObject = self.as_ref();
            unsafe {
                NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                    PANEL_CAPTURE_DELAY,
                    target,
                    sel!(onStartupSnapshot:),
                    None,
                    false,
                );
            }
        }

        /// Разделитель расширен и иконки панели спрятаны — снимаем их.
        #[unsafe(method(onStartupSnapshot:))]
        fn on_startup_snapshot(&self, _timer: *mut AnyObject) {
            self.start_startup_snapshot();
        }

        #[unsafe(method(onOpenAccessibility:))]
        fn on_open_accessibility(&self, _sender: *mut AnyObject) {
            crate::settings::open_accessibility_pane();
        }

        #[unsafe(method(onOpenScreenRecording:))]
        fn on_open_screen_recording(&self, _sender: *mut AnyObject) {
            crate::settings::open_screen_recording_pane();
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
            let both_ok = sx.is_some() && sx != Some(0.0) && ax.is_some() && ax != Some(0.0);

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
                crate::log::append("placement: лимит попыток исчерпан — сдаюсь (guard защитит)");
                return;
            }

            // Эскалация: после нескольких безуспешных попыток одиночное пересоздание
            // не помогает (новый айтем садится на тот же x=0) → пересоздать ОБА.
            if attempts % PLACEMENT_ESCALATE_AFTER == 0 {
                let mut items_ref = self.ivars().items.borrow_mut();
                if let Some(items) = items_ref.as_mut() {
                    unsafe { status_bar::recreate_both(items, mtm, target, lang) };
                }
                return;
            }

            // Обычный retry: пересоздать конкретный застрявший айтем.
            if sx == Some(0.0) && self.ivars().spacer_retries.get() > 0 {
                self.ivars()
                    .spacer_retries
                    .set(self.ivars().spacer_retries.get() - 1);
                let mut items_ref = self.ivars().items.borrow_mut();
                if let Some(items) = items_ref.as_mut() {
                    unsafe { status_bar::recreate_spacer(items, mtm, lang) };
                }
                return;
            }
            if ax == Some(0.0) && self.ivars().anchor_retries.get() > 0 {
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
        /// Мышь в полосе menu bar → гасим таймер коллапса. Мышь ушла → взводим.
        #[unsafe(method(onMouseMoved))]
        fn on_mouse_moved(&self) {
            if self.ivars().hidden.get() {
                return; // показывать нечего — следить незачем
            }
            if auto_collapse::mouse_in_menu_bar_strip(self.mtm()) || self.mouse_in_panel() {
                self.cancel_collapse_timer();
            } else if self.ivars().collapse_timer.borrow().is_none() {
                self.arm_collapse_timer();
            }
        }

        /// Окна иконок встали после раскрытия — снимаем те, что ушли под чёлку.
        #[unsafe(method(onPanelCapture:))]
        fn on_panel_capture(&self, _timer: *mut AnyObject) {
            if self.ivars().hidden.get() {
                return;
            }
            if crate::reveal::is_on() {
                if !crate::reveal::menu_open() {
                    crate::capture::capture_ids(&self.ivars().panel_ids.borrow());
                }
                return;
            }
            let ids = crate::capture::capture_under_notch(self.ivars().divider_window.get());
            *self.ivars().panel_ids.borrow_mut() = ids;
            self.enter_narrow_mode();
        }

        /// Снимки иконок под чёлкой готовы.
        #[unsafe(method(setPanelIcons:ids:))]
        fn set_panel_icons(&self, images: &NSArray<NSImage>, ids: &NSArray<NSNumber>) {
            let target: &AnyObject = self.as_ref();
            self.ivars()
                .panel
                .borrow_mut()
                .get_or_insert_with(|| Panel::new(self.mtm()))
                .set_icons(self.mtm(), target, images, ids);
            if self.ivars().startup_steps_pending.get() > 0 {
                self.finish_startup_step();
            } else {
                let ids = ids.iter().map(|id| id.unsignedIntValue()).collect();
                crate::click::remember_owners(ids, false);
            }
        }

        /// Тема macOS сменилась: панель сразу берёт снимки из кэша новой темы,
        /// а открытая панель ещё и переснимается, когда строка меню перерисуется.
        #[unsafe(method(onThemeChanged:))]
        fn on_theme_changed(&self, _notification: &NSNotification) {
            let theme = crate::theme::current();
            let target: &AnyObject = self.as_ref();
            let mut panel = self.ivars().panel.borrow_mut();
            let Some(panel) = panel.as_mut() else { return };
            panel.set_theme(self.mtm(), target, theme);
            if panel.is_visible() {
                unsafe {
                    NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                        PANEL_CAPTURE_DELAY,
                        target,
                        sel!(onPanelCapture:),
                        None,
                        false,
                    );
                }
            }
        }

        /// Кнопки иконок под чёлкой найдены в Accessibility на старте.
        #[unsafe(method(onOwnersRemembered))]
        fn on_owners_remembered(&self) {
            self.finish_startup_step();
        }

        /// Длина разделителя панели — для показа меню у чёлки и пробы шторки.
        #[unsafe(method(setPanelDividerLength:))]
        fn set_panel_divider_length(&self, length: f64) {
            if let Some(divider) = self.ivars().divider.borrow().as_ref() {
                divider.setLength(length);
            }
        }

        /// Клик по клону в панели — открываем меню настоящей иконки у чёлки.
        /// Без промежутка у чёлки или разделителя — просто нажимаем иконку.
        #[unsafe(method(onCloneClick:))]
        fn on_clone_click(&self, sender: &NSButton) {
            let icon_id = sender.tag() as u32;
            if crate::reveal::press(icon_id) {
                return;
            }
            self.enter_narrow_mode();
            if !crate::reveal::press(icon_id) {
                crate::click::click_window(icon_id);
            }
        }

        /// Сменился рабочий стол или открылось окно на весь экран — сворачиваем, чтобы
        /// шторка не осталась поверх чужого экрана.
        #[unsafe(method(onSpaceChanged:))]
        fn on_space_changed(&self, _notification: &NSNotification) {
            if !self.ivars().hidden.get() && crate::reveal::is_on() {
                self.toggle();
            }
        }

        /// Таймер коллапса дожил: мышь была вне полоса COLLAPSE_DELAY секунд.
        /// Сворачиваем, если всё ещё показано.
        #[unsafe(method(onAutoCollapse:))]
        fn on_auto_collapse(&self, _timer: *mut AnyObject) {
            self.ivars().collapse_timer.borrow_mut().take();
            if self.ivars().hidden.get() || self.ivars().applying.get() || crate::reveal::menu_open() {
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
            startup_steps_pending: Cell::new(0),
            panel: RefCell::new(None),
            panel_refresh_timer: RefCell::new(None),
            settings: RefCell::new(None),
            divider: RefCell::new(None),
            applying: Cell::new(false),
            apply_waits: Cell::new(0),
            divider_window: Cell::new(None),
            mouse_taken: Cell::new(false),
            panel_ids: RefCell::new(Vec::new()),
        });
        unsafe { msg_send![super(this), init] }
    }

    /// Переключает видимость зоны слева от якоря — с проверкой порядка.
    fn toggle(&self) {
        let items_ref = self.ivars().items.borrow();
        let Some(items) = items_ref.as_ref() else {
            crate::log::append("toggle: айтемы ещё не созданы — игнор");
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
            (SPACER_WIDTH_SHOWN, ANCHOR_SYMBOL_SHOWN)
        };

        if going_to_hide {
            crate::reveal::exit_now(self.mtm());
        }
        items.spacer.setLength(spacer_width);
        status_bar::set_anchor_symbol(items, self.mtm(), anchor_symbol);

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

    /// Панель открыта ровно тогда, когда иконки раскрыты.
    fn sync_panel(&self, items: &StatusItems, hidden: bool) {
        let mut panel = self.ivars().panel.borrow_mut();
        if hidden || !crate::settings::show_panel() {
            if let Some(panel) = panel.as_mut() {
                panel.hide();
            }
            self.leave_narrow_mode();
            if let Some(timer) = self.ivars().panel_refresh_timer.borrow_mut().take() {
                timer.invalidate();
            }
            return;
        }
        let Some(anchor_window) = items.anchor.button(self.mtm()).and_then(|b| b.window()) else {
            return;
        };
        panel
            .get_or_insert_with(|| Panel::new(self.mtm()))
            .show_below(&anchor_window);
        self.capture_panel_when_settled();
        let target: &AnyObject = self.as_ref();
        let refresh = unsafe {
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                PANEL_REFRESH_INTERVAL,
                target,
                sel!(onPanelCapture:),
                None,
                true,
            )
        };
        if let Some(old) = self.ivars().panel_refresh_timer.borrow_mut().replace(refresh) {
            old.invalidate();
        }
    }

    fn mouse_in_panel(&self) -> bool {
        self.ivars().panel.borrow().as_ref().is_some_and(Panel::contains_mouse)
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
    /// отписывается от нотификации и запускает стартовое авто-скрытие.
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
        let both_ok = sx.is_some() && sx != Some(0.0) && ax.is_some() && ax != Some(0.0);
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
        if !crate::divider::is_enabled() {
            self.start_startup_snapshot();
            return;
        }
        *self.ivars().divider.borrow_mut() = Some(crate::divider::create(self.mtm()));
        let target: &AnyObject = self.as_ref();
        unsafe {
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                APPLY_SETTLE_DELAY,
                target,
                sel!(onDividerPlaced:),
                None,
                false,
            );
        }
    }

    /// Пока строка раскрыта: снимок иконок под чёлкой и поиск их кнопок, потом авто-скрытие.
    fn start_startup_snapshot(&self) {
        self.ivars().startup_steps_pending.set(2);
        let ids = crate::capture::capture_under_notch(self.ivars().divider_window.get());
        crate::click::remember_owners(ids, true);
    }

    /// Возвращает мышь, если она забрана; повторный вызов ничего не делает.
    fn give_back_mouse(&self) {
        if self.ivars().mouse_taken.replace(false) {
            crate::mover::release_mouse();
        }
    }

    /// Идёт ли применение раскладки: флаг, индикатор в настройках.
    fn set_applying(&self, applying: bool) {
        self.ivars().applying.set(applying);
        if let Some(settings) = self.ivars().settings.borrow().as_ref() {
            settings.set_applying(applying);
        }
    }

    /// Окно разделителя панели среди окон строки меню. Уже известное берём по номеру;
    /// иначе ищем разложенное окно в точке разделителя, не входящее в `known`, и запоминаем.
    fn find_divider_window(&self, known: &[u32]) -> Option<u32> {
        let layout = crate::capture::icon_layout();
        if let Some(id) = self.ivars().divider_window.get() {
            if layout.iter().any(|window| window.id == id && window.width > 0.0) {
                return Some(id);
            }
        }
        let divider_x = {
            let divider = self.ivars().divider.borrow();
            status_bar::item_origin_x(divider.as_ref()?, self.mtm())?
        };
        let id = layout
            .iter()
            .find(|window| (window.x - divider_x).abs() < crate::capture::POSITION_TOLERANCE && window.width > 0.0 && !known.contains(&window.id))
            .map(|window| window.id)?;
        self.ivars().divider_window.set(Some(id));
        Some(id)
    }

    /// Левый край окна разделителя панели по данным WindowServer.
    fn divider_window_x(&self) -> Option<f64> {
        let id = self.ivars().divider_window.get()?;
        crate::capture::icon_layout().into_iter().find(|window| window.id == id).map(|window| window.x)
    }

    /// Ставит разделителю ширину, при которой иконки панели уходят внутрь экрана.
    fn widen_divider(&self) {
        let Some(divider_id) = self.ivars().divider_window.get() else {
            crate::log::append("divider: окно не найдено — ширину не меняю");
            return;
        };
        let width = crate::divider::hiding_width(divider_id);
        if let Some(divider) = self.ivars().divider.borrow().as_ref() {
            divider.setLength(width);
        }
    }

    /// Закрывает один стартовый шаг; после последнего — стартовое авто-скрытие.
    fn finish_startup_step(&self) {
        let left = self.ivars().startup_steps_pending.get().saturating_sub(1);
        self.ivars().startup_steps_pending.set(left);
        if left == 0 && !self.ivars().hidden.get() {
            self.toggle();
        }
    }

    /// Включает узкий режим панели, если панель открыта, в ней есть иконки и у чёлки есть место.
    fn enter_narrow_mode(&self) {
        let panel_visible = self.ivars().panel.borrow().as_ref().is_some_and(Panel::is_visible);
        if !panel_visible || self.ivars().applying.get() || self.ivars().panel_ids.borrow().is_empty() {
            return;
        }
        let wide = self.ivars().divider.borrow().as_ref().map(|divider| divider.length());
        let target = self.ivars().divider_window.get().zip(wide).filter(|(_, wide)| *wide > crate::divider::NARROW_WIDTH);
        if let Some(((divider_id, wide), gap)) = target.zip(self.notch_gap()) {
            crate::reveal::enter(divider_id, gap, wide);
        }
    }

    /// Выключает узкий режим: разделитель получит прежнюю длину.
    fn leave_narrow_mode(&self) {
        crate::reveal::exit(self.ivars().divider_window.get());
    }

    /// Снимает панель, как только раскрытая строка встала: разделитель вышел на экран
    /// и окна иконок перестали двигаться. Разделителя не дождались — снимаем по таймауту.
    fn capture_panel_when_settled(&self) {
        let divider_id = self.ivars().divider_window.get();
        thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs_f64(PANEL_CAPTURE_DELAY);
            let divider_shown = || {
                crate::capture::icon_layout().iter().any(|window| Some(window.id) == divider_id && window.x >= 0.0)
            };
            while divider_id.is_some() && !divider_shown() && Instant::now() < deadline {
                thread::sleep(PANEL_SETTLE_POLL);
            }
            crate::mover::wait_until_still();
            DispatchQueue::main().exec_async(|| {
                let mtm = MainThreadMarker::new().expect("main queue");
                if let Some(delegate) = NSApplication::sharedApplication(mtm).delegate() {
                    let delegate: &AnyObject = delegate.as_ref();
                    let _: () = unsafe { msg_send![delegate, onPanelCapture: std::ptr::null_mut::<AnyObject>()] };
                }
            });
        });
    }

    /// Участок строки меню от правого края чёлки до ≡◂, в координатах CG: только здесь
    /// строка меняется, когда разделитель панели сужен.
    fn notch_gap(&self) -> Option<CGRect> {
        let mtm = self.mtm();
        let (notch_right, spacer_x) = {
            let items_ref = self.ivars().items.borrow();
            let items = items_ref.as_ref()?;
            let screen = items.spacer.button(mtm)?.window()?.screen()?;
            (screen.auxiliaryTopRightArea().origin.x, status_bar::item_origin_x(&items.spacer, mtm)?)
        };
        let height = crate::capture::icon_layout().first()?.height;
        if notch_right <= 0.0 || spacer_x <= notch_right {
            crate::log::append("reveal: участка у чёлки нет");
            return None;
        }
        Some(CGRect::new(CGPoint::new(notch_right, 0.0), CGSize::new(spacer_x - notch_right, height)))
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
    /// нуля растягивало бы спейсер через весь экран и уносило якорь (journal 006).
    /// Если координаты невалидны — безопасный отказ (лучше не спрятать, чем уронить).
    fn spacer_is_left_of_anchor(&self, items: &StatusItems) -> bool {
        let mtm = self.mtm();
        let spacer_x = status_bar::item_origin_x(&items.spacer, mtm);
        let anchor_x = status_bar::item_origin_x(&items.anchor, mtm);

        match (spacer_x, anchor_x) {
            (Some(sx), Some(ax)) => {
                // Оба должны быть размещены (x>0) и спейсер строго левее якоря.
                let placed = sx > 0.0 && ax > 0.0;
                let ok = placed && sx < ax;
                if !ok {
                    let why = if placed { "правее якоря" } else { "не размещён (x=0)" };
                    crate::log::append(&format!("guard: spacer.x={sx} anchor.x={ax} — спейсер {why}"));
                }
                ok
            }
            _ => {
                crate::log::append("guard: координаты ещё недоступны → отказ (безопасно)");
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
        crate::log::append("СТОП: спейсер не левее якоря — скрытие заблокировано, показываю ⚠");
    }

    /// Ширина спейсера в скрытом состоянии: ширина экрана + запас, ограниченная.
    fn hidden_width(&self) -> f64 {
        let screen_width = NSScreen::mainScreen(self.mtm())
            .map(|screen| screen.frame().size.width)
            .unwrap_or(SCREEN_WIDTH_FALLBACK);
        (screen_width + HIDDEN_WIDTH_MARGIN).clamp(HIDDEN_WIDTH_MIN, HIDDEN_WIDTH_MAX)
    }
}
