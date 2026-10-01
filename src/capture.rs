//! Снимки иконок, которые при раскрытии не поместились и ушли под чёлку.

use std::sync::{Arc, Mutex};

use block2::RcBlock;
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{msg_send, AnyThread, MainThreadMarker, Message};
use objc2_app_kit::{NSApplication, NSImage, NSRunningApplication};
use objc2_core_foundation::CFRetained;
use objc2_core_graphics::{
    kCGNullWindowID, kCGStatusWindowLevel, CGImage, CGWindowListCopyWindowInfo,
    CGWindowListOption,
};
use objc2_foundation::{NSArray, NSDictionary, NSError, NSNumber, NSSize, NSString};
use objc2_screen_capture_kit::{
    SCContentFilter, SCScreenshotManager, SCShareableContent, SCStreamConfiguration,
};

const CONTROL_CENTER_BUNDLE: &str = "com.apple.controlcenter";
// HARDCODE: предел высоты окна иконки строки меню; вынести в конфиг позже.
const MAX_ICON_HEIGHT: f64 = 50.0;

/// Окно иконки строки меню: номер CG-окна и размер в pt.
struct IconWindow {
    id: u32,
    width: f64,
    height: f64,
}

/// Готовый снимок, передаётся из потока ScreenCaptureKit в главный поток.
struct Shot {
    id: u32,
    image: CFRetained<CGImage>,
    width: f64,
    height: f64,
}

/// Снимает иконки под чёлкой и по готовности отдаёт их делегату приложения
/// через `setPanelIcons:ids:` (картинки и номера их окон, слева направо).
pub fn capture_under_notch() {
    let windows = under_notch_windows();
    crate::log::append(&format!(
        "capture: под чёлкой окон {}: {:?}",
        windows.len(),
        windows.iter().map(|w| w.id).collect::<Vec<_>>()
    ));
    if windows.is_empty() {
        deliver(Vec::new());
        return;
    }

    let handler = RcBlock::new(move |content: *mut SCShareableContent, error: *mut NSError| {
        let Some(content) = (unsafe { Retained::retain(content) }) else {
            log_error("SCShareableContent", error);
            deliver(Vec::new());
            return;
        };
        capture_windows(&content, &windows);
    });
    unsafe {
        SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(
            true, false, &handler,
        );
    }
}

/// Каждое окно — отдельным фильтром: несколько окон в одном фильтре дают −3811.
fn capture_windows(content: &SCShareableContent, windows: &[IconWindow]) {
    let shareable = unsafe { content.windows() };
    let results: Arc<Mutex<(Vec<Option<Shot>>, usize)>> =
        Arc::new(Mutex::new(((0..windows.len()).map(|_| None).collect(), windows.len())));

    for (index, icon) in windows.iter().enumerate() {
        let Some(window) = shareable.iter().find(|w| unsafe { w.windowID() } == icon.id) else {
            crate::log::append(&format!("capture: окна {} нет в ScreenCaptureKit", icon.id));
            finish_one(&results, index, None);
            continue;
        };
        let filter = unsafe {
            SCContentFilter::initWithDesktopIndependentWindow(SCContentFilter::alloc(), &window)
        };
        let scale = unsafe { filter.pointPixelScale() } as f64;
        let config = unsafe { SCStreamConfiguration::new() };
        unsafe {
            config.setWidth((icon.width * scale).round() as usize);
            config.setHeight((icon.height * scale).round() as usize);
            config.setShowsCursor(false);
            config.setIgnoreShadowsSingleWindow(true);
        }

        let results = Arc::clone(&results);
        let (id, width, height) = (icon.id, icon.width, icon.height);
        let handler = RcBlock::new(move |image: *mut CGImage, error: *mut NSError| {
            let shot = std::ptr::NonNull::new(image).map(|image| Shot {
                id,
                image: unsafe { CFRetained::retain(image) },
                width,
                height,
            });
            if shot.is_none() {
                log_error(&format!("окно {id}"), error);
            }
            finish_one(&results, index, shot);
        });
        unsafe {
            SCScreenshotManager::captureImageWithFilter_configuration_completionHandler(
                &filter,
                &config,
                Some(&handler),
            );
        }
    }
}

fn finish_one(results: &Arc<Mutex<(Vec<Option<Shot>>, usize)>>, index: usize, shot: Option<Shot>) {
    let mut guard = results.lock().unwrap();
    guard.0[index] = shot;
    guard.1 -= 1;
    if guard.1 == 0 {
        deliver(guard.0.drain(..).flatten().collect());
    }
}

