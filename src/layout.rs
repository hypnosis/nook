//! Порядок иконок строки меню — один реестр на всё приложение: панель и редактор строятся из него.

use crate::capture::IconWindow;

/// Иконки левее ≡◂ слева направо: панель — левее разделителя, основной ряд — правее.
/// Ряд иконки и порядок нарисованных иконок задаёт строка меню. Порядок ненарисованных
/// (иконки панели, иконки под чёлкой) строка не показывает честно — он держится реестром
/// и меняется бросками в редакторе. Пока идёт перенос, реестр со строкой не сверяется.
#[derive(Default)]
pub struct Layout {
    panel: Vec<u32>,
    main: Vec<u32>,
    moving: bool,
    /// Ручной порядок, отложенный на время автоматического расположения.
    manual: Option<(Vec<u32>, Vec<u32>)>,
}

impl Layout {
    pub fn panel(&self) -> &[u32] {
        &self.panel
    }

    pub fn main(&self) -> &[u32] {
        &self.main
    }

    pub fn is_moving(&self) -> bool {
        self.moving
    }

    pub fn set_moving(&mut self, moving: bool) {
        self.moving = moving;
    }

    /// Включено автоматическое расположение: ручной порядок откладывается до выключения.
    pub fn enter_automatic(&mut self) {
        if self.manual.is_none() {
            self.manual = Some((self.panel.clone(), self.main.clone()));
        }
    }

    /// Автоматическое расположение выключено: возвращается отложенный ручной порядок.
    pub fn leave_automatic(&mut self) {
        if let Some((panel, main)) = self.manual.take() {
            self.update(panel, main);
        }
    }

    /// Порядок после удачного броска в редакторе.
    pub fn set(&mut self, panel: &[u32], main: &[u32]) {
        self.update(panel.to_vec(), main.to_vec());
    }

    /// Сверяет реестр со строкой `windows` (окна иконок слева направо). Пропавшие иконки
    /// уходят; новые и сменившие ряд встают сразу за ближайшим соседом слева по строке;
    /// нарисованные иконки ряда занимают свои места в порядке строки. Ряд — сторона
    /// разделителя; при `automatic` разделителя нет, и в панели всё, что macOS не нарисовала.
    /// Строка свёрнута, разделителя нет или он узкий, идёт перенос — не сверяет и возвращает `false`.
    pub fn sync(&mut self, windows: &[IconWindow], automatic: bool) -> bool {
        if self.moving {
            return false;
        }
        let Some(spacer) = windows.iter().find(|window| window.name == crate::status_bar::SPACER_AUTOSAVE) else {
            return false;
        };
        if !spacer.onscreen {
            return false;
        }
        let divider = if automatic {
            None
        } else {
            match windows.iter().find(|window| window.name == crate::divider::AUTOSAVE) {
                Some(divider) if !crate::divider::is_narrow(divider) => Some(divider),
                _ => return false,
            }
        };
        let icons: Vec<&IconWindow> = windows
            .iter()
            .filter(|window| window.x < spacer.x && window.name != crate::divider::AUTOSAVE)
            .collect();
        let in_panel = |window: &IconWindow| match divider {
            Some(divider) => window.x < divider.x,
            None => !window.onscreen,
        };

        let (mut panel, mut main) = (self.panel.clone(), self.main.clone());
        let present = |id: &u32| icons.iter().any(|icon| icon.id == *id);
        panel.retain(present);
        main.retain(present);
        for (index, icon) in icons.iter().enumerate() {
            let (row, other) = if in_panel(icon) { (&mut panel, &mut main) } else { (&mut main, &mut panel) };
            if row.contains(&icon.id) {
                continue;
            }
            other.retain(|id| *id != icon.id);
            let at = icons[..index]
                .iter()
                .rev()
                .find_map(|left| row.iter().position(|id| *id == left.id))
                .map_or(0, |place| place + 1);
            row.insert(at, icon.id);
        }
        follow_drawn(&mut panel, &icons);
        follow_drawn(&mut main, &icons);
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

/// Места нарисованных иконок ряда `row` занимают те же иконки, но в порядке строки `icons`;
/// ненарисованные остаются на своих местах.
fn follow_drawn(row: &mut [u32], icons: &[&IconWindow]) {
    let drawn: Vec<u32> = icons
        .iter()
        .filter(|icon| icon.onscreen && row.contains(&icon.id))
        .map(|icon| icon.id)
        .collect();
    let slots = row.iter_mut().filter(|id| drawn.contains(&**id));
    for (slot, id) in slots.zip(drawn.iter()) {
        *slot = *id;
    }
}
