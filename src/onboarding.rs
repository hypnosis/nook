//! Окно разрешений: показывается при запуске, пока хотя бы одного разрешения нет.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{msg_send, sel, MainThreadMarker};
use objc2_app_kit::{
    NSApplication, NSButton, NSLayoutAttribute, NSStackView, NSStackViewGravity,
    NSUserInterfaceLayoutOrientation, NSView, NSViewController, NSWindow, NSWindowStyleMask,
    NSWindowTitleVisibility,
};
use objc2_foundation::{NSArray, NSEdgeInsets, NSSize, NSString};

use crate::permissions::PermissionRows;
use crate::strings::{self, Lang};
use crate::ui_style::{self, label, wrapping_label, SPACING};

// HARDCODE: размеры окна разрешений; вынести в конфиг позже.
const CONTENT_WIDTH: f64 = 440.0;
const INSET: f64 = 20.0;
/// Return нажимает кнопку по умолчанию.
const DEFAULT_KEY: &str = "\r";

pub struct Onboarding {
    window: Retained<NSWindow>,
    permissions: PermissionRows,
}

impl Onboarding {
    /// `target` — контроллер: `onOpenScreenRecording:`, `onOpenAccessibility:`.
    pub fn new(mtm: MainThreadMarker, target: &AnyObject, lang: Lang) -> Self {
        let title = label(mtm, strings::onboarding_title(lang));
        title.setFont(Some(&ui_style::section_font()));
        let text = wrapping_label(mtm, strings::onboarding_text(lang));
        text.setPreferredMaxLayoutWidth(CONTENT_WIDTH);
        let permissions = PermissionRows::new(mtm, target, lang);
        permissions
            .view()
            .widthAnchor()
            .constraintLessThanOrEqualToConstant(CONTENT_WIDTH)
            .setActive(true);

        // Без target кнопка шлёт performClose: по цепочке ответчиков — до своего окна.
        let done = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str(strings::onboarding_done(lang)),
                None,
                Some(sel!(performClose:)),
                mtm,
            )
        };
        done.setKeyEquivalent(&NSString::from_str(DEFAULT_KEY));
        let footer = NSStackView::new(mtm);
        footer.addView_inGravity(&done, NSStackViewGravity::Trailing);
        footer
            .widthAnchor()
            .constraintEqualToConstant(CONTENT_WIDTH)
            .setActive(true);

        let content = NSStackView::stackViewWithViews(
            &NSArray::from_slice(&[&*title as &NSView, &*text, permissions.view(), &*footer]),
            mtm,
        );
        content.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        content.setAlignment(NSLayoutAttribute::Leading);
        content.setSpacing(SPACING);
        content.setEdgeInsets(NSEdgeInsets {
            top: INSET,
            left: INSET,
            bottom: INSET,
            right: INSET,
        });

        let controller = NSViewController::new(mtm);
        controller.setView(&content);
        let window = NSWindow::windowWithContentViewController(&controller);
        window.setStyleMask(NSWindowStyleMask::Titled | NSWindowStyleMask::Closable);
        window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
        unsafe { window.setReleasedWhenClosed(false) };
        // Высота подписей с переносом известна только после раскладки по ширине.
        content.layoutSubtreeIfNeeded();
        let size: NSSize = unsafe { msg_send![&*content, fittingSize] };
        window.setContentSize(size);

        Self {
            window,
            permissions,
        }
    }

    pub fn window(&self) -> &NSWindow {
        &self.window
    }

    pub fn show(&self, mtm: MainThreadMarker) {
        self.permissions.refresh();
        NSApplication::sharedApplication(mtm).activate();
        self.window.center();
        self.window.makeKeyAndOrderFront(None);
    }

    /// Перечитывает разрешения: их могли выдать в Системных настройках.
    pub fn refresh(&self) {
        self.permissions.refresh();
    }
}