/// Переносит снимки в главный поток и отдаёт делегату приложения.
fn deliver(shots: Vec<Shot>) {
    DispatchQueue::main().exec_async(move || {
        let mtm = MainThreadMarker::new().expect("main queue");
        let images: Vec<Retained<NSImage>> = shots
            .iter()
            .map(|shot| {
                NSImage::initWithCGImage_size(
                    NSImage::alloc(),
                    &shot.image,
                    NSSize::new(shot.width, shot.height),
                )
            })
            .collect();
        let ids: Vec<Retained<NSNumber>> =
            shots.iter().map(|shot| NSNumber::new_u32(shot.id)).collect();
        crate::log::append(&format!("capture: снято {}", images.len()));
        let images = NSArray::from_retained_slice(&images);
        let ids = NSArray::from_retained_slice(&ids);
        if let Some(delegate) = NSApplication::sharedApplication(mtm).delegate() {
            let delegate: &AnyObject = delegate.as_ref();
            let _: () = unsafe { msg_send![delegate, setPanelIcons: &*images, ids: &*ids] };
        }
    });
}

/// Окна иконок ControlCenter, которые стоят на экране (x ≥ 0), но не нарисованы — они под чёлкой.
fn under_notch_windows() -> Vec<IconWindow> {
    let Some(owner_pid) = control_center_pid() else {
        crate::log::append("capture: ControlCenter не найден");
        return Vec::new();
    };
    let Some(list) = window_list(CGWindowListOption::OptionAll, kCGNullWindowID) else {
        return Vec::new();
    };

    let mut windows: Vec<(f64, IconWindow)> = list
        .iter()
        .filter_map(|info| {
            let layer = number(&info, "kCGWindowLayer")?.intValue();
            let pid = number(&info, "kCGWindowOwnerPID")?.intValue();
            let onscreen = number(&info, "kCGWindowIsOnscreen").is_some_and(|n| n.boolValue());
            let id = number(&info, "kCGWindowNumber")?.unsignedIntValue();
            let name = info
                .objectForKey(&NSString::from_str("kCGWindowName"))
                .and_then(|v| v.downcast::<NSString>().ok())
                .map(|s| s.to_string())
                .unwrap_or_default();
            let (x, y, width, height) = bounds(&info)?;

            let is_icon = layer as isize == kCGStatusWindowLevel as isize
                && pid == owner_pid
                && y == 0.0
                && height <= MAX_ICON_HEIGHT
                && !name.contains("Clone");
            (is_icon && x >= 0.0 && !onscreen).then_some((x, IconWindow { id, width, height }))
        })
        .collect();
    windows.sort_by(|a, b| a.0.total_cmp(&b.0));
    windows.into_iter().map(|(_, w)| w).collect()
}

/// Текущая рамка окна в глобальных координатах CG (начало — левый верхний угол).
pub fn window_bounds(id: u32) -> Option<(f64, f64, f64, f64)> {
    // Запрос одного окна (OptionIncludingWindow) для иконок под чёлкой возвращает пусто.
    let list = window_list(CGWindowListOption::OptionAll, kCGNullWindowID)?;
    let info = list
        .iter()
        .find(|info| number(info, "kCGWindowNumber").is_some_and(|n| n.unsignedIntValue() == id))?;
    bounds(&info)
}

/// CFArray из CGWindowListCopyWindowInfo бесшовно приводится к NSArray<NSDictionary>.
fn window_list(
    option: CGWindowListOption,
    id: u32,
) -> Option<Retained<NSArray<NSDictionary<NSString, AnyObject>>>> {
    let list = CGWindowListCopyWindowInfo(option, id)?;
    let list: &NSArray<NSDictionary<NSString, AnyObject>> =
        unsafe { &*(CFRetained::as_ptr(&list).as_ptr() as *const _) };
    Some(list.retain())
}

fn bounds(info: &NSDictionary<NSString, AnyObject>) -> Option<(f64, f64, f64, f64)> {
    let bounds = info
        .objectForKey(&NSString::from_str("kCGWindowBounds"))?
        .downcast::<NSDictionary>()
        .ok()?;
    let bound = |key: &str| -> Option<f64> {
        bounds
            .objectForKey(&NSString::from_str(key))?
            .downcast::<NSNumber>()
            .ok()
            .map(|n| n.doubleValue())
    };
    Some((bound("X")?, bound("Y")?, bound("Width")?, bound("Height")?))
}

fn number(info: &NSDictionary<NSString, AnyObject>, key: &str) -> Option<Retained<NSNumber>> {
    info.objectForKey(&NSString::from_str(key))?.downcast::<NSNumber>().ok()
}

fn control_center_pid() -> Option<i32> {
    let apps = NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(
        CONTROL_CENTER_BUNDLE,
    ));
    apps.firstObject().map(|app| app.processIdentifier())
}

fn log_error(what: &str, error: *mut NSError) {
    let text = unsafe { error.as_ref() }
        .map(|e| e.localizedDescription().to_string())
        .unwrap_or_else(|| "без описания".into());
    crate::log::append(&format!("capture: {what}: ошибка {text}"));
}
