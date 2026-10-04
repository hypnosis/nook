//! Перенос одной настоящей иконки строки меню к соседу: событиями мыши с Cmd,
//! адресованными окнам, запасной путь — перетаскивание. На время переноса курсор
//! спрятан, а физическая мышь отвязана от него.

use std::cell::Cell;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use dispatch2::DispatchQueue;
use objc2::runtime::AnyObject;
use objc2::{msg_send, MainThreadMarker};
use objc2_app_kit::NSApplication;
use objc2_core_foundation::{kCFBooleanTrue, CFString, CFType, CGPoint};
use objc2_core_graphics::{
    kCGEventFilterMaskPermitAllEvents, CGAssociateMouseAndMouseCursorPosition, CGDisplayHideCursor,
    CGDisplayShowCursor, CGEvent, CGEventField, CGEventFilterMask, CGEventFlags, CGEventSource,
    CGEventSourceStateID, CGEventSuppressionState, CGEventTapLocation, CGEventType, CGMainDisplayID,
    CGMouseButton, CGWarpMouseCursorPosition,
};

use crate::capture::IconWindow;

// HARDCODE: паузы между событиями переноса; вынести в конфиг позже.
const FAST_STEP: Duration = Duration::from_millis(12);
const SAFE_STEP: Duration = Duration::from_millis(50);
const SETTLE_TIMEOUT: Duration = Duration::from_millis(250);
const SETTLE_POLL: Duration = Duration::from_millis(5);
const STILL_POLL: Duration = Duration::from_millis(10);
/// Сколько замеров подряд без изменений считать концом анимации.
const STILL_READS: u32 = 2;
const STILL_TIMEOUT: Duration = Duration::from_millis(600);
/// Насколько заходить за край соседа, чтобы встать перед ним или после него.
const DROP_OFFSET: f64 = 2.0;

/// Сколько ждать ответа иконки на нажатие или отпускание, адресованное окну.
const TARGETED_TIMEOUT: Duration = Duration::from_millis(150);
/// Поле события с номером окна; в публичных заголовках его нет.
const WINDOW_ID_FIELD: CGEventField = CGEventField(0x33);

/// Способ переноса.
#[derive(Clone, Copy)]
enum Method {
    /// Нажатие и отпускание адресованы окнам, курсор по строке не ведётся.
    Targeted,
    /// Cmd+drag по координатам: (пауза между событиями, вести ли через середину).
    Drag(Duration, bool),
}

/// Способы переноса по очереди.
const METHODS: [Method; 4] = [
    Method::Targeted,
    Method::Drag(FAST_STEP, false),
    Method::Drag(FAST_STEP, true),
    Method::Drag(SAFE_STEP, true),
];

