//! Шторка: окно со снимком участка строки меню поверх самой строки. Пока шторка
//! висит, перестановки иконок под ней не видны.

use objc2::rc::Retained;
use objc2::{AnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSImage, NSImageView, NSScreen, NSStatusWindowLevel, NSWindow,
    NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_core_foundation::CGRect;
use objc2_core_graphics::CGImage;
use objc2_foundation::{NSPoint, NSRect, NSSize};

pub struct Shroud {
    window: Retained<NSWindow>,
}

impl Shroud {
    /// Окно без рамки: окна с заголовком macOS не пускает в полосу строки меню.
    pub fn new(mtm: MainThreadMarker) -> Self {
        let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1.0, 1.0));
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                rect,
                NSWindowStyleMask::Borderless,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe { window.setReleasedWhenClosed(false) };
        window.setLevel(NSStatusWindowLevel + 1);
        window.setIgnoresMouseEvents(true);
        window.setOpaque(false);
        window.setBackgroundColor(Some(&NSColor::clearColor()));
        window.setHasShadow(false);
        window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        Self { window }
    }

    /// Показывает `image` в прямоугольнике `rect` — координаты CG, от левого верхнего
    /// угла основного экрана, как у окон иконок.
    pub fn show(&self, mtm: MainThreadMarker, image: &CGImage, rect: CGRect) {
        // Начало координат Cocoa — низ основного экрана (первого в списке), а не экрана с активным окном.
        let Some(primary) = NSScreen::screens(mtm).firstObject() else { return };
        let top = primary.frame().origin.y + primary.frame().size.height;
        let size = NSSize::new(rect.size.width, rect.size.height);
        let frame = NSRect::new(NSPoint::new(rect.origin.x, top - rect.origin.y - rect.size.height), size);
        let picture = NSImage::initWithCGImage_size(NSImage::alloc(), image, size);
        self.window.setContentView(Some(&NSImageView::imageViewWithImage(&picture, mtm)));
        self.window.setFrame_display(frame, true);
        self.window.orderFrontRegardless();
    }

    pub fn hide(&self) {
        self.window.orderOut(None);
    }
}
