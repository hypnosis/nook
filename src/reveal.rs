//! Узкий режим панели. Пока панель открыта, разделитель узкий и иконки панели стоят
//! у чёлки, поэтому приложение открывает меню рядом с панелью. Перестройку строки
//! закрывает шторка со снимком полосы у чёлки; когда строка встала, шторка остаётся
//! только от чёлки до основного ряда.

use std::cell::RefCell;
use std::thread;
use std::time::{Duration, Instant};

use dispatch2::DispatchQueue;
use objc2::runtime::AnyObject;
use objc2::{msg_send, MainThreadMarker};
use objc2_app_kit::NSApplication;
use objc2_core_foundation::{CFRetained, CGPoint, CGRect, CGSize};
use objc2_core_graphics::CGImage;

use crate::shroud::Shroud;

// HARDCODE: тайминги узкого режима; вынести в конфиг позже.
const PAINT_DELAY: Duration = Duration::from_millis(20);
const LAYOUT_POLL: Duration = Duration::from_millis(5);
const LAYOUT_TIMEOUT: Duration = Duration::from_millis(500);
/// Иконка, уходящая обратно в спрятанные, ещё гаснет после того, как встала на место.
const FADE_DELAY: Duration = Duration::from_millis(150);
const MENU_POLL: Duration = Duration::from_millis(50);
const MENU_APPEAR_TIMEOUT: Duration = Duration::from_millis(1500);
const MENU_MAX_OPEN: Duration = Duration::from_secs(120);
/// Окно разделителя не шире этого — он сужен.
const NARROW_WINDOW: f64 = 40.0;

#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Off,
    Entering,
    Ready,
}

struct Mode {
    stage: Stage,
    /// Растёт при каждом включении и выключении: шаги прежнего включения его не застают.
    generation: u64,
    /// Длина разделителя до сужения.
    wide: f64,
    /// Иконка, нажатая, пока режим включался или шло прошлое нажатие, и время клика.
    pending: Option<(u32, Instant)>,
    /// Идёт нажатие: меню иконки открыто.
    busy: bool,
    shroud: Option<Shroud>,
}

thread_local! {
    static MODE: RefCell<Mode> = const {
        RefCell::new(Mode {
            stage: Stage::Off,
            generation: 0,
            wide: 0.0,
            pending: None,
            busy: false,
            shroud: None,
        })
    };
}

/// Узкий режим включён или включается.
pub fn is_on() -> bool {
    MODE.with_borrow(|mode| mode.stage != Stage::Off)
}

/// Узкий режим включается: строка меню ещё перестраивается.
pub fn is_entering() -> bool {
    MODE.with_borrow(|mode| mode.stage == Stage::Entering)
}

/// Идёт нажатие из панели: меню иконки открыто.
pub fn menu_open() -> bool {
    MODE.with_borrow(|mode| mode.busy)
}

/// Включает узкий режим. `gap` — полоса от чёлки до ≡◂ в координатах CG,
/// `wide` — длина разделителя, которую потом вернёт `exit`.
pub fn enter(divider_id: u32, gap: CGRect, wide: f64) {
    let generation = MODE.with_borrow_mut(|mode| {
        mode.generation += 1;
        mode.stage = Stage::Entering;
        mode.wide = wide;
        mode.pending = None;
        mode.generation
    });
    thread::spawn(move || {
        let started = Instant::now();
        let Some(image) = crate::capture::screen_rect(gap) else {
            crate::log::append("reveal: снимка полосы нет — узкий режим не включаю");
            on_main(move |_| {
                MODE.with_borrow_mut(|mode| {
                    if mode.generation == generation {
                        mode.stage = Stage::Off;
                        hide_shroud(mode);
                    }
                })
            });
            return;
        };
        let full = image.clone();
        on_main(move |mtm| {
            MODE.with_borrow_mut(|mode| {
                if mode.generation == generation {
                    show_shroud(mode, mtm, &full, gap);
                }
            })
        });
        thread::sleep(PAINT_DELAY);
        on_main(move |mtm| {
            if MODE.with_borrow(|mode| mode.generation == generation) {
                set_divider(mtm, crate::divider::NARROW_WIDTH);
            }
        });
        wait_divider(divider_id, true);
        crate::mover::wait_until_still();
        let strip = strip_of(&image, gap, divider_id);
        on_main(move |mtm| {
            let pending = MODE.with_borrow_mut(|mode| {
                if mode.generation != generation {
                    return None;
                }
                if let Some((image, rect)) = &strip {
                    show_shroud(mode, mtm, image, *rect);
                }
                mode.stage = Stage::Ready;
                mode.pending.take()
            });
            if let Some((icon_id, clicked)) = pending {
                press_now(icon_id, clicked);
            }
        });
        crate::log::append(&format!("reveal: узкий режим готов за {} мс", started.elapsed().as_millis()));
    });
}

