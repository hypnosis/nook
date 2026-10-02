//! Меню спрятанной иконки рядом с её копией в панели. На время клика разделитель
//! сужается, и иконки панели встают у чёлки; приложение открывает меню под своей
//! иконкой, то есть рядом с панелью. Шторка со снимком промежутка у чёлки закрывает
//! перестройку строки. Когда меню закрылось, ширина и строка возвращаются.

use std::cell::RefCell;
use std::thread;
use std::time::{Duration, Instant};

use dispatch2::DispatchQueue;
use objc2::runtime::AnyObject;
use objc2::{msg_send, MainThreadMarker};
use objc2_app_kit::NSApplication;
use objc2_core_foundation::CGRect;

use crate::shroud::Shroud;

// HARDCODE: тайминги показа меню у копии; вынести в конфиг позже.
const PAINT_DELAY: Duration = Duration::from_millis(50);
const LAYOUT_POLL: Duration = Duration::from_millis(5);
const LAYOUT_TIMEOUT: Duration = Duration::from_millis(500);
/// Приложение успевает разложить меню по новому месту иконки.
const PRESS_DELAY: Duration = Duration::from_millis(50);
/// Иконка, уходящая обратно в спрятанные, ещё гаснет после того, как встала на место.
const FADE_DELAY: Duration = Duration::from_millis(150);
const MENU_POLL: Duration = Duration::from_millis(50);
const MENU_APPEAR_TIMEOUT: Duration = Duration::from_millis(1500);
const MENU_MAX_OPEN: Duration = Duration::from_secs(120);
/// Окно разделителя не шире этого — он сужен.
const NARROW_WINDOW: f64 = 40.0;

thread_local! {
    static SHROUD: RefCell<Option<Shroud>> = const { RefCell::new(None) };
}

/// Где и как показать меню иконки с окном `icon_id`.
pub struct Reveal {
    pub icon_id: u32,
    pub divider_id: u32,
    /// Промежуток от чёлки до первой иконки правее разделителя, координаты CG.
    pub gap: CGRect,
    pub narrow: f64,
    pub wide: f64,
}

/// Открывает меню иконки у чёлки. По окончании зовёт `onRevealDone` у делегата.
pub fn open_menu(reveal: Reveal) {
    let pid = crate::click::owner_pid(reveal.icon_id);
    thread::spawn(move || {
        let started = Instant::now();
        let Some(before) = crate::capture::screen_rect(reveal.gap) else {
            crate::log::append("reveal: снимка промежутка нет — обычный клик");
            crate::click::click_window(reveal.icon_id);
            on_main(done);
            return;
        };
        let gap = reveal.gap;
        on_main(move |mtm| {
            SHROUD.with(|cell| cell.borrow_mut().get_or_insert_with(|| Shroud::new(mtm)).show(mtm, &before, gap))
        });
        thread::sleep(PAINT_DELAY);

        let narrow = reveal.narrow;
        on_main(move |mtm| set_divider(mtm, narrow));
        wait_divider(reveal.divider_id, true);
        crate::mover::wait_until_still();
        thread::sleep(PRESS_DELAY);
        log_layout(reveal.gap);

        let windows_before = pid.map(crate::capture::onscreen_windows_of).unwrap_or_default();
        crate::click::click_window(reveal.icon_id);
        crate::log::append(&format!(
            "reveal: окно {} нажато у чёлки через {} мс",
            reveal.icon_id,
            started.elapsed().as_millis()
        ));
        wait_menu_closed(pid, &windows_before);

        let wide = reveal.wide;
        on_main(move |mtm| set_divider(mtm, wide));
        wait_divider(reveal.divider_id, false);
        crate::mover::wait_until_still();
        thread::sleep(FADE_DELAY);
        on_main(|_| {
            SHROUD.with(|cell| {
                if let Some(shroud) = cell.borrow().as_ref() {
                    shroud.hide();
                }
            })
        });
        crate::log::append(&format!("reveal: строка вернулась, всего {} мс", started.elapsed().as_millis()));
        on_main(done);
    });
}

/// Пишет в лог шторку и нарисованные окна иконок, которые она должна закрыть.
fn log_layout(gap: CGRect) {
    let shown: Vec<String> = crate::capture::icon_layout()
        .iter()
        .filter(|w| w.onscreen && w.x < gap.origin.x + gap.size.width)
        .map(|w| format!("{}@{}+{}", w.id, w.x, w.width))
        .collect();
    crate::log::append(&format!(
        "reveal: шторка x={} w={}, под ней нарисованы {}",
        gap.origin.x,
        gap.size.width,
        shown.join(" ")
    ));
}

/// Ждёт, пока окно разделителя станет узким (`narrow`) или снова широким.
fn wait_divider(divider_id: u32, narrow: bool) {
    let deadline = Instant::now() + LAYOUT_TIMEOUT;
    while Instant::now() < deadline {
        let width = crate::capture::icon_layout().into_iter().find(|w| w.id == divider_id).map(|w| w.width);
        if width.is_some_and(|width| (width <= NARROW_WINDOW) == narrow) {
            return;
        }
        thread::sleep(LAYOUT_POLL);
    }
    crate::log::append(&format!("reveal: разделитель не стал {}", if narrow { "узким" } else { "широким" }));
}

/// Ждёт, пока у приложения `pid` появится новое окно (меню или поповер) и закроется.
fn wait_menu_closed(pid: Option<i32>, windows_before: &[u32]) {
    let Some(pid) = pid else {
        crate::log::append("reveal: процесс иконки неизвестен — жду только появления");
        thread::sleep(MENU_APPEAR_TIMEOUT);
        return;
    };
    let appear_deadline = Instant::now() + MENU_APPEAR_TIMEOUT;
    let menu = loop {
        if let Some(id) = crate::capture::onscreen_windows_of(pid).into_iter().find(|id| !windows_before.contains(id)) {
            break id;
        }
        if Instant::now() > appear_deadline {
            crate::log::append("reveal: окно меню не появилось");
            return;
        }
        thread::sleep(MENU_POLL);
    };
    crate::log::append(&format!("reveal: меню открыто, окно {menu}"));
    let close_deadline = Instant::now() + MENU_MAX_OPEN;
    while crate::capture::onscreen_windows_of(pid).contains(&menu) {
        if Instant::now() > close_deadline {
            crate::log::append("reveal: меню открыто слишком долго — возвращаю строку");
            return;
        }
        thread::sleep(MENU_POLL);
    }
    crate::log::append("reveal: меню закрыто");
}

fn on_main(work: impl FnOnce(MainThreadMarker) + Send) {
    DispatchQueue::main().exec_sync(move || work(MainThreadMarker::new().expect("main queue")));
}

fn set_divider(mtm: MainThreadMarker, length: f64) {
    if let Some(delegate) = NSApplication::sharedApplication(mtm).delegate() {
        let delegate: &AnyObject = delegate.as_ref();
        let _: () = unsafe { msg_send![delegate, setPanelDividerLength: length] };
    }
}

fn done(mtm: MainThreadMarker) {
    if let Some(delegate) = NSApplication::sharedApplication(mtm).delegate() {
        let delegate: &AnyObject = delegate.as_ref();
        let _: () = unsafe { msg_send![delegate, onRevealDone] };
    }
}
