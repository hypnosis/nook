//! Настроечные числа поведения: геометрия строки меню, паузы, таймауты, допуски.
//! Размеры окон и отступы интерфейса живут в `ui_style` и рядом со своими окнами.

use std::time::Duration;

// Строка меню: ширины и допуски, pt.

/// Ширина узкого элемента строки: спейсер в показанном состоянии и разделитель панели.
pub const NARROW_ITEM_WIDTH: f64 = 12.0;
/// Меньше этой ширины раздутый элемент заставляет macOS перемешивать строку.
pub const HIDING_WIDTH_MIN: f64 = 500.0;
/// Запас спейсера сверх ширины экрана, чтобы вытолкнуть крайние иконки.
pub const HIDDEN_WIDTH_MARGIN: f64 = 200.0;
pub const HIDDEN_WIDTH_MAX: f64 = 4000.0;
/// Ширина экрана, если macOS её не отдала.
pub const SCREEN_WIDTH_FALLBACK: f64 = 1728.0;
/// Окно элемента строки меню шире его длины на эти поля.
pub const WINDOW_CHROME: f64 = 16.0;
/// Запас от левого края экрана для самой левой иконки панели.
pub const SCREEN_MARGIN: f64 = 20.0;
/// Окно узкого разделителя не шире этого.
pub const NARROW_WINDOW_MAX: f64 = 40.0;
/// Ближе этого к правому краю чёлки macOS иконки не рисует.
pub const NOTCH_CLEARANCE: f64 = 28.0;
/// Подсветка нажатой иконки шире её окна: шторка начинается на столько левее самой левой иконки.
pub const SHROUD_MARGIN: f64 = 6.0;
/// Высота полосы строки меню с запасом: мышь ниже неё считается ушедшей.
pub const MENU_BAR_STRIP: f64 = 40.0;
/// Окно выше этого — не иконка строки меню.
pub const MAX_ICON_HEIGHT: f64 = 50.0;
/// Допуск сравнения координат окон строки меню.
pub const POSITION_TOLERANCE: f64 = 1.0;
/// Насколько центр элемента Accessibility может отличаться от центра окна иконки.
pub const MATCH_TOLERANCE: f64 = 6.0;
/// Насколько заходить за край соседа, чтобы встать перед ним или после него.
pub const DROP_OFFSET: f64 = 2.0;
/// Сдвиг точки нажатия и броска от края, чтобы попасть внутрь окна, а не на границу.
pub const DROP_NUDGE: f64 = 1.0;

// Паузы и таймауты.

/// Шаг опроса строки меню, пока окна встают на места.
pub const BAR_POLL: Duration = Duration::from_millis(5);
/// Задержка автосворачивания после ухода мыши из строки меню, с.
pub const COLLAPSE_DELAY: f64 = 3.0;

/// Интервал проверки, встали ли айтемы Nook на места после старта, с.
pub const PLACEMENT_CHECK_INTERVAL: f64 = 0.3;
/// Как часто сторож сверяет порядок элементов Nook, с.
pub const ORDER_CHECK_INTERVAL: f64 = 0.5;
/// Столько проверок подряд нарушение должно держаться, чтобы сторож сработал.
pub const ORDER_BAD_READS: u32 = 3;
/// После смены экранов сторож молчит столько секунд: macOS заново раскладывает строку.
pub const SCREEN_SETTLE: f64 = 2.0;
/// Места по умолчанию, когда macOS не помнит своих: `<` крайний правый, `|` вплотную левее.
pub const ANCHOR_DEFAULT_PLACE: f64 = 0.0;
pub const SPACER_DEFAULT_PLACE: f64 = 1.0;
/// Максимум пересозданий одного айтема, застрявшего неразмещённым.
pub const ANCHOR_MAX_RETRIES: u32 = 10;
/// После стольких безуспешных проверок пересоздаются оба айтема.
pub const PLACEMENT_ESCALATE_AFTER: u32 = 4;
/// Потолок проверок размещения.
pub const PLACEMENT_MAX_ATTEMPTS: u32 = 30;

/// Сколько ждать, что спейсер вышел на экран после раскрытия, с.
pub const PANEL_CAPTURE_DELAY: f64 = 0.3;
/// Сколько ждать, что показанный разделитель панели появился в строке.
pub const DIVIDER_SHOW_TIMEOUT: Duration = Duration::from_millis(500);
/// Пауза, чтобы macOS разложила строку после создания разделителя, с.
pub const DIVIDER_SETTLE_DELAY: f64 = 0.4;
/// Период чтения строки, пока она раскрыта, с.
pub const PANEL_REFRESH_INTERVAL: f64 = 1.0;
/// Сколько ждать ответа приложения через Accessibility, с.
pub const AX_TIMEOUT: f32 = 0.1;

/// Перенос без единого шага дольше этого считается зависшим, с.
pub const MOVE_WATCHDOG: f64 = 5.0;
/// Как часто сторож проверяет перенос, с.
pub const MOVE_WATCHDOG_POLL: f64 = 1.0;
/// Сколько ждать ответа иконки на событие, адресованное её окну.
pub const TARGETED_TIMEOUT: Duration = Duration::from_millis(150);
/// Шаги перетаскивания: быстрый и надёжный.
pub const FAST_STEP: Duration = Duration::from_millis(12);
pub const SAFE_STEP: Duration = Duration::from_millis(50);
/// Сколько ждать, что окно сдвинулось после перетаскивания.
pub const SETTLE_TIMEOUT: Duration = Duration::from_millis(250);
/// Строка считается успокоившейся после стольких одинаковых замеров подряд.
pub const STILL_READS: u32 = 2;
pub const STILL_POLL: Duration = Duration::from_millis(10);
pub const STILL_TIMEOUT: Duration = Duration::from_millis(600);

/// Сколько ждать снимка полосы строки для шторки.
pub const SHOT_TIMEOUT: Duration = Duration::from_secs(2);

/// Опрос и таймауты меню, открытого кликом по копии.
pub const MENU_POLL: Duration = Duration::from_millis(50);
pub const MENU_APPEAR_TIMEOUT: Duration = Duration::from_millis(1500);
pub const MENU_MAX_OPEN: Duration = Duration::from_secs(120);
