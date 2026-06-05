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
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2::sel;
use objc2_app_kit::{NSApplicationDelegate, NSScreen};
use objc2_foundation::{NSNotification, NSString, NSTimer};

use crate::status_bar::{
    self, StatusItems, ANCHOR_TITLE_BLOCKED, ANCHOR_TITLE_HIDDEN, ANCHOR_TITLE_SHOWN,
    SPACER_WIDTH_SHOWN,
};

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

/// Внутреннее состояние. `items` появляются после запуска. `hidden`: текущий режим.
pub struct ControllerIvars {
    items: RefCell<Option<StatusItems>>,
    hidden: Cell<bool>,
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
            let items = unsafe { status_bar::create(mtm, target) };

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
        #[unsafe(method(onAnchorClick:))]
        fn on_anchor_click(&self, _sender: *mut AnyObject) {
            self.toggle();
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
    }
);

impl Controller {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc().set_ivars(ControllerIvars {
            items: RefCell::new(None),
            hidden: Cell::new(false),
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
        let (spacer_width, anchor_title) = if going_to_hide {
            (self.hidden_width(), ANCHOR_TITLE_HIDDEN)
        } else {
            (SPACER_WIDTH_SHOWN, ANCHOR_TITLE_SHOWN)
        };

        items.spacer.setLength(spacer_width);
        self.set_anchor_title(items, anchor_title);

        crate::log::append(&format!(
            "клик: hidden={going_to_hide} spacer.length={spacer_width} anchor='{anchor_title}'"
        ));
        self.log_widths(items);
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

    /// Показывает знак блокировки на якоре и оставляет всё как есть.
    fn show_blocked(&self, items: &StatusItems) {
        self.set_anchor_title(items, ANCHOR_TITLE_BLOCKED);
        crate::log::append("СТОП: спейсер не левее < — скрытие заблокировано, показываю ⚠");
    }

    /// Ширина спейсера в скрытом состоянии: ширина экрана + запас, ограниченная.
    fn hidden_width(&self) -> f64 {
        let screen_width = NSScreen::mainScreen(self.mtm())
            .map(|screen| screen.frame().size.width)
            .unwrap_or(SCREEN_WIDTH_FALLBACK);
        (screen_width + HIDDEN_WIDTH_MARGIN).clamp(HIDDEN_WIDTH_MIN, HIDDEN_WIDTH_MAX)
    }

    fn set_anchor_title(&self, items: &StatusItems, title: &str) {
        if let Some(button) = items.anchor.button(self.mtm()) {
            button.setTitle(&NSString::from_str(title));
        }
    }

    fn log_widths(&self, items: &StatusItems) {
        crate::log::append(&format!(
            "  geometry: spacer.length={} anchor.length={}",
            items.spacer.length(),
            items.anchor.length()
        ));
    }
}
