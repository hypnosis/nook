//! Снимки иконок строки меню для панели и редактора.

use std::sync::{mpsc, Arc, Mutex, PoisonError};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, Message};
use objc2_app_kit::{NSImage, NSRunningApplication};
use objc2_core_foundation::{CFRetained, CGRect};
use objc2_core_graphics::{
    kCGNullWindowID, kCGStatusWindowLevel, CGImage, CGMainDisplayID, CGWindowListCopyWindowInfo,
    CGWindowListOption,
};
use objc2_foundation::{ns_string, NSArray, NSDictionary, NSError, NSNumber, NSSize, NSString};
use objc2_screen_capture_kit::{
    SCContentFilter, SCScreenshotManager, SCShareableContent, SCStreamConfiguration, SCWindow,
};

use crate::tuning::{MAX_ICON_HEIGHT, SHOT_TIMEOUT};

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

/// Снимает иконки с окнами `ids` и по готовности отдаёт панели картинки и номера окон в
/// порядке `ids`.
pub fn capture_for_panel(ids: &[u32]) {
    capture(windows_in_order(ids), Receiver::Panel);
}

/// Снимает иконки с окнами `ids` для редактора.
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

/// Список окон ScreenCaptureKit с прошлых снимков.
static CONTENT: Mutex<Option<SharedContent>> = Mutex::new(None);

/// Список окон ScreenCaptureKit: неизменяемый снимок, его можно передавать между потоками.
struct SharedContent(Retained<SCShareableContent>);
unsafe impl Send for SharedContent {}

/// Прошлый список окон, если в нём есть все окна `windows`.
fn cached_content(windows: &[IconWindow]) -> Option<Retained<SCShareableContent>> {
    let guard = CONTENT.lock().unwrap_or_else(PoisonError::into_inner);
    let content = &guard.as_ref()?.0;
    let known = unsafe { content.windows() };
    windows
        .iter()
        .all(|icon| known.iter().any(|window| unsafe { window.windowID() } == icon.id))
        .then(|| content.clone())
}

