//! Перестановка настоящих иконок строки меню поддельным Cmd+drag.
//!
//! Переносим как можно меньше: самая длинная цепочка элементов, которые уже стоят
//! в нужном порядке друг относительно друга, остаётся на месте, двигаются только
//! остальные. Иконки, которые не принимают поддельный перенос, остаются на месте,
//! а остальные расставляются вокруг них. Спрятанные у чёлки иконки мышью не взять, и стоят
//! они не на своём месте, поэтому в расчёт идут только нарисованные. На время переноса
//! курсор спрятан, а физическая мышь отвязана от него.

use std::cell::Cell;
use std::collections::HashSet;
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
/// Сколько добивочных проходов делать, если порядок после переносов не совпал.
const MAX_CORRECTIONS: u32 = 1;
/// Насколько заходить за край соседа, чтобы встать перед ним или после него.
const DROP_OFFSET: f64 = 2.0;
/// Вес неподвижной иконки в цепочке: она должна попасть в цепочку при любом раскладе.
const PINNED_WEIGHT: usize = 1000;
/// Насколько заходить в чёлку, бросая туда иконку.
// HARDCODE: заход в чёлку; вынести в конфиг позже.
const NOTCH_DROP_INSET: f64 = 5.0;

/// Сколько ждать ответа иконки на нажатие или отпускание, адресованное окну.
const TARGETED_TIMEOUT: Duration = Duration::from_millis(150);
/// Пауза между переносами: строка меню успевает принять следующий.
const MOVE_BUFFER: Duration = Duration::from_millis(25);
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

/// Номер текущего переноса: растёт при каждом запуске и отмене.
static RUN: AtomicU64 = AtomicU64::new(0);

thread_local! {
    /// Номер переноса, который ведёт этот поток.
    static OWN_RUN: Cell<u64> = const { Cell::new(0) };
}

