//! Нажатие настоящей иконки строки меню через Accessibility (AXPress).
//!
//! Иконка под чёлкой не нарисована, и клик мышью по её координатам до неё не доходит.
//! AXPress нажимает элемент без координат; сам элемент ищем по центру x окна иконки.

use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::Mutex;

use dispatch2::DispatchQueue;
use objc2::runtime::AnyObject;
use objc2::{msg_send, MainThreadMarker};
use objc2_app_kit::{NSApplication, NSWorkspace};
use objc2_application_services::{
    AXIsProcessTrusted, AXIsProcessTrustedWithOptions, AXUIElement, AXValue, AXValueType,
};
use objc2_core_foundation::{
    CFArray, CFDictionary, CFRetained, CFString, CFType, CGPoint, CGSize,
};
use objc2_foundation::{NSDictionary, NSNumber, NSString};

/// Насколько центр элемента Accessibility может отличаться от центра окна иконки, pt.
// HARDCODE: допуск сопоставления иконки и таймаут опроса приложения; вынести в конфиг позже.
const MATCH_TOLERANCE: f64 = 6.0;
const AX_TIMEOUT: f32 = 0.1;

/// Иконка строки меню в Accessibility: приложение-владелец и сам элемент.
struct AxItem {
    pid: i32,
    element: CFRetained<AXUIElement>,
}

// SAFETY: AXUIElement — неизменяемая CF-ссылка, вызовы Accessibility допустимы из любого потока.
unsafe impl Send for AxItem {}

/// Номер окна иконки → её элемент. Номер окна не меняется, пока приложение запущено,
/// поэтому кэш живёт всё время работы Nook и дополняется только новыми окнами.
static CACHE: Mutex<Option<HashMap<u32, AxItem>>> = Mutex::new(None);

/// В фоне находит элементы для окон панели, которых ещё нет в кэше.
/// С `notify` по окончании зовёт `onOwnersRemembered` у делегата приложения.
pub fn remember_owners(ids: Vec<u32>, notify: bool) {
    let missing: Vec<u32> = {
        let cache = CACHE.lock().unwrap();
        ids.into_iter()
            .filter(|id| cache.as_ref().is_none_or(|c| !c.contains_key(id)))
            .collect()
    };
    if missing.is_empty() {
        if notify {
            notify_remembered();
        }
        return;
    }
    let pids = running_pids();
    std::thread::spawn(move || {
        let found = match_windows(&missing, &pids);
        CACHE.lock().unwrap().get_or_insert_with(HashMap::new).extend(found);
        if notify {
            notify_remembered();
        }
    });
}

fn notify_remembered() {
    DispatchQueue::main().exec_async(|| {
        let mtm = MainThreadMarker::new().expect("main queue");
        if let Some(delegate) = NSApplication::sharedApplication(mtm).delegate() {
            let delegate: &AnyObject = delegate.as_ref();
            let _: () = unsafe { msg_send![delegate, onOwnersRemembered] };
        }
    });
}

/// Процесс приложения, чья иконка — окно `id`, если её элемент уже найден.
pub fn owner_pid(id: u32) -> Option<i32> {
    CACHE.lock().unwrap().as_ref()?.get(&id).map(|item| item.pid)
}

/// Нажимает настоящую иконку, чьё окно — `id`. AXPress ждёт ответа приложения,
/// поэтому поиск и нажатие идут в фоновом потоке.
pub fn click_window(id: u32) {
    if !unsafe { AXIsProcessTrusted() } {
        crate::log::append("click: нет права Accessibility — запрашиваю");
        request_accessibility();
        return;
    }
    let pids = running_pids();
    std::thread::spawn(move || {
        let cached = CACHE.lock().unwrap().as_mut().and_then(|cache| cache.remove(&id));
        let item = cached.or_else(|| match_windows(&[id], &pids).into_iter().next().map(|(_, item)| item));
        let Some(item) = item else {
            crate::log::append(&format!("click: для окна {id} иконки в AX нет"));
            return;
        };
        let result = unsafe { item.element.perform_action(&CFString::from_static_str("AXPress")) };
        if result.0 != 0 {
            crate::log::append(&format!("click: AXPress по окну {id} — ошибка {}", result.0));
        }
        CACHE.lock().unwrap().get_or_insert_with(HashMap::new).insert(id, item);
    });
}

