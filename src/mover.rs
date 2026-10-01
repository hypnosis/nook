//! Перестановка настоящих иконок строки меню поддельным Cmd+drag.
//!
//! Переносим как можно меньше: самая длинная цепочка элементов, которые уже стоят
//! в нужном порядке друг относительно друга, остаётся на месте, двигаются только
//! остальные. Иконки, которые не принимают поддельный перенос, остаются на месте,
//! а остальные расставляются вокруг них. На время переноса курсор спрятан,
//! а физическая мышь отвязана от него.

use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use dispatch2::DispatchQueue;
use objc2::runtime::AnyObject;
use objc2::{msg_send, MainThreadMarker};
use objc2_app_kit::NSApplication;
use objc2_core_foundation::CGPoint;
use objc2_core_graphics::{
    CGAssociateMouseAndMouseCursorPosition, CGDisplayHideCursor, CGDisplayShowCursor, CGEvent,
    CGEventFlags, CGEventTapLocation, CGEventType, CGMainDisplayID, CGMouseButton,
    CGWarpMouseCursorPosition,
};

use crate::capture::IconWindow;

// HARDCODE: паузы между событиями переноса; вынести в конфиг позже.
const FAST_STEP: Duration = Duration::from_millis(12);
const SAFE_STEP: Duration = Duration::from_millis(50);
const SETTLE_TIMEOUT: Duration = Duration::from_millis(250);
const SETTLE_POLL: Duration = Duration::from_millis(5);
const STILL_POLL: Duration = Duration::from_millis(30);
const STILL_TIMEOUT: Duration = Duration::from_millis(600);
/// Насколько заходить за край соседа, чтобы встать перед ним или после него.
const DROP_OFFSET: f64 = 2.0;
/// Вес неподвижной иконки в цепочке: она должна попасть в цепочку при любом раскладе.
const PINNED_WEIGHT: usize = 1000;

/// Способы переноса по очереди: (пауза между событиями, вести ли через середину, название).
const METHODS: [(Duration, bool, &str); 3] = [
    (FAST_STEP, false, "напрямую к цели"),
    (FAST_STEP, true, "через середину"),
    (SAFE_STEP, true, "с паузами"),
];

/// Способ, сработавший последним, — с него и начинаем.
static PREFERRED: AtomicUsize = AtomicUsize::new(0);

/// Окна, которые подтверждённо не принимают поддельный перенос; до перезапуска.
static IMMOVABLE: Mutex<Option<HashSet<u32>>> = Mutex::new(None);

