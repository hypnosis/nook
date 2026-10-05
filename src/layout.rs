//! Модель раскладки: единственное место, где решается, какие иконки в панели и в каком порядке.
//! Панель и редактор только показывают её вид.

/// Иконка прочитанной строки: номер окна, ключ для диска, если приложение известно,
/// и нарисована ли она. Очерёдность ненарисованных macOS путает, событием она не считается.
#[derive(Clone, Debug, PartialEq)]
pub struct Icon {
    pub id: u32,
    pub key: Option<String>,
    pub drawn: bool,
}

/// Строка меню целиком: иконки левее разделителя и правее него, слева направо.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reading {
    pub panel: Vec<Icon>,
    pub main: Vec<Icon>,
}

/// Чем новое чтение отличается от прошлого.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Change {
    /// Состав и очерёдность прежние: ширины, чёлка, первое чтение.
    Noise,
    /// Иконка появилась или пропала.
    Apps,
    /// Те же иконки стоят иначе: Cmd+drag или бросок.
    Moved,
}

pub struct Layout {
    /// Порядок панели на диске: ключи иконок, вместе с закрытыми приложениями.
    saved: Vec<String>,
    last: Option<Reading>,
    panel: Vec<u32>,
    main: Vec<u32>,
}

impl Layout {
    pub fn new(saved: Vec<String>) -> Self {
        Self { saved, last: None, panel: Vec::new(), main: Vec::new() }
    }

    pub fn panel(&self) -> &[u32] {
        &self.panel
    }

    pub fn main(&self) -> &[u32] {
        &self.main
    }

    /// Порядок панели для диска.
    pub fn saved(&self) -> &[String] {
        &self.saved
    }

    /// Принимает достоверное чтение строки. Состав и порядок меняются только по событию.
    pub fn observe(&mut self, reading: Reading) -> Change {
        let Some(last) = self.last.take() else {
            self.panel = by_saved(&reading.panel, &self.saved);
            self.main = ids(&reading.main);
            self.last = Some(reading);
            return Change::Noise;
        };
        let same_panel = sorted(ids(&last.panel)) == sorted(ids(&reading.panel));
        let same_main = sorted(ids(&last.main)) == sorted(ids(&reading.main)) && drawn(&last.main) == drawn(&reading.main);
        if same_panel && same_main {
            self.last = Some(reading);
            return Change::Noise;
        }
        let everything =
            |reading: &Reading| sorted(ids(&reading.panel).into_iter().chain(ids(&reading.main)).collect());
        let change = if everything(&last) == everything(&reading) { Change::Moved } else { Change::Apps };

        let now = ids(&reading.panel);
        let mut panel: Vec<u32> = self.panel.iter().copied().filter(|id| now.contains(id)).collect();
        for (index, icon) in reading.panel.iter().enumerate() {
            if panel.contains(&icon.id) {
                continue;
            }
            let at = self
                .saved_place(icon, &panel, &reading)
                .or_else(|| {
                    reading.panel[..index]
                        .iter()
                        .rev()
                        .find_map(|left| panel.iter().position(|id| *id == left.id))
                        .map(|place| place + 1)
                })
                .unwrap_or(0);
            panel.insert(at, icon.id);
        }
        self.panel = panel;
        self.main = ids(&reading.main);
        self.last = Some(reading);
        self.save();
        change
    }

    /// Порядок панели из редактора: состав прежний, меняется только очерёдность.
    /// Возвращает, изменился ли порядок.
    pub fn reorder_panel(&mut self, order: &[u32]) -> bool {
        let mut panel: Vec<u32> = order.iter().copied().filter(|id| self.panel.contains(id)).collect();
        panel.extend(self.panel.iter().copied().filter(|id| !order.contains(id)));
        if panel == self.panel {
            return false;
        }
        self.panel = panel;
        self.save();
        true
    }

    /// Место для иконки `icon` в панели `panel` по сохранённому порядку: перед первой
    /// иконкой, сохранённой правее неё. Ключа нет на диске — `None`.
    fn saved_place(&self, icon: &Icon, panel: &[u32], reading: &Reading) -> Option<usize> {
        let rank = |key: &str| self.saved.iter().position(|saved| saved == key);
        let own = rank(icon.key.as_deref()?)?;
        let later = panel
            .iter()
            .position(|id| key_of(reading, *id).and_then(rank).is_some_and(|other| other > own));
        Some(later.unwrap_or(panel.len()))
    }

