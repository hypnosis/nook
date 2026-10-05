//! Чтение строки меню: единственное место, откуда раскладка узнаёт, что стоит в строке.
//! Читает в фоне, когда строка встала, и отдаёт снимок делегату через `onBarRead`.

use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use dispatch2::DispatchQueue;
use objc2::runtime::AnyObject;
use objc2::{msg_send, MainThreadMarker};
use objc2_app_kit::NSApplication;

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
