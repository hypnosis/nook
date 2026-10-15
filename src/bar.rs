//! Чтение строки меню: единственное место, откуда раскладка узнаёт, что стоит в строке.
//! Читает в фоне, когда строка встала, и отдаёт снимок делегату через `onBarRead`.

use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use dispatch2::DispatchQueue;
use objc2::runtime::AnyObject;
use objc2::{msg_send, MainThreadMarker};
use objc2_app_kit::NSApplication;

use crate::capture::IconWindow;
use crate::layout::{Icon, Reading};
use crate::tuning::{
    BAR_POLL, HIDING_WIDTH_MIN, NARROW_WINDOW_MAX, PANEL_CAPTURE_DELAY, POSITION_TOLERANCE, SCREEN_MARGIN,
    WINDOW_CHROME,
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
}

impl Snapshot {
    /// Стороны разделителя. Разделителя нет или он узкий — `None`.
    pub fn reading(&self) -> Option<Reading> {
        let divider = self.divider.as_ref().filter(|divider| divider.width > NARROW_WINDOW_MAX)?;
        let icon = |slot: &Slot| Icon { id: slot.id, key: slot.key.clone(), drawn: slot.onscreen };
        let (panel, main): (Vec<&Slot>, Vec<&Slot>) = self.icons.iter().partition(|slot| slot.x < divider.x);
        Some(Reading { panel: panel.into_iter().map(icon).collect(), main: main.into_iter().map(icon).collect() })
    }

    /// Авторежим: в панели то, что macOS не нарисовала, в основном ряду — нарисованное.
    pub fn automatic(&self) -> (Vec<u32>, Vec<u32>) {
        let (panel, main): (Vec<&Slot>, Vec<&Slot>) = self.icons.iter().partition(|slot| !slot.onscreen);
        (panel.iter().map(|slot| slot.id).collect(), main.iter().map(|slot| slot.id).collect())
    }

    /// Длина разделителя, при которой иконки левее него спрятаны, а крайняя левая
    /// стоит на экране: там её снимает панель и нажимает Accessibility.
    pub fn hiding_width(&self) -> Option<f64> {
        let divider = self.divider.as_ref()?;
        let panel_width: f64 = self.icons.iter().filter(|slot| slot.x < divider.x).map(|slot| slot.width).sum();
        let next_x = self
            .icons
            .iter()
            .find(|slot| slot.x > divider.x + POSITION_TOLERANCE)
            .map_or(self.spacer_x, |slot| slot.x);
        let width = next_x - WINDOW_CHROME - panel_width - SCREEN_MARGIN;
        if width < HIDING_WIDTH_MIN {
            log::warn!("иконок панели слишком много ({panel_width} pt) — часть уйдёт за край");
        }
        Some(width.max(HIDING_WIDTH_MIN))
    }
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
        let deadline = Instant::now() + Duration::from_secs_f64(PANEL_CAPTURE_DELAY);
        while spacer_x().is_none() && Instant::now() < deadline {
            thread::sleep(BAR_POLL);
        }
        crate::mover::wait_until_still();
        let snapshot = read(epoch, true);
        *LATEST.lock().unwrap_or_else(PoisonError::into_inner) = snapshot;
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
    read(0, owners)
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

fn read(epoch: u64, owners: bool) -> Option<Snapshot> {
    let windows = crate::capture::icon_layout();
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
    Some(Snapshot { epoch, icons, divider, spacer_x })
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

    #[test]
    fn unplaced_or_missing_is_unknown() {
        let (a, s) = (window(0.0, 32.0, true), window(1000.0, 14.0, true));
        assert_eq!(classify(Some(&a), Some(&s), None, NOTCH), Order::Unknown);
        assert_eq!(classify(None, Some(&s), None, NOTCH), Order::Unknown);
    }
}