/// Один проход по всем приложениям: каждому окну — элемент с ближайшим центром x.
fn match_windows(ids: &[u32], pids: &[i32]) -> Vec<(u32, AxItem)> {
    let centers = crate::capture::window_centers(ids);
    let mut items = all_items(pids);
    let mut matched = Vec::new();
    for (id, center) in centers {
        let best = items
            .iter()
            .enumerate()
            .map(|(index, (item_center, _))| (index, (item_center - center).abs()))
            .filter(|(_, distance)| *distance <= MATCH_TOLERANCE)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((index, _)) = best {
            matched.push((id, items.swap_remove(index).1));
        }
    }
    matched
}

/// Все иконки строки меню всех приложений с центрами x. Приложения опрашиваются
/// одновременно: общее время — как у самого медленного, а не сумма.
fn all_items(pids: &[i32]) -> Vec<(f64, AxItem)> {
    std::thread::scope(|scope| {
        let workers: Vec<_> = pids.iter().map(|&pid| scope.spawn(move || app_items(pid))).collect();
        workers.into_iter().flat_map(|worker| worker.join().unwrap_or_default()).collect()
    })
}

/// Иконки строки меню одного приложения с центрами x.
fn app_items(pid: i32) -> Vec<(f64, AxItem)> {
    let app = unsafe { AXUIElement::new_application(pid) };
    unsafe { app.set_messaging_timeout(AX_TIMEOUT) };
    let Some(bar) = attribute(&app, "AXExtrasMenuBar") else { return Vec::new() };
    let Some(bar) = bar.downcast::<AXUIElement>().ok() else { return Vec::new() };
    let Some(children) = attribute(&bar, "AXChildren") else { return Vec::new() };
    let Some(children) = children.downcast::<CFArray>().ok() else { return Vec::new() };
    let mut items = Vec::new();
    for index in 0..children.count() {
        let item = unsafe { children.value_at_index(index) } as *mut AXUIElement;
        let Some(item) = NonNull::new(item) else { continue };
        let element = unsafe { CFRetained::retain(item) };
        if let Some(center) = center_x(&element) {
            items.push((center, AxItem { pid, element }));
        }
    }
    items
}

fn running_pids() -> Vec<i32> {
    NSWorkspace::sharedWorkspace()
        .runningApplications()
        .iter()
        .map(|app| app.processIdentifier())
        .collect()
}

/// Показывает системный запрос права Accessibility («Универсальный доступ»).
fn request_accessibility() {
    let options = NSDictionary::from_slices(
        &[&*NSString::from_str("AXTrustedCheckOptionPrompt")],
        &[&*NSNumber::new_bool(true)],
    );
    // NSDictionary бесшовно приводится к CFDictionary.
    let options: &CFDictionary = unsafe { &*(&*options as *const NSDictionary<NSString, NSNumber> as *const CFDictionary) };
    unsafe { AXIsProcessTrustedWithOptions(Some(options)) };
}

fn attribute(element: &AXUIElement, name: &'static str) -> Option<CFRetained<CFType>> {
    let mut value: *const CFType = std::ptr::null();
    let error = unsafe {
        element.copy_attribute_value(&CFString::from_static_str(name), NonNull::from(&mut value))
    };
    if error.0 != 0 {
        return None;
    }
    NonNull::new(value as *mut CFType).map(|value| unsafe { CFRetained::from_raw(value) })
}

fn center_x(item: &AXUIElement) -> Option<f64> {
    let position = attribute(item, "AXPosition")?.downcast::<AXValue>().ok()?;
    let size = attribute(item, "AXSize")?.downcast::<AXValue>().ok()?;
    let mut point = CGPoint::new(0.0, 0.0);
    let mut extent = CGSize::new(0.0, 0.0);
    let ok = unsafe {
        position.value(AXValueType::CGPoint, NonNull::from(&mut point).cast::<c_void>())
            && size.value(AXValueType::CGSize, NonNull::from(&mut extent).cast::<c_void>())
    };
    (ok && extent.width > 0.0).then_some(point.x + extent.width / 2.0)
}
