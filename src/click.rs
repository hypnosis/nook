//! Нажатие настоящей иконки строки меню через Accessibility (AXPress).
//!
//! Иконка под чёлкой не нарисована, и клик мышью по её координатам до неё не доходит.
//! AXPress нажимает элемент без координат; сам элемент ищем по центру x окна иконки.

use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::Mutex;

use objc2_app_kit::NSWorkspace;
use objc2_application_services::{AXIsProcessTrusted, AXUIElement, AXValue, AXValueType};
use objc2_core_foundation::{CFArray, CFRetained, CFString, CFType, CGPoint, CGSize};
use objc2_core_graphics::CGRequestPostEventAccess;

/// Насколько центр элемента Accessibility может отличаться от центра окна иконки, pt.
// HARDCODE: допуск сопоставления иконки; вынести в конфиг позже.
const MATCH_TOLERANCE: f64 = 6.0;
const AX_TIMEOUT: f32 = 0.5;

/// Номер окна иконки → pid приложения, которому она принадлежит. Заполняется при открытии панели.
static OWNERS: Mutex<Option<HashMap<u32, i32>>> = Mutex::new(None);

/// В фоне запоминает владельцев окон панели, чтобы клик спрашивал одно приложение, а не все.
pub fn remember_owners(ids: Vec<u32>) {
    let pids = running_pids();
    std::thread::spawn(move || {
        let mut owners = HashMap::new();
        for id in ids {
            let Some(center) = window_center(id) else { continue };
            if let Some((pid, _)) = find_item(&pids, center) {
                owners.insert(id, pid);
            }
        }
        crate::log::append(&format!("click: владельцы окон {owners:?}"));
        *OWNERS.lock().unwrap() = Some(owners);
    });
}

/// Нажимает настоящую иконку, чьё окно — `id`. AXPress ждёт закрытия меню,
/// поэтому поиск и нажатие идут в фоновом потоке.
pub fn click_window(id: u32) {
    if !unsafe { AXIsProcessTrusted() } {
        crate::log::append("click: нет права Accessibility — запрашиваю");
        CGRequestPostEventAccess();
        return;
    }
    let Some(center) = window_center(id) else {
        crate::log::append(&format!("click: окна {id} больше нет"));
        return;
    };
    let owner = OWNERS.lock().unwrap().as_ref().and_then(|owners| owners.get(&id).copied());
    let pids = owner.map_or_else(running_pids, |pid| vec![pid]);

    std::thread::spawn(move || {
        let started = std::time::Instant::now();
        let Some((pid, item)) = find_item(&pids, center) else {
            crate::log::append(&format!("click: для окна {id} (центр {center}) иконки в AX нет"));
            return;
        };
        crate::log::append(&format!(
            "click: окно {id} → pid {pid} (кэш {}) за {} мс, AXPress",
            owner.is_some(),
            started.elapsed().as_millis()
        ));
        let result = unsafe { item.perform_action(&CFString::from_static_str("AXPress")) };
        crate::log::append(&format!("click: AXPress pid {pid} → {}", result.0));
    });
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

/// Иконка строки меню любого приложения, чей центр x ближе всего к `center`.
fn find_item(pids: &[i32], center: f64) -> Option<(i32, CFRetained<AXUIElement>)> {
    let mut best: Option<(f64, i32, CFRetained<AXUIElement>)> = None;
    for &pid in pids {
        let app = unsafe { AXUIElement::new_application(pid) };
        unsafe { app.set_messaging_timeout(AX_TIMEOUT) };
        let Some(bar) = attribute(&app, "AXExtrasMenuBar") else { continue };
        let Some(bar) = bar.downcast::<AXUIElement>().ok() else { continue };
        let Some(children) = attribute(&bar, "AXChildren") else { continue };
        let Some(children) = children.downcast::<CFArray>().ok() else { continue };
        for index in 0..children.count() {
            let item = unsafe { children.value_at_index(index) } as *const AXUIElement;
            let Some(item) = NonNull::new(item as *mut AXUIElement) else { continue };
            let item = unsafe { CFRetained::retain(item) };
            let Some(item_center) = center_x(&item) else { continue };
            let distance = (item_center - center).abs();
            if distance <= MATCH_TOLERANCE && best.as_ref().is_none_or(|b| distance < b.0) {
                best = Some((distance, pid, item));
            }
        }
    }
    best.map(|(_, pid, item)| (pid, item))
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
