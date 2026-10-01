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

/// Пункт меню «Запускать при входе» (тумблер автозапуска).
pub fn menu_login(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Open at Login",
        Lang::Ru => "Запускать при входе",
    }
}

/// Пункт меню «Настройки…».
pub fn menu_settings(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Settings…",
        Lang::Ru => "Настройки…",
    }
}

/// Заголовок окна настроек.
pub fn settings_title(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Nook Settings",
        Lang::Ru => "Настройки Nook",
    }
}

/// Раздел «Разрешения».
pub fn settings_permissions(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Permissions",
        Lang::Ru => "Разрешения",
    }
}

pub fn settings_accessibility(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Accessibility",
        Lang::Ru => "Универсальный доступ",
    }
}

pub fn settings_screen_recording(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Screen Recording",
        Lang::Ru => "Запись экрана",
    }
}

pub fn settings_granted(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "✓ Allowed",
        Lang::Ru => "✓ Разрешено",
    }
}

pub fn settings_denied(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Not allowed",
        Lang::Ru => "Нет доступа",
    }
}

/// Кнопка перехода в Системные настройки, когда разрешения нет.
pub fn settings_allow(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Allow…",
        Lang::Ru => "Разрешить…",
    }
}

/// Разделы боковой панели настроек.
pub fn settings_general(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "General",
        Lang::Ru => "Основные",
    }
}

pub fn settings_layout(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Layout",
        Lang::Ru => "Расположение",
    }
}

pub fn settings_show_panel(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Show panel",
        Lang::Ru => "Показывать панель",
    }
}

/// Ряды редактора.
pub fn editor_main(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Menu bar",
        Lang::Ru => "Основной ряд",
    }
}

pub fn editor_panel(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Panel",
        Lang::Ru => "Панель",
    }
}

pub fn editor_apply(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Apply",
        Lang::Ru => "Применить",
    }
}

pub fn editor_applying(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Applying changes…",
        Lang::Ru => "Применяю изменения…",
    }
}

pub fn editor_hint(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Drag icons between rows.",
        Lang::Ru => "Перетаскивайте иконки между рядами.",
    }
}

pub fn settings_version(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Version",
        Lang::Ru => "Версия",
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
