//! Панель под строкой меню: показывается, пока иконки раскрыты.
//!
//! Снимки иконок хранятся отдельно для светлой и тёмной темы, поэтому смена темы
//! перерисовывает панель сразу. Пустой панель не показывается: она ждёт снимков,
//! а без иконок не появляется вовсе.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSButton, NSEvent, NSEventMask, NSImage, NSPanel, NSStatusWindowLevel, NSView,
    NSWindow, NSWindowButton, NSWindowStyleMask, NSWindowTitleVisibility,
};
use objc2_foundation::{NSArray, NSNumber, NSPoint, NSRect, NSSize};

use crate::theme::{self, Theme};

// HARDCODE: размеры панели; вынести в конфиг позже.
const PANEL_WIDTH: f64 = 240.0;
const PANEL_HEIGHT: f64 = 32.0;
const PANEL_GAP: f64 = 4.0;
const ICON_SPACING: f64 = 4.0;
const PANEL_PADDING: f64 = 8.0;

/// Снимки иконок для одной темы.
struct Icons {
    ids: Vec<u32>,
    images: Vec<Retained<NSImage>>,
    /// Снято в этой теме; иначе это инверсия снимков из другой темы.
    real: bool,
}

pub struct Panel {
    window: Retained<NSPanel>,
    theme: Theme,
    cache: [Option<Icons>; 2],
    /// Куда поставить панель, когда придут снимки: правый край и низ.
    pending: Option<(f64, f64)>,
}

impl Panel {
    pub fn new(mtm: MainThreadMarker) -> Self {
        let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(PANEL_WIDTH, PANEL_HEIGHT));
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::FullSizeContentView
            | NSWindowStyleMask::NonactivatingPanel;
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
        window.setBecomesKeyOnlyIfNeeded(true);
        window.setTitlebarAppearsTransparent(true);
        window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
        for button in [
            NSWindowButton::CloseButton,
            NSWindowButton::MiniaturizeButton,
            NSWindowButton::ZoomButton,
        ] {
            if let Some(button) = window.standardWindowButton(button) {
                button.setHidden(true);
            }
        }
        window.setMovable(false);
        window.setHasShadow(true);
        Self { window, theme: theme::current(), cache: [None, None], pending: None }
    }

    /// Показывает панель под строкой меню, правым краем у окна якоря. Если снимков
    /// для текущей темы ещё нет, панель появится, когда они придут.
    pub fn show_below(&mut self, anchor_window: &NSWindow) {
        let Some(screen) = anchor_window.screen() else { return };
        let right = anchor_window.frame().origin.x + anchor_window.frame().size.width;
        let top = screen.visibleFrame().origin.y + screen.visibleFrame().size.height;
        let bottom = top - PANEL_HEIGHT - PANEL_GAP;
        if !self.has_icons() {
            self.pending = Some((right, bottom));
            return;
        }
        self.place(right, bottom);
    }

    fn place(&self, right: f64, bottom: f64) {
        let width = self.window.frame().size.width;
        let origin = NSPoint::new(right - width, bottom);
        self.window.setFrameOrigin(origin);
        self.window.orderFrontRegardless();
    }

    /// Запоминает снимки для текущей темы и перерисовывает панель. Для другой темы
    /// готовит инверсию, если настоящих снимков той темы нет или набор иконок сменился.
    pub fn set_icons(
        &mut self,
        mtm: MainThreadMarker,
        target: &AnyObject,
        images: &NSArray<NSImage>,
        ids: &NSArray<NSNumber>,
    ) {
        let ids: Vec<u32> = ids.iter().map(|id| id.unsignedIntValue()).collect();
        let fitted: Vec<Retained<NSImage>> =
            images.iter().map(|image| theme::fit(&image, self.theme)).collect();
        let opposite = self.theme.opposite();
        let stale = self.cache[opposite.index()].as_ref().is_none_or(|icons| !icons.real || icons.ids != ids);
        if stale {
            self.cache[opposite.index()] = Some(Icons {
                ids: ids.clone(),
                images: fitted.iter().map(|image| theme::fit(image, opposite)).collect(),
                real: false,
            });
        }
        self.cache[self.theme.index()] = Some(Icons { ids, images: fitted, real: true });
        self.render(mtm, target);
        if self.has_icons() {
            if let Some((right, bottom)) = self.pending.take() {
                self.place(right, bottom);
            }
        } else if self.window.isVisible() {
            let frame = self.window.frame();
            self.pending = Some((frame.origin.x + frame.size.width, frame.origin.y));
            self.window.orderOut(None);
        }
    }

    /// Есть что показать в текущей теме.
    fn has_icons(&self) -> bool {
        self.cache[self.theme.index()].as_ref().is_some_and(|icons| !icons.images.is_empty())
    }

    /// Меняет иконки под новую тему из кэша, без пересъёмки. Фон и скругление
    /// рисует сама macOS как у обычного окна.
    pub fn set_theme(&mut self, mtm: MainThreadMarker, target: &AnyObject, theme: Theme) {
        if theme == self.theme {
            return;
        }
        self.theme = theme;
        self.render(mtm, target);
    }

    /// Ставит клоны иконок в ряд и подгоняет ширину, правый край остаётся на месте.
    /// Клик по клону зовёт `onCloneClick:` у target; tag кнопки — номер окна настоящей иконки.
    fn render(&self, mtm: MainThreadMarker, target: &AnyObject) {
        let Some(icons) = self.cache[self.theme.index()].as_ref() else { return };
        if self.update_in_place(icons) {
            return;
        }
        let content = NSView::new(mtm);
        let mut x = PANEL_PADDING;
        for (image, &id) in icons.images.iter().zip(&icons.ids) {
            let size = image.size();
            let view = unsafe {
                NSButton::buttonWithImage_target_action(
                    image,
                    Some(target),
                    Some(sel!(onCloneClick:)),
                    mtm,
                )
            };
            view.setBordered(false);
            view.setRefusesFirstResponder(true);
            view.sendActionOn(NSEventMask::LeftMouseDown);
            view.setTag(id as isize);
            let y = ((PANEL_HEIGHT - size.height) / 2.0).max(0.0);
            view.setFrame(NSRect::new(NSPoint::new(x, y), size));
            content.addSubview(&view);
            x += size.width + ICON_SPACING;
        }
        let width = if icons.images.is_empty() { PANEL_WIDTH } else { x - ICON_SPACING + PANEL_PADDING };
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
    }

    /// Меняет картинки в тех же кнопках, если набор и размеры иконок прежние:
    /// пересозданная кнопка теряет клик, пришедшийся на обновление.
    fn update_in_place(&self, icons: &Icons) -> bool {
        let Some(content) = self.window.contentView() else { return false };
        let buttons: Vec<Retained<NSButton>> =
            content.subviews().iter().filter_map(|view| view.downcast::<NSButton>().ok()).collect();
        let unchanged = buttons.len() == icons.ids.len()
            && buttons
                .iter()
                .zip(&icons.ids)
                .zip(&icons.images)
                .all(|((button, &id), image)| button.tag() as u32 == id && button.frame().size == image.size());
        if !unchanged {
            return false;
        }
        for (button, image) in buttons.iter().zip(&icons.images) {
            button.setImage(Some(image));
        }
        true
    }

    pub fn hide(&mut self) {
        self.pending = None;
        self.window.orderOut(None);
    }

    pub fn is_visible(&self) -> bool {
        self.window.isVisible()
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
