# ТЗ: TuffBox GitHub Pack — git-дистрибуция сборок (A + packwiz)

- **Статус:** проект спецификации (на утверждении)
- **Дата:** 2026-08-14
- **Автор:** TuffBox
- **Связь:** `docs/13-tuffswarm-network.md` (транспорт дистрибуции — отдельный контур от краш-знаний TuffSwarm)

---

## 1. Цель и не-цели

### Цель
Дать авторам модпаков собственную **git-подобную систему** шаринга сборок поверх **GitHub**, превосходящую Modrinth Launcher по:

- отсутствию аккаунт-стены для потребителей (анонимный read публичного репо);
- отсутствию рекламных баннеров;
- работе на cracked/offline аккаунтах (нет привязки к MS OAuth);
- нативной поддержке **самописных модов** (бинари в GitHub Releases);
- **двустороннему** вкладу друзей через PR (чего нет у Modrinth);
- бесплатной git-истории / diff / rollback / CDN.

Сервер не поднимаем: переиспользуем существующую инфраструктуру мира (GitHub API, Releases CDN, git).

### Не-цели (в этой итерации)
- Не заменяем TuffSwarm (контур краш-знаний / Creation Marketplace) — это другой транспорт.
- Не делаем свой хостинг модов и не конкурируем с Modrinth как платформой.
- Кастом-бинари через PR от друзей — фаза 2 (см. §6).
- Монетизация / creator features — вне scope.

---

## 2. Зафиксированные решения (из уточнений)

| Тема | Решение |
|------|---------|
| Onboarding друга | Share-ссылка, **без аккаунта GitHub** (анонимный read публичного репо через GitHub API/raw) |
| Направление | **Двустороннее**: друг может предложить изменение через PR |
| Хранение самописных модов | **GitHub Release assets** (CDN, лимит 2 GB/файл, без LFS-billing) по тегу версии |
| Глубина интеграции | **Export-снапшот при пуше**: локально собираем сборку, «Publish to GitHub» снапшотит манифест + кастомные моды |
| Формат манифеста | **A + packwiz (B)**: репо читается и TuffBox-друзьями, и packwiz/Prism-юзерами |
| Auth автора | GitHub OAuth **device flow** (без секретов в бинаре), токен в OS keyring |

---

## 3. Архитектура: layout репозитория

```
<repo>/
  modpack.json        # TuffBox-нативный манифест (source of truth, правится руками)
  index.toml          # packwiz index (генерируется из modpack.json)
  options.toml        # mc version + loader (packwiz)
  mods/*.pw.toml      # packwiz per-mod файлы (генерируются)
  README.md           # авто: share-ссылка + инструкция для packwiz/Prism
```

- **Обычные моды** (Modrinth/CF): в репо хранятся **только ссылки + точная версия + хеш**. Бинари не копируются.
- **Кастомные моды** (самописные): как **Release-ассеты** тега `vX.Y.Z` (CDN).
  - Для packwiz-интеропа: зеркальная копия < 50 MB в `mods/` как packwiz `file`-мод; большие остаются TuffBox-only (через asset-URL).
- `modpack.json` — расширение текущего `manifest.rs` (`SourceKind::Modrinth | Curseforge` + version + sha1; кастомные — `local_path` + asset ref).

---

## 4. Publish flow (автор)

Команда «Publish to GitHub»:

1. Сериализация проекта → `modpack.json` (`SourceKind` + версия + hash; кастомные — локальный путь).
2. Генерация packwiz-файлов (`index.toml`, `mods/*.pw.toml`, `options.toml`) — переиспользуем `crates/tuffbox-core/src/packwiz.rs`.
3. Semver bump → тег `vX.Y.Z`.
4. Заливка кастомных бинарей как Release-ассетов тега (GitHub API, device-flow токен).
5. Коммит манифеста + packwiz-файлов, пуш тега, Release Notes (changelog = diff манифеста vs пред. тег).
6. Выдача share-линка: `https://github.com/<user>/<repo>` + `tuffbox://github/<user>/<repo>` deep link.

**Auth:** GitHub OAuth device flow (`integrations.rs`, токен в OS keyring рядом с `swarm.*`).

---

## 5. Consume flow (друг, анонимно)

1. Вставляет share-линк → TuffBox парсит `<user>/<repo>`.
2. Тянет `modpack.json` latest-тега (анонимный GitHub API / raw).
3. Diff против локального lockfile: added / removed / bumped; новые кастом-ассеты.
4. Apply:
   - линкованные моды → `provider/` (Modrinth/CF) по точным версиям;
   - кастомные → скачивание Release-ассета.
   - Переиспользуем `updater.rs` (check/apply) + `mod_files.rs` (download + SHA1-verify).
5. Rollback = предыдущий тег.

---

## 6. PR flow (друг предлагает изменение)

1. Друг форкает репо (TuffBox открывает fork-URL или через API).
2. Правит `modpack.json` (добавить мод-ссылку / bump версии) — либо в TuffBox «Suggest mod», либо прямо на GitHub.
3. Открывает PR. Автор мержит (GitHub UI или TuffBox merge-хелпер) → все друзья получают при пуле.
4. **Фаза 2:** кастом-бинари через PR (автор пуллит форк, пере-релизит ассеты).

Объём MVP: PR правит **манifest (ссылки)**. Кастом-бинари через PR — отдельная итерация.

---

