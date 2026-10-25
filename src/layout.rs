//! Модель раскладки: единственное место, где решается, какие иконки в панели и в каком порядке.
//! Порядок на диске — намерение, чтение строки — факт. Намерение меняют бросок в редакторе и
//! Cmd+drag пользователя; осечка переноса Nook его не трогает. Панель и редактор только
//! показывают вид модели.

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
    /// Порядок панели на диске — намерение: ключи иконок, вместе с закрытыми приложениями.
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

    /// Принимает чтение строки, которую менял пользователь или приложения. Иконка, попавшая в
    /// панель, входит в намерение рядом с соседом; вынесенная из панели — выходит. Остальные
    /// ключи остаются, даже если иконка сейчас в основном ряду.
    pub fn observe(&mut self, reading: Reading) -> Change {
        self.observe_as(reading, false)
    }

    /// Принимает чтение после переноса Nook: вид следует строке, в намерение входят только
    /// иконки, появившиеся за это время левее разделителя.
    pub fn observe_own_move(&mut self, reading: Reading) -> Change {
        self.observe_as(reading, true)
    }

    /// Удачный бросок становится намерением: брошенная в панель иконка встаёт по порядку
    /// редактора `panel`, брошенная в основной ряд выходит из панели.
    pub fn accept_drop(&mut self, id: u32, panel: Option<&[u32]>) {
        match panel {
            Some(order) => {
                self.reorder_panel(order);
                self.order_saved_by_panel();
            }
            None => {
                let key = self.last.as_ref().and_then(|reading| key_of(reading, id)).map(str::to_owned);
                if let Some(key) = key {
                    let saved = self.saved.iter().filter(|saved| **saved != key).cloned().collect();
                    self.set_saved(saved);
                }
            }
        }
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
        self.order_saved_by_panel();
        true
    }

    /// Строка совпадает с намерением: каждая иконка с известным ключом стоит в панели ровно
    /// тогда, когда её ключ записан в порядке панели.
    pub fn bar_matches_saved(&self) -> bool {
        let Some(reading) = self.last.as_ref() else { return true };
        let placed = |icons: &[Icon], in_panel: bool| {
            icons.iter().all(|icon| icon.key.as_ref().is_none_or(|key| self.saved.contains(key) == in_panel))
        };
        placed(&reading.panel, true) && placed(&reading.main, false)
    }

    fn observe_as(&mut self, reading: Reading, own_move: bool) -> Change {
        let previous = self.last.clone();
        let change = self.take_reading(reading);
        if let Some(previous) = previous.filter(|_| change != Change::Noise) {
            self.follow_changes(&previous, own_move);
        }
        change
    }

    /// Вид модели по чтению: состав из строки, порядок панели по намерению.
    fn take_reading(&mut self, reading: Reading) -> Change {
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
                .unwrap_or_else(|| after_left(&panel, reading.panel[..index].iter().map(|left| left.id)));
            panel.insert(at, icon.id);
        }
        self.panel = panel;
        self.main = ids(&reading.main);
        self.last = Some(reading);
        change
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

    /// Намерение следует тому, что изменилось с прошлого чтения `previous`. После переноса Nook
    /// в счёт идут только новые иконки: свою перестановку Nook сверяет отдельно.
    fn follow_changes(&mut self, previous: &Reading, own_move: bool) {
        let Some(reading) = self.last.as_ref() else { return };
        let was_in_panel = |id: u32| previous.panel.iter().any(|icon| icon.id == id);
        let was_present = |id: u32| was_in_panel(id) || previous.main.iter().any(|icon| icon.id == id);
        let mut saved = self.saved.clone();
        for (place, id) in self.panel.iter().enumerate() {
            let Some(key) = key_of(reading, *id) else { continue };
            let arrived = if own_move { !was_present(*id) } else { !was_in_panel(*id) };
            if arrived && !saved.iter().any(|saved| saved == key) {
                let left = self.panel[..place].iter().filter_map(|left| key_of(reading, *left).map(str::to_owned));
                let at = after_left(&saved, left);
                saved.insert(at, key.to_owned());
            }
        }
        if !own_move {
            let left_panel: Vec<&str> = reading
                .main
                .iter()
                .filter(|icon| was_in_panel(icon.id))
                .filter_map(|icon| icon.key.as_deref())
                .collect();
            saved.retain(|key| !left_panel.contains(&key.as_str()));
        }
        self.set_saved(saved);
    }

    /// Ключи иконок панели на диске встают в порядке панели на свои же места; ключи закрытых
    /// приложений и иконок, стоящих сейчас в основном ряду, остаются где были.
    fn order_saved_by_panel(&mut self) {
        let Some(reading) = self.last.as_ref() else { return };
        let keys: Vec<String> = self.panel.iter().filter_map(|id| key_of(reading, *id).map(str::to_owned)).collect();
        let mut saved = self.saved.clone();
        let mut slots: Vec<usize> = keys.iter().filter_map(|key| saved.iter().position(|saved| saved == key)).collect();
        slots.sort_unstable();
        let known: Vec<String> = keys.iter().filter(|key| saved.contains(key)).cloned().collect();
        for (slot, key) in slots.into_iter().zip(known) {
            saved[slot] = key;
        }
        for (index, key) in keys.iter().enumerate() {
            if !saved.contains(key) {
                let at = after_left(&saved, keys[..index].iter().cloned());
                saved.insert(at, key.clone());
            }
        }
        self.set_saved(saved);
    }

    fn set_saved(&mut self, saved: Vec<String>) {
        if saved != self.saved {
            log::info!("панель: порядок на диске {saved:?}");
            self.saved = saved;
        }
    }
}