    /// Диск получает порядок панели; ключи закрытых приложений остаются рядом с прежними соседями.
    fn save(&mut self) {
        let Some(reading) = self.last.as_ref() else { return };
        let mut saved: Vec<String> =
            self.panel.iter().filter_map(|id| key_of(reading, *id).map(str::to_owned)).collect();
        let present =
            |key: &str| reading.panel.iter().chain(&reading.main).any(|icon| icon.key.as_deref() == Some(key));
        for (index, key) in self.saved.iter().enumerate() {
            if present(key) || saved.contains(key) {
                continue;
            }
            let at = self.saved[..index]
                .iter()
                .rev()
                .find_map(|left| saved.iter().position(|other| other == left))
                .map_or(0, |place| place + 1);
            saved.insert(at, key.clone());
        }
        if saved != self.saved {
            log::info!("панель: порядок на диске {saved:?}");
            self.saved = saved;
        }
    }
}

/// Иконки панели в сохранённом порядке; незнакомые встают за соседом слева по строке.
fn by_saved(icons: &[Icon], saved: &[String]) -> Vec<u32> {
    let rank = |icon: &Icon| icon.key.as_ref().and_then(|key| saved.iter().position(|saved| saved == key));
    let mut known: Vec<&Icon> = icons.iter().filter(|icon| rank(icon).is_some()).collect();
    known.sort_by_key(|icon| rank(icon));
    let mut panel: Vec<u32> = known.iter().map(|icon| icon.id).collect();
    for (index, icon) in icons.iter().enumerate() {
        if panel.contains(&icon.id) {
            continue;
        }
        let at = icons[..index]
            .iter()
            .rev()
            .find_map(|left| panel.iter().position(|id| *id == left.id))
            .map_or(0, |place| place + 1);
        panel.insert(at, icon.id);
    }
    panel
}

fn key_of(reading: &Reading, id: u32) -> Option<&str> {
    reading.panel.iter().chain(&reading.main).find(|icon| icon.id == id)?.key.as_deref()
}

fn ids(icons: &[Icon]) -> Vec<u32> {
    icons.iter().map(|icon| icon.id).collect()
}

fn drawn(icons: &[Icon]) -> Vec<u32> {
    icons.iter().filter(|icon| icon.drawn).map(|icon| icon.id).collect()
}

