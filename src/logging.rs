//! Лог Nook в системный журнал macOS: подсистема — bundle id, категория — модуль.
//!
//! Пишется только в режиме отладки: `defaults write com.hypnosis.nook debugLog -bool YES`,
//! смотреть `log stream --level debug --predicate 'subsystem == "com.hypnosis.nook"'`.
//! Без него Nook в журнал не пишет ничего.

use log::LevelFilter;
use oslog::OsLogger;

/// Подключает системный журнал; вызывается один раз на старте.
pub fn init() {
    let level = if crate::settings::debug_log() { LevelFilter::Debug } else { LevelFilter::Off };
    if let Err(error) = OsLogger::new(crate::permissions::BUNDLE_ID).level_filter(level).init() {
        eprintln!("nook: системный журнал не подключился: {error}");
    }
}