/// Выключает узкий режим при видимой строке: разделителю `divider_id` возвращается
/// прежняя длина (без него длину не трогаем), строка встаёт, шторка уходит.
pub fn exit(divider_id: Option<u32>) {
    let Some((generation, wide)) = switch_off() else { return };
    thread::spawn(move || {
        if divider_id.is_some() {
            on_main(move |mtm| {
                if MODE.with_borrow(|mode| mode.generation == generation) {
                    set_divider(mtm, wide);
                }
            });
        }
        hide_shroud_when_settled(generation, divider_id);
    });
}

/// Выключает узкий режим при сворачивании строки: разделитель сразу получает прежнюю
/// длину, а шторка уходит, когда спейсер увёл полосу за край и строка встала.
pub fn exit_now(mtm: MainThreadMarker, divider_id: Option<u32>) {
    let Some((generation, wide)) = switch_off() else { return };
    set_divider(mtm, wide);
    thread::spawn(move || hide_shroud_when_settled(generation, divider_id));
}

/// Убирает шторку выключения `generation`, когда разделитель снова широкий и строка встала.
fn hide_shroud_when_settled(generation: u64, divider_id: Option<u32>) {
    if let Some(divider_id) = divider_id {
        wait_divider(divider_id, false);
    }
    crate::mover::wait_until_still();
    thread::sleep(FADE_DELAY);
    on_main(move |_| {
        MODE.with_borrow_mut(|mode| {
            if mode.generation == generation {
                hide_shroud(mode);
            }
        })
    });
}

/// Переводит режим в выключенный: шаги прежнего включения и нажатия его больше не застают.
/// Возвращает новое поколение и длину разделителя до сужения; None — режим не был включён.
fn switch_off() -> Option<(u64, f64)> {
    MODE.with_borrow_mut(|mode| {
        if mode.stage == Stage::Off {
            return None;
        }
        mode.generation += 1;
        mode.stage = Stage::Off;
        mode.pending = None;
        Some((mode.generation, mode.wide))
    })
}

/// Нажимает иконку панели: в готовом режиме сразу, во включающемся или во время
/// прошлого нажатия — как только освободится. Без узкого режима возвращает false.
pub fn press(icon_id: u32) -> bool {
    let clicked = Instant::now();
    let (stage, busy) = MODE.with_borrow_mut(|mode| {
        if mode.stage == Stage::Entering || (mode.stage == Stage::Ready && mode.busy) {
            mode.pending = Some((icon_id, clicked));
        }
        (mode.stage, mode.busy)
    });
    match stage {
        Stage::Off => false,
        Stage::Ready if !busy => {
            press_now(icon_id, clicked);
            true
        }
        _ => true,
    }
}

