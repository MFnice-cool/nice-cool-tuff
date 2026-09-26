# 03. Архитектура TuffBox

## Слои приложения

```text
┌───────────────────────────────────────┐
│ UI: Svelte + TypeScript               │
├───────────────────────────────────────┤
│ Tauri Commands API                    │
├───────────────────────────────────────┤
│ Application Services                  │
│ - ProjectService                      │
│ - ModService                          │
│ - ResolverService                     │
│ - SnapshotService                     │
│ - RunService                          │
│ - CrashService                        │
│ - ExportService                       │
│ - AiAssistantService                  │
│ - TuffSwarmService (future)           │
├───────────────────────────────────────┤
│ Domain Core                           │
│ - Project model                       │
│ - Dependency graph                    │
│ - Version constraints                 │
│ - Compatibility rules                 │
│ - Lockfile                            │
│ - ActionPlan / Crash fingerprint      │
├───────────────────────────────────────┤
│ Infrastructure                        │
│ - SQLite                              │
│ - File system                         │
│ - Download/cache                      │
│ - External APIs                       │
│ - Minecraft process runner            │
│ - Remote Crash KB client (opt-in)     │
│ - TuffSwarm P2P (future)              │
│ - GitHub Pack Transport               │
└───────────────────────────────────────┘
```

## Основные сервисы

### ProjectService

Отвечает за:

- создание проекта;
- открытие проекта;
- чтение manifest;
- синхронизацию lockfile;
- профили проекта.

### ModService

Отвечает за:

- поиск модов;
- добавление модов;
- удаление модов;
- обновление модов;
- загрузку metadata;
- side detection.

### ResolverService

Отвечает за:

- построение графа;
- проверку missing dependencies;
- проверку conflicts;
- подбор версий;
- генерацию плана изменений.

### SnapshotService

Отвечает за:

- auto snapshots;
- manual snapshots;
- release snapshots;
- diff;
- rollback.

### RunService

Отвечает за:

- установку Java;
- подготовку runtime;
- запуск Minecraft;
- сбор логов;
- фиксацию результата запуска.

### CrashService

Отвечает за:

- поиск crash reports;
- парсинг stacktrace;
- извлечение suspected mods;
- связь crash с последними изменениями.

### AiAssistantService

Отвечает только за:

- объяснение ошибок;
- формирование гипотез;
- предложение плана исправления (`ActionPlan` JSON).

ИИ не применяет изменения напрямую.

### TuffSwarmService (future)

Будущий слой федеративной сети узлов:

- Knowledge Torrent — gossip / DHT `ExperienceCapsule` по fingerprint;
- Creation Marketplace — маршрутизация задач генерации + PoUW-награды;
- никогда не обходит snapshot / confirm / deterministic apply.

Спецификация: [`13-tuffswarm-network.md`](13-tuffswarm-network.md).

### ExportService

Отвечает за:

- `.mrpack`;
- zip;
- Prism instance;
- server pack;
- changelog.

## Командная модель

Все опасные изменения должны выполняться через plan/apply:

```text
User action
→ build change plan
→ show diff
→ create snapshot
→ apply deterministic changes
→ update graph
→ optional test run
```

## Пример опасных действий

- обновление модов;
- удаление модов;
- смена loader;
- применение AI fix;
- массовое изменение конфигов;
- импорт нового modpack;
- rollback.

GitHub Pack Transport (public repos, anonymous consume) is documented in [`15-github-pack-transport.md`](15-github-pack-transport.md) and is not part of TuffSwarm.
