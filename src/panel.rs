//! Панель под строкой меню: показывается, пока иконки раскрыты.
//!
//! Рисует иконки, которые модель раскладки отвела панели, в её порядке. Картинки
//! хранятся по иконке отдельно для светлой и тёмной темы: неудачный снимок оставляет
//! прежнюю картинку, смена темы перерисовывает панель сразу. Без картинок панель не
//! показывается.

use std::collections::HashMap;

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

/// Картинка иконки в одной теме; `real` — снята в этой теме, иначе инверсия из другой.
struct Picture {
    image: Retained<NSImage>,
    real: bool,
}

pub struct Panel {
    window: Retained<NSPanel>,
    theme: Theme,
    pictures: [HashMap<u32, Picture>; 2],
    /// Иконки панели слева направо — вид модели раскладки.
    order: Vec<u32>,
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
        Self {
            window,
            theme: theme::current(),
            pictures: [HashMap::new(), HashMap::new()],
            order: Vec::new(),
            pending: None,
        }
    }

    /// Показывает панель под строкой меню, правым краем у окна якоря. Если картинок
    /// ещё нет, панель появится, когда они придут.
    pub fn show_below(&mut self, anchor_window: &NSWindow) {
        let Some(screen) = anchor_window.screen() else { return };
        let right = anchor_window.frame().origin.x + anchor_window.frame().size.width;
        let top = screen.visibleFrame().origin.y + screen.visibleFrame().size.height;
        let bottom = top - PANEL_HEIGHT - PANEL_GAP;
        self.pending = Some((right, bottom));
        self.place_if_ready();
    }

    fn place_if_ready(&mut self) {
        if !self.has_icons() {
            if self.window.isVisible() {
                let frame = self.window.frame();
                self.pending = Some((frame.origin.x + frame.size.width, frame.origin.y));
                self.window.orderOut(None);
            }
            return;
        }
        let Some((right, bottom)) = self.pending.take() else { return };
        let width = self.window.frame().size.width;
        self.window.setFrameOrigin(NSPoint::new(right - width, bottom));
        self.window.orderFrontRegardless();
    }

    /// Иконки панели слева направо. Картинки других иконок больше не нужны.
    pub fn set_order(&mut self, mtm: MainThreadMarker, target: &AnyObject, order: &[u32]) {
        if self.order == order {
            return;
        }
        self.order = order.to_vec();
        for pictures in &mut self.pictures {
            pictures.retain(|id, _| order.contains(id));
        }
        self.render(mtm, target);
        self.place_if_ready();
    }

    /// Свежие снимки для текущей темы; для другой темы — инверсия, если настоящих там нет.
    pub fn set_icons(
        &mut self,
        mtm: MainThreadMarker,
        target: &AnyObject,
        images: &NSArray<NSImage>,
        ids: &NSArray<NSNumber>,
    ) {
        let opposite = self.theme.opposite();
        for (id, image) in ids.iter().map(|id| id.unsignedIntValue()).zip(images.iter()) {
            if !self.order.contains(&id) {
                continue;
            }
            let fitted = theme::fit(&image, self.theme);
            if self.pictures[opposite.index()].get(&id).is_none_or(|picture| !picture.real) {
                let image = theme::fit(&fitted, opposite);
                self.pictures[opposite.index()].insert(id, Picture { image, real: false });
            }
            self.pictures[self.theme.index()].insert(id, Picture { image: fitted, real: true });
        }
        self.render(mtm, target);
        self.place_if_ready();
    }

    /// Есть что показать в текущей теме.
    fn has_icons(&self) -> bool {
        self.order.iter().any(|id| self.pictures[self.theme.index()].contains_key(id))
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

    /// Картинки иконок панели по порядку; иконка без картинки пока не показывается.
    fn visible(&self) -> Vec<(u32, &Retained<NSImage>)> {
        let pictures = &self.pictures[self.theme.index()];
        self.order.iter().filter_map(|id| pictures.get(id).map(|picture| (*id, &picture.image))).collect()
    }

    /// Ставит клоны иконок в ряд и подгоняет ширину, правый край остаётся на месте.
    /// Клик по клону зовёт `onCloneClick:` у target; tag кнопки — номер окна настоящей иконки.
    fn render(&self, mtm: MainThreadMarker, target: &AnyObject) {
        let icons = self.visible();
        if self.update_in_place(&icons) {
            return;
        }
        let content = NSView::new(mtm);
        let mut x = PANEL_PADDING;
        for (id, image) in &icons {
            let size = image.size();
            let view = unsafe {
                NSButton::buttonWithImage_target_action(image, Some(target), Some(sel!(onCloneClick:)), mtm)
            };
            view.setBordered(false);
            view.setRefusesFirstResponder(true);
            view.sendActionOn(NSEventMask::LeftMouseDown);
            view.setTag(*id as isize);
            let y = ((PANEL_HEIGHT - size.height) / 2.0).max(0.0);
            view.setFrame(NSRect::new(NSPoint::new(x, y), size));
            content.addSubview(&view);
            x += size.width + ICON_SPACING;
        }
        let width = if icons.is_empty() { PANEL_WIDTH } else { x - ICON_SPACING + PANEL_PADDING };
        let frame = self.window.frame();
        let right = frame.origin.x + frame.size.width;
        self.window.setContentView(Some(&content));
        self.window.setFrame_display(
            NSRect::new(NSPoint::new(right - width, frame.origin.y), NSSize::new(width, PANEL_HEIGHT)),
            true,
        );
    }

    /// Меняет картинки в тех же кнопках, если набор и размеры иконок прежние:
    /// пересозданная кнопка теряет клик, пришедшийся на обновление.
    fn update_in_place(&self, icons: &[(u32, &Retained<NSImage>)]) -> bool {
        let Some(content) = self.window.contentView() else { return false };
        let buttons: Vec<Retained<NSButton>> =
            content.subviews().iter().filter_map(|view| view.downcast::<NSButton>().ok()).collect();
        let unchanged = buttons.len() == icons.len()
            && buttons
                .iter()
                .zip(icons)
                .all(|(button, (id, image))| button.tag() as u32 == *id && button.frame().size == image.size());
        if !unchanged {
            return false;
        }
        for (button, (_, image)) in buttons.iter().zip(icons) {
            button.setImage(Some(image));
        }
        true
    }

    pub fn hide(&mut self) {
        self.pending = None;
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
