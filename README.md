# TuffBox IDE

**TuffBox** — IDE для разработки Minecraft-сборок: управление модами как зависимостями, граф совместимости, снапшоты, тестовые запуски, экспорт и ИИ-анализ краш-логов.

Моды, библиотеки, загрузчики, конфиги и профили описываются как **граф зависимостей**. ИИ анализирует краш-логи, находит подозрительные узлы в графе и предлагает план исправления — но не принимает архитектурные решения за систему.

## Структура проекта

```text
crates/
  tuffbox-core/    — детерминированное ядро: граф, резолвер, lockfile, провайдеры, снапшоты, экспорт, лаунчер
  tuffbox-cli/     — CLI / dev harness

apps/
  tuffbox-desktop/ — Tauri + Svelte desktop-оболочка

docs/              — архитектурная документация
schemas/           — JSON Schema для проекта и узлов графа
examples/          — примеры проектов и графов
```

## Возможности

- **Граф зависимостей** — моды, конфликты, missing deps, дубликаты, side mismatch
- **Lockfile** — фиксация версий, хешей, source metadata и рёбер графа
- **Снапшоты** — создание, сравнение, diff (inline), откат состояния проекта
- **Провайдеры** — Modrinth API + локальные JAR с вычислением хешей
- **Запуск Minecraft** — скачивание клиента, библиотек, natives, assets, профиля Fabric/Quilt, фоновый Java-процесс
- **Экспорт** — `.mrpack`, zip, Prism instance, server pack
- **Schema migration** — версионирование схемы проекта
- **Config editor** — безопасное редактирование конфигов
- **CI** — GitHub Actions (rust.yml)
- **Desktop UI** — Tauri + Svelte, dark theme, sidebar, Play, модальные окна, джава-пикер, лог запуска

## Сборка

```bash
cargo build --release
cargo run --bin tuffbox-cli -- --help
```

## Лицензия

MIT OR Apache-2.0
