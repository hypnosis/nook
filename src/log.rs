//! Диагностический лог в файл, только в сборке с флагом `debug-log`: обычная сборка молчит.
//! Приложение — агент без окна, stdout не виден, поэтому лог смотрят через
//! `tail -f /tmp/nook-debug.log`.

use std::fs::OpenOptions;
use std::io::Write;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const LOG_PATH: &str = "/tmp/nook-debug.log";

/// Дописывает строку в лог-файл с меткой «секунды.миллисекунды» от старта эпохи.
///
/// Намеренно не паникует при ошибке записи: лог — диагностика, его отказ
/// не должен ронять само приложение. Если файл недоступен — молча пропускаем.
pub fn append(message: &str) {
    if !cfg!(feature = "debug-log") {
        return;
    }
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let (seconds, millis) = (now.as_secs(), now.subsec_millis());

    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(LOG_PATH) {
        let _ = writeln!(file, "[{seconds}.{millis:03}] {message}");
    }
}

/// Перезаписывает лог-файл с нуля. Вызывается один раз на старте,
/// чтобы каждый запуск читался отдельно, без хвоста прошлых сессий.
pub fn reset() {
    if !cfg!(feature = "debug-log") {
        return;
    }
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(LOG_PATH)
    {
        let _ = writeln!(file, "=== nook session start ===");
    }
}

// TODO: временный замер иконок у камеры — убрать после разбора.
// HARDCODE: период замера; убрать вместе с замером.
const PROBE_INTERVAL: Duration = Duration::from_millis(300);

/// Пишет в лог строку меню из трёх источников — окна Core Graphics, элементы
/// Accessibility и Preferred Position из настроек приложений — каждый раз, когда хоть
/// один из них изменился. `notch` — края чёлки.
pub fn start_bar_probe(notch: (f64, f64)) {
    if !cfg!(feature = "debug-log") {
        return;
    }
    append(&format!("замер: чёлка {:.0}..{:.0}", notch.0, notch.1));
    std::thread::spawn(|| {
        let mut last = (Vec::new(), Vec::new(), Vec::new());
        loop {
            let now = (
                crate::capture::bar_windows_probe(),
                crate::click::ax_bar_probe(),
                crate::capture::preferred_positions_probe(),
            );
            if now != last {
                append(&format!("замер CG: {}", now.0.join(" | ")));
                append(&format!("замер AX: {}", now.1.join(" | ")));
                append(&format!("замер PP: {}", now.2.join(" | ")));
                last = now;
            }
            std::thread::sleep(PROBE_INTERVAL);
        }
    });
}
