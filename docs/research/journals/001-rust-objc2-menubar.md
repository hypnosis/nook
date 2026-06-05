# Исследование: menu bar утилита на Rust + objc2 под macOS Tahoe

Дата: 2026-06-04. Источник: web-исследование (perplexity + исходники Ice/HiddenBar на GitHub). Это вход для ADR 001 и ADR 004.

## Краткий вывод

Аналог **HiddenBar** на Rust + objc2 (спейсер, свои иконки) — реально и без хаков, надёжно на Tahoe. Аналог **Ice/Bartender** (чужие иконки + второй ряд) — возможен, но это территория хаков (Screen Recording + синтетические CGEvent + CGWindowList), и именно этот слой ломается на Tahoe.

## Стек крейтов (на 2026)

| Крейт | Версия | Статус |
|-------|--------|--------|
| `objc2` | 0.6.4 | Основа, живой (мейнтейнер madsmtm) |
| `objc2-app-kit` | 0.3.2 | AppKit; фичи `NSStatusItem`, `NSStatusBar`, `NSStatusBarButton` |
| `objc2-foundation` | в синхроне | NSString, NSImage |
| `objc2-core-foundation` | актуален | CFTypes, CGFloat, CGRect |
| `objc2-core-graphics` | актуален (многое unsafe) | для CGEvent/CGWindowList — только если путь B |

Старый `cocoa`/`objc` — **deprecated**, официально вытеснены objc2. Брать только objc2-семейство.

⚠️ docs.rs декларирует поддержку до macOS 15.5 (Sequoia). На Tahoe (26) должно работать (биндинги зовут рантайм-API), но новые символы могут быть не сгенерированы — **проверить руками на железе**.

## Status item: минимальный паттерн

1. `NSApplication::sharedApplication(mtm)` + `setActivationPolicy(.Accessory)` — agent без иконки в Dock (= `LSUIElement`, но в рантайме; удобно для unbundled бинаря).
2. Главный поток обязателен — objc2 форсит через `MainThreadMarker`. Отдельный runloop не нужен, штатный `app.run()`.
3. `NSStatusBar::systemStatusBar()` → `statusItemWithLength(NSVariableStatusItemLength)`.
4. Иконка: `statusItem.button()` → `setImage`. Картинку пометить template (`setTemplate(true)`) для свет/тёмной полоски. SF Symbol — `imageWithSystemSymbolName_accessibilityDescription`.
5. Клик: либо `setMenu(menu)` (меню само), либо `button.setTarget` + `setAction(sel)`; обработчик — свой класс через `define_class!` (objc2), не старый `delegate!`.

## Как прячутся чужие иконки — реальный механизм (критично)

**Закон:** приложение управляет только своими `NSStatusItem`. Узнать, спрятана ли своя иконка за нотчем, публичного API нет.

- **Путь A — спейсер (HiddenBar, и база Ice).** Свой невидимый разделитель-`NSStatusItem`; при сворачивании `length` ставится огромным (HiddenBar: btnSeparate.length ~500–4000; Ice ControlItem: `Lengths.expanded = 10_000`) → иконки слева уезжают за край экрана. Без Accessibility, без Screen Recording. Иконки не перехватываются — пользователь сам раскладывает их Cmd+drag относительно разделителя.
- **Путь B — управление чужими (Ice/Bartender).** Чтение чужих иконок через `CGWindowList` (windowID, owningApplication, frame). Перемещение — синтетические `CGEvent` (эмуляция Cmd+drag, `MenuBarItemManager.swift`). Screen Recording — чтобы нарисовать картинки чужих иконок во втором ряду. Хрупко: ломается на Tahoe.

## Tahoe (macOS 26)

- Apple **переписала строку меню**: Bartender 5 «broke beyond repair», Bartender 6 — ghost-clicks/утечки, старые Ice (≤0.11.12) — пустой layout, понадобились beta-сборки.
- Tahoe добавил штатное: «Allow in the Menu Bar», Cmd+drag перестановка, автоскрытие, прозрачный фон.
- Tahoe ужесточил права на управление чужими иконками.
- **Надёжным остаётся спейсер-подход (путь A).** Управление чужими иконками — зона постоянной поломки.

## Второй ряд (полоска под строкой меню)

На чистых NSStatusItem нереализуем. Нужен отдельный borderless `NSPanel`.
Как у Ice (`IceBar.swift`): `NSPanel`, styleMask `[.nonactivatingPanel, .fullSizeContentView, .borderless]`, `level = .mainMenu + 1`, позиция под строкой меню. **Содержимое — картинки иконок, снятые через Screen Recording** (`MenuBarItemImageCache` → `ScreenCapture.captureWindows`). Клик по иконке во втором ряду = перетаскивание реальной через CGEvent — самое хрупкое место.

→ Поэтому второй ряд = NSPanel + захват экрана + синтетические события. На Rust+objc2 каждый кирпич выразим, но это самая Tahoe-нестабильная часть. См. ADR 004.

## Референсы

- HiddenBar (путь A): `github.com/dwarvesf/hidden` — `Features/StatusBar/StatusBarController.swift`.
- Ice (путь B): `github.com/jordanbaird/Ice` — `ControlItem.swift` (спейсер), `MenuBarItemManager.swift` (CGEvent), `MenuBarItem.swift` (CGWindowList), `MenuBarItemImageCache.swift` (захват), `IceBar.swift` (второй ряд).
- Rust tray-крейты (референс objc2-вызовов NSStatusItem, но БЕЗ скрытия/второго ряда): `tray`, `tray-icon` (v0.21, в Tauri), `tray-rs` (github.com/nobane/tray-rs), `tray-item`, `trayicon-rs`.

## Открытые вопросы / проверить руками

1. objc2-app-kit 0.3.2 на macOS 26 — доступны ли символы NSStatusBar/NSStatusItem (доки кончаются на 15.5).
2. Спейсер-коллапс (`length ≈ 10000`) на переписанной строке меню Tahoe — главный тест пути A.
3. Путь B: живы ли синтетические CGEvent и CGWindowList для menu-bar-окон на Tahoe 26.3+.
4. ScreenCaptureKit vs CGWindowListCreateImage (последний deprecated) — для второго ряда.
5. Права (Screen Recording/Accessibility) корректно работают только у подписанного бандла с Info.plist — проверить на упаковке.