fn capture(windows: Vec<IconWindow>, receiver: Receiver) {
    if windows.is_empty() {
        deliver(Vec::new(), receiver);
        return;
    }
    if let Some(content) = cached_content(&windows) {
        capture_windows(&content, &windows, receiver);
        return;
    }

    let handler = RcBlock::new(
        move |content: *mut SCShareableContent, error: *mut NSError| {
            let Some(content) = (unsafe { Retained::retain(content) }) else {
                log_error("SCShareableContent", error);
                deliver(Vec::new(), receiver);
                return;
            };
            *CONTENT.lock().unwrap_or_else(PoisonError::into_inner) = Some(SharedContent(content.clone()));
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

/// Переносит снимки в главный поток и отдаёт контроллеру.
fn deliver(shots: Vec<Shot>, receiver: Receiver) {
    crate::controller::on_main(move |controller| {
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
        match receiver {
            Receiver::Panel => controller.show_panel_icons(&images, &ids),
            Receiver::Editor => controller.show_editor_icons(&images, &ids),
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
            let layer = number(&info, ns_string!("kCGWindowLayer"))?.intValue();
            let pid = number(&info, ns_string!("kCGWindowOwnerPID"))?.intValue();
            if layer as isize != kCGStatusWindowLevel as isize || pid != owner_pid {
                return None;
            }
            let (x, y, width, height) = bounds(&info)?;
            if y != 0.0 || height > MAX_ICON_HEIGHT {
                return None;
            }
            let name = info
                .objectForKey(ns_string!("kCGWindowName"))
                .and_then(|v| v.downcast::<NSString>().ok())
                .map(|s| s.to_string())
                .unwrap_or_default();
            if name.contains("Clone") || name == PRIVACY_INDICATOR {
                return None;
            }
            let onscreen = number(&info, ns_string!("kCGWindowIsOnscreen")).is_some_and(|n| n.boolValue());
            let id = number(&info, ns_string!("kCGWindowNumber"))?.unsignedIntValue();
            let window = IconWindow {
                id,
                name,
                x,
                width,
                height,
                onscreen,
            };
            keep(&window).then_some((x, window))
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
            number(info, ns_string!("kCGWindowOwnerPID")).is_some_and(|owner| owner.intValue() == pid)
        })
        .filter_map(|info| number(&info, ns_string!("kCGWindowNumber")).map(|n| n.unsignedIntValue()))
        .collect()
}

/// Окно `id` процесса `pid` на экране. Сначала спрашивается одно окно; ответ «нет» проверяется
/// по полному списку окон на экране.
pub fn window_onscreen(id: u32, pid: i32) -> bool {
    let single = window_list(CGWindowListOption::OptionIncludingWindow, id)
        .and_then(|list| list.firstObject())
        .is_some_and(|info| number(&info, ns_string!("kCGWindowIsOnscreen")).is_some_and(|n| n.boolValue()));
    single || onscreen_windows_of(pid).contains(&id)
}

/// Снимок участка `rect` главного экрана без окон `hidden`.
/// Блокирует поток до готовности снимка, поэтому только из фонового потока.
pub fn screen_rect(rect: CGRect, hidden: &[u32]) -> Option<CFRetained<CGImage>> {
    let hidden = hidden.to_vec();
    let (sender, receiver) = mpsc::channel();
    let handler = RcBlock::new(move |content: *mut SCShareableContent, error: *mut NSError| {
        let Some(content) = (unsafe { Retained::retain(content) }) else {
            log_error("SCShareableContent", error);
            let _ = sender.send(None);
            return;
        };
        let displays = unsafe { content.displays() };
        let Some(display) = displays.iter().find(|d| unsafe { d.displayID() } == CGMainDisplayID()) else {
            let _ = sender.send(None);
            return;
        };
        let excluded: Vec<Retained<SCWindow>> = unsafe { content.windows() }
            .iter()
            .filter(|window| hidden.contains(&unsafe { window.windowID() }))
            .collect();
        let filter = unsafe {
            SCContentFilter::initWithDisplay_excludingWindows(
                SCContentFilter::alloc(),
                &display,
                &NSArray::from_retained_slice(&excluded),
            )
        };
        let scale = unsafe { filter.pointPixelScale() } as f64;
        let config = unsafe { SCStreamConfiguration::new() };
        unsafe {
            config.setSourceRect(rect);
            config.setWidth((rect.size.width * scale).round() as usize);
            config.setHeight((rect.size.height * scale).round() as usize);
            config.setShowsCursor(false);
        }
        let sender = sender.clone();
        let done = RcBlock::new(move |image: *mut CGImage, error: *mut NSError| {
            let image = std::ptr::NonNull::new(image).map(|image| unsafe { CFRetained::retain(image) });
            if image.is_none() {
                log_error("снимок участка", error);
            }
            let _ = sender.send(image);
        });
        unsafe {
            SCScreenshotManager::captureImageWithFilter_configuration_completionHandler(&filter, &config, Some(&done));
        }
    });
    unsafe {
        SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(
            false, true, &handler,
        );
    }
    receiver.recv_timeout(SHOT_TIMEOUT).ok().flatten()
}

/// Центры x окон `ids` в глобальных координатах CG — за один запрос списка окон.
pub fn window_centers(ids: &[u32]) -> Vec<(u32, f64)> {
    // Запрос одного окна (OptionIncludingWindow) для иконок под чёлкой возвращает пусто.
    let Some(list) = window_list(CGWindowListOption::OptionAll, kCGNullWindowID) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|info| {
            let id = number(&info, ns_string!("kCGWindowNumber"))?.unsignedIntValue();
            if !ids.contains(&id) {
                return None;
            }
            let (x, _, width, _) = bounds(&info)?;
            Some((id, x + width / 2.0))
        })
        .collect()
}

/// Рамка окна `id` (x, y, ширина, высота) в координатах CG.
pub fn window_rect(id: u32) -> Option<(f64, f64, f64, f64)> {
    let list = window_list(CGWindowListOption::OptionAll, kCGNullWindowID)?;
    list.iter()
        .find(|info| number(info, ns_string!("kCGWindowNumber")).is_some_and(|n| n.unsignedIntValue() == id))
        .and_then(|info| bounds(&info))
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
        .objectForKey(ns_string!("kCGWindowBounds"))?
        .downcast::<NSDictionary>()
        .ok()?;
    let bound = |key: &NSString| -> Option<f64> {
        bounds
            .objectForKey(key)?
            .downcast::<NSNumber>()
            .ok()
            .map(|n| n.doubleValue())
    };
    Some((bound(ns_string!("X"))?, bound(ns_string!("Y"))?, bound(ns_string!("Width"))?, bound(ns_string!("Height"))?))
}

fn number(info: &NSDictionary<NSString, AnyObject>, key: &NSString) -> Option<Retained<NSNumber>> {
    info.objectForKey(key)?
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
