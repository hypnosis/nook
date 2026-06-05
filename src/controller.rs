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

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
use objc2::MainThreadMarker;
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly, Message};
use objc2::sel;
use objc2_app_kit::{
    NSApplication, NSApplicationDelegate, NSEventType, NSScreen,
};
use objc2_foundation::{NSNotification, NSPoint, NSTimer};

use crate::auto_collapse::{self, MouseMonitor};
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

/// Задержка перед стартовым авто-скрытием. Координаты айтемов доступны не сразу
/// после запуска, а после стабилизации RunLoop — за 1с они уже валидны.
// HARDCODE: задержка стартового авто-скрытия; вынести в конфиг позже.
const STARTUP_HIDE_DELAY: f64 = 1.0;

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
            crate::log::append("applicationDidFinishLaunching: создаю айтемы");

            let mtm = self.mtm();
            let target: &AnyObject = self.as_ref();
            let items = unsafe { status_bar::create(mtm, target, self.ivars().lang) };

            *self.ivars().items.borrow_mut() = Some(items);
            self.ivars().hidden.set(false);
            crate::log::append("айтемы созданы, состояние: показано (hidden=false)");

            // Одноразовый таймер: через STARTUP_HIDE_DELAY попробовать авто-скрытие
            // (координаты для guard к этому моменту уже стабильны).
            let target: &AnyObject = self.as_ref();
            unsafe {
                NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                    STARTUP_HIDE_DELAY,
                    target,
                    sel!(onStartupTimer:),
                    None,
                    false,
                );
            }
            crate::log::append(&format!(
                "запланировано стартовое авто-скрытие через {STARTUP_HIDE_DELAY}с"
            ));
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

        /// Пункт меню «О clearbar» — пишем версию в лог (диалог — позже).
        #[unsafe(method(onAbout:))]
        fn on_about(&self, _sender: *mut AnyObject) {
            crate::log::append(&format!("О clearbar: версия {}", env!("CARGO_PKG_VERSION")));
        }

        /// Пункт меню «Выход» — завершаем приложение.
        #[unsafe(method(onQuit:))]
        fn on_quit(&self, _sender: *mut AnyObject) {
            crate::log::append("меню: Выход");
            NSApplication::sharedApplication(self.mtm()).terminate(None);
        }

        /// Сработал стартовый таймер: пытаемся спрятать, если ещё показаны.
        /// `toggle` сам проверит guard и откажется, если порядок неверный.
        #[unsafe(method(onStartupTimer:))]
        fn on_startup_timer(&self, _timer: *mut AnyObject) {
            crate::log::append("стартовый таймер: пробую авто-скрытие");
            if self.ivars().hidden.get() {
                crate::log::append("  уже скрыто — пропускаю");
                return;
            }
            self.toggle();
        }

        /// Движение мыши (от монитора). Работает только когда иконки показаны.
        /// Мышь в полосе menu bar → гасим таймер коллапса. Мышь ушла → взводим.
        #[unsafe(method(onMouseMoved))]
        fn on_mouse_moved(&self) {
            if self.ivars().hidden.get() {
                return; // показывать нечего — следить незачем
            }
            if auto_collapse::mouse_in_menu_bar_strip(self.mtm()) {
                self.cancel_collapse_timer();
            } else if self.ivars().collapse_timer.borrow().is_none() {
                self.arm_collapse_timer();
            }
        }

        /// Таймер коллапса дожил: мышь была вне полоса COLLAPSE_DELAY секунд.
        /// Сворачиваем, если всё ещё показано.
        #[unsafe(method(onAutoCollapse:))]
        fn on_auto_collapse(&self, _timer: *mut AnyObject) {
            self.ivars().collapse_timer.borrow_mut().take();
            if self.ivars().hidden.get() {
                return;
            }
            crate::log::append("авто-collapse: мышь ушла, сворачиваю");
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

        items.spacer.setLength(spacer_width);
        status_bar::set_anchor_symbol(items, self.mtm(), anchor_symbol);

        crate::log::append(&format!(
            "клик: hidden={going_to_hide} spacer.length={spacer_width} anchor='{anchor_symbol}'"
        ));
        self.log_widths(items);

        // Смена состояния гасит взведённый таймер коллапса.
        self.cancel_collapse_timer();
        // Показали → следим за мышью (для автосворачивания). Скрыли → перестаём.
        if going_to_hide {
            self.stop_mouse_watch();
        } else {
            self.start_mouse_watch();
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
        crate::log::append("авто-collapse: мышь ушла из полосы, взвёл таймер");
    }

    /// Проверка порядка: левый край спейсера должен быть строго левее якоря.
    /// Если координаты ещё недоступны — считаем порядок неверным (безопасный
    /// отказ: лучше не спрятать, чем уронить якорь).
    fn spacer_is_left_of_anchor(&self, items: &StatusItems) -> bool {
        let mtm = self.mtm();
        let spacer_x = status_bar::item_origin_x(&items.spacer, mtm);
        let anchor_x = status_bar::item_origin_x(&items.anchor, mtm);

        match (spacer_x, anchor_x) {
            (Some(sx), Some(ax)) => {
                let ok = sx < ax;
                crate::log::append(&format!(
                    "guard: spacer.x={sx} anchor.x={ax} → spacer {} anchor",
                    if ok { "ЛЕВЕЕ ✓" } else { "ПРАВЕЕ ✗ (поправь Cmd+drag)" }
                ));
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

        // Позиция под кнопкой: левый-нижний угол её bounds.
        let origin = NSPoint::new(0.0, button.bounds().size.height + 4.0);
        menu.popUpMenuPositioningItem_atLocation_inView(None, origin, Some(&button));
        crate::log::append("показано контекстное меню (правый клик)");
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

    fn log_widths(&self, items: &StatusItems) {
        crate::log::append(&format!(
            "  geometry: spacer.length={} anchor.length={}",
            items.spacer.length(),
            items.anchor.length()
        ));
    }
}
