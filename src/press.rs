//! Нажатие на копию в панели: настоящая иконка нажимается там, где она спрятана,
//! строка меню не меняется. Пока меню иконки открыто, строка не сворачивается.

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Instant;

use crate::tuning::{MENU_APPEAR_TIMEOUT, MENU_MAX_OPEN, MENU_POLL};

static MENU_OPEN: AtomicBool = AtomicBool::new(false);

/// Меню иконки, нажатой из панели, ещё открыто.
pub fn menu_open() -> bool {
    MENU_OPEN.load(Ordering::SeqCst)
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
        wait_menu_closed(pid, &windows_before, clicked);
        MENU_OPEN.store(false, Ordering::SeqCst);
    });
}

/// Ждёт, пока у приложения `pid` появится новое окно (меню или поповер) и закроется.
fn wait_menu_closed(pid: Option<i32>, windows_before: &[u32], clicked: Instant) {
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
    log::debug!("клик → меню {} мс", clicked.elapsed().as_millis());
    let close_deadline = Instant::now() + MENU_MAX_OPEN;
    while crate::capture::onscreen_windows_of(pid).contains(&menu) {
        if Instant::now() > close_deadline {
            log::debug!("меню открыто слишком долго — больше не жду");
            return;
        }
        thread::sleep(MENU_POLL);
    }
}
