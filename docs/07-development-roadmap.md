# 07. План разработки TuffBox IDE

Этот roadmap спроектирован так, чтобы разработку можно было вести итеративно: сначала создать маленькое рабочее ядро, затем постепенно добавлять IDE-функции, запуск Minecraft, анализ крашей и экспорт.

## Главный принцип разработки

Не начинать с красивого интерфейса и не начинать с ИИ.

Правильный порядок:

```text
Domain model
→ Project manifest
→ Dependency graph
→ Resolver
→ Snapshots
→ CLI/dev harness
→ Desktop UI
→ Test launch
→ Crash parsing
→ AI explanation
→ Export/release manager
```

ИИ подключается только после того, как есть детерминированная основа: проект, граф, lockfile, снапшоты и логи.

---

# Stage 0. Product Foundation

## Цель

Зафиксировать границы продукта, MVP и технические правила, чтобы не расползтись в «ещё один лаунчер».

## Результат

- Документы продукта.
- MVP scope.
- Технические ограничения.
- Формат проекта TuffBox v0.1.

## Что сделать

- Уточнить user flow разработчика сборки.
- Уточнить MVP-функции.
- Утвердить стек.
- Утвердить модель: manifest + lockfile + dependency graph + snapshots.
- Описать, что делает ИИ и что ИИ не делает.

## Готово, когда

- Есть папка `docs/` с продуктовой и технической концепцией.
- Есть первые JSON schemas.
- Есть примеры проекта и графа.

Статус: **почти готово**.

---

# Stage 1. Core Domain Model

## Цель

Создать реальное ядро `tuffbox-core`, которое умеет описывать проект, моды, версии, зависимости и граф.

## Результат

Rust crate без GUI.

```text
crates/tuffbox-core/
  src/
    lib.rs
    project.rs
    manifest.rs
    lockfile.rs
    graph.rs
    resolver.rs
    diagnostics.rs
    change_plan.rs
```

## Что сделать

- Перенести концептуальные типы из `src-concepts/rust/domain.rs` в реальный crate.
- Описать `ProjectManifest`.
- Описать `Lockfile`.
- Описать `DependencyGraph`.
- Описать `GraphNode` и `GraphEdge`.
- Описать `Diagnostic`.
- Описать `ChangePlan`.
- Добавить сериализацию через `serde`.
- Добавить unit-тесты.

## Минимальные тесты

- Загружается example manifest.
- Создаётся граф из manifest.
- Находится конфликт `A conflicts B`.
- Находится missing dependency.
- Resolver возвращает diagnostics, но не меняет проект.

## Готово, когда

Можно запустить тесты:

```bash
cargo test -p tuffbox-core
```

И получить диагностический отчёт по example project.

---

# Stage 2. Project Manifest + Lockfile

## Цель

Сделать воспроизводимый формат проекта: manifest описывает намерения, lockfile фиксирует конкретные версии и файлы.

## Результат

TuffBox-проект можно загрузить, проверить и сохранить обратно.

## Что сделать

- `ProjectManifest`:
  - project metadata;
  - Minecraft version;
  - loader;
  - profiles;
  - declared mods;
  - overrides paths.
- `TuffboxLockfile`:
  - resolved mod versions;
  - file hashes;
  - source metadata;
  - dependency edges;
  - generated timestamp.
- Версионирование схемы.
- Миграции схемы в будущем.

## Команды для dev harness

```bash
tuffbox project validate examples/sample-project.tuffbox.json
tuffbox project lock examples/sample-project.tuffbox.json
tuffbox graph print examples/sample-project.tuffbox.json
```

## Готово, когда

- Manifest валидируется.
- Lockfile создаётся.
- Повторный lock даёт одинаковый результат на одинаковых входных данных.

---

# Stage 3. Deterministic Dependency Graph & Resolver

## Цель

Сделать главную техническую основу TuffBox: граф зависимостей и resolver.

## Результат

Система умеет находить проблемы в сборке без ИИ.

## Что сделать

### Graph builder

- Узлы:
  - Minecraft;
  - loader;
  - Java;
  - mods;
  - profiles;
  - config/script files позже.
- Рёбра:
  - requires;
  - optional;
  - conflicts;
  - replaces;
  - requires_loader;
  - requires_minecraft;
  - requires_java.

### Resolver diagnostics

- Missing dependency.
- Version mismatch.
- Direct conflict.
- Duplicate mod.
- Wrong side in profile.
- Unknown side.
- Deprecated/local-only warning.

### Change plans

Resolver должен возвращать план, а не применять изменения.

```text
diagnostics → change plan → user review → snapshot → apply
```

## Готово, когда

Для sample graph resolver выдаёт понятные diagnostics и потенциальный fix plan.

---

# Stage 4. Provider Layer: Modrinth + Local JAR

## Цель

Научить TuffBox получать реальные metadata модов.

## Результат

Можно добавить мод из Modrinth или локальный `.jar` и получить зависимости.

## Что сделать

### Provider interface