/// Чем закончилась попытка перенести одно окно.
enum Outcome {
    Moved,
    /// Перенос был нужен, но окно не сдвинулось ни одним способом.
    Stuck,
    /// Переносить не стали: окно не найдено, не нарисовано или уже на месте.
    Skipped,
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

/// Расставляет окна `order` слева направо в этом порядке. По окончании зовёт
/// `onLayoutApplied` у делегата приложения.
pub fn arrange(order: Vec<u32>) {
    thread::spawn(move || {
        let started = Instant::now();
        let home = CGEvent::location(CGEvent::new(None).as_deref());
        let (moved, skipped) = arrange_all(&order);
        post(CGEventType::MouseMoved, home, CGEventFlags::empty());
        CGWarpMouseCursorPosition(home);
        wait_until_still();
        let result: Vec<u32> = current_order(&order).iter().map(|window| window.id).collect();
        crate::log::append(&format!(
            "mover: перенесено {moved}, пропущено {skipped:?} за {} мс, порядок {}",
            started.elapsed().as_millis(),
            if result == order { "совпал" } else { "НЕ совпал" }
        ));
        DispatchQueue::main().exec_async(|| {
            let mtm = MainThreadMarker::new().expect("main queue");
            if let Some(delegate) = NSApplication::sharedApplication(mtm).delegate() {
                let delegate: &AnyObject = delegate.as_ref();
                let _: () = unsafe { msg_send![delegate, onLayoutApplied] };
            }
        });
    });
}

/// Переносит элементы вне цепочки. Окно, которое не сдвинулось, становится
/// подозреваемым: план пересчитывается так, чтобы оно стояло на месте. Неподвижным
/// подозреваемый признаётся, только если в этом же применении сдвинулось другое окно.
fn arrange_all(order: &[u32]) -> (usize, Vec<u32>) {
    let known = immovable();
    let mut suspects: HashSet<u32> = HashSet::new();
    let mut moved = 0;
    let mut skipped = Vec::new();
    for _pass in 0..=order.len() {
        let pinned: HashSet<u32> = known.union(&suspects).copied().collect();
        let current: Vec<u32> = current_order(order).iter().map(|window| window.id).collect();
        let stable = stable_chain(order, &current, &pinned);
        let mut placed: Vec<u32> = stable.clone();
        let mut replan = false;
        skipped.clear();
        for (place, &id) in order.iter().enumerate() {
            if stable.contains(&id) {
                continue;
            }
            if pinned.contains(&id) {
                skipped.push(id);
                continue;
            }
            let before = order[place + 1..].iter().find(|next| stable.contains(next)).copied();
            let after = order[..place].iter().rev().find(|previous| placed.contains(previous)).copied();
            match move_next_to(id, before, after) {
                Outcome::Moved => moved += 1,
                Outcome::Skipped => skipped.push(id),
                Outcome::Stuck => {
                    suspects.insert(id);
                    replan = true;
                    break;
                }
            }
            placed.push(id);
        }
        if !replan {
            break;
        }
    }
    confirm_immovable(&suspects, moved);
    (moved, skipped)
}

/// Подозреваемые становятся неподвижными, только если перенос в целом работает.
fn confirm_immovable(suspects: &HashSet<u32>, moved: usize) {
    if suspects.is_empty() {
        return;
    }
    if moved == 0 {
        crate::log::append(&format!(
            "mover: не сдвинулось ничего — {suspects:?} неподвижными не считаю"
        ));
        return;
    }
    let mut guard = IMMOVABLE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.get_or_insert_with(HashSet::new).extend(suspects);
    crate::log::append(&format!("mover: окна {suspects:?} неподвижны, ставлю остальные вокруг"));
}

fn immovable() -> HashSet<u32> {
    let guard = IMMOVABLE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.clone().unwrap_or_default()
}

/// Самая тяжёлая подпоследовательность `current`, которая уже идёт в порядке `order`.
/// Обычное окно весит 1, неподвижное — `PINNED_WEIGHT`.
fn stable_chain(order: &[u32], current: &[u32], pinned: &HashSet<u32>) -> Vec<u32> {
    let ranks: Vec<usize> = current
        .iter()
        .filter_map(|id| order.iter().position(|wanted| wanted == id))
        .collect();
    let weight = |i: usize| if pinned.contains(&order[ranks[i]]) { PINNED_WEIGHT } else { 1 };
    let mut total: Vec<usize> = (0..ranks.len()).map(weight).collect();
    let mut previous = vec![None; ranks.len()];
    for i in 0..ranks.len() {
        for j in 0..i {
            if ranks[j] < ranks[i] && total[j] + weight(i) > total[i] {
                total[i] = total[j] + weight(i);
                previous[i] = Some(j);
            }
        }
    }
    let mut chain = Vec::new();
    let mut cursor = (0..ranks.len()).max_by_key(|&i| total[i]);
    while let Some(i) = cursor {
        chain.push(order[ranks[i]]);
        cursor = previous[i];
    }
    chain
}

/// Ставит `id` перед `before` или, если его нет, после `after`.
fn move_next_to(id: u32, before: Option<u32>, after: Option<u32>) -> Outcome {
    let layout = crate::capture::icon_layout();
    let find = |wanted: u32| layout.iter().find(|window| window.id == wanted);
    let Some(item) = find(id) else { return Outcome::Skipped };
    if !item.onscreen {
        crate::log::append(&format!("mover: окно {id} не нарисовано (под чёлкой) — пропускаю"));
        return Outcome::Skipped;
    }
    let (target_x, touching) = match (before.and_then(find), after.and_then(find)) {
        (Some(next), _) => (next.x + DROP_OFFSET, item.x + item.width - next.x),
        (None, Some(previous)) => {
            (previous.x + previous.width - DROP_OFFSET, previous.x + previous.width - item.x)
        }
        (None, None) => return Outcome::Skipped,
    };
    if touching.abs() < 1.0 || (item.x..=item.x + item.width).contains(&target_x) {
        crate::log::append(&format!("mover: окно {id} уже на месте"));
        return Outcome::Skipped;
    }
    let from = CGPoint::new(item.x + item.width / 2.0, item.height / 2.0);
    let to = CGPoint::new(target_x, from.y);
    let started = Instant::now();
    let preferred = PREFERRED.load(Ordering::Relaxed);
    let attempts = std::iter::once(preferred).chain((0..METHODS.len()).filter(|&m| m != preferred));
    for method in attempts {
        let (step, via_middle, name) = METHODS[method];
        drag(from, to, step, via_middle);
        if settled(id, item.x) {
            PREFERRED.store(method, Ordering::Relaxed);
            crate::log::append(&format!(
                "mover: окно {id} перенесено ({name}) за {} мс",
                started.elapsed().as_millis()
            ));
            return Outcome::Moved;
        }
    }
    crate::log::append(&format!("mover: окно {id} не сдвинулось"));
    Outcome::Stuck
}

/// Окна из `order` в их нынешнем порядке слева направо.
fn current_order(order: &[u32]) -> Vec<IconWindow> {
    crate::capture::icon_layout()
        .into_iter()
        .filter(|window| order.contains(&window.id))
        .collect()
}

/// Ждёт, пока окно `id` уйдёт с `start_x`.
fn settled(id: u32, start_x: f64) -> bool {
    let deadline = Instant::now() + SETTLE_TIMEOUT;
    while Instant::now() < deadline {
        thread::sleep(SETTLE_POLL);
        let x = crate::capture::icon_layout().into_iter().find(|window| window.id == id).map(|window| window.x);
        if x.is_some_and(|x| (x - start_x).abs() >= 1.0) {
            return true;
        }
    }
    false
}

/// Ждёт, пока строка меню доиграет анимацию: два замера подряд совпали.
fn wait_until_still() {
    let positions = || -> Vec<(u32, f64)> {
        crate::capture::icon_layout().iter().map(|window| (window.id, window.x)).collect()
    };
    let deadline = Instant::now() + STILL_TIMEOUT;
    let mut last = positions();
    while Instant::now() < deadline {
        thread::sleep(STILL_POLL);
        let now = positions();
        if now == last {
            return;
        }
        last = now;
    }
    crate::log::append("mover: строка меню не успокоилась — считаю по последнему замеру");
}

/// Cmd+drag: нажать, (провести через середину,) довести до цели и отпустить.
fn drag(from: CGPoint, to: CGPoint, step: Duration, via_middle: bool) {
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

fn post(kind: CGEventType, at: CGPoint, flags: CGEventFlags) {
    let event = CGEvent::new_mouse_event(None, kind, at, CGMouseButton::Left);
    CGEvent::set_flags(event.as_deref(), flags);
    CGEvent::post(CGEventTapLocation::HIDEventTap, event.as_deref());
}
