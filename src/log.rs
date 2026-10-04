//! Диагностический лог в файл, только в сборке с флагом `debug-log`: обычная сборка молчит.
//! Приложение — агент без окна, stdout не виден, поэтому лог смотрят через
//! `tail -f /tmp/nook-debug.log`.

use std::fs::OpenOptions;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

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
