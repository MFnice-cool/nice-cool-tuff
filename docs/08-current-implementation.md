# 08. Текущая реализация

## Что создано

Начата Stage 1–3 разработка.

**TuffSwarm (федеративная сеть):** не реализована. Foundation — локальный ActionPlan, Crash KB / authored export и opt-in remote KB client. Архитектура сети: [`13-tuffswarm-network.md`](13-tuffswarm-network.md).

**GitHub Pack Transport:** `crates/tuffbox-core/src/github_pack/` — schema v2 transport envelope (signed metadata + manifest), Release asset materialization with hash/size checks, atomic stage-then-promote install, pinned-signer updates (no unsigned downgrade), and commit-SHA-pinned apply with snapshot rollback. Desktop commands + UI: `github_pack_commands.rs`, `GithubPackTransport.svelte`, `GithubPackUpdateBanner.svelte`. Контракт: [`15-github-pack-transport.md`](15-github-pack-transport.md).

Добавлен Rust workspace:

```text
Cargo.toml
crates/
  tuffbox-core/
  tuffbox-cli/
```

## tuffbox-core

Реализованы базовые модули:

```text
manifest.rs      # ProjectManifest, mods, profiles, loader, source metadata
graph.rs         # DependencyGraph, GraphNode, GraphEdge
diagnostics.rs   # Diagnostic model
crash.rs         # Crash parser / Diagnose 2.0 models and heuristics
change_plan.rs   # ChangePlan and ChangeAction
lockfile.rs      # initial lockfile generator
resolver.rs      # deterministic resolver skeleton
```

## Что уже умеет core

- Загружать `sample-project.tuffbox.json`.
- Строить граф зависимостей из manifest.
- Создавать узлы:
  - Minecraft version;
  - loader;
  - Java;
  - profiles;
  - mods.
- Создавать рёбра:
  - requires;
  - optional;
  - conflicts;
  - breaks_with;
  - replaces;
  - requires_loader;
  - requires_minecraft;
  - requires_java;
  - included_in_profile.
- Находить diagnostics:
  - missing dependency;
  - direct conflicts;
  - duplicate mod nodes;
  - wrong side in profile;
  - unknown side;
  - profile edge pointing to unknown mod.
- Создавать простой fix plan для:
  - missing dependency;
  - direct conflict;
  - crash suspects из `crash-reports/*.txt` и `logs/latest.log`.
- Анализировать краши в Diagnose 2.0:
  - находить `crash-reports/*.txt`;
  - читать tail `logs/latest.log`;
  - извлекать parser signals по `Mod File`, `Caused by`, `Mixin`, `Exception`, `Suspected Mods`, OpenGL debug, performance stalls и resource warnings;
  - маппить suspected mods на manifest по id/name/fileName/projectId;
  - формировать deterministic fix plan без авто-применения.
- Генерировать lockfile из manifest + graph с:
  - dependency edges;
  - source metadata (project_id, file_id, url, path);
  - file hashes;
  - generated timestamp.
- Создавать, списывать, сравнивать и откатывать snapshots.
- Работать с провайдерами контента:
  - `ContentProvider` trait;
  - `ModrinthProvider`: search, project info, versions, hashes, download URL, dependencies;
  - `LocalJarProvider`: sha1/sha512, file info.
- Базовый Test Launcher:
  - поиск Java (JAVA_HOME / PATH);
  - парсинг major-версии Java;
  - подготовка instance directory (mods, configs);
  - реальный запуск профиля с установкой клиента/loader и записью логов.

## tuffbox-desktop

Реализован Tauri + Svelte desktop shell (Stage 7):

```text
apps/tuffbox-desktop/
  src/           # Svelte frontend
  src-tauri/     # Rust backend
```

- Dark theme в стиле Modrinth/Lunar: `#0b0b0d` фон, `#1bd96a` зелёный акцент, карточки с большим border-radius, стеклянный header.
- Компактная sidebar с крупными иконками Lucide (как у Modrinth/Lunar).
- Главный экран (Home):
  - большая кнопка Play для выбранной сборки;
  - плитки последних сборок (хранятся в localStorage, не копируются в IDE);
  - плитка "Add instance" для открытия `.tuffbox.json`, `.mrpack` или Prism `.zip`.
