# 01 CORE — FFI-каркас и разделитель в строке меню

Дата создания: 2026-06-05
Завершён: 2026-06-05 (ШАГ 1, проверено вживую на Tahoe)

## Цель

Поднять минимальное приложение на Rust + objc2, которое живёт в строке меню одним `NSStatusItem` (разделитель `<`) и реагирует на клик по нему.

## Объём работ

- [x] `cargo init`, базовая структура проекта, сборка под macOS 26 Tahoe.
- [x] Зависимости: objc2 0.6.4, objc2-app-kit 0.3.2, objc2-foundation 0.3.2 (старый `cocoa` крейт не используем). Добавлен objc2-core-foundation.
- [x] Запуск как agent без Dock: `NSApplication` с `setActivationPolicy(.Accessory)`.
- [x] Весь UI на главном потоке через `MainThreadMarker`.
- [x] Создать `NSStatusItem` через `NSStatusBar systemStatusBar → statusItemWithLength`.
- [x] Разделитель `<` как заголовок кнопки: `button().setTitle(...)` (вместо template image — для ШАГА 1 хватает текста-символа).
- [x] Обработчик клика: `setTarget` / `setAction` (`onAnchorClick:`).

## Критерий готовности

`cargo run` запускает приложение; в строке меню виден разделитель `<`; клик по нему срабатывает. В Dock иконки нет.

## Итог (ШАГ 1)

Сделано через делегат-класс `Controller` (`define_class!`, `NSApplicationDelegate`); айтемы создаются в `applicationDidFinishLaunching:`. Делегат держит айтемы живыми весь сеанс. Логи пишутся в файл (`src/log.rs`). Сборка/запуск — `bundle.sh` (.app + Info.plist `LSUIElement` + ad-hoc codesign) и `ctl.sh`. Код: `src/main.rs`, `src/controller.rs`, `src/status_bar.rs`.

Отступление от плана: разделитель — текст-символ `<`, а не template image (для ШАГА 1 достаточно). Иконку-картинку оставили на этап брендинга (спринт 06).