/// Нажимает иконку на месте, у чёлки, не трогая курсор, и ждёт, пока её меню закроется.
/// `clicked` — когда пришёл клик по копии.
fn press_now(icon_id: u32, clicked: Instant) {
    let generation = MODE.with_borrow_mut(|mode| {
        mode.busy = true;
        mode.generation
    });
    let pid = crate::click::owner_pid(icon_id);
    thread::spawn(move || {
        let windows_before = pid.map(crate::capture::onscreen_windows_of).unwrap_or_default();
        crate::click::click_window(icon_id);
        wait_menu_closed(pid, &windows_before, clicked);
        on_main(move |_| {
            let pending = MODE.with_borrow_mut(|mode| {
                mode.busy = false;
                if mode.generation != generation || mode.stage != Stage::Ready {
                    return None;
                }
                mode.pending.take()
            });
            if let Some((next, clicked)) = pending {
                press_now(next, clicked);
            }
        });
    });
}

/// Часть снимка от чёлки до правого края узкого разделителя: только там стоят иконки
/// панели, правее основной ряд не двигается и остаётся живым.
fn strip_of(image: &CGImage, gap: CGRect, divider_id: u32) -> Option<(CFRetained<CGImage>, CGRect)> {
    let divider = crate::capture::icon_layout().into_iter().find(|w| w.id == divider_id)?;
    let width = divider.x + divider.width - gap.origin.x;
    if width <= 0.0 || width >= gap.size.width {
        return None;
    }
    let scale = CGImage::width(Some(image)) as f64 / gap.size.width;
    let pixels = (width * scale).round();
    let crop = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(pixels, CGImage::height(Some(image)) as f64));
    let cropped = CGImage::with_image_in_rect(Some(image), crop)?;
    Some((cropped, CGRect::new(gap.origin, CGSize::new(pixels / scale, gap.size.height))))
}

fn show_shroud(mode: &mut Mode, mtm: MainThreadMarker, image: &CGImage, rect: CGRect) {
    mode.shroud.get_or_insert_with(|| Shroud::new(mtm)).show(mtm, image, rect);
}

fn hide_shroud(mode: &mut Mode) {
    if let Some(shroud) = &mode.shroud {
        shroud.hide();
    }
}

/// Ждёт, пока окно разделителя станет узким (`narrow`) или снова широким.
pub fn wait_divider(divider_id: u32, narrow: bool) {
    let deadline = Instant::now() + LAYOUT_TIMEOUT;
    while Instant::now() < deadline {
        let width = crate::capture::icon_layout().into_iter().find(|w| w.id == divider_id).map(|w| w.width);
        if width.is_some_and(|width| (width <= NARROW_WINDOW) == narrow) {
            return;
        }
        thread::sleep(LAYOUT_POLL);
    }
    crate::log::append(&format!("reveal: разделитель не стал {}", if narrow { "узким" } else { "широким" }));
}

/// Ждёт, пока у приложения `pid` появится новое окно (меню или поповер) и закроется.
fn wait_menu_closed(pid: Option<i32>, windows_before: &[u32], clicked: Instant) {
    let Some(pid) = pid else {
        crate::log::append("reveal: процесс иконки неизвестен — жду только появления");
        thread::sleep(MENU_APPEAR_TIMEOUT);
        return;
    };
    let appear_deadline = Instant::now() + MENU_APPEAR_TIMEOUT;
    let menu = loop {
        if let Some(id) = crate::capture::onscreen_windows_of(pid).into_iter().find(|id| !windows_before.contains(id)) {
            break id;
        }
        if Instant::now() > appear_deadline {
            crate::log::append("reveal: окно меню не появилось");
            return;
        }
        thread::sleep(MENU_POLL);
    };
    crate::log::append(&format!("reveal: клик → меню {} мс", clicked.elapsed().as_millis()));
    let close_deadline = Instant::now() + MENU_MAX_OPEN;
    while crate::capture::onscreen_windows_of(pid).contains(&menu) {
        if Instant::now() > close_deadline {
            crate::log::append("reveal: меню открыто слишком долго — больше не жду");
            return;
        }
        thread::sleep(MENU_POLL);
    }
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
