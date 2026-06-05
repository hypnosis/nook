# Исследование: ICE-подобный «второй ряд» на Tahoe + Rust — реализуемость

Дата: 2026-06-05. Прицельное исследование пути B (второй ряд как у Ice) под macOS 26 Tahoe на Rust+objc2. Продолжение [001-rust-objc2-menubar.md], вход для развилки ADR 004.

## Краткий вердикт

ICE-бар на Tahoe+Rust **реален, но это порт хакерского кода Ice один-в-один**, а не «чистая» утилита. Все кирпичи покрыты objc2-app-kit + objc2-core-graphics — Rust ничего не блокирует. Риск не в Rust, а в технике: на Tahoe Apple перенесла рендер чужих NSStatusItem в Control Center (отдельный процесс, layer 25 `kCGStatusWindowLevel`), из-за чего у Ice 0.11.12 layout стал пустым. Сам NSPanel второго ряда — самая лёгкая и стабильная часть. Самое хрупкое — перемещение чужих иконок синтетическими CGEvent на приватных полях и снятие их картинок через deprecated CGWindowList. Порог боли высокий, но проходимый за недели, если копировать решения Ice.

## Как Ice выжил на Tahoe (пруфы)

Хронология релизов:
- `0.11.12` (2024-10-29) — последний до-Tahoe, полностью сломан на 26.
- `0.11.13-dev.1` (2025-06-20, под бету Tahoe): *«Apple made significant changes to the menu bar that require new solutions»*. Восстановили toggle, IceBar, layout, appearance.
- `0.11.13-dev.2` (2025-09-16, под релиз Tahoe): *«fixes a majority of known issues introduced in macOS 26 Tahoe»*, адаптация под Liquid Glass.

Что сломалось (корень):
- **Чужие NSStatusItem больше не отдельные окна — их рендерит Control Center.** Иконка должна зарегистрироваться окном на layer 25 (`kCGStatusWindowLevel`). Пруфы: CodexBar #802 (*«hand-off to Control Center for rendering never completed»*), BetterDisplay #5314 (NSStatusItem visibility loop), Ice #664 (*«the entire thing appears white»*; мейнтейнер: *«Ice 0.11.12 is broken in Tahoe because Apple has fundamentally revised the menu bar»*).
- Открытые баги Tahoe на июнь 2026 (всё ещё не закрыты): #951, #916 (пустой layout 26.3/26.4), #947/#867 (краши NSStatusBarWindow, EXC_BREAKPOINT), #840 (краш при включении IceBar).

**Ice НЕ перешёл на новые официальные API — остался на старых хаках, подкрутил под новый рендер.**

## Минимальные права

| Право | Для чего | Можно убрать? |
|---|---|---|
| Screen Recording | снимать картинки чужих иконок для отрисовки во втором ряду (CGWindowList capture оффскрин-иконок) | Да, если рисовать свои плашки/ярлыки вместо снимков чужих иконок. Для самого NSPanel — не нужен. |
| Accessibility | не обязателен — перемещение через CGEvent+postToPid, позиции из CGWindowList | Практически да |
| «Allow in the Menu Bar» (тумблер System Settings, НЕ API) | чтобы СВОЯ иконка показалась на Tahoe | Нет — новое требование Tahoe к своему статус-айтему |

Путь без Screen Recording существует, но это уже не «как Ice»: второй ряд показывает не снимки чужих иконок, а свои кликабельные плашки (= лаунчер).

**Официального программного API для управления чужими иконками / overflow в Tahoe НЕТ.** Спекулятивные `StatusBarManager`, `MenuBarConfiguration`, `MenuBarOverflowManager`, `SBAllowInMenuBar` — галлюцинации Perplexity, в первоисточниках Apple и в коде Ice их нет. Реален только пользовательский тумблер. Требует ручной проверки macOS 26 SDK, но сигнал из кода Ice сильный.

## Портируемость на Rust по кирпичам

