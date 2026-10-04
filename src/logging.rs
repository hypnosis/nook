//! Лог Nook в системный журнал macOS: подсистема — bundle id, категория — модуль.
//!
//! По умолчанию пишутся сбои (`warn!`) и ключевые события (`info!`). Подробности
//! (`debug!`) включает `defaults write com.hypnosis.nook debugLog -bool YES`, смотреть:
//! `log stream --level info --predicate 'subsystem == "com.hypnosis.nook"'`.

use log::LevelFilter;
use oslog::OsLogger;

/// Подключает системный журнал; вызывается один раз на старте.
pub fn init() {
    let level = if crate::settings::debug_log() { LevelFilter::Debug } else { LevelFilter::Info };
    if let Err(error) = OsLogger::new(crate::permissions::BUNDLE_ID).level_filter(level).init() {
        eprintln!("nook: системный журнал не подключился: {error}");
    }
}