```rust
trait ContentProvider {
    async fn search(...);
    async fn get_project(...);
    async fn get_versions(...);
    async fn get_file(...);
    async fn resolve_dependencies(...);
}
```

### Modrinth provider

- Поиск проектов.
- Получение версий.
- Фильтр по Minecraft version.
- Фильтр по loader.
- Dependencies.
- Hashes.
- Download URL.

### Local JAR provider

- Добавление `.jar`.
- Вычисление sha1/sha512.
- Чтение fabric.mod.json / mods.toml, если возможно.
- Пометка `source=local`.

## Готово, когда

CLI/dev harness может:

```bash
tuffbox modrinth search sodium --mc 1.20.1 --loader fabric
tuffbox project add-mod sodium
tuffbox project resolve
```

---

# Stage 5. Snapshot System

## Цель

Сделать безопасные изменения и rollback.

## Результат

Любое опасное действие можно откатить.

## Что сделать

- Manual snapshot.
- Auto snapshot before dangerous action.
- Snapshot metadata:
  - id;
  - name;
  - created_at;
  - reason;
  - changed files;
  - manifest copy;
  - lockfile copy.
- Diff между snapshot.
- Rollback.

## Важное решение

На MVP можно хранить снапшоты простым способом: копировать manifest, lockfile и изменённые файлы. Позже оптимизировать через content-addressed storage.

## Готово, когда

Можно выполнить:

```bash
tuffbox snapshot create "before-update"
tuffbox project add-mod iris
tuffbox snapshot diff before-update current
tuffbox snapshot rollback before-update
```

---

# Stage 6. Minimal CLI / Dev Harness

## Цель

Дать возможность быстро тестировать core без GUI.

## Почему это важно

Для эффективной разработки сначала нужен CLI, потому что:

- проще тестировать;
- проще писать автотесты;
- проще отлаживать resolver;
- GUI не блокирует core-разработку.

## Команды MVP

```bash
tuffbox project new
tuffbox project validate
tuffbox project lock
tuffbox graph print
tuffbox graph diagnostics
tuffbox modrinth search
tuffbox mod add
tuffbox mod remove
tuffbox resolve
tuffbox snapshot create
tuffbox snapshot list
tuffbox snapshot rollback
```

## Готово, когда

Через CLI можно создать проект, добавить мод, построить граф, увидеть diagnostics и сделать snapshot.

---

# Stage 7. Desktop Shell + Basic UI

## Цель

Сделать первый Tauri UI поверх уже работающего core.

## Результат

Пользователь может открыть проект и увидеть его состояние.

## Экраны MVP

- Dashboard.
- Mods.
- Dependency Graph.
- Diagnostics.
- Snapshots.
- Settings.

## Что сделать

- Tauri app.
- Svelte UI.
- Tauri commands:
  - open project;
  - validate project;
  - list mods;
  - get graph;
  - get diagnostics;
  - create snapshot;
  - rollback snapshot.
- Простая визуализация графа.

## Готово, когда

В UI можно открыть sample project, увидеть моды, граф, diagnostics и создать snapshot.

---

# Stage 8. Config Editor

## Цель

Добавить IDE-ощущение: редактирование конфигов внутри проекта.

## Результат

Можно редактировать `.json`, `.toml`, `.properties`, `.cfg`, `.js`, `.zs`.

## Что сделать

- File tree.
- CodeMirror 6.
- Syntax highlighting.
- Search in project.
- Save.
- Auto snapshot перед массовыми изменениями.
- Format JSON/TOML.

## Не делать в MVP

- Полный IntelliSense KubeJS.
- Визуальный редактор рецептов.
- Сложные schemas для всех модов.

## Готово, когда

Пользователь может открыть config, изменить его, сохранить, увидеть изменение в истории и откатить snapshot.

---

# Stage 9. Test Launcher

## Цель

Замкнуть цикл разработки: изменить сборку → запустить → получить результат.

## Результат

TuffBox может запускать тестовую instance и собирать логи.

## Что сделать

- Подготовка instance directory.
- Download/install Minecraft libraries/assets.
- Loader installation для Fabric first.
- Java detection.
- Memory settings.
- Process runner.
- Log capture.
- Run history.
- Auto snapshot before run.

## MVP loader order

1. Fabric.
2. Quilt.
3. Forge.
4. NeoForge.

## Готово, когда

Можно нажать `Run`, Minecraft запускается, а TuffBox сохраняет run record и logs.

---

# Stage 10. Crash Parser

## Цель

Научиться анализировать краш без ИИ.

## Результат

TuffBox извлекает stacktrace, suspected classes, suspected mods и recent changes.

## Что сделать

- Поиск `crash-reports/*.txt`.
- Чтение `latest.log`.
- Выделение stacktrace.
- Поиск mod ids в логе.
- Сопоставление package/class names с mod jar metadata.
- Связь с последними изменениями.
- Crash report model.

## Готово, когда

После краша UI показывает:

- время;
- profile;
- exit code;
- suspected mods;
- important stacktrace;
- recent changes before crash.

---

# Stage 11. AI Crash Explanation

## Цель

Подключить ИИ только поверх уже подготовленного crash context.

