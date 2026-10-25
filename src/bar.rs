//! Чтение строки меню: единственное место, откуда раскладка узнаёт, что стоит в строке.
//! Читает в фоне, когда строка встала, и отдаёт снимок делегату через `onBarRead`.

use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::Instant;

use dispatch2::DispatchQueue;
use objc2::runtime::AnyObject;
use objc2::{msg_send, MainThreadMarker};
use objc2_app_kit::NSApplication;

use crate::capture::IconWindow;
use crate::layout::{Icon, Reading};
use crate::tuning::{
    BAR_POLL, HIDING_WIDTH_MIN, NARROW_ITEM_WIDTH, NARROW_WINDOW_MAX, NOTCH_CLEARANCE, PANEL_CAPTURE_DELAY,
    POSITION_TOLERANCE, SCREEN_MARGIN, SHROUD_MARGIN, WINDOW_CHROME,
};

/// Окно иконки в прочитанной строке.
pub struct Slot {
    pub id: u32,
    pub x: f64,
    pub width: f64,
    pub onscreen: bool,
    /// Приложение и номер иконки внутри него; приложение не найдено — `None`.
    pub key: Option<String>,
}

/// Строка, прочитанная за один раз, когда она стояла.
pub struct Snapshot {
    /// Поколение строки на момент чтения: Nook с тех пор её менял — снимок устарел.
    pub epoch: u64,
    /// Иконки левее ≡◂ слева направо, без разделителя.
    pub icons: Vec<Slot>,
    pub divider: Option<Slot>,
    spacer_x: f64,
    /// Правый край меню активного приложения.
    menu_right: Option<f64>,
}

impl Snapshot {
    /// Стороны разделителя. Узкий разделитель, которого macOS спрятала, — `None`: иконкам у
    /// чёлки она даёт условные x, и стороны по ним не определить. Длинный спрятан всегда,
    /// и всё левее него уходит за ним по порядку.
    pub fn reading(&self) -> Option<Reading> {
        let divider = self.divider.as_ref().filter(|divider| divider.onscreen || divider.width > NARROW_WINDOW_MAX)?;
        let icon = |slot: &Slot| Icon { id: slot.id, key: slot.key.clone(), drawn: slot.onscreen };
        let (panel, main): (Vec<&Slot>, Vec<&Slot>) = self.icons.iter().partition(|slot| slot.x < divider.x);
        Some(Reading { panel: panel.into_iter().map(icon).collect(), main: main.into_iter().map(icon).collect() })
    }

    /// Авторежим: в панели то, что macOS не нарисовала, в основном ряду — нарисованное.
    pub fn automatic(&self) -> (Vec<u32>, Vec<u32>) {
        let (panel, main): (Vec<&Slot>, Vec<&Slot>) = self.icons.iter().partition(|slot| !slot.onscreen);
        (panel.iter().map(|slot| slot.id).collect(), main.iter().map(|slot| slot.id).collect())
    }

    /// x первой иконки основного ряда, без основного ряда — x спейсера.
    pub fn main_left(&self) -> Option<f64> {
        let divider = self.divider.as_ref()?;
        Some(self.icons.iter().find(|slot| slot.x > divider.x + POSITION_TOLERANCE).map_or(self.spacer_x, |slot| slot.x))
    }

    /// Длина разделителя. Узкий, пока справа от чёлки хватает места ему и самой широкой иконке
    /// панели: иконки панели стоят у чёлки. Иначе, и когда macOS спрятала узкий, — самый короткий
    /// из тех, что macOS прячет всегда: иконки панели встают вплотную левее него, ближе к чёлке.
    pub fn divider_length(&self, notch_right: Option<f64>) -> Option<f64> {
        let divider = self.divider.as_ref()?;
        let main_left = self.main_left()?;
        let panel: Vec<&Slot> = self.icons.iter().filter(|slot| slot.x < divider.x).collect();
        let evicted = divider.width <= NARROW_WINDOW_MAX && !divider.onscreen;
        let widest = panel.iter().map(|slot| slot.width).fold(0.0, f64::max);
        let room = notch_right.map_or(f64::INFINITY, |notch| main_left - notch);
        if !evicted && room >= NOTCH_CLEARANCE + NARROW_ITEM_WIDTH + WINDOW_CHROME + widest {
            return Some(NARROW_ITEM_WIDTH);
        }
        let panel_width: f64 = panel.iter().map(|slot| slot.width).sum();
        if main_left - WINDOW_CHROME - HIDING_WIDTH_MIN - panel_width < SCREEN_MARGIN {
            log::warn!("иконок панели слишком много ({panel_width} pt) — часть уйдёт за край");
        }
        Some(HIDING_WIDTH_MIN)
    }