- Настройки сборки (Project Settings):
  - Minecraft version, loader и его версия — select'ы с официальным Mojang API и мета-API loader'ов;
  - Java executable с иконкой лупы: при нажатии открывается окно со списком найденных Java (PATH, JAVA_HOME, реестр Windows, типичные папки), можно выбрать из списка или указать путь вручную; рядом отображается версия Java;
  - ползунок памяти с метками (1 GB – 16 GB);
  - JVM arguments;
  - сохранение изменений в manifest.
- Окно лога запуска (Launch Log): открывается при нажатии Play, показывает последние строки `logs/latest.log` и обновляется каждую секунду.
- Асинхронный запуск Minecraft: процесс стартует в фоновом потоке, UI не блокируется.
- Mods: установленный список отображается длинными компактными карточками, Add Modrinth/CurseForge использует плитки с иконками, version/side/source-тегами и корректным Installed detection по slug/projectId. Иконки установленных Modrinth-модов догружаются lazy-командой `get_modrinth_project_icon`. Перед установкой показывается install plan с dependencies; bulk install + deps для MR и CF через один snapshot. **Import…** (`import_local_content_files`) копирует local jars/zips и регистрирует через sync; **Resync** сканирует content folders. Wrong-loader и duplicate-jar панели; missing deps → Resolve stage. Trail History/Resolve/Snapshots после мутаций. Перед изменением manifest создаётся auto snapshot (+ pack events).
- Graph: вместо сырого JSON добавлен визуальный обзор графа — runtime/profile/mod-ноды, счетчики, карточка выбранного узла, прямые связи и панель missing dependencies.
- IDE Workflow: добавлен DaVinci Resolve-like production flow: Brief → Setup → Content → Resolve → Tune → History → Test → Diagnose → Snapshots → Export → Release. Вкладки этапов перенесены в нижний sticky rail, рабочая область занимает около 76% высоты экрана, лишние hero/output panels удалены. Brief сохраняется в manifest, Test запускает реальные profiles, пишет историю запусков в `.tuffbox/test-runs.json` и показывает `latest.log`, Export собирает `.mrpack`, server pack, Prism zip и CurseForge zip, Release делает version bump, validation, artifact checklist, changelog и release snapshot.
- Config Editor: добавлена вкладка для просмотра и редактирования файлов `config/`, `defaultconfigs/`, `kubejs/` и `scripts/` с whitelist расширений, ограничением размера и auto snapshot перед сохранением.
- Change History: новая IDE-вкладка показывает историю изменений timeline-деревом по категориям Mods/Configs/Shaders/Resource Packs/World/Data/Other, поддерживает сворачиваемые preview, persistent tracked-folder чекбоксы, регистрацию выбранных папок snapshot'ом и rollback отдельного tracked file.
- Schema migrations: core умеет нормализовать manifest/lockfile schema `0.1`/`0.1.0` к текущей `0.1.0`, а desktop backend получил команды статуса и миграции manifest.
- Snapshots: UI получил rollback, compare panel и inline text diff для tracked changed files; diff теперь сравнивает содержимое файлов, а не только списки путей. Master–detail layout показывает `actionsSummary` (человекочитаемые действия), tags/`planSource`, changed files, related pack events; `get_snapshot_detail` синтезирует legacy-описания из `plan.json` / resolutions / crash marker / events. Auto-snapshot пишет `operation` + `actionsSummary` + `actor` в `snapshot.json`; crash_fix также сохраняет `plan.json` в каталоге снапшота. Delete snapshot, фильтры Auto/Manual/Crash, Jump to History.
- Diagnose 2.0: Diagnostics page расширена до crash parser workspace — список crash reports, открытие выбранного отчёта, tail `latest.log` и `launcher.log`, grouped parser signals (Entrypoint/Loader mismatch/Render/Performance), crash report sections (`-- Head --`, `-- Mods --`, `-- System Details --`) с preview и parsed Mods section, suspected mods panel, последние snapshots/изменения рядом и кнопка **Create fix plan**.
- Diagnostics/Settings: переоформлены в едином стиле.
- Поддержка импорта:
  - `.mrpack` — парсинг `modrinth.index.json`, создание `tuffbox.json` в выбранной папке;
  - Prism instance `.zip` — парсинг `instance.cfg`, создание `tuffbox.json`;
  - папка Minecraft instance — автоопределение Fabric/Forge/Neoforge и версии Minecraft по модам в `mods/`.
