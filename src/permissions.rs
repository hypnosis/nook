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
use objc2_foundation::{NSArray, NSString, NSURL};

use crate::strings::{self, Lang};
use crate::ui_style;

const ACCESSIBILITY_PANE: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";
const SCREEN_RECORDING_PANE: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture";
/// Зазор между названием разрешения и пояснением под ним.
// HARDCODE: зазор строки разрешения; вынести в конфиг позже.
const DETAIL_GAP: f64 = 2.0;

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
                let name = stack(
                    mtm,
                    &[
                        &*ui_style::wrapping_label(mtm, permission.title(lang)),
                        &*detail_label(mtm, permission.detail(lang)),
                    ],
                    NSUserInterfaceLayoutOrientation::Vertical,
                );
                name.setSpacing(DETAIL_GAP);
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

fn detail_label(mtm: MainThreadMarker, text: &str) -> Retained<NSTextField> {
    let detail = ui_style::wrapping_label(mtm, text);
    detail.setFont(Some(&ui_style::footer_font()));
    detail.setTextColor(Some(&ui_style::secondary_label_color()));
    detail
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