    /// Для журнала: сколько нарисованных иконок панели не закрыто полосой `shown` и сколько
    /// иконок основного ряда macOS не нарисовала.
    pub fn shroud_report(&self, shown: Option<(f64, f64)>) -> (usize, usize) {
        let Some(divider) = self.divider.as_ref() else { return (0, 0) };
        let covered = |slot: &Slot| {
            shown.is_some_and(|(left, right)| {
                slot.x >= left - POSITION_TOLERANCE && slot.x + slot.width <= right + POSITION_TOLERANCE
            })
        };
        let outside = self.icons.iter().filter(|slot| slot.onscreen && slot.x < divider.x && !covered(slot)).count();
        let main_hidden = self.icons.iter().filter(|slot| !slot.onscreen && slot.x > divider.x).count();
        (outside, main_hidden)
    }

    /// Полоса шторки (левый и правый x): от самой левой нарисованной иконки панели с запасом
    /// на подсветку до основного ряда. Иконок панели в строке не видно, разделитель не нарисован
    /// или меню активного приложения заходит в полосу — `None`.
    pub fn shroud_strip(&self) -> Option<(f64, f64)> {
        let divider = self.divider.as_ref().filter(|divider| divider.onscreen)?;
        let left = self
            .icons
            .iter()
            .filter(|slot| slot.onscreen && slot.x < divider.x)
            .map(|slot| slot.x - SHROUD_MARGIN)
            .reduce(f64::min)?;
        if menu_covers(self.menu_right, left) {
            return None;
        }
        Some((left, self.main_left()?))
    }
}

/// Меню приложения с правым краем `menu_right` заходит правее `left`. Там, где лежит меню,
/// macOS сама прячет иконки под ним, и шторка закрыла бы само меню.
pub fn menu_covers(menu_right: Option<f64>, left: f64) -> bool {
    menu_right.is_some_and(|right| right > left)
}

/// Состояние элементов Nook в строке.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    /// `<` виден, разделитель левее `|`, `|` левее `<`.
    Ok,
    /// Окна ещё нет или оно не размещено: ждём.
    Unknown,
    /// `<` виден, но порядок нарушен.
    Misordered,
    /// `<` за краем или под чёлкой.
    Hidden,
}

/// Вердикт по окнам Nook в строке; строка — их координаты для журнала.
pub fn nook_order(notch_right: Option<f64>) -> (Order, String) {
    let windows = crate::capture::icon_layout();
    let find = |name: &str| windows.iter().find(|window| window.name == name);
    let anchor = find(crate::status_bar::ANCHOR_AUTOSAVE);
    let spacer = find(crate::status_bar::SPACER_AUTOSAVE);
    let divider = find(crate::divider::AUTOSAVE);
    let at = |window: Option<&IconWindow>| window.map(|w| format!("{}+{}{}", w.x, w.width, if w.onscreen { "" } else { " скрыт" }));
    let coords = format!("< {:?}, | {:?}, разделитель {:?}, чёлка {notch_right:?}", at(anchor), at(spacer), at(divider));
    if !windows.iter().any(|window| window.onscreen) {
        return (Order::Unknown, format!("строка меню скрыта; {coords}"));
    }
    (classify(anchor, spacer, divider, notch_right), coords)
}

fn classify(
    anchor: Option<&IconWindow>,
    spacer: Option<&IconWindow>,
    divider: Option<&IconWindow>,
    notch_right: Option<f64>,
) -> Order {
    let (Some(anchor), Some(spacer)) = (anchor, spacer) else { return Order::Unknown };
    if anchor.x == UNPLACED_X || spacer.x == UNPLACED_X {
        return Order::Unknown;
    }
    if !anchor.onscreen || notch_right.is_some_and(|notch| anchor.x < notch) {
        return Order::Hidden;
    }
    let divider_x = divider.filter(|divider| divider.width > 0.0 && divider.x != UNPLACED_X).map(|divider| divider.x);
    if spacer.x >= anchor.x || divider_x.is_some_and(|x| x >= spacer.x) {
        return Order::Misordered;
    }
    Order::Ok
}

/// x окна, которое macOS ещё не разместила.
const UNPLACED_X: f64 = 0.0;

static LATEST: Mutex<Option<Snapshot>> = Mutex::new(None);

