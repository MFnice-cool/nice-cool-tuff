# 12. План заимствования функций из packwiz и ServerPackCreator

> Цель: взять проверенные идеи и паттерны из
> [packwiz](https://github.com/packwiz/packwiz) (Go) и
> [ServerPackCreator](https://github.com/Griefed/ServerPackCreator) (Kotlin/Java)
> и адаптировать их под архитектуру TuffBox IDE (см. `docs/03-architecture.md`,
> `docs/10-unified-model-and-modules.md`).
> ТОЛЬКО ПЛАН — без реализации.

---

## 0. Контекст: что уже есть в TuffBox

Из `docs/04-feature-map.md` и структуры репозитория (`crates/`, `apps/`,
`bridges/`, `share/`):

- P0: workspace, manifest, lockfile, profiles (client/server/dev/release), mod
  list, Modrinth import, local jar import, dependency graph, conflict detection,
  snapshots, rollback, test launch, export `.mrpack`.
- P1: AI crash explanation, change plan, safe/risky updates, diff snapshots,
  changelog, export Prism instance, side labeling, search, formatting.
- P2: CurseForge import/export, **server pack builder**, KubeJS/CraftTweaker
  snippets, migration Forge→NeoForge.

Оба анализируемых репозитория напрямую закрывают P0/P1/P2, особенно:
модель метаданных модов, хеширование/индекс, кеширование загрузок,
сервер-пак билдер, сканирование модов, version meta, плагины.

---

## 1. packwiz — что брать

### 1.1 Модель метаданных мода (per-mod `.pw.toml`) → `crates/mod-manifest`
Источник: `packwiz/core/mod.go`

- Каждый мод = отдельный TOML-файл с `name`, `filename`, `side`
  (`server`/`client`/`both`/``), `pin`, `download{url,hash-format,hash,mode}`,
  `update{...}` (произвольный map для апдейтеров), опционально `option`
  (optional/description/default).
- Прямая польза для TuffBox:
  - Side labeling (P1) уже встроен в модель — не надо изобретать.
  - `pin` → защита от авто-обновления при safe/risky updates (P1).
  - `update` map → плагиновая система апдейтеров (Modrinth/CF/GitHub),
    расширяемая без правки ядра.
- Адаптация: заменить `ModDownload.Mode` (`url` / `metadata:curseforge`) на
  enum-вариант в Rust; `update` map → `serde_json::Value` или типизированный
  `enum UpdateSource`.

### 1.2 Pack manifest + PackFormat + авто-миграция → `crates/workspace`
Источник: `packwiz/core/pack.go`

- `Pack` с `pack-format` вида `packwiz:1.1.0`, semver-констрейнты
  `PackFormatConstraintAccepted`/`SuggestUpgrade`, авто-миграция
  `packwiz:1.0.0 → 1.1.0` при загрузке.
- Польза: TuffBox manifest (P0) получает версионирование формата и
  безопасную миграцию схемы при обновлениях IDE (нужно для changelog/diff
  снапшотов P1).
- `GetCompatibleLoaders` / `GetLoaders` (quilt→fabric, neoforge→forge
  обратная совместимость) → готовая логика для профилей и migration advisor
  (P2 Forge→NeoForge).

### 1.3 Index + хеширование + gitignore-подобный exclude → `crates/index`
Источник: `packwiz/core/index.go`

- `Index` = TOML со списком файлов и их хешами (`hash-format`, `files[]`).
- `Refresh()` рекурсивно обходит папку, уважает `.packwizignore`
  (gitignore-синтаксис, `ignoreDefaults`), пересчитывает хеши, удаляет
  отсутствующие файлы.
- Прогресс-бар (`mpb`) при хешировании — адаптировать в UI-прогресс мост
  (`bridges/`).
- Польза:
  - Lockfile (P0) = по сути index с хешами. Переиспользуем подход.
  - `.packwizignore` → аналог `.tuffboxignore` для экспорта/снапшотов.
  - `FindMod`/`LoadAllMods` → быстрый поиск мета-файлов (search P1).

### 1.4 Download cache с content-addressable хранением → `crates/download`
Источник: `packwiz/core/download.go`

- `CacheIndex` (версионированный JSON), content-addressed хранение
  `cache/<sha256[:2]>/<sha256[2:]>`, переиспользование уже скачанных файлов
  (`GetHandleFromHash`), ре-хеширование при необходимости
  (`GetHandleFromHashForce`, `rehashFile`), импорт внешних файлов
  (`MoveImportFiles`), валидация хеша при скачивании (`teeHashes`).
- Польза:
  - Кеш загрузок для Modrinth/local jar import (P0) и server pack builder (P2)
    — не качать одно и то же.
  - Проверка целостности (hash mismatch → ошибка) → надежность IDE.

### 1.5 Updaters (Modrinth/CF/GitHub) как плагины → `crates/updaters`
Источник: `packwiz/modrinth/`, `packwiz/curseforge/`, `packwiz/github/`

- Каждый источник реализует `Updaters` map: `ParseUpdate`, `GetUpdates`,
  `LatestVersion`.
- `modrinth/updater.go` — получение последней версии под нужный MC/loader.
- Польза: safe/risky updates (P1) = сравнение текущей версии с
  `LatestVersion()` апдейтера + флаг `pin`.

### 1.6 CurseForge interop (import/export) → `crates/curseforge`
Источник: `packwiz/curseforge/packinterop/`, `curseforge/{import,export,install}.go`

- `minecraftinstance.go`, `manifest.go` — парсинг форматов CurseForge
  (`minecraftinstance.json`, `manifest.json`), `murmur2/hash.go` (хеш
  совместимости с CF).
- `export.go`/`import.go` — двусторонний обмен с CF-паком.
- Польза: прямо закрывает P2 CurseForge import/export.

### 1.7 Slugify для имён модов → `crates/mod-manifest`
Источник: `packwiz/core/mod.go` `SlugifyName`

- Регулярки для приведения имени мода к slug (для файлов/ID). Полезно при
  импорте и генерации имён.

---

## 2. ServerPackCreator — что брать

### 2.1 Server Pack Builder (главная ценность, P2) → `crates/server-pack`
Источник: `serverpackcreator-api/.../api/serverpack/`, `config/`

- Генерация сервер-пака: копирование модов, удаление client-only модов
  (`clientMods` whitelist/blacklist), инклюжены (`inclusions` с source/
  destination/filters), генерация start-скриптов (`.sh`/`.bat`), встраивание
  `server.properties`/`server-icon.png` (с дефолтами), опциональный
  CurseForge-zip.
- `ConfigCheck` — раздельная валидация: config-ошибки, modpack-ошибки,
  inclusion-ошибки, script-ошибки (каждый список + `passed` булев).
- Польза: основа для P2 server pack builder + валидация конфигов (P0 basic
  config editor). Адаптировать модель `PackConfig` (Toml через nightconfig)
  в Rust-структуру манифеста сервер-пака TuffBox.

### 2.2 Mod scanning (определение loader/side/метаданных) → `crates/mod-scan`
Источник: `.../api/modscanning/` — `FabricScanner`, `QuiltScanner`,
`ForgeAnnotationScanner`, `ForgeTomlScanner`, `NeoForgeTomlScanner`,
`JsonBasedScanner`, `ModScanner` (интерфейс).

- Каждый сканер читает специфичные для загрузчика метаданные (fabric.mod.json,
  quilt, forge @Mod аннотации / mods.toml, neoforge.mods.toml).
- Польза:
  - Авто-определение side (client/server) модов → side labeling (P1) без
    ручного ввода.
  - Детекция loader-а пака → профили/миграция (P2).
  - Дополняет packwiz-модель, где side задаётся вручную.

### 2.3 Version meta (MC / Forge / NeoForge / Fabric / Quilt) → `crates/version-meta`
Источник: `.../api/versionmeta/` — `VersionMeta`, `minecraft/`, `forge/`,
`neoforge/`, `fabric/`, `quilt/`, `legacyfabric/`.

- Централизованное получение допустимых версий MC, загрузчиков и их
  совместимости (из официальных/manifest-источников), кеширование.
- Польза:
  - Валидация `minecraftVersion`/`modLoaderVersion` в манифесте (P0/P1).
  - Подсказки версий в UI, детекция несовместимых комбинаций (conflict
    detection P0).
  - Migration advisor Forge→NeoForge (P2) опирается на эти данные.

### 2.4 Плагинная система (ServerPackCreator plugins) → `crates/plugins`
Источник: `.../api/plugins/` — `Plugin`, `Extension*`, `ServerPack*`,
`BasePlugin`, `Custom*.kt`, `serverpackcreator-plugin-example`.

- Расширения точек жизненного цикла (pre/post config check, pre/post server
  pack generation) через SPI.
- Польза: точки расширения для KubeJS/CraftTweaker snippets (P2) и
  пользовательских скриптов без правки ядра IDE.

### 2.5 Launcher manifests (Prism/MMC/ATLauncher/GDLauncher/CurseForge)
Источник: `misc/launcher-manifests/`

- Готовые шаблоны манифестов для экспорта в разные лаунчеры.
- Польза: прямо закрывает P1 export Prism instance (+ другие лаунчеры
  бесплатно).

### 2.6 Utilities (архивация, копирование, строки) → `crates/util`
Источник: `.../api/utilities/` (много `common/*Utilities.kt`, `Zip`-утилиты,
`StringUtilities`, `FileUtilities`).

- Утилиты упаковки zip, фильтрации файлов, конкатенации путей.
- Польза: переиспользовать логику архивации для `.mrpack` (P0) и CF-zip (P2).

---

## 3. Сводная карта: репозиторий → фича TuffBox

| Источник | Функция | Цель TuffBox | Приоритет | Статус |
|---|---|---|---|---|
| packwiz mod.go | per-mod meta + side + pin | mod-manifest, side labeling, pin | P0/P1 | ✅ side scan + `pinned()` |
| packwiz pack.go | pack-format + миграция + loaders | manifest, миграция схемы, профили | P0/P2 | ✅ уже есть в manifest.rs |
| packwiz index.go | index + хеши + ignore | lockfile, снапшоты, search | P0/P1 | ✅ уже есть (lockfile/index) |
| packwiz download.go | content-addressable cache | кеш загрузок, целостность | P0/P2 | ✅ `download_cache.rs` |
| packwiz updaters | Modrinth/CF/GitHub апдейтеры | safe/risky updates | P1 | ✅ `updater.rs` (Modrinth) |
| packwiz curseforge | CF import/export + murmur2 | CurseForge import/export | P2 | ◑ murmur2 ✅; CF import есть |
| SPC serverpack | server-pack builder + ConfigCheck | server pack builder | P2 | ✅ уже есть в exporter.rs |
| SPC modscanning | loader/side сканеры | auto side labeling, loader detect | P1/P2 | ✅ `mod_scan.rs` |
| SPC versionmeta | MC/loader версии | валидация, подсказки, миграция | P0/P1/P2 | ✅ уже есть в versions.rs |
| SPC plugins | SPI расширения | KubeJS/CT snippets точки | P2 | ☐ отложено |
| SPC launcher-manifests | шаблоны лаунчеров | Prism/export | P1 | ✅ Prism import/export есть |
| SPC utilities | zip/файлы/строки | архивация mrpack/cf | P0/P2 | ✅ уже есть |

---

## 4. Порядок реализации — ИТОГИ

Анализ показал, что бóльшая часть плана **уже реализована** в TuffBox
(manifest, lockfile, providers Modrinth/CF/local, importers Modrinth/CF/Prism,
exporter server-pack + CF manifest + Prism, version meta, slugify). До-реализованы
только подлинно отсутствующие куски (новые модули в `tuffbox-core`):

1. **`mod_scan.rs`** (P1, из SPC modscanning + packwiz side): авто-детект
   side/client-server из `fabric.mod.json` / `quilt.mod.json` /
   `META-INF/mods.toml` / `META-INF/neoforge.mods.toml`. `scan_manifest_mods`
   проходит манифест и возвращает (mod id → side).
2. **`updater.rs`** (P1, из packwiz updaters): `check_mod_update` /
   `check_manifest_updates` / `classify_update` (Safe vs Risky), поддержка
   `pin` через `ModSpec::pinned()` (токен `"pinned"` в `status`).
3. **`download_cache.rs`** (P0/P2, из packwiz download.go): content-addressable
   кеш `.tuffbox/cache/<aa>/<bb...>` по SHA-256, валидация хеша, импорт,
   `prune`, reuse.
4. **`murmur2.rs`** (P2, из packwiz curseforge/murmur2): хеш для CF-фингерпринтов.

Не реализовано (отложено): плагинная SPI для KubeJS/CraftTweaker (п. 2.4) —
требует архитектурного решения по точкам расширения вне ядра.

> Замечание: `tuffbox-cli` не собирается из-за несвязанного предсуществующего
> несоответствия сигнатуры `TestLauncher::build_command` (вне зоны этого
> изменения). `tuffbox-core` и все тесты проходят.

---

## 5. Риски и замечания

- **Язык**: packwiz — Go, SPC — Kotlin/Java. Переносим НЕ кодом, а паттернами
  и моделями данных; адаптируем под Rust-крейты TuffBox.
- **Лицензии**: packwiz — MIT, SPC — LGPL-2.1. При заимствовании логики
  учитывать совместимость с лицензией TuffBox (проверить в README).
- **Объём**: server pack builder и mod scanning — самые объёмные; выносим в
  отдельные крейты и делаем заглушки интерфейсов до полной реализации.
