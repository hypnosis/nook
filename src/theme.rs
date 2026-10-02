//! Тема macOS (светлая или тёмная) и подгонка снимков иконок под неё:
//! в светлой теме иконки тёмные, в тёмной — светлые.

use std::ffi::c_void;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{sel, AnyThread, Message};
use objc2_app_kit::{NSColor, NSImage};
use objc2_core_foundation::{CFRetained, CGRect};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGBitmapContextCreateImage, CGColorSpace, CGContext, CGImage,
    CGImageAlphaInfo,
};
use objc2_foundation::{NSDistributedNotificationCenter, NSSize, NSString, NSUserDefaults};

const STYLE_KEY: &str = "AppleInterfaceStyle";
const THEME_CHANGED: &str = "AppleInterfaceThemeChangedNotification";
/// Серый фон панели, как у меню macOS без прозрачности.
const LIGHT_BACKGROUND: f64 = 234.0 / 255.0;
// HARDCODE: временно, взять точный цвет со скриншота тёмной темы.
const DARK_BACKGROUND: f64 = 40.0 / 255.0;
/// Иконка темнее этого — тёмная, светлее `LIGHT_TONE` — светлая; между ними цветная, не трогаем.
const DARK_TONE: f64 = 0.35;
const LIGHT_TONE: f64 = 0.65;
/// Пиксели прозрачнее этого в тон иконки не входят.
const MIN_ALPHA: u8 = 128;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Theme {
    Light,
    Dark,
}

impl Theme {
    pub fn opposite(self) -> Self {
        match self {
            Theme::Light => Theme::Dark,
            Theme::Dark => Theme::Light,
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

/// Тема из системных настроек.
pub fn current() -> Theme {
    let style = NSUserDefaults::standardUserDefaults().stringForKey(&NSString::from_str(STYLE_KEY));
    if style.is_some_and(|style| style.to_string() == "Dark") {
        Theme::Dark
    } else {
        Theme::Light
    }
}

/// Подписывает `target` на смену темы: macOS зовёт `selector` с уведомлением.
pub fn observe(target: &AnyObject) {
    unsafe {
        NSDistributedNotificationCenter::defaultCenter().addObserver_selector_name_object(
            target,
            sel!(onThemeChanged:),
            Some(&NSString::from_str(THEME_CHANGED)),
            None,
        );
    }
}

pub fn panel_background(theme: Theme) -> Retained<NSColor> {
    let white = match theme {
        Theme::Light => LIGHT_BACKGROUND,
        Theme::Dark => DARK_BACKGROUND,
    };
    NSColor::colorWithWhite_alpha(white, 1.0)
}

/// Снимок, подогнанный под тему: иконка не того тона инвертируется, цветная остаётся.
pub fn fit(image: &NSImage, theme: Theme) -> Retained<NSImage> {
    let Some(mut pixels) = Pixels::read(image) else { return image.retain() };
    let wanted_dark = theme == Theme::Light;
    let needs_flip = match pixels.tone() {
        Some(tone) if tone < DARK_TONE => !wanted_dark,
        Some(tone) if tone > LIGHT_TONE => wanted_dark,
        _ => false,
    };
    if !needs_flip {
        return image.retain();
    }
    pixels.invert();
    pixels.to_image(image.size()).unwrap_or_else(|| image.retain())
}

/// Пиксели снимка в RGBA с умноженной альфой.
struct Pixels {
    data: Vec<u8>,
    width: usize,
    height: usize,
}

impl Pixels {
    fn read(image: &NSImage) -> Option<Self> {
        let cg = unsafe { image.CGImageForProposedRect_context_hints(std::ptr::null_mut(), None, None) }?;
        let (width, height) = (CGImage::width(Some(&cg)), CGImage::height(Some(&cg)));
        let mut pixels = Self { data: vec![0; width * height * 4], width, height };
        let context = pixels.context()?;
        let rect = CGRect::new(Default::default(), objc2_core_foundation::CGSize::new(width as f64, height as f64));
        CGContext::draw_image(Some(&context), rect, Some(&cg));
        Some(pixels)
    }

    fn context(&mut self) -> Option<CFRetained<CGContext>> {
        let space = CGColorSpace::new_device_rgb()?;
        unsafe {
            CGBitmapContextCreate(
                self.data.as_mut_ptr() as *mut c_void,
                self.width,
                self.height,
                8,
                self.width * 4,
                Some(&space),
                CGImageAlphaInfo::PremultipliedLast.0,
            )
        }
    }

    /// Средняя яркость непрозрачных пикселей, 0 — чёрный, 1 — белый.
    fn tone(&self) -> Option<f64> {
        let (mut sum, mut count) = (0.0, 0usize);
        for pixel in self.data.chunks_exact(4) {
            let alpha = pixel[3];
            if alpha < MIN_ALPHA {
                continue;
            }
            let channel = |value: u8| f64::from(value) / f64::from(alpha);
            sum += 0.299 * channel(pixel[0]) + 0.587 * channel(pixel[1]) + 0.114 * channel(pixel[2]);
            count += 1;
        }
        (count > 0).then(|| sum / count as f64)
    }

    /// Инверсия цвета с сохранением прозрачности.
    fn invert(&mut self) {
        for pixel in self.data.chunks_exact_mut(4) {
            let alpha = pixel[3];
            for channel in &mut pixel[..3] {
                *channel = alpha.saturating_sub(*channel);
            }
        }
    }

    fn to_image(&mut self, size: NSSize) -> Option<Retained<NSImage>> {
        let context = self.context()?;
        let cg = CGBitmapContextCreateImage(Some(&context))?;
        Some(NSImage::initWithCGImage_size(NSImage::alloc(), &cg, size))
    }
}
