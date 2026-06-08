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
    NSApplication, NSApplicationDelegate, NSEventType, NSScreen, NSWindowDidMoveNotification,
};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSPoint, NSTimer};

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
        }

        /// Пункт меню «Выход» — завершаем приложение.
        #[unsafe(method(onQuit:))]
        fn on_quit(&self, _sender: *mut AnyObject) {
            crate::log::append("меню: Выход");
            NSApplication::sharedApplication(self.mtm()).terminate(None);
        }

        /// ОСНОВНОЙ триггер: айтем получил/сменил позицию. Если оба размещены —
        /// завершаем стартовую настройку (авто-скрытие). Это точное событие,
        /// в отличие от поллинга по таймеру.
        #[unsafe(method(onWindowMoved:))]
        fn on_window_moved(&self, _notification: &NSNotification) {
            self.finish_placement_if_ready("event");
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
            crate::log::append(&format!("placement: spacer.x={sx:?} anchor.x={ax:?}"));

            let lang = self.ivars().lang;
            let target: &AnyObject = self.as_ref();
            let both_ok = sx.is_some() && sx != Some(0.0) && ax.is_some() && ax != Some(0.0);

            // Оба размещены → общий финиш (событие могло уже его сделать) + стоп таймера.
            if both_ok {
                self.stop_placement_timer(timer);
                self.finish_placement_if_ready("timer");
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
            anchor_retries: Cell::new(ANCHOR_MAX_RETRIES),
            spacer_retries: Cell::new(ANCHOR_MAX_RETRIES),
            placement_attempts: Cell::new(0),
            placement_done: Cell::new(false),
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
    fn finish_placement_if_ready(&self, source: &str) {
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
        crate::log::append(&format!(
            "placement готово [{source}]: spacer.x={sx:?} anchor.x={ax:?} — стартовое авто-скрытие"
        ));
        if !self.ivars().hidden.get() {
            self.toggle();
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
                let why = if ok {
                    "ЛЕВЕЕ ✓"
                } else if !placed {
                    "НЕ РАЗМЕЩЁН (x=0) ✗"
                } else {
                    "ПРАВЕЕ ✗ (поправь Cmd+drag)"
                };
                crate::log::append(&format!("guard: spacer.x={sx} anchor.x={ax} → spacer {why} anchor"));
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
