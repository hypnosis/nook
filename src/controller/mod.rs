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
use std::time::Instant;

use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
use objc2::MainThreadMarker;
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSApplicationDelegate, NSButton, NSScreen, NSStatusItem, NSSwitch};
use objc2_foundation::NSNotification;

use crate::auto_collapse::MouseMonitor;
use crate::layout::Layout;
use crate::onboarding::Onboarding;
use crate::panel::Panel;
use crate::settings::Settings;
use crate::shroud::Shroud;
use crate::timer::Timer;
use crate::status_bar::StatusItems;
use crate::strings::{self, Lang};
use crate::tuning::ANCHOR_MAX_RETRIES;

mod collapse;
mod moves;
mod reading;
mod shroud;
mod startup;
mod ui;
mod watchdog;

/// Внутреннее состояние.
/// - `items` появляются после запуска. `hidden`: текущий режим.
/// - `monitor` следит за мышью, пока иконки показаны (для автосворачивания).
/// - `collapse_timer` — взведённый таймер коллапса (мышь ушла из полосы).
/// - `lang` — язык интерфейса, определён один раз на старте.
pub struct ControllerIvars {
    items: RefCell<Option<StatusItems>>,
    hidden: Cell<bool>,
    monitor: RefCell<Option<MouseMonitor>>,
    collapse_timer: RefCell<Option<Timer>>,
    lang: Lang,
    /// Сколько раз ещё можно пересоздать застрявший якорь.
    anchor_retries: Cell<u32>,
    /// Сколько раз ещё можно пересоздать застрявший спейсер.
    spacer_retries: Cell<u32>,
    /// Сколько проверок размещения прошло без успеха.
    placement_attempts: Cell<u32>,
    /// Порядок элементов Nook нарушен: ничего не прячется, на якоре ⚠.
    blocked: Cell<bool>,
    /// Сколько проверок подряд сторож видит нарушение.
    bad_reads: Cell<u32>,
    /// Когда сменились экраны: строка в это время раскладывается заново.
    screens_changed: Cell<Option<Instant>>,
    /// Стартовое авто-скрытие уже запущено: его зовут и событие NSWindowDidMove, и таймер.
    placement_done: Cell<bool>,
    /// Идёт старт: строка раскрыта, пока не сняты иконки панели.
    starting: Cell<bool>,
    /// На старте macOS не помнила место разделителя: раскладка восстанавливается переносами.
    restore_on_start: Cell<bool>,
    panel: RefCell<Option<Panel>>,
    /// Таймер чтения строки: живёт, пока строка раскрыта.
    bar_timer: RefCell<Option<Timer>>,
    /// Таймер сторожа: живёт, пока строка раскрыта и элементы Nook размещены.
    order_timer: RefCell<Option<Timer>>,
    /// Таймер проверки размещения на старте: живёт, пока элементы Nook не встали.
    placement_timer: RefCell<Option<Timer>>,
    /// Таймер сторожа переноса: живёт, пока идёт перенос.
    move_watchdog: RefCell<Option<Timer>>,
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
    /// Перенос Nook, итог которого сверит следующее чтение.
    own_move: RefCell<Option<moves::OwnMove>>,
    /// Мышь отвязана от курсора на время переноса.
    mouse_taken: Cell<bool>,
    /// Шторка над иконками панели у чёлки, пока строка раскрыта.
    shroud: RefCell<Option<Shroud>>,
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
            self.launch();
        }
    }

    impl Controller {
        #[unsafe(method(onAnchorClick:))]
        fn on_anchor_click(&self, sender: *mut AnyObject) {
            self.anchor_clicked(sender);
        }

        #[unsafe(method(onToggleLogin:))]
        fn on_toggle_login(&self, _sender: *mut AnyObject) {
            self.toggle_login();
        }

        #[unsafe(method(onOpenSettings:))]
        fn on_open_settings(&self, _sender: *mut AnyObject) {
            self.open_settings();
        }

        #[unsafe(method(onWindowClose:))]
        fn on_window_close(&self, notification: &NSNotification) {
            self.window_closing(notification);
        }

        #[unsafe(method(onWindowFocus:))]
        fn on_window_focus(&self, _notification: &NSNotification) {
            self.refresh_windows();
        }

        #[unsafe(method(onToggleShowPanel:))]
        fn on_toggle_show_panel(&self, sender: &NSSwitch) {
            self.show_panel_switched(sender);
        }

        #[unsafe(method(onToggleAutomaticLayout:))]
        fn on_toggle_automatic_layout(&self, sender: &NSSwitch) {
            self.automatic_layout_switched(sender);
        }

        #[unsafe(method(onEditorNeedsIcons))]
        fn on_editor_needs_icons(&self) {
            self.editor_needs_icons();
        }

        #[unsafe(method(onTileDropped:))]
        fn on_tile_dropped(&self, id: u32) {
            self.tile_dropped(id);
        }

        #[unsafe(method(onMoveWatchdog:))]
        fn on_move_watchdog(&self, _timer: *mut AnyObject) {
            self.check_move();
        }

        #[unsafe(method(onDividerPlaced:))]
        fn on_divider_placed(&self, _timer: *mut AnyObject) {
            self.divider_placed();
        }

        /// Чтение строки по таймеру.
        #[unsafe(method(onBarTick:))]
        fn on_bar_tick(&self, _timer: *mut AnyObject) {
            if !self.ivars().hidden.get() {
                self.request_read();
            }
        }

        #[unsafe(method(onOrderCheck:))]
        fn on_order_check(&self, _timer: *mut AnyObject) {
            self.check_order();
        }

        #[unsafe(method(onScreensChanged:))]
        fn on_screens_changed(&self, _notification: &NSNotification) {
            self.screens_changed();
        }

        #[unsafe(method(onOpenAccessibility:))]
        fn on_open_accessibility(&self, _sender: *mut AnyObject) {
            crate::permissions::open_accessibility_pane();
        }

        #[unsafe(method(onOpenScreenRecording:))]
        fn on_open_screen_recording(&self, _sender: *mut AnyObject) {
            crate::permissions::open_screen_recording_pane();
        }

        #[unsafe(method(onResetPermissions:))]
        fn on_reset_permissions(&self, _sender: *mut AnyObject) {
            self.reset_permissions();
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

        #[unsafe(method(onPlacementCheck:))]
        fn on_placement_check(&self, _timer: *mut AnyObject) {
            self.check_placement();
        }

        #[unsafe(method(onMouseMoved))]
        fn on_mouse_moved(&self) {
            self.mouse_moved();
        }

        #[unsafe(method(onThemeChanged:))]
        fn on_theme_changed(&self, _notification: &NSNotification) {
            self.theme_changed();
        }

        #[unsafe(method(onAppActivated:))]
        fn on_app_activated(&self, _notification: &NSNotification) {
            self.app_activated();
        }

        /// Клик по копии в панели — нажимаем настоящую иконку там, где она спрятана.
        #[unsafe(method(onCloneClick:))]
        fn on_clone_click(&self, sender: &NSButton) {
            crate::press::press(sender.tag() as u32);
        }

        #[unsafe(method(onAutoCollapse:))]
        fn on_auto_collapse(&self, _timer: *mut AnyObject) {
            self.auto_collapse();
        }
    }
);