/// Чем закончилась попытка перенести одно окно.
enum Outcome {
    Moved,
    /// Перенос был нужен, но окно не сдвинулось ни одним способом.
    Stuck,
    /// Перенос нужен, но иконка спрятана у чёлки: мышью её не взять.
    Hidden,
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

/// Расставляет окна `order` слева направо в этом порядке. `own` — наш разделитель:
/// неподвижным он не бывает. По окончании зовёт `onLayoutApplied:landed:` у делегата
/// приложения: спрятала ли чёлка иконку, которую надо было перенести, и встали ли
/// нарисованные иконки по порядку (ни одной не видно — не встали).
pub fn arrange(order: Vec<u32>, own: u32, notch_edge: Option<f64>) {
    let run = RUN.fetch_add(1, Ordering::SeqCst) + 1;
    *last_step() = Some(Instant::now());
    thread::spawn(move || {
        OWN_RUN.set(run);
        let started = Instant::now();
        crate::click::remember_owners_now(&order);
        crate::log::append(&format!("mover: владельцы иконок найдены за {} мс", started.elapsed().as_millis()));
        permit_local_events();
        let order = with_standing_panel(order, own);
        let home = CGEvent::location(CGEvent::new(None).as_deref());
        let (mut skipped, mut cramped) = arrange_all(&order, own);
        if notch_edge.is_some_and(|edge| main_row_hidden(&order, own) && push_into_notch(&order, own, edge)) {
            (skipped, cramped) = arrange_all(&order, own);
        }
        if is_cancelled() {
            crate::log::append("mover: перенос отменён");
            return;
        }
        post(CGEventType::MouseMoved, home, CGEventFlags::empty());
        CGWarpMouseCursorPosition(home);
        let result = visible_order(&order);
        let landed = !cramped && !result.is_empty() && result == wanted_of(&order, &result);
        if !landed {
            crate::log::append(&format!("mover: порядок не совпал {result:?}, пропущено {skipped:?}"));
        }
        crate::log::append(&format!("mover: перенос занял {} мс", started.elapsed().as_millis()));
        *last_step() = None;
        DispatchQueue::main().exec_async(move || {
            let mtm = MainThreadMarker::new().expect("main queue");
            if let Some(delegate) = NSApplication::sharedApplication(mtm).delegate() {
                let delegate: &AnyObject = delegate.as_ref();
                let _: () = unsafe { msg_send![delegate, onLayoutApplied: cramped, landed: landed] };
            }
        });
    });
}

/// Ставит одну иконку `id` левее `before` и правее `after` и сообщает `onTileMoved:`,
/// встала ли она туда.
pub fn move_one(id: u32, before: Option<u32>, after: Option<u32>) {
    let run = RUN.fetch_add(1, Ordering::SeqCst) + 1;
    *last_step() = Some(Instant::now());
    thread::spawn(move || {
        OWN_RUN.set(run);
        let started = Instant::now();
        crate::click::remember_owners_now(&[id]);
        permit_local_events();
        let home = CGEvent::location(CGEvent::new(None).as_deref());
        let outcome = move_next_to(id, before, after, false);
        post(CGEventType::MouseMoved, home, CGEventFlags::empty());
        CGWarpMouseCursorPosition(home);
        let placed = matches!(outcome, Outcome::Moved | Outcome::Skipped) && wait_between(id, before, after);
        crate::log::append(&format!(
            "mover: окно {id} {} за {} мс",
            if placed { "встало" } else { "не встало" },
            started.elapsed().as_millis()
        ));
        *last_step() = None;
        DispatchQueue::main().exec_async(move || {
            let mtm = MainThreadMarker::new().expect("main queue");
            if let Some(delegate) = NSApplication::sharedApplication(mtm).delegate() {
                let delegate: &AnyObject = delegate.as_ref();
                let _: () = unsafe { msg_send![delegate, onTileMoved: placed] };
            }
        });
    });
}

/// Ждёт, пока окно `id` встанет между `after` и `before`; не встало за `SETTLE_TIMEOUT` — `false`.
fn wait_between(id: u32, before: Option<u32>, after: Option<u32>) -> bool {
    let deadline = Instant::now() + SETTLE_TIMEOUT;
    loop {
        if stands_between(id, before, after) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(SETTLE_POLL);
    }
}

/// Окно `id` стоит правее `after` и левее `before` — тех из них, что нарисованы.
/// Спрятанное у чёлки окно стоит левее всех нарисованных.
fn stands_between(id: u32, before: Option<u32>, after: Option<u32>) -> bool {
    let layout = crate::capture::icon_layout();
    let Some(item) = layout.iter().find(|window| window.id == id) else { return false };
    let drawn_x = |wanted: Option<u32>| {
        wanted.and_then(|wanted| layout.iter().find(|window| window.id == wanted && window.onscreen)).map(|window| window.x)
    };
    let (left, right) = (drawn_x(after), drawn_x(before));
    if !item.onscreen {
        return left.is_none() && right.is_some();
    }
    (left.is_some() || right.is_some()) && left.is_none_or(|x| x < item.x) && right.is_none_or(|x| item.x < x)
}

/// Переносит нарисованные окна вне цепочки. Окно, которое не сдвинулось, до конца этого
/// применения стоит на месте: план пересчитывается, остальные ставятся вокруг него.
/// Если после прохода порядок всё же не совпал, делается добивочный проход.
/// Возвращает пропущенные окна и признак, что иконку не перенести из-за чёлки.
fn arrange_all(order: &[u32], own: u32) -> (Vec<u32>, bool) {
    // Разделитель не переносим: иконки встают вокруг него.
    let mut pinned: HashSet<u32> = HashSet::from([own]);
    let mut skipped = Vec::new();
    let mut cramped = false;
    let mut corrections = 0;
    loop {
        if is_cancelled() {
            break;
        }
        wait_order_still(order);
        let current = visible_order(order);
        let wanted = wanted_of(order, &current);
        let stable = stable_chain(&wanted, &current, &pinned);
        if stable.len() == current.len() {
            break;
        }
        if corrections > 0 {
            crate::log::append(&format!("mover: порядок не совпал ({current:?}) — добиваю"));
        }
        let mut placed: Vec<u32> = stable.clone();
        let mut replan = false;
        let mut moved_now = 0;
        skipped.clear();
        for (place, &id) in wanted.iter().enumerate() {
            if stable.contains(&id) {
                continue;
            }
            if pinned.contains(&id) {
                if id != own {
                    skipped.push(id);
                }
                continue;
            }
            let before = wanted[place + 1..].iter().find(|next| stable.contains(next)).copied();
            let after = wanted[..place].iter().rev().find(|previous| placed.contains(previous)).copied();
            match move_next_to(id, before, after, moved_now > 0) {
                Outcome::Moved => moved_now += 1,
                Outcome::Hidden => {
                    cramped = true;
                    skipped.push(id);
                }
                Outcome::Skipped => {}
                Outcome::Stuck => {
                    pinned.insert(id);
                    replan = true;
                    break;
                }
            }
            placed.push(id);
        }
        if replan {
            continue;
        }
        if moved_now == 0 {
            break;
        }
        if corrections == MAX_CORRECTIONS {
            wait_order_still(order);
            break;
        }
        corrections += 1;
    }
    pinned.remove(&own);
    if !pinned.is_empty() {
        crate::log::append(&format!("mover: окна {pinned:?} не сдвинулись, остальные поставил вокруг"));
    }
    (skipped, cramped)
}

/// Тесная строка: у ≡◂ видны не те окна. Как человек руками, бросаем в чёлку (`edge` — её
/// правый край) видимое окно, которому по `order` место левее спрятанных, и смотрим, что
/// вылезло, — пока видимыми не останутся последние окна `order`. Порядок между ними потом
/// ставит обычный перенос. Свой разделитель `own` не бросаем. Возвращает, бросили ли что-то.
fn push_into_notch(order: &[u32], own: u32, edge: f64) -> bool {
    let mut pushed = false;
    for _ in 0..order.len() * 2 {
        if is_cancelled() {
            break;
        }
        wait_order_still(order);
        let visible = visible_order(order);
        let tail = &order[order.len().saturating_sub(visible.len())..];
        let Some(&id) = visible.iter().find(|&&id| id != own && !tail.contains(&id)) else { break };
        let layout = crate::capture::icon_layout();
        let Some(item) = layout.iter().find(|window| window.id == id) else { break };
        mark_step();
        let from = CGPoint::new(item.x + item.width / 2.0, item.height / 2.0);
        let to = CGPoint::new(edge - NOTCH_DROP_INSET, from.y);
        let (step, via_middle) = match METHODS[PREFERRED.load(Ordering::Relaxed)] {
            Method::Drag(step, via_middle) => (step, via_middle),
            Method::Targeted => (FAST_STEP, false),
        };
        drag(from, to, step, via_middle);
        if !settled(id, item.x) {
            crate::log::append(&format!("mover: окно {id} в чёлку не ушло"));
            break;
        }
        pushed = true;
    }
    pushed
}

/// `order` с иконками панели (всё левее `own`) в том порядке, в каком они стоят в строке:
/// панель рисует их по реестру, в строке им нужно только быть левее разделителя.
fn with_standing_panel(order: Vec<u32>, own: u32) -> Vec<u32> {
    let Some(own_place) = order.iter().position(|&id| id == own) else { return order };
    let (panel, rest) = order.split_at(own_place);
    let standing = current_order(panel);
    let mut arranged: Vec<u32> = standing.iter().map(|window| window.id).collect();
    let absent: Vec<u32> = panel.iter().filter(|id| !arranged.contains(id)).copied().collect();
    arranged.extend(absent);
    arranged.extend_from_slice(rest);
    arranged
}

/// Иконка основного ряда (правее `own` в `order`) не нарисована: строке тесно.
fn main_row_hidden(order: &[u32], own: u32) -> bool {
    let Some(own_place) = order.iter().position(|&id| id == own) else { return false };
    let main = &order[own_place + 1..];
    current_order(main).iter().any(|window| !window.onscreen)
}

/// Окна `order`, которые сейчас нарисованы, слева направо.
fn visible_order(order: &[u32]) -> Vec<u32> {
    current_order(order).iter().filter(|window| window.onscreen).map(|window| window.id).collect()
}

/// Окна `order`, которые есть среди `present`, в порядке `order`.
fn wanted_of(order: &[u32], present: &[u32]) -> Vec<u32> {
    order.iter().filter(|id| present.contains(id)).copied().collect()
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

/// Ставит `id` перед `before` или, если его нет, после `after`; бросаем на ближний
/// край соседа со стороны, откуда едет иконка. После предыдущего переноса (`after_move`)
/// выдерживается короткая пауза.
fn move_next_to(id: u32, before: Option<u32>, after: Option<u32>, after_move: bool) -> Outcome {
    mark_step();
    if after_move {
        thread::sleep(MOVE_BUFFER);
    }
    let layout = crate::capture::icon_layout();
    let Some(item) = layout.iter().find(|window| window.id == id) else { return Outcome::Skipped };
    let others: Vec<&IconWindow> = layout.iter().filter(|window| window.id != id && window.onscreen).collect();
    let position = |wanted: u32| others.iter().position(|window| window.id == wanted);
    let (left, right) = match (before.and_then(position), after.and_then(position)) {
        (Some(next), _) => (next.checked_sub(1).map(|i| others[i]), Some(others[next])),
        (None, Some(previous)) => (Some(others[previous]), others.get(previous + 1).copied()),
        (None, None) => return Outcome::Skipped,
    };
    // HARDCODE: проба — адресный перенос иконки, спрятанной у чёлки; убрать после пробы.
    if !item.onscreen {
        let destination = match (left, right) {
            (Some(previous), _) => Destination::RightOf(previous),
            (None, Some(next)) => Destination::LeftOf(next),
            _ => return Outcome::Hidden,
        };
        crate::log::append(&format!(
            "проба: окно {id} под чёлкой, x {}, процесс {:?}",
            item.x,
            crate::click::owner_pid(id)
        ));
        let started = Instant::now();
        let moved = press_and_release(item, destination);
        crate::log::append(&format!(
            "проба: окно {id} {} за {} мс",
            if moved { "сдвинулось" } else { "не сдвинулось" },
            started.elapsed().as_millis()
        ));
        return if moved { Outcome::Moved } else { Outcome::Hidden };
    }
    let after_left = left.is_none_or(|window| window.x < item.x);
    let before_right = right.is_none_or(|window| item.x < window.x);
    if after_left && before_right {
        return Outcome::Skipped;
    }
    if !item.onscreen {
        crate::log::append(&format!("mover: окно {id} не нарисовано (под чёлкой) — пропускаю"));
        return Outcome::Hidden;
    }
    let (destination, target_x) = match (left, right) {
        (_, Some(next)) if item.x > next.x => (Destination::LeftOf(next), next.x + DROP_OFFSET),
        (Some(previous), _) => (Destination::RightOf(previous), previous.x + previous.width - DROP_OFFSET),
        _ => return Outcome::Skipped,
    };
    let from = CGPoint::new(item.x + item.width / 2.0, item.height / 2.0);
    let to = CGPoint::new(target_x, from.y);
    let started = Instant::now();
    let preferred = PREFERRED.load(Ordering::Relaxed);
    // Адресный — всегда первым: осечка одной иконки не переводит следующие на перетаскивание.
    let attempts = std::iter::once(0)
        .chain(std::iter::once(preferred).filter(|&m| m != 0))
        .chain((1..METHODS.len()).filter(move |&m| m != preferred));
    for method in attempts {
        let moved = match METHODS[method] {
            Method::Targeted => press_and_release(item, destination),
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

/// Ждёт, пока порядок окон `order` в строке простоит `STILL_READS` замеров подряд.
/// Сравнивается порядок, а не координаты: иконки с живой шириной не мешают.
fn wait_order_still(order: &[u32]) {
    let started = Instant::now();
    let snapshot = || -> Vec<(u32, bool)> {
        current_order(order).iter().map(|window| (window.id, window.onscreen)).collect()
    };
    let mut last = snapshot();
    let mut unchanged = 0;
    while unchanged < STILL_READS {
        if started.elapsed() >= STILL_TIMEOUT {
            crate::log::append("mover: порядок не успокоился — считаю по последнему замеру");
            return;
        }
        thread::sleep(STILL_POLL);
        let now = snapshot();
        if now == last {
            unchanged += 1;
        } else {
            unchanged = 0;
            last = now;
        }
    }
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
