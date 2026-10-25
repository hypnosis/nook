//! Шторка: крышка со снимком пустой полосы строки меню поверх иконок панели. Пока строка
//! раскрыта, иконки панели стоят у чёлки под ней: их не видно, и мышь до них не доходит.
//! Клик по крышке — как по пустой строке меню, Nook при этом не становится активным.

use std::sync::{Mutex, PoisonError};
use std::thread;

use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{msg_send, AnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSColor, NSImage, NSImageView, NSPanel, NSScreen, NSStatusWindowLevel,
    NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_core_foundation::{CFRetained, CGPoint, CGRect, CGSize};
use objc2_core_graphics::CGImage;
use objc2_foundation::{NSPoint, NSRect, NSSize};

/// Картинка пустой полосы, её место и поколение строки, для которого она снята.
type Picture = (CFRetained<CGImage>, CGRect, u64);

static PICTURE: Mutex<Option<Picture>> = Mutex::new(None);

pub struct Shroud {
    window: Retained<NSPanel>,
    /// Полоса, которую шторка закрывает сейчас или закрывала последней, в координатах CG.
    rect: Option<CGRect>,
    /// Картинка могла устареть: шторка показана заново или сменилась тема.
    stale: bool,
}

impl Shroud {
    /// Окно без рамки: окна с заголовком macOS не пускает в полосу строки меню.
    pub fn new(mtm: MainThreadMarker) -> Self {
        let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1.0, 1.0));
        let window = NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(mtm),
            rect,
            NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
            NSBackingStoreType::Buffered,
            false,
        );
        unsafe { window.setReleasedWhenClosed(false) };
        window.setLevel(NSStatusWindowLevel + 1);
        window.setBecomesKeyOnlyIfNeeded(true);
        window.setHidesOnDeactivate(false);
        window.setOpaque(false);
        window.setBackgroundColor(Some(&NSColor::clearColor()));
        window.setHasShadow(false);
        window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        Self { window, rect: None, stale: true }
    }

    /// Показывает `image` в прямоугольнике `rect` — координаты CG, от левого верхнего
    /// угла основного экрана, как у окон иконок.
    pub fn show(&mut self, mtm: MainThreadMarker, image: &CGImage, rect: CGRect) {
        // Начало координат Cocoa — низ основного экрана (первого в списке), а не экрана с активным окном.
        let Some(primary) = NSScreen::screens(mtm).firstObject() else {
            return;
        };
        let top = primary.frame().origin.y + primary.frame().size.height;
        let size = NSSize::new(rect.size.width, rect.size.height);
        let frame = NSRect::new(NSPoint::new(rect.origin.x, top - rect.origin.y - rect.size.height), size);
        let picture = NSImage::initWithCGImage_size(NSImage::alloc(), image, size);
        self.window.setContentView(Some(&NSImageView::imageViewWithImage(&picture, mtm)));
        self.window.setFrame_display(frame, true);
        self.window.orderFrontRegardless();
        self.rect = Some(rect);
        self.stale = false;
    }

    /// Снова показывает прошлую картинку на прошлом месте; следующее чтение её переснимет.
    pub fn show_again(&mut self) {
        if self.rect.is_some() {
            self.window.orderFrontRegardless();
            self.stale = true;
        }
    }

    /// Картинку пора переснять: сменилась тема строки.
    pub fn mark_stale(&mut self) {
        self.stale = true;
    }

    /// Шторке нужна новая картинка для полосы `left..right`: полоса другая или картинка устарела.
    pub fn needs_shot(&self, left: f64, right: f64) -> bool {
        self.stale
            || self.rect.is_none_or(|rect| {
                (rect.origin.x - left).abs() > 0.5 || (rect.origin.x + rect.size.width - right).abs() > 0.5
            })
    }

    pub fn hide(&self) {
        self.window.orderOut(None);
    }

    /// Шторка сейчас на экране.
    pub fn is_shown(&self) -> bool {
        self.window.isVisible()
    }

    /// Полоса последнего показа: по ней шторка встаёт при следующем раскрытии.
    pub fn rect(&self) -> Option<CGRect> {
        self.rect
    }

    /// Номер окна шторки: снимки полосы её исключают.
    pub fn window_id(&self) -> u32 {
        self.window.windowNumber() as u32
    }
}

/// Снимок полосы `left..right` во всю высоту строки без окон строки меню и самой шторки
/// `shroud_id`: пустая строка. Картинка и её прямоугольник в координатах CG.
fn shoot(left: f64, right: f64, shroud_id: u32) -> Option<(CFRetained<CGImage>, CGRect)> {
    let layout = crate::capture::icon_layout();
    let height = layout.first().map(|window| window.height).filter(|_| right > left)?;
    let rect = CGRect::new(CGPoint::new(left, 0.0), CGSize::new(right - left, height));
    let mut hidden: Vec<u32> = layout.iter().map(|window| window.id).collect();
    hidden.push(shroud_id);
    crate::capture::screen_rect(rect, &hidden).map(|image| (image, rect))
}

/// Снимает полосу сразу, блокируя поток: перед раскрытием и сужением, пока картинки нет.
pub fn shoot_now(left: f64, right: f64, shroud_id: u32) -> Option<(CFRetained<CGImage>, CGRect)> {
    shoot(left, right, shroud_id)
}

/// Снимает полосу в фоне и отдаёт картинку делегату через `onShroudPicture`.
pub fn shoot_later(left: f64, right: f64, shroud_id: u32, epoch: u64) {
    thread::spawn(move || {
        let Some((image, rect)) = shoot(left, right, shroud_id) else {
            log::debug!("шторка: снимка полосы {left}..{right} нет");
            return;
        };
        *PICTURE.lock().unwrap_or_else(PoisonError::into_inner) = Some((image, rect, epoch));
        notify(Event::Picture);
    });
}

/// Последняя снятая картинка; забирается один раз.
pub fn take_picture() -> Option<Picture> {
    PICTURE.lock().unwrap_or_else(PoisonError::into_inner).take()
}

/// Ждёт, пока строка встанет после сворачивания, и зовёт `onCollapseSettled` у делегата.
pub fn hide_when_still() {
    thread::spawn(|| {
        crate::mover::wait_until_still();
        notify(Event::CollapseSettled);
    });
}

/// О чём шторка сообщает делегату.
#[derive(Clone, Copy)]
enum Event {
    Picture,
    CollapseSettled,
}

fn notify(event: Event) {
    DispatchQueue::main().exec_async(move || {
        let mtm = MainThreadMarker::new().expect("main queue");
        if let Some(delegate) = NSApplication::sharedApplication(mtm).delegate() {
            let delegate: &AnyObject = delegate.as_ref();
            match event {
                Event::Picture => unsafe { msg_send![delegate, onShroudPicture] },
                Event::CollapseSettled => unsafe { msg_send![delegate, onCollapseSettled] },
            }
        }
    });
}