fn sorted(mut ids: Vec<u32>) -> Vec<u32> {
    ids.sort_unstable();
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(panel: &[(u32, &str)], main: &[(u32, &str)]) -> Reading {
        let row = |icons: &[(u32, &str)]| {
            icons.iter().map(|(id, key)| Icon { id: *id, key: Some(key.to_string()), drawn: true }).collect()
        };
        Reading { panel: row(panel), main: row(main) }
    }

    fn saved(keys: &[&str]) -> Vec<String> {
        keys.iter().map(|key| key.to_string()).collect()
    }

    /// Панель CleanShot, AdGuard, Ollama; основной ряд Hammer, Mask. В строке панель в другом порядке.
    fn start() -> Layout {
        let mut layout = Layout::new(saved(&["CleanShot", "AdGuard", "Ollama"]));
        layout.observe(reading(&[(1, "Ollama"), (2, "CleanShot"), (3, "AdGuard")], &[(10, "Hammer"), (11, "Mask")]));
        layout
    }

    #[test]
    fn first_reading_takes_saved_order() {
        let layout = start();
        assert_eq!(layout.panel(), &[2, 3, 1]);
        assert_eq!(layout.main(), &[10, 11]);
    }

    #[test]
    fn scenario_1_2_noise_changes_nothing() {
        let mut layout = start();
        let shuffled = reading(&[(3, "AdGuard"), (1, "Ollama"), (2, "CleanShot")], &[(10, "Hammer"), (11, "Mask")]);
        assert_eq!(layout.observe(shuffled), Change::Noise);
        assert_eq!(layout.panel(), &[2, 3, 1]);
        assert_eq!(layout.saved(), saved(&["CleanShot", "AdGuard", "Ollama"]));
    }

    #[test]
    fn scenario_1_hidden_main_icons_shuffle_is_noise() {
        let mut layout = Layout::new(Vec::new());
        let mut first = reading(&[(1, "Ollama")], &[(10, "Hammer"), (11, "Mask"), (12, "Clock")]);
        first.main[0].drawn = false;
        first.main[1].drawn = false;
        layout.observe(first);
        let mut shuffled = reading(&[(1, "Ollama")], &[(11, "Mask"), (10, "Hammer"), (12, "Clock")]);
        shuffled.main[0].drawn = false;
        shuffled.main[1].drawn = false;
        assert_eq!(layout.observe(shuffled), Change::Noise);
        assert_eq!(layout.main(), &[10, 11, 12]);
    }

    #[test]
    fn scenario_3_4_5_same_bar_keeps_layout() {
        let mut layout = start();
        let same = reading(&[(1, "Ollama"), (2, "CleanShot"), (3, "AdGuard")], &[(10, "Hammer"), (11, "Mask")]);
        assert_eq!(layout.observe(same.clone()), Change::Noise);
        assert_eq!(layout.observe(same), Change::Noise);
        assert_eq!(layout.panel(), &[2, 3, 1]);
    }

    #[test]
    fn scenario_6_restart_restores_order_with_new_windows() {
        let mut layout = Layout::new(saved(&["CleanShot", "AdGuard", "Ollama"]));
        layout.observe(reading(&[(51, "AdGuard"), (52, "Ollama"), (53, "CleanShot")], &[(60, "Hammer")]));
        assert_eq!(layout.panel(), &[53, 51, 52]);
    }

    #[test]
    fn scenario_7_drop_inside_panel_reorders() {
        let mut layout = start();
        assert!(layout.reorder_panel(&[1, 2, 3]));
        assert_eq!(layout.panel(), &[1, 2, 3]);
        assert_eq!(layout.saved(), saved(&["Ollama", "CleanShot", "AdGuard"]));
    }

    #[test]
    fn scenario_7_drop_from_main_lands_where_dropped() {
        let mut layout = start();
        let moved = reading(&[(1, "Ollama"), (2, "CleanShot"), (3, "AdGuard"), (10, "Hammer")], &[(11, "Mask")]);
        assert_eq!(layout.observe(moved), Change::Moved);
        layout.reorder_panel(&[10, 2, 3, 1]);
        assert_eq!(layout.panel(), &[10, 2, 3, 1]);
        assert_eq!(layout.main(), &[11]);
    }

    #[test]
    fn scenario_8_app_in_main_leaves_panel_alone() {
        let mut layout = start();
        let opened =
            reading(&[(1, "Ollama"), (2, "CleanShot"), (3, "AdGuard")], &[(10, "Hammer"), (12, "Vorssaint"), (11, "Mask")]);
        assert_eq!(layout.observe(opened), Change::Apps);
        assert_eq!(layout.panel(), &[2, 3, 1]);
        assert_eq!(layout.main(), &[10, 12, 11]);
    }

    #[test]
    fn scenario_8_closed_app_returns_to_its_place() {
        let mut layout = start();
        let closed = reading(&[(1, "Ollama"), (2, "CleanShot")], &[(10, "Hammer"), (11, "Mask")]);
        assert_eq!(layout.observe(closed), Change::Apps);
        assert_eq!(layout.panel(), &[2, 1]);
        assert_eq!(layout.saved(), saved(&["CleanShot", "AdGuard", "Ollama"]));
        let reopened = reading(&[(1, "Ollama"), (2, "CleanShot"), (30, "AdGuard")], &[(10, "Hammer"), (11, "Mask")]);
        assert_eq!(layout.observe(reopened), Change::Apps);
        assert_eq!(layout.panel(), &[2, 30, 1]);
    }

    #[test]
    fn scenario_8_new_icon_left_of_divider_follows_neighbour() {
        let mut layout = start();
        let opened =
            reading(&[(1, "Ollama"), (40, "Docker"), (2, "CleanShot"), (3, "AdGuard")], &[(10, "Hammer"), (11, "Mask")]);
        assert_eq!(layout.observe(opened), Change::Apps);
        assert_eq!(layout.panel(), &[2, 3, 1, 40]);
        assert_eq!(layout.saved(), saved(&["CleanShot", "AdGuard", "Ollama", "Docker"]));
    }

    #[test]
    fn scenario_9_cmd_drag_moves_between_rows() {
        let mut layout = start();
        let out = reading(&[(1, "Ollama"), (2, "CleanShot")], &[(3, "AdGuard"), (10, "Hammer"), (11, "Mask")]);
        assert_eq!(layout.observe(out), Change::Moved);
        assert_eq!(layout.panel(), &[2, 1]);
        assert_eq!(layout.main(), &[3, 10, 11]);
        assert_eq!(layout.saved(), saved(&["CleanShot", "Ollama"]));
        let back = reading(&[(1, "Ollama"), (2, "CleanShot"), (11, "Mask")], &[(3, "AdGuard"), (10, "Hammer")]);
        assert_eq!(layout.observe(back), Change::Moved);
        assert_eq!(layout.panel(), &[2, 11, 1]);
    }

    #[test]
    fn scenario_9_cmd_drag_inside_main_follows_bar() {
        let mut layout = start();
        let swapped = reading(&[(1, "Ollama"), (2, "CleanShot"), (3, "AdGuard")], &[(11, "Mask"), (10, "Hammer")]);
        assert_eq!(layout.observe(swapped), Change::Moved);
        assert_eq!(layout.main(), &[11, 10]);
        assert_eq!(layout.panel(), &[2, 3, 1]);
    }

    #[test]
    fn unknown_owner_keeps_bar_neighbour() {
        let mut layout = Layout::new(saved(&["AdGuard"]));
        let mut first = reading(&[(3, "AdGuard"), (1, "Ollama")], &[]);
        first.panel[1].key = None;
        layout.observe(first);
        assert_eq!(layout.panel(), &[3, 1]);
    }
}
