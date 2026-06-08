//! Автозапуск при входе в систему через `SMAppService.mainApp` (нативный API
//! macOS 13+, штатный путь на Tahoe). Без login-хелперов и LaunchAgent-плистов —
//! система сама регистрирует текущий `.app` как login item.
//!
//! Тонкий фасад над `unsafe` ServiceManagement: контроллер видит только
//! `is_enabled()` и `toggle()`, не зная деталей API.
//!
//! # ВАЖНО (см. ADR 008 и 009)
//! `mainAppService()` регистрирует путь к ТОМУ `.app`, из которого запущен
//! процесс. Включать автозапуск нужно только когда Nook запущен из
//! `/Applications/Nook.app` — иначе в LoginItems попадёт dev-путь (`target/debug`),
//! что засоряет ControlCenter ad-hoc-записями и ломает размещение якоря (x=0).

use objc2_service_management::{SMAppService, SMAppServiceStatus};

/// Включён ли автозапуск сейчас.
///
/// `true` при `Enabled` (готов к запуску) и при `RequiresApproval` (пользователь
/// включил, но macOS ждёт подтверждения в Системных настройках) — в обоих случаях
/// пользователь намеренно включил автозапуск, и галочка в меню должна стоять.
pub fn is_enabled() -> bool {
    // SAFETY: mainAppService/status — корректные вызовы ServiceManagement,
    // объект живёт в пределах функции, главный поток не требуется для чтения статуса.
    let status = unsafe { SMAppService::mainAppService().status() };
    status == SMAppServiceStatus::Enabled || status == SMAppServiceStatus::RequiresApproval
}

/// Переключает автозапуск: выключенный — регистрирует, включённый — снимает.
///
/// Ошибка регистрации — внешняя нестабильность (ServiceManagement отказал):
/// логируем с контекстом и не паникуем, приложение продолжает работать.
/// Путь bundle пишем в лог, чтобы при разборе было видно, что зарегистрировали.
pub fn toggle() {
    let service = unsafe { SMAppService::mainAppService() };

    // SAFETY: вызовы register/unregister на валидном объекте service.
    let result = unsafe {
        if is_enabled() {
            service.unregisterAndReturnError().map(|_| "автозапуск выключен")
        } else {
            service.registerAndReturnError().map(|_| "автозапуск включён")
        }
    };

    match result {
        Ok(action) => crate::log::append(action),
        Err(error) => crate::log::append(&format!("автозапуск: ошибка — {error:?}")),
    }
}