| Кирпич | Как у Ice (реально) | Крейт + версия | Сложность | Риск |
|---|---|---|---|---|
| NSPanel второго ряда | NSPanel, styleMask [.nonactivatingPanel,.fullSizeContentView,.borderless], level=.mainMenu+1, collectionBehavior [.fullScreenAuxiliary,.ignoresCycle,.moveToActiveSpace], originY=screen.maxY-1-menuBarHeight-frameHeight | objc2-app-kit 0.3.2 | Низкая | Низкий — чистый AppKit |
| Снимок чужих иконок | ScreenCapture.captureWindows → CGWindowList. Комментарий Ice: *«ScreenCaptureKit doesn't support capturing composite images of offscreen menu bar items»* | objc2-core-graphics (CGWindowList, deprecated); screencapturekit v7 / objc2-screen-capture-kit 0.3.2 НЕ заменяют | Средняя | Высокий — deprecated API, нужен Screen Recording |
| Перемещение иконки (Cmd-drag) | CGEvent.menuBarItemEvent: flags=.maskCommand, приватные поля .eventTargetUnixProcessID, .mouseEventWindowUnderMousePointer, .windowID; старт (20000,20000); event.postToPid(pid); CGEventSource(.hidSystemState) | objc2-core-graphics (CGEvent, setIntegerValueField, postToPid) | Высокая | Очень высокий — приватные CGEventField, тайминги, воркэраунды. Самый хрупкий код Ice |
| Чтение окон иконок | CGWindowListCopyWindowInfo (позиции, owner PID) | objc2-core-graphics | Средняя | Средний/высокий — на Tahoe метаданные menu-bar-окон обеднели |
| Свой статус-айтем (триггер) | NSStatusItem | objc2-app-kit 0.3.2 | Низкая | Низкий |

Приватные CGEventField-константы в Rust придётся задавать числами вручную (unsafe, без типобезопасности).

## Новые подходы 2026 / штатные Tahoe API

- Официального API для второго ряда/overflow нет (Ice остался на хаках). Tahoe дал только тумблер «Allow in the Menu Bar» + автоскрытие в notch — про свою иконку, не про чужие.
- ScreenCaptureKit на Tahoe доступен из Rust: `screencapturekit` v7 (обновлён 2025-12, фичи macos_14_2, macos_26_0); `objc2-screen-capture-kit 0.3.2`. Но для оффскрин menu-bar-иконок SCK не подходит — не спасает от deprecated CGWindowList.
- **Эталон пути A:** `nanobar` (yansircc, релизы фев 2026, crates.io) — ~90 строк, NSStatusItem-пушер раздувается до 10000pt, выталкивает иконки за край. *«No accessibility, no Screen Recording, no private APIs, no SIP»*. Работает на Tahoe. Для пути B готового Rust-проекта нет.

## Вердикт по объёму и хрупкости

- NSPanel-второй ряд сам по себе — день-два, низкий риск. Со своими плашками (без снимков чужих) — надёжный «полу-IceBar» без Screen Recording и CGEvent-хаков.
- Полный IceBar как у Ice (реальные снимки + клик двигает реальную иконку) — порт MenuBarItemManager (тысячи строк, тайминги, приватные поля) + capture на deprecated API. Недели + постоянное сопровождение: каждое обновление 26.x потенциально ломает (у Ice сейчас открыты краши и пустой layout на 26.3/26.4).
- Самое хрупкое: (1) синтетический Cmd-drag на приватных CGEventField; (2) capture оффскрин-иконок через deprecated CGWindowList; (3) иконки в Control Center — фундамент, который может меняться без предупреждения.

## Что проверить руками

1. macOS 26 SDK / AppKit headers — есть ли реально публичный API для overflow/управления чужими иконками (ожидание: нет).
2. Собрать `nanobar` на своей Tahoe-машине — подтвердить, что objc2-спейсер (путь A) работает сейчас.
3. Прогнать `screencapturekit` v7 с macos_26_0 на захвате menu bar — отдаёт ли что-то по оффскрин-иконкам (ожидание: нет).
4. Скачать Ice 0.11.13-dev.2, потыкать IceBar на 26.x — оценить стабильность вживую (issues #840, #947, #951 — глючит).
5. Проверить приватные CGEventField-константы в objc2-core-graphics — объявлены или хардкодить числами.

## Файлы-ориентиры Ice (для порта пути B)
- `Ice/UI/IceBar/IceBar.swift` — NSPanel второго ряда
- `Ice/MenuBar/MenuBarItems/MenuBarItemManager.swift` — CGEvent-перемещение
- `Ice/Utilities/ScreenCapture.swift` — CGWindowList capture
- `Ice/MenuBar/MenuBarItems/MenuBarItem.swift` — чтение окон
