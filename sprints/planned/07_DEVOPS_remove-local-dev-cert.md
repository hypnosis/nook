# 07 — Убрать локальный сертификат подписи

Создан: 2026-10-01

## Цель

Убрать временную локальную подпись «Nook Local Dev», когда появится настоящий Developer ID (см. ADR 011).

## Объём работ

- [ ] Удалить `scripts/install-local.sh` (помечен `HARDCODE`).
- [ ] Удалить сертификат и ключ «Nook Local Dev» из связки ключей входа (Связка ключей → Мои сертификаты).
- [ ] Подписывать сборки Developer ID в `bundle.sh` / `make-dmg.sh`.

## Критерий готовности

`security find-identity -p codesigning` не показывает «Nook Local Dev», в репозитории нет упоминаний `Nook Local Dev`, права Nook (запись экрана, Accessibility) переживают пересборку за счёт Developer ID.
