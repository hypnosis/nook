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

/// Кнопка перехода в Системные настройки, когда разрешения нет.
pub fn settings_reset_permissions(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Reset permissions",
        Lang::Ru => "Сбросить разрешения",
    }
}

pub fn settings_reset_permissions_detail(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Removes Nook from System Settings and restarts it. Grant the permissions again after the restart.",
        Lang::Ru => "Убирает Nook из Системных настроек и перезапускает его. После перезапуска выдайте разрешения заново.",
    }
}

pub fn settings_reset_button(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Reset",
        Lang::Ru => "Сбросить",
    }
}

pub fn settings_permissions_stale(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Permissions were granted before but don't work now — this happens after an update. Reset them and grant again.",
        Lang::Ru => "Разрешения выдавались раньше, но сейчас не действуют — так бывает после обновления. Сбросьте их и выдайте заново.",
    }
}

pub fn permission_open_settings(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Open System Settings",
        Lang::Ru => "Открыть настройки",
    }
}

pub fn permission_screen_recording_detail(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Shows hidden icons in the panel. Only the menu bar is captured.",
        Lang::Ru => "Показывает спрятанные иконки в панели. Снимается только строка меню.",
    }
}

pub fn permission_accessibility_detail(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Opens icon menus when you click them in the panel.",
        Lang::Ru => "Открывает меню иконок по клику в панели.",
    }
}

/// Окно разрешений при запуске.
pub fn onboarding_title(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Nook needs two permissions",
        Lang::Ru => "Nook нужны два разрешения",
    }
}

pub fn onboarding_text(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Hiding icons works without them, but the panel can't show or click hidden icons.",
        Lang::Ru => "Без них скрытие иконок работает, но панель не сможет показывать и нажимать спрятанные иконки.",
    }
}

pub fn onboarding_done(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Done",
        Lang::Ru => "Готово",
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
        Lang::En => "Show extra panel below the menu bar",
        Lang::Ru => "Показывать дополнительную панель под строкой меню",
    }
}

pub fn settings_automatic_layout(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Automatic layout",
        Lang::Ru => "Автоматическое расположение",
    }
}

pub fn settings_automatic_layout_detail(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "The panel shows icons that don't fit in the menu bar.",
        Lang::Ru => "В панели отображаются иконки, которым не хватило места в строке меню.",
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

pub fn menu_hide_app(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Hide Nook",
        Lang::Ru => "Скрыть Nook",
    }
}

pub fn menu_quit_app(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Quit Nook",
        Lang::Ru => "Завершить Nook",
    }
}

pub fn menu_file(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "File",
        Lang::Ru => "Файл",
    }
}

pub fn menu_close_window(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Close Window",
        Lang::Ru => "Закрыть окно",
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