## 7. Безопасность / вирусы (бьём боль Modrinth)

- Точные версии + хеш в манифесте → tamper-evident; друг верифицирует SHA1 при скачивании (уже в `mod_files.rs`).
- **Ed25519-подпись манифеста** (как `ExperienceCapsule` в TuffSwarm) → end-to-end целостность от автора.
- Trust-бейдж для кастом/PR-модов на базе co-occurrence графа (`mod_cooccurrence_pairs`) + `soft-verify`.
- GitHub-идентичность автора + опц. signed commits как сигнал доверия.

---

## 8. Интеграция с кодом TuffBox (reuse-map)

### Новое
- `crates/tuffbox-core/src/github_pack.rs` — транспорт `ModpackRemote` (параллельно TuffSwarm-транспортам, но для дистрибуции сборок).
- Desktop: `integrations.rs` (GitHub OAuth device flow + keyring), `GitHubPublishDialog.svelte`, `SharePackDialog.svelte`, consume-UI в Library/Mods.

### Переиспользуем (уже есть)
- `crates/tuffbox-core/src/packwiz.rs` — генерация packwiz-файлов.
- `crates/tuffbox-core/src/provider/{modrinth,curseforge}.rs` — резолв ссылок → бинари.
- `crates/tuffbox-core/src/updater.rs` — применение изменений (diff).
- `crates/tuffbox-core/src/mod_files.rs` — скачивание + verify хеша.
- `crates/tuffbox-core/src/exporter.rs` — добавить GitHub-экспорт рядом с mrpack/zip.
- `crates/tuffbox-core/src/manifest.rs` — `SourceKind`.

**Инвариант (из docs/13):** TuffSwarm = краш-знания; GitHub-pack = дистрибуция. Держим раздельно, но тот же pluggable-паттерн транспортов.

---

## 9. Roadmap: заимствования из Prism / Modrinth (вне git-системы)

Для базового комфорта лаунчера (не про дистрибуцию). TuffBox уже силён по loaders/dependency-детекту (`adapters/neoforge.rs`, `crash.rs`), поэтому только реальные зазоры:

### 9.1 Java auto-download / install (самый большой зазор)
- **Prism:** `launcher/minecraft/launch/AutoInstallJava.cpp` + `java/JavaUtils.cpp` + настройка `AutomaticJavaDownload`.
- **Modrinth:** crate `daedalus` (Rust-эталон, ближе к ядру TuffBox).
- **TuffBox сейчас:** лишь ссылка на adoptium (`crash_assistant.rs:346,356`), ручной `JavaPickerModal`. Реального fetch+install JDK нет.
- **Действие:** auto-detect нужной Java под MC-версию + download+install+switch.

### 9.2 Offline / cracked аккаунты (username-only)
- **Prism:** `minecraft/auth/AuthSession.cpp:21 MakeOffline`, `LaunchMode::Offline`, `--offline`, MS device-code `auth/steps/MSADeviceCodeStep.cpp`.
- **Действие:** добавить account-type `offline` (имя игрока); гарантировать запуск без MS. Прямое преимущество над Modrinth Launcher.

### 9.3 Install-time dependency resolution + changelog (UI)
- **Prism:** `mod/tasks/GetModDependenciesTask.cpp` + `ui/dialogs/ResourceUpdateDialog.cpp` (ставит required-deps при установке, показывает changelog).
- **Modrinth:** `dependencies-stage.ts` + `ContentDependencyWarningModal`, `changelog.ts`.
- **TuffBox:** граф зависимостей мощнее, но **UI установки зависимостей** беднее. Переиспользовать граф, добавить install-time резолв + модалку + «что изменилось».

### 9.4 Менеджер ресурспаков / шейдеров / датапаков
- **Prism:** `ResourceDownloadDialog`, `DataPackFolderModel`, `ShaderPackFolderModel`.
- **Действие:** расширить каталог TuffBox (`Mods.svelte`) шейдерами/ресурспаками.

### 9.5 Обогащение Modrinth-провайдера
- **Modrinth:** crate `labrinth` (Modrinth API client) — фасеты поиска, фильтр версий по loader+MC, teams, dependencies.
- **Действие:** обогатить `provider/modrinth.rs` + улучшить фильтр совместимых версий в каталоге.

### 9.6 Не берём (YAGNI)
- Prism: ATL / Technic / FTB интеграции (нишевые, лицензионные).
- Modrinth: монетизация создателей / creator features.

---

## 10. Тесты

- **Unit:** round-trip `modpack.json`; генерация packwiz-файлов валидна для packwiz; корректность `diff(old,new)`.
- **Integration (mock GitHub):** publish → consume анонимно → идентичный набор модов; скачивание кастом-ассета + hash-check.
- **PR:** fork + PR merge → re-publish.
- **Security:** несовпадение хеша отбрасывается; подпись манифеста верифицируется.
- **Java auto (9.1):** детект нужной версии + установка + запуск на чистой машине.

---

## 11. Риски / открытые вопросы

- GitHub API rate limits для анонимного read (60 req/h/IP) — нужен fallback на raw + кэш.
- Приватные репо: другу нужен токен (не в MVP; публичные по умолчанию).
- Конфликт packwiz-интеропа для кастомных модов > 50 MB — оставляем TuffBox-only.
- Подпись манифеста: нужен ли отдельный device-key или переиспользовать TuffSwarm `device_signing_key`?