- Главный экран:
  - кнопка Play отображает под надписью **Play** название сборки, версию Minecraft и модлоадер;
  - отдельная боковая кнопка **Open IDE** открывает production workflow по этапам, чтобы лаунчер не смешивался с рабочей IDE;
  - меню быстрых действий (троеточие) на плитке сборки: Change Version, Create Desktop Shortcut, Download Server Pack, Links, Open Folder, Create logs.zip, Copy Modpack Link, Profile Options, Clone as..., Share Profile, Repair Profile, Remove from launcher, Delete Profile.
- Модальное окно **Add Instance**: имя, выбор версии Minecraft (популярные версии сверху, затем релизы по убыванию), выбор loader (Vanilla/Fabric/Forge/NeoForge/Quilt) и его версии (по умолчанию последняя stable), папка для сохранения.
- Test Runs: вкладка Test ведёт историю запусков в `.tuffbox/test-runs.json`, показывает status/duration и умеет сохранять логи конкретного run в `.tuffbox/test-runs/<run-id>/`.
- Реальный запуск Minecraft:
  - скачивание client jar, библиотек, natives и assets по манифесту Mojang;
  - загрузка профиля Fabric/Quilt из мета-API, корректный разбор `mainClass`, проверка sha1 loader-библиотек и sequential retry/fallback для нестабильных загрузок с Fabric Maven;
  - формирование classpath через системный path separator и корректная подстановка `${library_directory}` в JVM arguments;
  - запуск Java-процесса в фоновом потоке (`spawn_and_track`), UI не блокируется;
  - использование выбранной в настройках Java или автоопределение;
  - лог пишется в `logs/latest.log`.
- Tauri commands:
  - `validate_project` — открыть и валидировать project manifest;
  - `list_mods` — список модов;
  - `get_project_brief` / `update_project_brief` — сохранение pre-production brief в manifest с auto snapshot;
  - `list_profiles` / `list_test_runs` / `capture_test_run_logs` — профили Test page, история запусков и capture логов по run;
  - `search_modrinth_mods` / `get_modrinth_project_icon` — поиск Modrinth с фильтрами текущих Minecraft/loader и lazy-загрузка иконок;
  - `add_modrinth_mod` / `remove_project_mod` / `update_project_mod` — безопасное управление модами из UI с auto snapshot;
  - `list_config_files` / `read_config_file` / `write_config_file` — безопасный Config Editor для текстовых конфигов проекта;
  - `get_project_schema_status` / `migrate_project_schema` — проверка и миграция schemaVersion manifest с auto snapshot;
  - `get_graph` — граф зависимостей;
  - `get_diagnostics` — диагностики;
  - `get_crash_diagnosis` / `create_crash_fix_plan` — Diagnose 2.0: crash reports, latest.log, suspected mods, recent snapshots и план исправления;
  - `list_snapshots` / `create_snapshot` / `diff_snapshots` / `get_snapshot_file_diff` / `get_snapshot_detail` / `delete_snapshot` / `rollback_snapshot` — управление snapshots, detail с actionsSummary, rollback и inline сравнение tracked changed files;
  - `validate_modrinth_export` / `generate_release_changelog` / `update_project_version` / `create_release_snapshot` / `list_release_artifacts` / `create_release_draft` — release workflow, artifact registry и draft metadata;
  - `export_modrinth_pack` — базовый экспорт `.mrpack` с remote mod downloads и overrides;
  - `export_server_pack` — базовый server pack zip: server-safe mods, configs/scripts, download manifest, README и start scripts;
  - `export_prism_instance` / `export_curseforge_pack` — базовые Prism/CurseForge zip builders с metadata, overrides и `tuffbox.remote-mods.json`;
  - `generate_lockfile` — генерация lockfile;
  - `launch_profile` — подготовка и запуск профиля;
  - `import_project` — импорт `.mrpack` / Prism `.zip`.