/// Куда поставить иконку: вплотную левее окна или вплотную правее.
#[derive(Clone, Copy)]
enum Destination<'a> {
    LeftOf(&'a IconWindow),
    RightOf(&'a IconWindow),
}

/// Способ, сработавший последним: следующий перенос пробует его сразу после адресного.
static PREFERRED: AtomicUsize = AtomicUsize::new(0);

/// Когда перенос последний раз сделал шаг; `None` — переноса нет.
static LAST_STEP: Mutex<Option<Instant>> = Mutex::new(None);

/// Окна иконок, которые не приняли ни один способ переноса.
static STUCK: Mutex<Vec<u32>> = Mutex::new(Vec::new());

/// Номер текущего переноса: растёт при каждом запуске и отмене.
static RUN: AtomicU64 = AtomicU64::new(0);

thread_local! {
    /// Номер переноса, который ведёт этот поток.
    static OWN_RUN: Cell<u64> = const { Cell::new(0) };
}

/// Чем закончилась попытка перенести одно окно.
enum Outcome {
    Moved,
    /// Окно не сдвинулось ни одним способом.
    Stuck,
    /// Окно не сдвинулось по адресу, а перетащить его нельзя: оно или сосед не нарисованы.
    Missed,
    /// Переносить не стали: окно не найдено или уже на месте.
    Skipped,
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGSMainConnectionID() -> i32;
    fn CGSSetConnectionProperty(connection: i32, target: i32, key: &CFString, value: &CFType) -> i32;
}

/// Разрешает прятать курсор, пока приложение в фоне: без этого `CGDisplayHideCursor`
/// у агента без активного окна не действует.
pub fn allow_cursor_hiding_in_background() {
    let Some(enabled) = (unsafe { kCFBooleanTrue }) else { return };
    let key = CFString::from_static_str("SetsCursorInBackground");
    let error = unsafe {
        let connection = CGSMainConnectionID();
        CGSSetConnectionProperty(connection, connection, &key, enabled)
    };
    if error != 0 {
        crate::log::append(&format!("mover: SetsCursorInBackground не включился ({error})"));
    }
}

/// Прячет курсор и отвязывает от него физическую мышь. Только из главного потока.
pub fn take_mouse() {
    CGDisplayHideCursor(CGMainDisplayID());
    CGAssociateMouseAndMouseCursorPosition(false);
}

pub fn release_mouse() {
    CGAssociateMouseAndMouseCursorPosition(true);
    CGDisplayShowCursor(CGMainDisplayID());
}

/// Перенос идёт, но шага не было дольше `limit` — значит, завис.
pub fn is_stalled(limit: Duration) -> bool {
    last_step().is_some_and(|last| last.elapsed() > limit)
}

fn last_step() -> std::sync::MutexGuard<'static, Option<Instant>> {
    LAST_STEP.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn mark_step() {
    if !is_cancelled() {
        *last_step() = Some(Instant::now());
    }
}

/// Отменяет идущий перенос: его поток больше не шлёт событий мыши и не отчитывается.
pub fn cancel() {
    RUN.fetch_add(1, Ordering::SeqCst);
    *last_step() = None;
}

/// Перенос этого потока отменён или сменён новым.
fn is_cancelled() -> bool {
    OWN_RUN.get() != RUN.load(Ordering::SeqCst)
}

/// Ставит одну иконку `id` левее `before` и правее `after` (соседи по строке, один из них
/// может быть разделителем) и сообщает `onTileMoved`, что перенос закончен. Иконка, которая
/// не приняла ни один способ переноса, до перезапуска Nook больше не переносится.
pub fn move_one(id: u32, before: Option<u32>, after: Option<u32>) {
    let run = RUN.fetch_add(1, Ordering::SeqCst) + 1;
    *last_step() = Some(Instant::now());
    thread::spawn(move || {
        OWN_RUN.set(run);
        let started = Instant::now();
        let moved = if stuck().contains(&id) {
            crate::log::append(&format!("mover: окно {id} не переносится — пропускаю"));
            false
        } else {
            crate::click::remember_owners_now(&[id]);
            permit_local_events();
            let home = CGEvent::location(CGEvent::new(None).as_deref());
            let outcome = move_next_to(id, before, after);
            post(CGEventType::MouseMoved, home, CGEventFlags::empty());
            CGWarpMouseCursorPosition(home);
            if matches!(outcome, Outcome::Stuck) {
                stuck().push(id);
            }
            matches!(outcome, Outcome::Moved | Outcome::Skipped)
        };
        crate::log::append(&format!(
            "mover: окно {id} {} за {} мс",
            if moved { "перенесено" } else { "не перенесено" },
            started.elapsed().as_millis()
        ));
        *last_step() = None;
        DispatchQueue::main().exec_async(move || {
            let mtm = MainThreadMarker::new().expect("main queue");
            if let Some(delegate) = NSApplication::sharedApplication(mtm).delegate() {
                let delegate: &AnyObject = delegate.as_ref();
                let _: () = unsafe { msg_send![delegate, onTileMoved] };
            }
        });
    });
}

fn stuck() -> std::sync::MutexGuard<'static, Vec<u32>> {
    STUCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Стоит ли окно `id` правее `after` и левее `before`. Сравниваются только окна, которые
/// нарисованы оба или не нарисованы оба: ненарисованные macOS собирает левее нарисованных.
/// Сравнить не с кем — `None`; окна `id` нет — `Some(false)`.
fn placement(layout: &[IconWindow], id: u32, before: Option<u32>, after: Option<u32>) -> Option<bool> {
    let Some(item) = layout.iter().find(|window| window.id == id) else { return Some(false) };
    let comparable = |wanted: Option<u32>| {
        wanted
            .and_then(|wanted| layout.iter().find(|window| window.id == wanted))
            .filter(|window| window.onscreen == item.onscreen)
    };
    let (left, right) = (comparable(after), comparable(before));
    if left.is_none() && right.is_none() {
        return None;
    }
    Some(left.is_none_or(|left| left.x < item.x) && right.is_none_or(|right| item.x < right.x))
}

/// Ставит `id` к соседу: левее `before`, если его край на экране, иначе правее `after`.
/// Сначала адресный перенос, потом перетаскивание — только если иконка и сосед нарисованы.
fn move_next_to(id: u32, before: Option<u32>, after: Option<u32>) -> Outcome {
    mark_step();
    let layout = crate::capture::icon_layout();
    let Some(item) = layout.iter().find(|window| window.id == id) else { return Outcome::Skipped };
    if placement(&layout, id, before, after) == Some(true) {
        return Outcome::Skipped;
    }
    let find = |wanted: Option<u32>| wanted.and_then(|wanted| layout.iter().find(|window| window.id == wanted));
    let left_of = find(before).map(Destination::LeftOf);
    let right_of = find(after).map(Destination::RightOf);
    let on_screen = |destination: &Destination| match destination {
        Destination::LeftOf(target) => target.x >= 0.0,
        Destination::RightOf(target) => target.x + target.width >= 0.0,
    };
    let Some(destination) = left_of.filter(on_screen).or(right_of.filter(on_screen)).or(left_of).or(right_of) else {
        return Outcome::Skipped;
    };
    let target = match destination {
        Destination::LeftOf(window) | Destination::RightOf(window) => window,
    };
    let can_drag = item.onscreen && target.onscreen;
    let from = CGPoint::new(item.x + item.width / 2.0, item.height / 2.0);
    let to = match destination {
        Destination::LeftOf(next) => CGPoint::new(next.x + DROP_OFFSET, from.y),
        Destination::RightOf(previous) => CGPoint::new(previous.x + previous.width - DROP_OFFSET, from.y),
    };
    let started = Instant::now();
    let preferred = PREFERRED.load(Ordering::Relaxed);
    // Адресный — всегда первым: осечка одной иконки не переводит следующие на перетаскивание.
    let attempts = std::iter::once(0)
        .chain(std::iter::once(preferred).filter(|&m| m != 0))
        .chain((1..METHODS.len()).filter(move |&m| m != preferred));
    for method in attempts {
        let moved = match METHODS[method] {
            Method::Targeted => press_and_release(item, destination),
            Method::Drag(..) if !can_drag => continue,
            Method::Drag(step, via_middle) => {
                drag(from, to, step, via_middle);
                settled(id, item.x)
            }
        };
        if moved {
            PREFERRED.store(method, Ordering::Relaxed);
            let moved_in = started.elapsed().as_millis();
            // Перетаскивание ведётся по координатам: следующему нужны устоявшиеся.
            let still_in = match METHODS[method] {
                Method::Targeted => 0,
                Method::Drag(..) => wait_window_still(id).as_millis(),
            };
            crate::log::append(&format!(
                "mover: окно {id} перенесено способом {method} за {moved_in} мс, замерло за {still_in} мс"
            ));
            return Outcome::Moved;
        }
    }
    crate::log::append(&format!("mover: окно {id} не сдвинулось"));
    if can_drag {
        Outcome::Stuck
    } else {
        Outcome::Missed
    }
}

/// Ждёт, пока окно `id` уйдёт с `start_x`.
fn settled(id: u32, start_x: f64) -> bool {
    wait_moved(id, start_x, SETTLE_TIMEOUT).is_some()
}

/// Ждёт, пока окно `id` уйдёт с `start_x`, и возвращает его новое место.
fn wait_moved(id: u32, start_x: f64, timeout: Duration) -> Option<f64> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        thread::sleep(SETTLE_POLL);
        if let Some(x) = window_x(id).filter(|x| (x - start_x).abs() >= crate::capture::POSITION_TOLERANCE) {
            return Some(x);
        }
    }
    None
}

