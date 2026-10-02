//! Порядок иконок строки меню — один на всё приложение: панель и редактор строятся из него.

use crate::capture::{self, POSITION_TOLERANCE};

/// Иконки левее ≡◂ слева направо: панель — левее разделителя, основной ряд — правее.
/// Пока идёт «Применить», порядок замирает.
#[derive(Default)]
pub struct Layout {
    panel: Vec<u32>,
    main: Vec<u32>,
    applying: bool,
}

impl Layout {
    pub fn panel(&self) -> &[u32] {
        &self.panel
    }

    pub fn main(&self) -> &[u32] {
        &self.main
    }

    pub fn is_applying(&self) -> bool {
        self.applying
    }

    /// Начало «Применить»: порядок не читается до `end_apply`.
    pub fn begin_apply(&mut self) {
        self.applying = true;
    }

    pub fn end_apply(&mut self) {
        self.applying = false;
    }

    /// Порядок совпадает с желаемым `panel` и `main`; иконки, которых нет в желаемом, не в счёт.
    pub fn matches(&self, panel: &[u32], main: &[u32]) -> bool {
        let wanted = |id: &&u32| panel.contains(id) || main.contains(id);
        self.panel.iter().filter(wanted).eq(panel) && self.main.iter().filter(wanted).eq(main)
    }

    /// Читает порядок из строки меню. `divider` — окно разделителя панели (None — разделителя
    /// нет), `spacer_x` — левый край ≡◂. Широкий разделитель прячет иконки панели по порядку,
    /// а основной ряд остаётся виден. При узком панель — всё левее него, но крайние иконки
    /// macOS прячет у чёлки и может переставить между собой: их порядок остаётся прежним.
    /// Окна разделителя нет или узкий спрятан — порядок не читается. Возвращает, прочитан ли он.
    pub fn refresh(&mut self, divider: Option<u32>, spacer_x: f64) -> bool {
        if self.applying {
            return false;
        }
        let windows = capture::icon_layout();
        let divider_window = match divider {
            Some(id) => match windows.iter().find(|window| window.id == id) {
                Some(window) => Some(window),
                None => return false,
            },
            None => None,
        };
        let narrow_x = divider_window.filter(|window| crate::divider::is_narrow(window)).map(|window| window.x);
        if narrow_x.is_some() && divider_window.is_some_and(|window| !window.onscreen) {
            return false;
        }
        let mut hidden = Vec::new();
        let mut panel = Vec::new();
        let mut main = Vec::new();
        for window in windows
            .iter()
            .filter(|window| window.x < spacer_x - POSITION_TOLERANCE && Some(window.id) != divider)
        {
            let in_panel = match narrow_x {
                Some(x) => capture::is_panel_icon(window, x),
                None => divider.is_some() && !window.onscreen,
            };
            if !in_panel {
                main.push(window.id);
            } else if narrow_x.is_some() && !window.onscreen {
                hidden.push(window.id);
            } else {
                panel.push(window.id);
            }
        }
        hidden.sort_by_key(|id| self.panel.iter().position(|known| known == id).unwrap_or(usize::MAX));
        let panel: Vec<u32> = hidden.into_iter().chain(panel).collect();
        if panel != self.panel || main != self.main {
            crate::log::append(&format!("порядок: панель {panel:?}, основной ряд {main:?}"));
            self.panel = panel;
            self.main = main;
        }
        true
    }
}
