# 02. Стек TuffBox

## Рекомендуемый стек

```text
Desktop: Tauri v2
Core: Rust
Frontend: Svelte + TypeScript
Database: SQLite
Editor: CodeMirror 6
Async/download: Tokio + reqwest
Cache: content-addressed storage
Graph/resolver: Rust core module
AI: external LLM API через отдельный service layer
```

## Почему Rust

Rust подходит для:

- работы с файловой системой;
- запуска Minecraft как процесса;
- хэширования файлов;
- кэширования;
- распаковки архивов;
- построения dependency graph;
- deterministic resolver;
- безопасного snapshot/rollback;
- CLI-версии в будущем.

## Почему Tauri, а не Electron

- меньше потребление памяти;
- быстрее старт;
- Rust backend из коробки;
- хороший компромисс между красивым UI и низким overhead.

## Почему Svelte

- лёгкий frontend;
- меньше runtime;
- удобно делать быстрый интерфейс;
- хорошо подходит для low-end устройств.

## Почему SQLite

SQLite хранит:

- проекты;
- индексы модов;
- lockfile metadata;
- историю изменений;
- runs;
- crash reports;
- snapshots;
- export jobs;
- кэш metadata провайдеров.

## Почему CodeMirror 6

CodeMirror легче Monaco и достаточно хорош для:

- `.toml`;
- `.json`;
- `.json5`;
- `.properties`;
- `.cfg`;
- `.js` / KubeJS;
- `.zs` / CraftTweaker;
- поиска по проекту.

## Провайдеры контента

### MVP

- Modrinth API;
- local `.jar`;
- local folder import;
- `.mrpack` import/export.

### Позже

- CurseForge API;
- GitHub Releases;
- direct URL;
- Prism/MultiMC import;
- server pack templates.

## Архитектурный принцип

Frontend не должен решать зависимости. UI только показывает состояние и вызывает команды core.

```text
UI → command → Rust core → deterministic result → UI state
```
