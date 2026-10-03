//! Разрешения Nook: проверка, переход к ним в Системных настройках и строки
//! «название — статус или кнопка» для окна разрешений и раздела настроек.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{sel, MainThreadMarker};
use objc2_app_kit::{
    NSButton, NSGridCellPlacement, NSGridView, NSLayoutAttribute, NSStackView, NSTextField,
    NSUserInterfaceLayoutOrientation, NSView, NSWorkspace,
};
use objc2_application_services::AXIsProcessTrusted;
use objc2_core_graphics::CGPreflightScreenCaptureAccess;
use objc2_foundation::{NSArray, NSString, NSURL, NSUserDefaults};
use std::process::Command;

use crate::strings::{self, Lang};
use crate::ui_style;

const ACCESSIBILITY_PANE: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";
const SCREEN_RECORDING_PANE: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture";
const GRANTED_VERSION_KEY: &str = "permissionsGrantedVersion";
const TCCUTIL: &str = "/usr/bin/tccutil";
/// Совпадает с CFBundleIdentifier в Info.plist.
const BUNDLE_ID: &str = "com.hypnosis.nook";

#[derive(Clone, Copy)]
enum Permission {
    ScreenRecording,
    Accessibility,
}

const ALL: [Permission; 2] = [Permission::ScreenRecording, Permission::Accessibility];

impl Permission {
    fn is_granted(self) -> bool {
        match self {
            Self::ScreenRecording => CGPreflightScreenCaptureAccess(),
            Self::Accessibility => unsafe { AXIsProcessTrusted() },
        }
    }

    /// Имя службы для `tccutil`.
    fn tcc_service(self) -> &'static str {
        match self {
            Self::ScreenRecording => "ScreenCapture",
            Self::Accessibility => "Accessibility",
        }
    }

    /// Действие контроллера, открывающее раздел разрешения в Системных настройках.
    fn open_action(self) -> Sel {
        match self {
            Self::ScreenRecording => sel!(onOpenScreenRecording:),
            Self::Accessibility => sel!(onOpenAccessibility:),
        }
    }

    fn title(self, lang: Lang) -> &'static str {
        match self {
            Self::ScreenRecording => strings::settings_screen_recording(lang),
            Self::Accessibility => strings::settings_accessibility(lang),
        }
    }

    fn detail(self, lang: Lang) -> &'static str {
        match self {
            Self::ScreenRecording => strings::permission_screen_recording_detail(lang),
            Self::Accessibility => strings::permission_accessibility_detail(lang),
        }
    }
}

pub fn all_granted() -> bool {
    ALL.iter().all(|permission| permission.is_granted())
}

/// Все разрешения есть — запоминает версию Nook, которой они выданы.
pub fn remember_if_granted() {
    if !all_granted() {
        return;
    }
    let version = NSString::from_str(env!("CARGO_PKG_VERSION"));
    // SAFETY: строка — допустимое значение для NSUserDefaults.
    unsafe {
        NSUserDefaults::standardUserDefaults()
            .setObject_forKey(Some(&version), &NSString::from_str(GRANTED_VERSION_KEY));
    }
}

/// Разрешения выдавали прошлой версии, а у этой их нет: записи в Системных настройках
/// остались от прежней подписи и включены впустую. Убирает их, чтобы macOS спросила заново.
/// Возвращает, был ли сброс.
pub fn reset_stale_after_update() -> bool {
    let granted_version = NSUserDefaults::standardUserDefaults()
        .stringForKey(&NSString::from_str(GRANTED_VERSION_KEY))
        .map(|version| version.to_string());
    let updated = granted_version.is_some_and(|version| version != env!("CARGO_PKG_VERSION"));
    if !updated || all_granted() {
        return false;
    }
    for permission in ALL.iter().filter(|permission| !permission.is_granted()) {
        let reset = Command::new(TCCUTIL)
            .args(["reset", permission.tcc_service(), BUNDLE_ID])
            .output();
        if !reset.is_ok_and(|output| output.status.success()) {
            crate::log::append(&format!("permissions: сброс {} не удался", permission.tcc_service()));
        }
    }
    true
}

pub fn open_accessibility_pane() {
    open_url(ACCESSIBILITY_PANE);
}

pub fn open_screen_recording_pane() {
    open_url(SCREEN_RECORDING_PANE);
}

fn open_url(url: &str) {
    if let Some(url) = NSURL::URLWithString(&NSString::from_str(url)) {
        NSWorkspace::sharedWorkspace().openURL(&url);
    }
}

/// Строки разрешений: пока разрешения нет — кнопка в Системные настройки,
/// выдано — «✓ Разрешено» на её месте.
pub struct PermissionRows {
    grid: Retained<NSGridView>,
    rows: [(Permission, Retained<NSTextField>, Retained<NSButton>); 2],
}

impl PermissionRows {
    /// `target` — контроллер с `onOpenScreenRecording:` и `onOpenAccessibility:`.
    pub fn new(mtm: MainThreadMarker, target: &AnyObject, lang: Lang) -> Self {
        let rows = ALL.map(|permission| {
            let status = ui_style::label(mtm, strings::settings_granted(lang));
            status.setTextColor(Some(&ui_style::granted_color()));
            let button = ui_style::button(
                mtm,
                strings::permission_open_settings(lang),
                target,
                permission.open_action(),
            );
            (permission, status, button)
        });
        let cells: Vec<(Retained<NSStackView>, Retained<NSStackView>)> = rows
            .iter()
            .map(|(permission, status, button)| {
                let name =
                    ui_style::title_with_detail(mtm, permission.title(lang), permission.detail(lang));
                let state = stack(
                    mtm,
                    &[&**status, &**button],
                    NSUserInterfaceLayoutOrientation::Horizontal,
                );
                (name, state)
            })
            .collect();
        let grid_rows: Vec<[&NSView; 2]> = cells
            .iter()
            .map(|(name, state)| [&**name as &NSView, &**state])
            .collect();
        let grid = ui_style::grid(mtm, &grid_rows);
        grid.setYPlacement(NSGridCellPlacement::Center);
        let this = Self { grid, rows };
        this.refresh();
        this
    }

    pub fn view(&self) -> &NSView {
        &self.grid
    }

    /// Перечитывает разрешения: их могли выдать в Системных настройках.
    pub fn refresh(&self) {
        for (permission, status, button) in &self.rows {
            let granted = permission.is_granted();
            status.setHidden(!granted);
            button.setHidden(granted);
        }
    }
}

fn stack(
    mtm: MainThreadMarker,
    views: &[&NSView],
    orientation: NSUserInterfaceLayoutOrientation,
) -> Retained<NSStackView> {
    let stack = NSStackView::stackViewWithViews(&NSArray::from_slice(views), mtm);
    stack.setOrientation(orientation);
    stack.setAlignment(match orientation {
        NSUserInterfaceLayoutOrientation::Vertical => NSLayoutAttribute::Leading,
        _ => NSLayoutAttribute::CenterY,
    });
    stack
}
