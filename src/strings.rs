//! Локализация интерфейса: английский и русский.
//!
//! Язык определяется один раз по системным настройкам (`preferredLanguages`).
//! Без тяжёлых фреймворков — маленькая таблица строк. Если язык системы русский,
//! отдаём русские строки, иначе английские (дефолт).

use objc2_foundation::NSLocale;

/// Язык интерфейса.
#[derive(Clone, Copy, PartialEq)]
pub enum Lang {
    En,
    Ru,
}

/// Определяет язык по первому предпочитаемому языку системы.
/// `ru*` → русский, всё остальное → английский.
pub fn detect() -> Lang {
    let preferred = NSLocale::preferredLanguages();
    if let Some(first) = preferred.iter().next() {
        if first.to_string().to_lowercase().starts_with("ru") {
            return Lang::Ru;
        }
    }
    Lang::En
}

/// Пункт меню «О программе».
pub fn menu_about(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "About clearbar",
        Lang::Ru => "О clearbar",
    }
}

/// Пункт меню «Выход».
pub fn menu_quit(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Quit",
        Lang::Ru => "Выход",
    }
}

/// Тултип якоря: что делать при ошибке порядка.
pub fn anchor_tooltip(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "The anchor must sit after the cutter (cutter on its left). Fix with Cmd+drag.",
        Lang::Ru => "Якорь должен идти за cutter (cutter слева от него). Поправь Cmd+drag.",
    }
}

/// Тултип cutter-разделителя.
pub fn cutter_tooltip(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Cutter — boundary of the hide zone",
        Lang::Ru => "Cutter — граница зоны скрытия",
    }
}
