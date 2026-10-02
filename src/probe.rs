//! Временная проба шторки без мыши. При старте, если есть файл-метка: снимок
//! промежутка у чёлки → шторка → узкий разделитель → прежняя ширина → без шторки.
//! В лог идут раскладка окон и расхождение снимков с исходным.

use std::cell::RefCell;
use std::ffi::c_void;
use std::thread;
use std::time::Duration;

use dispatch2::DispatchQueue;
use objc2::runtime::AnyObject;
use objc2::{msg_send, MainThreadMarker};
use objc2_app_kit::NSApplication;
use objc2_core_foundation::{CGRect, CGSize};
use objc2_core_graphics::{CGBitmapContextCreate, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo};

use crate::shroud::Shroud;

// HARDCODE: временная проба шторки — удалить вместе с модулем после проверки.
const MARKER: &str = "/tmp/nook-probe-shroud";
const PAINT_DELAY: Duration = Duration::from_millis(150);
const HOLD: Duration = Duration::from_millis(2500);

thread_local! {
    static SHROUD: RefCell<Option<Shroud>> = const { RefCell::new(None) };
}

/// Есть ли метка; метка удаляется, чтобы проба шла один раз.
pub fn take_request() -> bool {
    std::fs::remove_file(MARKER).is_ok()
}

/// Проба в фоне. `gap` — промежуток у чёлки в координатах CG, `narrow` и `wide` —
/// длины разделителя. По окончании зовёт `probeFinished` у делегата.
pub fn run(gap: CGRect, narrow: f64, wide: f64) {
    thread::spawn(move || {
        crate::log::append(&format!(
            "probe: промежуток x={} w={} h={}, разделитель {wide} → {narrow} → {wide}",
            gap.origin.x, gap.size.width, gap.size.height
        ));
        let Some(before) = crate::capture::screen_rect(gap) else {
            crate::log::append("probe: исходный снимок не получен — отмена");
            on_main(finish);
            return;
        };
        log_layout("исходно");
        let image = before.clone();
        on_main(move |mtm| {
            SHROUD.with(|cell| cell.borrow_mut().get_or_insert_with(|| Shroud::new(mtm)).show(mtm, &image, gap))
        });
        thread::sleep(PAINT_DELAY);
        compare("шторка висит", &before, gap);

        on_main(move |mtm| set_divider(mtm, narrow));
        crate::mover::wait_until_still();
        log_layout("узкий, под шторкой");
        compare("узкий, под шторкой", &before, gap);
        thread::sleep(HOLD);

        on_main(move |mtm| set_divider(mtm, wide));
        crate::mover::wait_until_still();
        log_layout("широкий, под шторкой");
        compare("широкий, под шторкой", &before, gap);

        on_main(|_| {
            SHROUD.with(|cell| {
                if let Some(shroud) = cell.borrow().as_ref() {
                    shroud.hide();
                }
            })
        });
        thread::sleep(PAINT_DELAY);
        compare("без шторки", &before, gap);
        on_main(finish);
    });
}

fn on_main(work: impl FnOnce(MainThreadMarker) + Send) {
    DispatchQueue::main().exec_sync(move || work(MainThreadMarker::new().expect("main queue")));
}

fn set_divider(mtm: MainThreadMarker, length: f64) {
    if let Some(delegate) = NSApplication::sharedApplication(mtm).delegate() {
        let delegate: &AnyObject = delegate.as_ref();
        let _: () = unsafe { msg_send![delegate, setPanelDividerLength: length] };
    }
}

fn finish(mtm: MainThreadMarker) {
    if let Some(delegate) = NSApplication::sharedApplication(mtm).delegate() {
        let delegate: &AnyObject = delegate.as_ref();
        let _: () = unsafe { msg_send![delegate, probeFinished] };
    }
}

fn log_layout(label: &str) {
    let windows: Vec<String> = crate::capture::icon_layout()
        .iter()
        .map(|w| format!("{}@{}+{}{}", w.id, w.x, w.width, if w.onscreen { "" } else { "(скрыто)" }))
        .collect();
    crate::log::append(&format!("probe: раскладка [{label}]: {}", windows.join(" ")));
}

/// Снимает `gap` ещё раз и пишет, сколько пикселей отличается от `before`.
fn compare(label: &str, before: &CGImage, gap: CGRect) {
    let Some(now) = crate::capture::screen_rect(gap) else {
        crate::log::append(&format!("probe: [{label}] снимок не получен"));
        return;
    };
    let (a, b) = (rgba(before), rgba(&now));
    if a.len() != b.len() {
        crate::log::append(&format!("probe: [{label}] размеры снимков разные"));
        return;
    }
    let (mut changed, mut max_delta) = (0usize, 0u8);
    for (pa, pb) in a.chunks_exact(4).zip(b.chunks_exact(4)) {
        let delta = pa.iter().zip(pb).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0);
        if delta > 0 {
            changed += 1;
            max_delta = max_delta.max(delta);
        }
    }
    crate::log::append(&format!(
        "probe: [{label}] отличается пикселей {changed} из {}, максимум {max_delta}",
        a.len() / 4
    ));
}

fn rgba(image: &CGImage) -> Vec<u8> {
    let (width, height) = (CGImage::width(Some(image)), CGImage::height(Some(image)));
    let mut data = vec![0u8; width * height * 4];
    let Some(space) = CGColorSpace::new_device_rgb() else { return Vec::new() };
    let context = unsafe {
        CGBitmapContextCreate(
            data.as_mut_ptr() as *mut c_void,
            width,
            height,
            8,
            width * 4,
            Some(&space),
            CGImageAlphaInfo::PremultipliedLast.0,
        )
    };
    let Some(context) = context else { return Vec::new() };
    let rect = CGRect::new(Default::default(), CGSize::new(width as f64, height as f64));
    CGContext::draw_image(Some(&context), rect, Some(image));
    data
}