/// Место в `list` сразу за ближайшим соседом слева из `left` (ближний последним), которого
/// `list` уже знает; таких нет — начало.
fn after_left<T: PartialEq>(list: &[T], left: impl DoubleEndedIterator<Item = T>) -> usize {
    left.rev().find_map(|item| list.iter().position(|known| *known == item)).map_or(0, |place| place + 1)
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
        let at = after_left(&panel, icons[..index].iter().map(|left| left.id));
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
    fn failed_own_move_keeps_intent() {
        let mut layout = start();
        let missed = reading(&[(1, "Ollama")], &[(2, "CleanShot"), (3, "AdGuard"), (10, "Hammer"), (11, "Mask")]);
        layout.observe_own_move(missed);
        assert_eq!(layout.panel(), &[1]);
        assert!(!layout.bar_matches_saved());
        assert_eq!(layout.saved(), saved(&["CleanShot", "AdGuard", "Ollama"]));
    }

    #[test]
    fn app_change_after_failed_move_keeps_intent() {
        let mut layout = start();
        layout.observe_own_move(reading(&[(1, "Ollama")], &[(2, "CleanShot"), (3, "AdGuard"), (10, "Hammer")]));
        let opened = reading(&[(1, "Ollama")], &[(2, "CleanShot"), (3, "AdGuard"), (10, "Hammer"), (12, "Docker")]);
        assert_eq!(layout.observe(opened), Change::Apps);
        assert_eq!(layout.saved(), saved(&["CleanShot", "AdGuard", "Ollama"]));
    }

    #[test]
    fn user_change_after_failed_move_keeps_intent() {
        let mut layout = start();
        layout.observe_own_move(reading(&[(1, "Ollama")], &[(2, "CleanShot"), (3, "AdGuard"), (10, "Hammer")]));
        let dragged = reading(&[(1, "Ollama"), (10, "Hammer")], &[(2, "CleanShot"), (3, "AdGuard")]);
        assert_eq!(layout.observe(dragged), Change::Moved);
        assert_eq!(layout.saved(), saved(&["CleanShot", "AdGuard", "Ollama", "Hammer"]));
        layout.reorder_panel(&[10, 1]);
        assert_eq!(layout.saved(), saved(&["CleanShot", "AdGuard", "Hammer", "Ollama"]));
    }

    #[test]
    fn new_icon_during_own_move_joins_intent() {
        let mut layout = start();
        let moved = reading(&[(2, "CleanShot"), (3, "AdGuard"), (1, "Ollama"), (40, "Docker")], &[(10, "Hammer")]);
        layout.observe_own_move(moved);
        assert_eq!(layout.saved(), saved(&["CleanShot", "AdGuard", "Ollama", "Docker"]));
        assert!(layout.bar_matches_saved());
    }

    #[test]
    fn accepted_drops_change_only_the_dropped_icon() {
        let mut layout = start();
        layout.observe_own_move(reading(&[(1, "Ollama"), (2, "CleanShot"), (10, "Hammer")], &[(3, "AdGuard")]));
        layout.accept_drop(10, Some(&[10, 2, 1]));
        assert_eq!(layout.saved(), saved(&["Hammer", "CleanShot", "AdGuard", "Ollama"]));
        layout.observe_own_move(reading(&[(1, "Ollama"), (2, "CleanShot")], &[(10, "Hammer"), (3, "AdGuard")]));
        layout.accept_drop(10, None);
        assert_eq!(layout.saved(), saved(&["CleanShot", "AdGuard", "Ollama"]));
    }

    #[test]
    fn restored_bar_matches_intent() {
        let mut layout = start();
        layout.observe_own_move(reading(&[(2, "CleanShot"), (3, "AdGuard"), (1, "Ollama")], &[(10, "Hammer")]));
        assert!(layout.bar_matches_saved());
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
