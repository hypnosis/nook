//! Нажатие настоящей иконки строки меню через Accessibility (AXPress).
//!
//! Иконка под чёлкой не нарисована, и клик мышью по её координатам до неё не доходит.
//! AXPress нажимает элемент без координат; сам элемент ищем по центру x окна иконки.

use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::Mutex;
use std::time::Instant;

use objc2_app_kit::NSWorkspace;
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
pub fn remember_owners(ids: Vec<u32>) {
    let missing: Vec<u32> = {
        let cache = CACHE.lock().unwrap();
        ids.into_iter()
            .filter(|id| cache.as_ref().is_none_or(|c| !c.contains_key(id)))
            .collect()
    };
    if missing.is_empty() {
        return;
    }
    let pids = running_pids();
    std::thread::spawn(move || {
        let started = Instant::now();
        let found = match_windows(&missing, &pids);
        let mut cache = CACHE.lock().unwrap();
        let cache = cache.get_or_insert_with(HashMap::new);
        let count = found.len();
        cache.extend(found);
        crate::log::append(&format!(
            "click: кэш +{count} из {} за {} мс, всего {}",
            missing.len(),
            started.elapsed().as_millis(),
            cache.len()
        ));
    });
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
        let started = Instant::now();
        let cached = CACHE.lock().unwrap().as_mut().and_then(|cache| cache.remove(&id));
        let from_cache = cached.is_some();
        let item = cached.or_else(|| match_windows(&[id], &pids).into_iter().next().map(|(_, item)| item));
        let Some(item) = item else {
            crate::log::append(&format!("click: для окна {id} иконки в AX нет"));
            return;
        };
        let found_ms = started.elapsed().as_millis();
        let result = unsafe { item.element.perform_action(&CFString::from_static_str("AXPress")) };
        crate::log::append(&format!(
            "click: окно {id} pid {} (кэш {from_cache}) найдено за {found_ms} мс, AXPress → {} за {} мс",
            item.pid,
            result.0,
            started.elapsed().as_millis() - found_ms
        ));
        CACHE.lock().unwrap().get_or_insert_with(HashMap::new).insert(id, item);
    });
}

/// Один проход по всем приложениям: каждому окну — элемент с ближайшим центром x.
fn match_windows(ids: &[u32], pids: &[i32]) -> Vec<(u32, AxItem)> {
    let centers: Vec<(u32, f64)> = ids.iter().filter_map(|&id| Some((id, window_center(id)?))).collect();
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

/// Все иконки строки меню всех приложений с центрами x.
fn all_items(pids: &[i32]) -> Vec<(f64, AxItem)> {
    let mut items = Vec::new();
    for &pid in pids {
        let app = unsafe { AXUIElement::new_application(pid) };
        unsafe { app.set_messaging_timeout(AX_TIMEOUT) };
        let Some(bar) = attribute(&app, "AXExtrasMenuBar") else { continue };
        let Some(bar) = bar.downcast::<AXUIElement>().ok() else { continue };
        let Some(children) = attribute(&bar, "AXChildren") else { continue };
        let Some(children) = children.downcast::<CFArray>().ok() else { continue };
        for index in 0..children.count() {
            let item = unsafe { children.value_at_index(index) } as *mut AXUIElement;
            let Some(item) = NonNull::new(item) else { continue };
            let element = unsafe { CFRetained::retain(item) };
            if let Some(center) = center_x(&element) {
                items.push((center, AxItem { pid, element }));
            }
        }
    }
    items
}

fn window_center(id: u32) -> Option<f64> {
    crate::capture::window_bounds(id).map(|(x, _, width, _)| x + width / 2.0)
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
