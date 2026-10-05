//! Снимки иконок строки меню для панели и редактора.

use std::sync::{Arc, Mutex, PoisonError};

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

use crate::tuning::MAX_ICON_HEIGHT;

const CONTROL_CENTER_BUNDLE: &str = "com.apple.controlcenter";
/// Окно системного индикатора камеры, микрофона и записи экрана: его место решает macOS,
/// и появляется оно само, пока идёт запись — в том числе наши снимки.
const PRIVACY_INDICATOR: &str = "AudioVideoModule";

/// Окно иконки строки меню: номер CG-окна, имя, левый край и размер в pt, нарисовано ли.
pub struct IconWindow {
    pub id: u32,
    pub name: String,
    pub x: f64,
    pub width: f64,
    pub height: f64,
    pub onscreen: bool,
}

/// Кому отдать готовые снимки.
#[derive(Clone, Copy)]
enum Receiver {
    Panel,
    Editor,
}

/// Готовый снимок, передаётся из потока ScreenCaptureKit в главный поток.
struct Shot {
    id: u32,
    image: CFRetained<CGImage>,
    width: f64,
    height: f64,
}

/// Снимки по местам и сколько ещё ждать.
type Shots = Arc<Mutex<(Vec<Option<Shot>>, usize)>>;

/// Снимает иконки с окнами `ids` и по готовности отдаёт панели через делегата
/// приложения `setPanelIcons:ids:` — картинки и номера окон в порядке `ids`.
pub fn capture_for_panel(ids: &[u32]) {
    capture(windows_in_order(ids), Receiver::Panel);
}

/// Снимает иконки с окнами `ids` для редактора: делегату через `setEditorIcons:ids:`.
pub fn capture_for_editor(ids: &[u32]) {
    capture(windows_in_order(ids), Receiver::Editor);
}

fn windows_in_order(ids: &[u32]) -> Vec<IconWindow> {
    let mut windows = icon_windows(|window| ids.contains(&window.id));
    windows.sort_by_key(|window| ids.iter().position(|id| *id == window.id));
    windows
}

/// Все окна иконок строки меню слева направо.
pub fn icon_layout() -> Vec<IconWindow> {
    icon_windows(|_| true)
}

fn capture(windows: Vec<IconWindow>, receiver: Receiver) {
    if windows.is_empty() {
        deliver(Vec::new(), receiver);
        return;
    }

    let handler = RcBlock::new(
        move |content: *mut SCShareableContent, error: *mut NSError| {
            let Some(content) = (unsafe { Retained::retain(content) }) else {
                log_error("SCShareableContent", error);
                deliver(Vec::new(), receiver);
                return;
            };
            capture_windows(&content, &windows, receiver);
        },
    );
    unsafe {
        SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(
            true, false, &handler,
        );
    }
}

/// Каждое окно — отдельным фильтром: несколько окон в одном фильтре дают −3811.
fn capture_windows(content: &SCShareableContent, windows: &[IconWindow], receiver: Receiver) {
    let shareable = unsafe { content.windows() };
    let results: Shots = Arc::new(Mutex::new((
        (0..windows.len()).map(|_| None).collect(),
        windows.len(),
    )));

    for (index, icon) in windows.iter().enumerate() {
        let Some(window) = shareable
            .iter()
            .find(|w| unsafe { w.windowID() } == icon.id)
        else {
            log::debug!("окна {} нет в ScreenCaptureKit", icon.id);
            finish_one(&results, index, None, receiver);
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
            finish_one(&results, index, shot, receiver);
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

fn finish_one(
    results: &Shots,
    index: usize,
    shot: Option<Shot>,
    receiver: Receiver,
) {
    let mut guard = results.lock().unwrap_or_else(PoisonError::into_inner);
    guard.0[index] = shot;
    guard.1 -= 1;
    if guard.1 == 0 {
        deliver(guard.0.drain(..).flatten().collect(), receiver);
    }
}

/// Переносит снимки в главный поток и отдаёт делегату приложения.
fn deliver(shots: Vec<Shot>, receiver: Receiver) {
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
        let ids: Vec<Retained<NSNumber>> = shots
            .iter()
            .map(|shot| NSNumber::new_u32(shot.id))
            .collect();
        let images = NSArray::from_retained_slice(&images);
        let ids = NSArray::from_retained_slice(&ids);
        if let Some(delegate) = NSApplication::sharedApplication(mtm).delegate() {
            let delegate: &AnyObject = delegate.as_ref();
            let _: () = match receiver {
                Receiver::Panel => unsafe {
                    msg_send![delegate, setPanelIcons: &*images, ids: &*ids]
                },
                Receiver::Editor => unsafe {
                    msg_send![delegate, setEditorIcons: &*images, ids: &*ids]
                },
            };
        }
    });
}

/// Окна иконок ControlCenter слева направо, которые проходят `keep`.
fn icon_windows(keep: impl Fn(&IconWindow) -> bool) -> Vec<IconWindow> {
    let Some(owner_pid) = control_center_pid() else {
        log::warn!("ControlCenter не найден");
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
                && !name.contains("Clone")
                && name != PRIVACY_INDICATOR;
            let window = IconWindow {
                id,
                name,
                x,
                width,
                height,
                onscreen,
            };
            (is_icon && keep(&window)).then_some((x, window))
        })
        .collect();
    windows.sort_by(|a, b| a.0.total_cmp(&b.0));
    windows.into_iter().map(|(_, w)| w).collect()
}

/// Номера окон процесса `pid`, которые сейчас на экране.
pub fn onscreen_windows_of(pid: i32) -> Vec<u32> {
    let Some(list) = window_list(CGWindowListOption::OptionOnScreenOnly, kCGNullWindowID) else {
        return Vec::new();
    };
    list.iter()
        .filter(|info| {
            number(info, "kCGWindowOwnerPID").is_some_and(|owner| owner.intValue() == pid)
        })
        .filter_map(|info| number(&info, "kCGWindowNumber").map(|n| n.unsignedIntValue()))
        .collect()
}

/// Центры x окон `ids` в глобальных координатах CG — за один запрос списка окон.
pub fn window_centers(ids: &[u32]) -> Vec<(u32, f64)> {
    // Запрос одного окна (OptionIncludingWindow) для иконок под чёлкой возвращает пусто.
    let Some(list) = window_list(CGWindowListOption::OptionAll, kCGNullWindowID) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|info| {
            let id = number(&info, "kCGWindowNumber")?.unsignedIntValue();
            if !ids.contains(&id) {
                return None;
            }
            let (x, _, width, _) = bounds(&info)?;
            Some((id, x + width / 2.0))
        })
        .collect()
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
    info.objectForKey(&NSString::from_str(key))?
        .downcast::<NSNumber>()
        .ok()
}

pub fn control_center_pid() -> Option<i32> {
    let apps = NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(
        CONTROL_CENTER_BUNDLE,
    ));
    apps.firstObject().map(|app| app.processIdentifier())
}

fn log_error(what: &str, error: *mut NSError) {
    let text = unsafe { error.as_ref() }
        .map(|e| e.localizedDescription().to_string())
        .unwrap_or_else(|| "без описания".into());
    log::warn!("{what}: ошибка {text}");
}
