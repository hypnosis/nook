//! Порядок иконок строки меню — один на всё приложение: панель и редактор строятся из него.

use crate::capture::{self, POSITION_TOLERANCE};

/// Иконки левее ≡◂ слева направо, как они должны стоять: панель — левее разделителя,
/// основной ряд — правее. Порядок задают первое чтение строки и «Применить»; строка меню
/// только сообщает, какие иконки в каком ряду есть. Пока идёт «Применить», порядок замирает.
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

    /// Порядок, который поставило «Применить».
    pub fn set(&mut self, panel: &[u32], main: &[u32]) {
        self.update(panel.to_vec(), main.to_vec());
    }

    /// Порядок совпадает с желаемым `panel` и `main`; иконки, которых нет в желаемом, не в счёт.
    pub fn matches(&self, panel: &[u32], main: &[u32]) -> bool {
        let wanted = |id: &&u32| panel.contains(id) || main.contains(id);
        self.panel.iter().filter(wanted).eq(panel) && self.main.iter().filter(wanted).eq(main)
    }

    /// Сверяет порядок со строкой меню: известные иконки остаются в своём ряду и на своём
    /// месте (при `automatic` ряд — где стоит), новые встают в конец ряда, где стоят,
    /// пропавшие убираются. Порядок пуст —
    /// берётся как стоит. `divider` — окно разделителя панели (None — его нет), `ignore` —
    /// окно убранного разделителя, которое ещё не исчезло; `spacer_x` — левый край ≡◂.
    /// Широкий разделитель прячет иконки панели, основной ряд остаётся виден; при узком
    /// панель — всё левее него. `automatic` — разделителя нет, и панель — всё, что macOS
    /// не уместила в строку. Окна разделителя нет, узкий спрятан или левее ≡◂ ничего
    /// не нашлось — строка не прочитана. Возвращает, прочитана ли она.
    pub fn refresh(&mut self, divider: Option<u32>, ignore: Option<u32>, spacer_x: f64, automatic: bool) -> bool {
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
        let mut panel = Vec::new();
        let mut main = Vec::new();
        for window in windows
            .iter()
            .filter(|window| {
                window.x < spacer_x - POSITION_TOLERANCE && Some(window.id) != divider && Some(window.id) != ignore
            })
        {
            let in_panel = match narrow_x {
                Some(x) => capture::is_panel_icon(window, x),
                None => (divider.is_some() || automatic) && !window.onscreen,
            };
            if in_panel {
                panel.push(window.id);
            } else {
                main.push(window.id);
            }
        }
        if panel.is_empty() && main.is_empty() {
            return false;
        }
        let (panel, main) = if automatic {
            // Набор панели решает macOS: иконка переходит в тот ряд, где стоит.
            (
                keep_order(&self.panel, &panel.clone(), &self.panel, panel),
                keep_order(&self.main, &main.clone(), &self.main, main),
            )
        } else {
            let present: Vec<u32> = panel.iter().chain(&main).copied().collect();
            let known: Vec<u32> = self.panel.iter().chain(&self.main).copied().collect();
            (
                keep_order(&self.panel, &present, &known, panel),
                keep_order(&self.main, &present, &known, main),
            )
        };
        self.update(panel, main);
        true
    }

    fn update(&mut self, panel: Vec<u32>, main: Vec<u32>) {
        if panel != self.panel || main != self.main {
            crate::log::append(&format!("порядок: панель {panel:?}, основной ряд {main:?}"));
            self.panel = panel;
            self.main = main;
        }
    }
}

/// Ряд реестра `row` без пропавших из строки (`present`), плюс новые иконки, которые
/// стоят в этом ряду (`standing`) и реестру ещё не известны (`known`).
fn keep_order(row: &[u32], present: &[u32], known: &[u32], standing: Vec<u32>) -> Vec<u32> {
    let mut kept: Vec<u32> = row.iter().filter(|id| present.contains(id)).copied().collect();
    kept.extend(standing.into_iter().filter(|id| !known.contains(id)));
    kept
}