- Запуск: `npm run tauri:dev` из `apps/tuffbox-desktop`.

## tuffbox-cli

Добавлен CLI/dev harness:

```bash
tuffbox project validate <manifest>
tuffbox project lock <manifest>
tuffbox project add-mod <manifest> <mod_id> [--side client|server|both]
tuffbox project remove-mod <manifest> <mod_id>
tuffbox project update-mod <manifest> <mod_id>
tuffbox graph print <manifest>
tuffbox graph diagnostics <manifest>
tuffbox resolve <manifest>
tuffbox snapshot create <project_dir> --name <name> [--reason <reason>]
tuffbox snapshot list <project_dir>
tuffbox snapshot diff <project_dir> <from> <to>
tuffbox snapshot rollback <project_dir> <id>
tuffbox modrinth search <query> [--mc <version>] [--loader <loader>]
tuffbox modrinth versions <project_id> [--mc <version>] [--loader <loader>]
tuffbox launch <manifest> [--profile <profile_id>]
```

Операции `add-mod`, `remove-mod` и `update-mod` автоматически создают snapshot перед изменениями.

## Статус сборки

Rust toolchain и Node.js подключены. Проект успешно собирается и проходит тесты:

```bash
cargo fmt
cargo test
cargo check -p tuffbox-desktop
npm run tauri:dev   # из apps/tuffbox-desktop
```

## Следующие задачи

1. Улучшить Crash parser: Forge/NeoForge sections, Fabric loader table, Quilt reports, deobfuscated stacktrace hints.
2. Подключить schema status/migration controls в Project Settings UI и расширить миграции под будущие версии.
3. Расширить Snapshot diff на manifest/lockfile и добавить side-by-side режим.
4. Улучшить Config Editor: подсветка синтаксиса/форматирование JSON/TOML и поиск по содержимому.
5. Добавить мини-карту графа и группировку по профилям поверх нового pan/zoom viewport.
6. Добавить change plan preview перед add/update/remove модов в UI.
7. Test Launcher: расширить установку Forge/NeoForge и улучшить захват логов/статуса процесса.

## Аудит и исправления (2026-07-05)

Проведён сквозной аудит кодовой базы (Rust workspace + Svelte/Tauri frontend) после первичного клонирования. Найденные и исправленные баги:

