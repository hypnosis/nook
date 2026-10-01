//! Панель под строкой меню: показывается, пока иконки раскрыты.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSButton, NSColor, NSEvent, NSImage, NSPanel, NSStatusWindowLevel,
    NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSArray, NSNumber, NSPoint, NSRect, NSSize};

// HARDCODE: размеры панели; вынести в конфиг позже.
const PANEL_WIDTH: f64 = 240.0;
const PANEL_HEIGHT: f64 = 32.0;
const PANEL_GAP: f64 = 4.0;
const ICON_SPACING: f64 = 4.0;
const PANEL_PADDING: f64 = 8.0;

pub struct Panel {
    window: Retained<NSPanel>,
}

impl Panel {
    pub fn new(mtm: MainThreadMarker) -> Self {
        let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(PANEL_WIDTH, PANEL_HEIGHT));
        let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel;
        let window: Retained<NSPanel> = unsafe {
            msg_send![
                NSPanel::alloc(mtm),
                initWithContentRect: rect,
                styleMask: style,
                backing: NSBackingStoreType::Buffered,
                defer: false
            ]
        };
        unsafe { window.setReleasedWhenClosed(false) };
        window.setLevel(NSStatusWindowLevel);
        window.setBackgroundColor(Some(&NSColor::colorWithWhite_alpha(0.12, 0.95)));
        window.setHasShadow(true);
        Self { window }
    }

    /// Показывает панель под строкой меню, правым краем у окна якоря.
    pub fn show_below(&self, anchor_window: &NSWindow) {
        let Some(screen) = anchor_window.screen() else { return };
        let right = anchor_window.frame().origin.x + anchor_window.frame().size.width;
        let top = screen.visibleFrame().origin.y + screen.visibleFrame().size.height;
        let width = self.window.frame().size.width;
        let origin = NSPoint::new(right - width, top - PANEL_HEIGHT - PANEL_GAP);
        self.window.setFrameOrigin(origin);
        self.window.orderFrontRegardless();
        crate::log::append(&format!("панель: показана x={} y={}", origin.x, origin.y));
    }

    /// Ставит клоны иконок в ряд и подгоняет ширину, правый край остаётся на месте.
    /// Клик по клону зовёт `onCloneClick:` у target; tag кнопки — номер окна настоящей иконки.
    pub fn set_icons(
        &self,
        mtm: MainThreadMarker,
        target: &AnyObject,
        images: &NSArray<NSImage>,
        ids: &NSArray<NSNumber>,
    ) {
        let content = NSView::new(mtm);
        let mut x = PANEL_PADDING;
        for (image, id) in images.iter().zip(ids.iter()) {
            let size = image.size();
            let view = unsafe {
                NSButton::buttonWithImage_target_action(
                    &image,
                    Some(target),
                    Some(sel!(onCloneClick:)),
                    mtm,
                )
            };
            view.setBordered(false);
            view.setTag(id.unsignedIntValue() as isize);
            let y = ((PANEL_HEIGHT - size.height) / 2.0).max(0.0);
            view.setFrame(NSRect::new(NSPoint::new(x, y), size));
            content.addSubview(&view);
            x += size.width + ICON_SPACING;
        }
        let width = if images.count() == 0 { PANEL_WIDTH } else { x - ICON_SPACING + PANEL_PADDING };
        let frame = self.window.frame();
        let right = frame.origin.x + frame.size.width;
        self.window.setContentView(Some(&content));
        self.window.setFrame_display(
            NSRect::new(
                NSPoint::new(right - width, frame.origin.y),
                NSSize::new(width, PANEL_HEIGHT),
            ),
            true,
        );
        crate::log::append(&format!("панель: иконок {} ширина {width}", images.count()));
    }

    pub fn hide(&self) {
        self.window.orderOut(None);
    }

    /// Мышь над видимой панелью — для автосворачивания это как мышь на строке меню.
    pub fn contains_mouse(&self) -> bool {
        if !self.window.isVisible() {
            return false;
        }
        let mouse = NSEvent::mouseLocation();
        let frame = self.window.frame();
        mouse.x >= frame.origin.x
            && mouse.x <= frame.origin.x + frame.size.width
            && mouse.y >= frame.origin.y
            && mouse.y <= frame.origin.y + frame.size.height
    }
}