/// Читает строку в фоне: ждёт, пока она раскрыта и стоит, и зовёт `onBarRead` у делегата.
/// Строка свёрнута — снимка нет, `take` вернёт `None`.
pub fn read_when_still(epoch: u64) {
    thread::spawn(move || {
        let deadline = Instant::now() + PANEL_CAPTURE_DELAY;
        while spacer_x().is_none() && Instant::now() < deadline {
            thread::sleep(BAR_POLL);
        }
        let windows = crate::mover::wait_until_still();
        let snapshot = read(&windows, epoch, true);
        let mut latest = LATEST.lock().unwrap_or_else(PoisonError::into_inner);
        // Чтение, закончившееся позже, не затирает более свежее.
        let older = matches!((&*latest, &snapshot), (Some(kept), Some(new)) if new.epoch < kept.epoch);
        if !older {
            *latest = snapshot;
        }
        drop(latest);
        DispatchQueue::main().exec_async(|| {
            let mtm = MainThreadMarker::new().expect("main queue");
            if let Some(delegate) = NSApplication::sharedApplication(mtm).delegate() {
                let delegate: &AnyObject = delegate.as_ref();
                let _: () = unsafe { msg_send![delegate, onBarRead] };
            }
        });
    });
}

/// Читает строку сразу, без ожидания. С `owners` ищет приложения иконок через
/// Accessibility — тогда не из главного потока.
pub fn read_now(owners: bool) -> Option<Snapshot> {
    read(&crate::capture::icon_layout(), 0, owners)
}

/// Последний снимок; забирается один раз.
pub fn take() -> Option<Snapshot> {
    LATEST.lock().unwrap_or_else(PoisonError::into_inner).take()
}

/// Левый край спейсера, если строка раскрыта.
fn spacer_x() -> Option<f64> {
    crate::capture::icon_layout()
        .into_iter()
        .find(|window| window.name == crate::status_bar::SPACER_AUTOSAVE && window.onscreen)
        .map(|window| window.x)
}