fn window_x(id: u32) -> Option<f64> {
    crate::capture::icon_layout().into_iter().find(|window| window.id == id).map(|window| window.x)
}

/// Ждёт, пока окно `id` простоит на месте `STILL_READS` замеров подряд. Возвращает,
/// сколько ждали.
fn wait_window_still(id: u32) -> Duration {
    let started = Instant::now();
    let mut last = window_x(id);
    let mut unchanged = 0;
    while started.elapsed() < STILL_TIMEOUT && unchanged < STILL_READS {
        thread::sleep(STILL_POLL);
        let now = window_x(id);
        if now == last {
            unchanged += 1;
        } else {
            unchanged = 0;
            last = now;
        }
    }
    started.elapsed()
}

/// Точки нажатия и отпускания у края соседа: иконка встаёт вплотную к нему.
fn target_points(item: &IconWindow, destination: Destination) -> (CGPoint, CGPoint) {
    match destination {
        Destination::LeftOf(target) => {
            let mut start = CGPoint::new(target.x, 0.0);
            let mut end = start;
            if item.x + item.width <= target.x {
                end.x -= item.width;
            } else {
                start.x -= 1.0;
            }
            (start, end)
        }
        Destination::RightOf(target) => {
            let mut start = CGPoint::new(target.x + target.width, 0.0);
            let mut end = start;
            if item.x <= target.x + target.width {
                end.x -= item.width;
            } else {
                start.x += 1.0;
            }
            (start, end)
        }
    }
}