## Результат

ИИ объясняет ошибку человеческим языком и предлагает план, который затем проверяется resolver.

## Что отправлять в ИИ

- Minecraft version.
- Loader/version.
- Java version.
- Suspected mods.
- Recent changes.
- Compact stacktrace.
- Diagnostics from resolver.
- Known graph conflicts.

## Что получать от ИИ

Структурированный JSON:

```json
{
  "humanExplanation": "...",
  "confidence": 0.78,
  "suspectedNodes": ["mod:oculus"],
  "recommendedPlan": [],
  "needsUserReview": true
}
```

## Готово, когда

AI explanation появляется в crash screen, но любые действия проходят через `ChangePlan`, snapshot и confirmation.

---

# Stage 12. Export & Release Manager

## Цель

Дать разработчику возможность выпускать сборку.

## Результат

Можно создать release snapshot и экспортировать pack.

## MVP export targets

- `.mrpack`.
- Plain zip.
- Prism/MultiMC instance zip.

## Позже

- CurseForge zip.
- Server pack zip.
- Modrinth draft release.
- GitHub release.

## Что сделать

- Release snapshot.
- Changelog from changes.
- Export profiles.
- Include/exclude client/server/debug files.
- Validate before export.

## Готово, когда

Пользователь может нажать `Release v0.1.0`, получить changelog и `.mrpack`.

---

# Stage 13. Alpha Release

## Цель

Дать первую версию реальным сборщикам.

## Состав Alpha

- Project workspace.
- Modrinth mods.
- Dependency graph.
- Diagnostics.
- Snapshots.
- Config editor.
- Fabric test launch.
- Crash parser.
- AI explanation.
- `.mrpack` export.

## Критерии Alpha

- 5–10 реальных пользователей могут собрать тестовый Fabric modpack.
- Можно поймать crash и получить объяснение.
- Можно откатиться после неудачного изменения.
- Можно экспортировать pack.

---

# Stage 14. Beta

## Цель

Расширить поддержку и подготовить продукт к публичному использованию.

## Что добавить

- Forge support.
- NeoForge support.
- CurseForge import/export.
- Server pack builder.
- Better graph UI.
- Better update manager.
- Performance audit.
- **Optimize pack** wizard (Content): curated Fabric Modrinth packs per MC version + custom MR→CF missing mods and safe config patches.
- Crash knowledge base.

---

# Stage 15. Public Release

## Цель

Публичный релиз TuffBox IDE.

## Требования

- Installer for Windows/Linux/macOS.
- Auto-update.
- Signed builds.
- Crash reporting opt-in.
- Documentation.
- RU/EN localization.
- Example projects.
- Onboarding.

---

# Stage 16. TuffSwarm Network

GitHub Pack Transport (friend-server packs over public GitHub repos) is a **separate** increment: [`15-github-pack-transport.md`](15-github-pack-transport.md). Do not fold it into TuffSwarm.

Stabilization status (branch `feat/github-pack-transport`): transport schema v2, signed envelope, release-asset materialize/verify, atomic install, pinned-signer rules, reviewed-commit apply, OAuth/publish/update UI hardening. Integration target: `feat/svelte-5-migration`.

## Цель

Федеративная сеть узлов TuffBox: обмен опытом крашей и marketplace генерации за полезную работу.

Канон: [`13-tuffswarm-network.md`](13-tuffswarm-network.md).

## Контуры

1. **Knowledge Torrent** — асинхронный opt-in обмен `ExperienceCapsule` (fingerprint → ActionPlan), без raw logs.
2. **Creation Marketplace** — задачи генерации (KubeJS / configs / recipes) на idle GPU; PoUW через Kudos / premium; заказчик = hard verifier.

## Фазы внутри Stage 16

| Phase | Содержание |
|-------|------------|
| C | P2P gossip / DHT lookup capsules |
| D | Creation jobs + verification + rewards |
| E | Lightweight adapters / federated updates |

## Preconditions (уже в продукте)

- Local `ActionPlan` + validate/apply.
- Authored Crash KB export (без notes).
- Opt-in remote KB client (`crash_remote`) — Phase B.

## Готово, когда (минимальный Stage 16C)

- Узел может опубликовать и получить capsule по fingerprint (opt-in).
- Полученный план проходит тот же confirm → snapshot → apply, что и локальный AI.
- Нет bulk dump корпуса KB.

---

# Рекомендуемый порядок реализации прямо сейчас

## Следующие 5 задач

1. Создать workspace настоящего проекта:

```text
crates/tuffbox-core
crates/tuffbox-cli
apps/tuffbox-desktop
```

2. Реализовать `tuffbox-core`:

- manifest;
- lockfile;
- graph;
- diagnostics.

3. Реализовать CLI-команды:

- validate;
- graph;
- diagnostics.

4. Написать тесты на sample graph.

5. Только потом начинать Tauri UI.

## Почему так

Если сразу делать UI, мы утонем в интерфейсе. Если сначала сделать core + CLI, TuffBox будет иметь крепкую основу, а UI станет оболочкой над работающей системой.