/// Выполняет `f` с контроллером на главном потоке: так фоновые потоки отдают ему результат.
pub fn on_main(f: impl FnOnce(&Controller) + Send + 'static) {
    DispatchQueue::main().exec_async(move || {
        let mtm = MainThreadMarker::new().expect("main queue");
        let Some(delegate) = NSApplication::sharedApplication(mtm).delegate() else { return };
        let delegate: &AnyObject = delegate.as_ref();
        if let Some(controller) = delegate.downcast_ref::<Controller>() {
            f(controller);
        }
    });
}

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
            blocked: Cell::new(false),
            bad_reads: Cell::new(0),
            screens_changed: Cell::new(None),
            placement_done: Cell::new(false),
            starting: Cell::new(false),
            restore_on_start: Cell::new(false),
            panel: RefCell::new(None),
            bar_timer: RefCell::new(None),
            order_timer: RefCell::new(None),
            placement_timer: RefCell::new(None),
            move_watchdog: RefCell::new(None),
            settings: RefCell::new(None),
            onboarding: RefCell::new(None),
            divider: RefCell::new(None),
            layout: RefCell::new(Layout::new(crate::settings::panel_order())),
            divider_window: Cell::new(None),
            epoch: Cell::new(0),
            moving: Cell::new(false),
            own_move: RefCell::new(None),
            mouse_taken: Cell::new(false),
            shroud: RefCell::new(None),
        });
        unsafe { msg_send![super(this), init] }
    }

    /// Правый край чёлки основного экрана, где живут иконки строки; без чёлки — `None`.
    fn notch_edge(&self) -> Option<f64> {
        let screens = NSScreen::screens(self.mtm());
        let area = screens.firstObject()?.auxiliaryTopRightArea();
        (area.size.width > 0.0).then_some(area.origin.x)
    }

    fn notch_right(&self) -> f64 {
        NSScreen::mainScreen(self.mtm()).map_or(0.0, |screen| screen.auxiliaryTopRightArea().origin.x)
    }
}