/// Перенос, адресованный окнам: нажатие с Cmd — самой иконке, отпускание — соседу, обе
/// точки у края соседа, курсор по строке не ведётся. Событие получает приложение иконки.
/// Отпускание уходит и тогда, когда иконка не взялась, чтобы она не осталась зажатой.
fn press_and_release(item: &IconWindow, destination: Destination) -> bool {
    if is_cancelled() {
        return false;
    }
    let target = match destination {
        Destination::LeftOf(window) | Destination::RightOf(window) => window,
    };
    let pid = crate::click::owner_pid(item.id);
    let (start, end) = target_points(item, destination);
    post_to_window(CGEventType::LeftMouseDown, start, item.id, pid, CGEventFlags::MaskCommand);
    let lifted_x = wait_moved(item.id, item.x, TARGETED_TIMEOUT);
    // Двойное отпускание: одиночное на Tahoe иногда оставляет иконку зажатой.
    for _ in 0..2 {
        post_to_window(CGEventType::LeftMouseUp, end, target.id, pid, CGEventFlags::empty());
    }
    let moved = wait_moved(item.id, item.x, TARGETED_TIMEOUT).is_some();
    if !moved {
        crate::log::append(&format!(
            "mover: окно {} не взялось по адресу (процесс {pid:?}, нажатие {})",
            item.id,
            if lifted_x.is_some() { "сдвинуло" } else { "не сдвинуло" }
        ));
    }
    moved
}

/// Разрешает настоящей мыши и клавиатуре работать, пока идут поддельные события.
fn permit_local_events() {
    let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState);
    let source = source.as_deref();
    for state in [
        CGEventSuppressionState::EventSuppressionStateRemoteMouseDrag,
        CGEventSuppressionState::EventSuppressionStateSuppressionInterval,
    ] {
        CGEventSource::set_local_events_filter_during_suppression_state(
            source,
            CGEventFilterMask(kCGEventFilterMaskPermitAllEvents),
            state,
        );
    }
    CGEventSource::set_local_events_suppression_interval(source, 0.0);
}

/// Ждёт, пока строка меню доиграет анимацию: `STILL_READS` замеров подряд совпали
/// с предыдущим.
pub fn wait_until_still() {
    let positions = || -> Vec<(u32, f64)> {
        crate::capture::icon_layout().iter().map(|window| (window.id, window.x)).collect()
    };
    let deadline = Instant::now() + STILL_TIMEOUT;
    let mut last = positions();
    let mut unchanged = 0;
    while Instant::now() < deadline {
        thread::sleep(STILL_POLL);
        let now = positions();
        if now == last {
            unchanged += 1;
            if unchanged == STILL_READS {
                return;
            }
        } else {
            unchanged = 0;
            last = now;
        }
    }
    crate::log::append("mover: строка меню не успокоилась — считаю по последнему замеру");
}

/// Cmd+drag: нажать, (провести через середину,) довести до цели и отпустить.
/// Отменённый перенос событий не шлёт.
fn drag(from: CGPoint, to: CGPoint, step: Duration, via_middle: bool) {
    if is_cancelled() {
        return;
    }
    let cmd = CGEventFlags::MaskCommand;
    post(CGEventType::MouseMoved, from, CGEventFlags::empty());
    post(CGEventType::LeftMouseDown, from, cmd);
    thread::sleep(step);
    if via_middle {
        post(CGEventType::LeftMouseDragged, CGPoint::new((from.x + to.x) / 2.0, from.y), cmd);
        thread::sleep(step);
    }
    post(CGEventType::LeftMouseDragged, to, cmd);
    thread::sleep(step);
    post(CGEventType::LeftMouseUp, to, cmd);
}

/// Событие мыши, адресованное окну `window` полями события, а не точкой под курсором.
/// Уходит в сессию и, если известен, процессу `pid` — приложению иконки.
fn post_to_window(kind: CGEventType, at: CGPoint, window: u32, pid: Option<i32>, flags: CGEventFlags) {
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState);
    let event = CGEvent::new_mouse_event(source.as_deref(), kind, at, CGMouseButton::Left);
    let event = event.as_deref();
    CGEvent::set_flags(event, flags);
    for field in [
        CGEventField::MouseEventWindowUnderMousePointer,
        CGEventField::MouseEventWindowUnderMousePointerThatCanHandleThisEvent,
        WINDOW_ID_FIELD,
    ] {
        CGEvent::set_integer_value_field(event, field, i64::from(window));
    }
    if let Some(pid) = pid {
        CGEvent::set_integer_value_field(event, CGEventField::EventTargetUnixProcessID, i64::from(pid));
    }
    CGEvent::post(CGEventTapLocation::SessionEventTap, event);
    if let Some(pid) = pid {
        CGEvent::post_to_pid(pid, event);
    }
}

fn post(kind: CGEventType, at: CGPoint, flags: CGEventFlags) {
    let event = CGEvent::new_mouse_event(None, kind, at, CGMouseButton::Left);
    CGEvent::set_flags(event.as_deref(), flags);
    CGEvent::post(CGEventTapLocation::HIDEventTap, event.as_deref());
}