- **Моды не скачивались физически.** `add_mod_from_modrinth`/`update_mod_from_modrinth`/bulk-install только писали метаданные в manifest — `.jar` никогда не попадал в `mods/`, поэтому Minecraft запускался без модов. Добавлен модуль `tuffbox-core/src/mod_files.rs` (`materialize_mod_file`, `ensure_project_mods_downloaded`) и подключён во все точки изменения манифеста (desktop commands, CLI, resolve/fix-plan actions) и как safety-net перед каждым test launch.
- **Локальные jar в `mods/` не сверялись с Modrinth.** `sync_mods_folder` помечал любой неизвестный `.jar` как непрозрачный `local`-мод. Добавлен `ModrinthProvider::get_version_by_hash` (`/v2/version_file/{sha1}`) и `identify_local_jar_via_modrinth`, которые распознают дроп-ин моды по содержимому файла и заполняют полноценные Modrinth-метаданные (версия, зависимости, апдейты).
- **Сборка не собиралась на Linux/macOS.** `winreg` был безусловной зависимостью `tuffbox-core`, хотя сам крейт компилируется только на Windows. Вынесен в `[target.'cfg(windows)'.dependencies]`.
- **`unified::recipe` не компилировался.** 7 из 10 парсеров рецептов (`Shapeless`, `Cooking`, `Smithing*`, `*ConditionalParser`) были объявлены, но не имели `impl RecipeParser`. Реализованы все парсеры и вынесен общий диспетчер `parser_for_type`, устранивший дублирование версии/loader-специфичной логики выбора парсера между `forge.rs`/`fabric.rs`/`neoforge.rs`.
- **Move-ошибки после введения `LoaderKind`.** `LoaderKind` не было `Copy`, что ломало `environment.rs`/`registry.rs`. Добавлены `Copy, PartialEq, Eq, Hash`.
- **Приватный ре-экспорт `LoaderKind`** из `environment.rs` ломал компиляцию адаптеров — заменён на `pub use`.
- **Снапшоты и lockfile использовали захардкоженную фейковую дату** (`2026-06-29T00:00:00Z`) вместо реального времени — все снапшоты имели одинаковый `created_at`, а два снапшота с одним именем в один "день" перезаписывали друг друга по ID. Добавлен `time_util.rs` с корректной конвертацией `SystemTime → RFC3339 UTC` (алгоритм `civil_from_days`), протестирован на известных эпохальных значениях, включая високосный год.
- **Оффлайн-запуск всегда использовал фиксированное имя `"Player"` и нулевой UUID.** Добавлено поле `ProfileSpec.player_name` + вычисление детерминированного offline UUID (`MD5("OfflinePlayer:{name}")`, тот же алгоритм, что у ванильного лаунчера) — проверено на эталонном значении для `"Notch"`. Поле добавлено в Project Settings UI.
- **`Mods.svelte` не компилировался.** Дублирующееся объявление `let contentFilter` с несовместимыми типами ломало сборку всего фронтенда (`vite build` падал). Удалён дубликат.
- **Потенциальное зависание/потеря логов Test Launcher.** `reader.lines().flatten()` на stdout/stderr процесса Minecraft тихо обрывает поток при первом non-UTF8 байте. Заменено на побайтовое чтение с `String::from_utf8_lossy` (`process::read_lines_lossy`), также используется в `read_log_tail`.
- **Граф зависимостей не был "как в Obsidian".** У `Graph.svelte` не было pan/zoom — фиксированный `viewBox`, узлы могли перекрываться. Добавлены: масштабирование колёсиком к курсору, перетаскивание фона, кнопки zoom in/out/fit, collision-force между узлами, затемнение несвязанных узлов/рёбер при выборе.
- Мелкое: UTF-8 BOM убран из 8 файлов (`overrides/*`, `unified/*`, `tag_normalizer.rs`, `lib.rs`), потерянные JVM multi-value аргументы (`ArgValue::Many`) в `mc_install.rs` теперь разворачиваются полностью, а не берётся только первый элемент.

Проверено end-to-end (реальная сеть, реальный Java 21): `tuffbox project add-mod ... sodium` скачивает настоящий `sodium-fabric-*.jar` в `mods/`; `tuffbox launch` полностью устанавливает vanilla/Fabric 1.21.1, запускает JVM, и Minecraft/Fabric Loader подтверждают `Loading 11 mods: - sodium ...` в логе (падение на GLFW/OpenGL ожидаемо — sandbox без дисплея).

## Второй раунд аудита: соответствие docs/10 и honesty-фиксы (2026-07-05, продолжение)

Проверка соответствия фактической реализации плану `docs/10-unified-model-and-modules.md` и остальным roadmap-документам выявила дополнительные критичные баги и заглушки, все исправлены:

- **Resourcepacks/shaderpacks/datapacks устанавливались в `mods/`.** `ModSpec` не хранил тип контента — любой Modrinth-проект (в т.ч. resourcepack/shader) материализовался в `mods/`, где loader либо игнорирует файл, либо (Forge/Fabric) пытается загрузить его как мод и падает. Добавлено поле `ModSpec.content_type: ContentType` (`Mod`/`Resourcepack`/`Shaderpack`/`Datapack`, определяется из Modrinth `project_type`), и вся цепочка материализации (`mod_files.rs`: `content_dir_for`, `materialize_mod_file`, `ensure_project_mods_downloaded`) теперь раскладывает файлы по `mods/`, `resourcepacks/`, `shaderpacks/`, `datapacks/` соответственно. Проверено end-to-end сетевым тестом — реальный `resourcepack` (Fresh Animations, 850 КБ) корректно лёг в `resourcepacks/`, не в `mods/`.
- **`.mrpack`-импорт терял resourcepacks/shaderpacks/datapacks.** `import_modrinth_pack` фильтровал только `mods/*` записи индекса — остальные типы контента внутри импортируемого модпака просто пропадали. Теперь распознаются все 4 префикса пути и им присваивается правильный `content_type`.
- **Вкладки Mods/Resourcepacks/Datapacks/Shaders в UI были декоративными.** Переключение вкладки не передавалось в поиск Modrinth — `search_modrinth_mods` всегда искал `project_type=mod`, независимо от выбранной вкладки. Добавлен параметр `project_type` в `ProviderSearchQuery`/`build_facets`, проброшен через backend-команду `content_type`, и UI использует его при переключении вкладки. Loader-фильтр также скрывается для нелоадер-контента (resourcepack/shader/datapack), для которого он не имеет смысла на Modrinth. Список установленных элементов теперь тоже фильтруется по `contentType`, а не показывает всё как моды.
- **Diagnostics: кнопка "Fix Issue" была полной подделкой.** `applyFix` не делала ни одного вызова к backend — только устанавливала текст `"Resolved conflict for ..."` и обновляла список, оставляя проект без изменений, но сообщая пользователю об успехе. Добавлена настоящая команда `apply_crash_fix_plan` (снапшот + применение реального `ChangePlan` из crash-диагностики через уже существующий `apply_change_action`), кнопка переименована в честное "Apply fix plan" и показывает, что было применено на самом деле.
- **Dashboard: 6 пунктов контекстного меню были `alert("not implemented yet")`.** Три из них были тривиально подключаемы к уже готовому backend или легко реализуемы честно: "Download Server Pack" → вызывает существующий `export_server_pack`; "Create logs.zip" → новая команда `create_logs_zip` (zip `logs/`+`crash-reports/`+`test-runs/`, core-функция `exporter::export_logs_zip`); "Repair Profile" → новая команда `repair_project` (пересинхронизация всех content-файлов через `mod_files.rs`, честно ограничена этим, без обещаний "исправить всё"); "Clone as..." → новая команда `clone_project` (копирование mods/config/overrides в новый проект). "Change Version" ведёт в Project Settings вместо пустого алерта. "Share Profile" и "Links" оставлены явно как нереализованные с честным объяснением (требуют облачный backend/публикацию — P3 по roadmap), а не притворяются рабочими.
- **Forge install: два бага, из-за которых Forge не запускался вообще.** (1) `parse_artifact_value` понимал только legacy-строковый формат `"@{path=...;url=...}"` для `downloads.artifact`, но реальный Modrinth launcher-meta API отдаёт для Forge обычный JSON-объект `{path, url, sha1, size}` — из-за этого `mcp_config` и другие библиотеки не скачивались, и install падал с сырым `No such file or directory`. (2) `processor_classpath` строил classpath процессоров Forge через захардкоженный `;` (Windows-only разделитель), что на Linux/macOS ломало весь classpath кроме первого jar → `ClassNotFoundException`. (3) Отдельно, `${classpath_separator}` в JVM-аргументах Forge (`-p <module-path>`) никогда не подставлялся, что оставляло буквальный текст плейсхолдера в module-path и роняло инициализацию `securejarhandler` с `InaccessibleObjectException`. Все три исправлены. Дополнительно: `find_java()` всегда выбирал самую новую установленную JVM без учёта того, что реально требуется — Forge 1.20.1 запускался на Java 21 вместо Java 17 и падал в module-system. Добавлены `jre::required_java_major` (границы версий по правилам Mojang) и `jre::find_runtime_for`/`TestLauncher::find_java_for_minecraft`, подключены в CLI и desktop launch path; лог теперь явно предупреждает при несовпадении версий. Проверено end-to-end реальной установкой Forge 47.2.20 на 1.20.1: все 10 install-процессоров прошли, ModLauncher стартовал на автоматически подобранной Java 17, FML дошёл до инициализации рендера (`glfwInit failed` — ожидаемо, headless sandbox).
- **Config Editor: не было форматирования JSON**, хотя это заявлено в roadmap (P1 "Format JSON/TOML"). Добавлена кнопка "Format" с pretty-print для `.json`-файлов (JSON5/TOML/CFG всё ещё не форматируются — это требует полноценного парсера с сохранением комментариев, отслеживается отдельным пунктом roadmap, не подделано).
- Мелкое: неиспользуемая декоративная кнопка "Show more" рядом с фильтром Loader (без обработчика) убрана.

Собран весь workspace (`cargo build --workspace`, `npm run build` для фронтенда) и прогнаны все Rust-тесты (35 passed) после каждого раунда правок.