fn read(windows: &[IconWindow], epoch: u64, owners: bool) -> Option<Snapshot> {
    let spacer = windows.iter().find(|window| window.name == crate::status_bar::SPACER_AUTOSAVE)?;
    if !spacer.onscreen {
        return None;
    }
    let spacer_x = spacer.x;
    let left: Vec<_> = windows.iter().filter(|window| window.x < spacer_x).collect();
    if owners {
        let ids: Vec<u32> = left.iter().map(|window| window.id).collect();
        crate::click::remember_owners_now(&ids);
    }
    let menu_right = owners.then(crate::click::app_menu_right).flatten();

    let mut icons = Vec::new();
    let mut divider = None;
    let mut seen: Vec<String> = Vec::new();
    for window in left {
        let is_divider = window.name == crate::divider::AUTOSAVE;
        let key = (!is_divider).then(|| crate::click::owner_name(window.id)).flatten().map(|name| {
            let index = seen.iter().filter(|other| **other == name).count();
            seen.push(name.clone());
            if index == 0 { name } else { format!("{name}#{}", index + 1) }
        });
        let slot = Slot { id: window.id, x: window.x, width: window.width, onscreen: window.onscreen, key };
        if is_divider {
            divider = (window.width > 0.0).then_some(slot);
        } else {
            icons.push(slot);
        }
    }
    let at = |slot: &Slot| {
        let hidden = if slot.onscreen { "" } else { " скрыт" };
        format!("{}:{}@{:.0}+{:.0}{hidden}", slot.id, slot.key.as_deref().unwrap_or("?"), slot.x, slot.width)
    };
    log::debug!(
        "строка: иконки [{}], разделитель {:?}, спейсер @{spacer_x:.0}, меню до {menu_right:?}",
        icons.iter().map(at).collect::<Vec<_>>().join(", "),
        divider.as_ref().map(at)
    );
    Some(Snapshot { epoch, icons, divider, spacer_x, menu_right })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(x: f64, width: f64, onscreen: bool) -> IconWindow {
        IconWindow { id: 0, name: String::new(), x, width, height: 24.0, onscreen }
    }

    const NOTCH: Option<f64> = Some(828.0);

    #[test]
    fn expanded_in_order_is_ok() {
        let (a, s, d) = (window(1102.0, 32.0, true), window(1080.0, 14.0, true), window(900.0, 500.0, false));
        assert_eq!(classify(Some(&a), Some(&s), Some(&d), NOTCH), Order::Ok);
    }

    #[test]
    fn collapsed_spacer_off_screen_is_ok() {
        let (a, s) = (window(1102.0, 32.0, true), window(-602.0, 1698.0, false));
        assert_eq!(classify(Some(&a), Some(&s), None, NOTCH), Order::Ok);
    }

    #[test]
    fn anchor_left_of_spacer_is_misordered() {
        let (a, s) = (window(900.0, 32.0, true), window(1113.0, 14.0, true));
        assert_eq!(classify(Some(&a), Some(&s), None, NOTCH), Order::Misordered);
    }

    #[test]
    fn divider_right_of_spacer_is_misordered() {
        let (a, s, d) = (window(1102.0, 32.0, true), window(1000.0, 14.0, true), window(1050.0, 14.0, true));
        assert_eq!(classify(Some(&a), Some(&s), Some(&d), NOTCH), Order::Misordered);
    }

    #[test]
    fn anchor_under_notch_is_hidden() {
        let (a, s) = (window(800.0, 32.0, true), window(700.0, 14.0, true));
        assert_eq!(classify(Some(&a), Some(&s), None, NOTCH), Order::Hidden);
    }

    #[test]
    fn anchor_off_screen_is_hidden() {
        let (a, s) = (window(8.0, 32.0, false), window(1155.0, 14.0, true));
        assert_eq!(classify(Some(&a), Some(&s), None, NOTCH), Order::Hidden);
    }

    #[test]
    fn no_notch_checks_only_order() {
        let (a, s) = (window(300.0, 32.0, true), window(200.0, 14.0, true));
        assert_eq!(classify(Some(&a), Some(&s), None, None), Order::Ok);
    }

    fn slot(x: f64, width: f64, onscreen: bool) -> Slot {
        Slot { id: x as u32, x, width, onscreen, key: None }
    }

    /// Узкий разделитель на 935: две иконки панели под чёлкой, две нарисованы справа от неё.
    fn narrow_bar(divider_onscreen: bool) -> Snapshot {
        Snapshot {
            epoch: 0,
            icons: vec![
                slot(794.0, 34.0, false),
                slot(828.0, 33.0, false),
                slot(861.0, 34.0, true),
                slot(895.0, 40.0, true),
                slot(963.0, 45.0, true),
                slot(1008.0, 34.0, true),
            ],
            divider: Some(slot(935.0, 28.0, divider_onscreen)),
            spacer_x: 1122.0,
            menu_right: Some(700.0),
        }
    }

    #[test]
    fn no_shroud_when_app_menu_reaches_strip() {
        let mut bar = narrow_bar(true);
        bar.menu_right = Some(900.0);
        assert_eq!(bar.shroud_strip(), None);
    }

    #[test]
    fn shroud_covers_drawn_panel_icons_up_to_main_row() {
        assert_eq!(narrow_bar(true).shroud_strip(), Some((861.0 - SHROUD_MARGIN, 963.0)));
    }

    #[test]
    fn shroud_reaches_spacer_without_main_row() {
        let mut bar = narrow_bar(true);
        bar.icons.truncate(4);
        assert_eq!(bar.shroud_strip(), Some((861.0 - SHROUD_MARGIN, 1122.0)));
    }

    #[test]
    fn no_shroud_without_drawn_panel_icons() {
        let mut bar = narrow_bar(true);
        bar.icons[2].onscreen = false;
        bar.icons[3].onscreen = false;
        assert_eq!(bar.shroud_strip(), None);
    }

    #[test]
    fn no_shroud_when_divider_hidden() {
        assert_eq!(narrow_bar(false).shroud_strip(), None);
        let mut bar = narrow_bar(true);
        bar.divider = None;
        assert_eq!(bar.shroud_strip(), None);
    }

    #[test]
    fn narrow_while_room_for_divider_and_widest_panel_icon() {
        assert_eq!(narrow_bar(true).divider_length(NOTCH), Some(NARROW_ITEM_WIDTH));
    }

    #[test]
    fn evicted_narrow_divider_turns_long() {
        assert_eq!(narrow_bar(false).divider_length(NOTCH), Some(HIDING_WIDTH_MIN));
        assert!(narrow_bar(false).reading().is_none());
    }

    #[test]
    fn tight_room_keeps_divider_long_and_room_brings_it_back() {
        let mut bar = narrow_bar(true);
        for (slot, x) in bar.icons.iter_mut().zip([20.0, 54.0, 88.0, 121.0]) {
            slot.x = x;
            slot.onscreen = false;
        }
        bar.divider = Some(slot(161.0, 802.0, false));
        assert_eq!(bar.divider_length(NOTCH), Some(NARROW_ITEM_WIDTH), "места до Docker хватает");
        assert!(bar.reading().is_some(), "длинному спрятанному разделителю строка верит");
        bar.icons[4].x = 894.0;
        assert_eq!(bar.divider_length(NOTCH), Some(HIDING_WIDTH_MIN), "тесно — остаётся длинным");
    }

    #[test]
    fn no_notch_keeps_divider_narrow() {
        assert_eq!(narrow_bar(true).divider_length(None), Some(NARROW_ITEM_WIDTH));
    }

    #[test]
    fn unplaced_or_missing_is_unknown() {
        let (a, s) = (window(0.0, 32.0, true), window(1000.0, 14.0, true));
        assert_eq!(classify(Some(&a), Some(&s), None, NOTCH), Order::Unknown);
        assert_eq!(classify(None, Some(&s), None, NOTCH), Order::Unknown);
    }
}
