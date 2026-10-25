//! Нажатие на копию в панели: настоящая иконка нажимается там, где она спрятана,
//! строка меню не меняется. Пока меню иконки открыто, строка не сворачивается.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::Instant;

use crate::tuning::{MENU_APPEAR_TIMEOUT, MENU_MAX_OPEN, MENU_POLL};

static MENU_OPEN: AtomicBool = AtomicBool::new(false);
/// Сколько окон, открытых нажатием, ещё на экране.
static WINDOWS_OPEN: AtomicUsize = AtomicUsize::new(0);

/// Меню иконки, нажатой из панели, ещё открыто.
pub fn menu_open() -> bool {
    MENU_OPEN.load(Ordering::SeqCst)
}

/// Нажатая иконка может быть подсвечена в строке: её меню или окно ещё на экране, сколько бы
/// оно ни было открыто. Подсветку macOS рисует в самой строке.
pub fn icon_highlighted() -> bool {
    menu_open() || WINDOWS_OPEN.load(Ordering::SeqCst) > 0
}

/// Нажимает иконку с окном `icon_id` и ждёт, пока её меню закроется. Пока меню прошлого
/// нажатия открыто, новое нажатие не идёт.
pub fn press(icon_id: u32) {
    if MENU_OPEN.swap(true, Ordering::SeqCst) {
        return;
    }
    let pid = crate::click::owner_pid(icon_id);
    let clicked = Instant::now();
    thread::spawn(move || {
        let windows_before = pid.map(crate::capture::onscreen_windows_of).unwrap_or_default();
        crate::click::click_window(icon_id);
        let mut held = true;
        let mut release = || {
            if std::mem::take(&mut held) {
                MENU_OPEN.store(false, Ordering::SeqCst);
            }
        };
        wait_menu_closed(pid, &windows_before, clicked, &mut release);
        release();
    });
}

/// Ждёт, пока у приложения `pid` появится новое окно (меню или поповер) и закроется. Окно
/// открыто дольше `MENU_MAX_OPEN` — `release` пускает новые нажатия, а окно ждётся дальше.
fn wait_menu_closed(pid: Option<i32>, windows_before: &[u32], clicked: Instant, release: &mut impl FnMut()) {
    let Some(pid) = pid else {
        log::debug!("процесс иконки неизвестен — жду только появления");
        thread::sleep(MENU_APPEAR_TIMEOUT);
        return;
    };
    let appear_deadline = Instant::now() + MENU_APPEAR_TIMEOUT;
    let menu = loop {
        if let Some(id) = crate::capture::onscreen_windows_of(pid).into_iter().find(|id| !windows_before.contains(id)) {
            break id;
        }
        if Instant::now() > appear_deadline {
            log::warn!("окно меню не появилось");
            return;
        }
        thread::sleep(MENU_POLL);
    };
    log::debug!("клик → меню {} мс, окно меню {menu}: {:?}", clicked.elapsed().as_millis(), crate::capture::window_rect(menu));
    WINDOWS_OPEN.fetch_add(1, Ordering::SeqCst);
    let close_deadline = Instant::now() + MENU_MAX_OPEN;
    let mut capped = false;
    while crate::capture::window_onscreen(menu, pid) {
        if !capped && Instant::now() > close_deadline {
            capped = true;
            release();
            log::debug!("меню открыто слишком долго — нажатия снова идут, окно {menu} ещё на экране");
        }
        thread::sleep(MENU_POLL);
    }
    WINDOWS_OPEN.fetch_sub(1, Ordering::SeqCst);
    log::debug!("окно меню {menu} закрылось");
}
