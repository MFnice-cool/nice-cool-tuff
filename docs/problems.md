# Known Problems / Известные проблемы

Рабочий список проблем, выявленных при использовании TuffBox IDE.
Обновляется по мере исправления. Формат: описание → статус → план/причина.

## 1. Долгая загрузка страницы Setup + всплывающие окна консоли
- **Симптом:** Страница/экран Setup грузится долго; при работе видны открывающиеся
  окна консоли (чёрные окна cmd).
- **Причина (предпол.):** Rust-бэкенд на Windows запускает дочерние процессы
  (`std::process::Command`) без флага `CREATE_NO_WINDOW`, из-за чего Windows
  показывает консольное окно на каждый spawn. Долгая загрузка может быть связана
  с синхронными вызовами при инициализации Setup (ожидание версий Minecraft/загрузчиков,
  проверка обновлений и т.п. в главном потоке UI).
- **Статус:** в работе (часть решена)
- **Что сделано:** флаг `CREATE_NO_WINDOW` (`0x08000000`) уже применяется ко всем
  дочерним процессам на Windows — `process.rs:65` (запуск игры/лог),
  `jre.rs:187` (java -version), `forge.rs:481` и `forge_install.rs:508`
  (forge-процессоры). Консольные окна при spawn больше не всплывают.
- **Осталось:** долгая загрузка Setup. `ProjectSettings` (вкладка setup) и
  `AddInstanceModal` грузят версии/шаблоны синхронно через `await` в `onMount`.
  Перенести в фоновую загрузку со скелетоном/индикатором (см. `AddInstanceModal.svelte:45-87`).
- **Сделано (2026-07-09):** `ProjectSettings.svelte` — `onMount` теперь грузит
  версии MC, java-превью и статус схемы параллельно через `Promise.all`, форма
  рендерится сразу (по `$projectPath`, а не `$projectInfo`), во время загрузки
  показывается индикатор `Loading instance settings…` и форма `.dimmed`.
  `AddInstanceModal.svelte` — `onMount` грузит версии MC и дефолтную папку
  параллельно (`Promise.all`), затем версии загрузчика. `svelte-check`: 0 errors.
- **Статус:** исправлено (асинхронность Setup/AddInstance реализована).

## 2. Вкладка Content не грузит моды
- **Симптом:** Вкладка Content пустая — моды не подгружаются вообще. Должна
  загружать список модов при её открытии.
- **Детали:** Иконки модов показывают плейсхолдеры (картинка-заглушка), будто
  изображения не загрузились. Кнопка "Add Mods" работает (там баг загрузки модов
  обойдён локально), но сама вкладка при открытии ничего не делает.
- **UI:** вкладка выглядит ужасно (плохая вёрстка).
- **Референс:** см. реализацию загрузки модов в `U:\TuffBox\AstralRinth-AR-0.10.2701\astralrinth`.
- **Статус:** исправлено (2026-07-10) — обновления модов, update-all, UI Content,
  окно прогресса скачивания с реальными шкалами.
- **Что сделано:** `Mods.svelte` грузит `list_mods` при активации вкладки
  (реактивный блок `Mods.svelte:522` + `load()` в `:522`), есть фильтры по
  контенту (mod/resourcepack/datapack/shader), кнопка Add Mods, sync, update,
  recommendations, version picker. Иконки берутся из `iconUrl`
  (`list_mods` строит `https://cdn.modrinth.com/data/{pid}/icon.png`, lib.rs:343)
  с корректным fallback-плейсхолдером (`Mods.svelte:483-485`). CSP в tauri = null,
  так что CDN-иконки грузятся.
- **Сделано (2026-07-09):** в `Mods.svelte` после `list_mods` запускается
  `hydrateMissingIcons()` — для модов с `projectId`, у которых `iconUrl` пуст
  (локальные jar'ы с известным Modrinth-id, или сбой CDN), иконка
  докачивается через `get_modrinth_project_icon` (lib.rs:441) параллельно
  (`Promise.all`), при ошибке остаётся letter-avatar fallback. `svelte-check`:
  0 errors.
- **Сделано (2026-07-10):**
  1. Update / Update all: применяют `versionId` из batch-check, удаляют старый
     jar при смене имени файла, не валят весь batch из‑за одного failed
     `get_project`.
  2. `download_project_mods_tracked` пишет реальный byte-progress в
     `DOWNLOAD_PROGRESS` и эмитит `mod-download-progress` /
     `mod-download-batch`.
  3. Content UI: hero, update panel, карточки с badge/анимацией; модалка
     прогресса со шкалами на каждый мод.
  4. `svelte-check`: 0 errors; `cargo check -p tuffbox-desktop`: ok.
- **Сделано (2026-07-10, Windows update transaction):** streaming download
  теперь заменяет одноимённый jar через platform-aware atomic persist, а
  single-mod update скачивает и проверяет файл до записи manifest. При ошибке
  восстанавливается предыдущий jar; учитывается суффикс `.disabled`, UI больше
  не скрывает `download.failed`.

## 3. Вкладка Recipes — непонятный парсинг, знаки вопроса
- **Симптом:** Не ясно, как парсится содержимое; весь текст покрыт знаками
  вопроса (����).
- **Причина (предпол.):** неверная кодировка при чтении/выводе (UTF-8 vs системная
  кодировка, особенно на Windows — cp1251), либо парсер выводит нечитаемые данные.
- **Статус:** исправлено (полноценный JEI UI, 2026-07-10)
- **Что сделано:**
  1. `recipe_layout.rs` — раскладка 3×3 по алгоритму JEI
     (`CraftingGridHelper.getCraftingIndex`), печь, кузня (горизонтально),
     камнерез; циклическая смена `one_of`/тегов.
  2. `recipe_scan.rs` — JAR + `datapacks/` + world datapacks + `kubejs/data`,
     лимит 8000, `RecipeScanResult` со статистикой; генерация/запись KubeJS
     remove-скриптов в `kubejs/server_scripts/tuffbox_recipe_removes.js`.
  3. `RecipeBrowser.svelte` — layout как JEI: category rail, MC-style панель
     крафта, ingredient list с пагинацией, bookmarks, history, поиск
     `@mod #tag &id $ $ -exclude`, R/U/B/←→/Backspace, Queue remove → disk.
  4. Forge/Fabric/NeoForge: пути `recipes` и `recipe`.
- **Live JEI (2026-07-10):** добавлен companion plugin для Fabric/NeoForge
  1.21.1. При запущенном клиенте Recipes получает реальные runtime categories,
  crafting stations, slot layouts, alternatives и локализованные имена через
  token-authenticated localhost bridge. После выхода UI автоматически
  возвращается к offline snapshot.

## 3b. Quest Editor — заглушка без сохранения
- **Симптом:** Quest editor парсил только заголовки глав, квесты не загружались,
  сохранение «coming soon».
- **Статус:** исправлено (2026-07-10)
- **Что сделано:** `quest_book.rs` — SNBT parser/serializer, `load_quest_book` /
  `save_quest_chapter` / `validate_quest_book`. `QuestEditor.svelte` — загрузка
  квестов, карта зависимостей, валидация, сохранение в SNBT с auto-snapshot.

## 4. Вкладка World — карта и инструменты (MCA Selector)

- **Симптом (исторический):** UI казался без карты — IDE stage «World / Ores»
  показывал только ore-chart; MCA-карта была спрятана ниже или в sidebar Ctrl+8.
- **Статус:** исправлено (2026-07-29)
- **Что сделано:**
  1. IDE stage **World / Map** (`world-map`) → полный [`World.svelte`](apps/tuffbox-desktop/src/components/World.svelte)
     с [`WorldMap`](apps/tuffbox-desktop/src/components/WorldMap.svelte) dock (как MCA Selector):
     select / delete / export-import / NBT / filters, spawn overlay, polygon.
  2. IDE stage **Ores / Heights** (`ore-gen`) → только график руды; ссылка на World map.
  3. Sidebar **World (Ctrl+8)** — тот же MCA map UI.
  4. Бэкап мира перед destructive ops остаётся в World header; delete подтверждается диалогом.
- **Бэкенд:** `region.rs` / `region_edit.rs` + `api.worlds.*` (anvil .mca).

## 5. Вкладка Resolve полностью сломана
- **Симптом:** нельзя взаимодействовать с графом (не кликабелен/не рендерится),
  надписи в карточках выезжают за пределы, неудобный UI.
- **Статус:** исправлено (быстрый offline/cache render + background refresh)
- **Что сделано:** `Graph.svelte` полноценно использует `d3-force` для
  раскладки, перетаскивание узлов (`handleNodeMouseDown`, Graph.svelte:360),
  пан/зум (`zoomBy`/`resetView`), клик по узлу выделяет, auto-install missing
  deps, change-plan. Карточки узлов ограничивают текст: `.node-label` и
  вложенный `.node-meta` имеют `overflow:hidden; text-overflow:ellipsis`
  (Graph.svelte:934-946).
- **Осталось:** проверить на больших графах (сотни узлов) — `forceCollide`
  радиус 46 и `distance(150)` могут давать налезающие карточки; при
  необходимости увеличить collide-радиус или уменьшить размер карточки в
  `.compact`-режиме.
- **Сделано (2026-07-09):** в `Graph.svelte` увеличен `forceCollide` радиус
  46→70 и link `distance` 150→170 (Graph.svelte:348-349) для лучшего разрежения
  узлов на больших графах; подписи в SVG-узлах обрезаются до 22 символов с
  многоточием (Graph.svelte:499), чтобы длинные label не налезали друг на
  друга. `svelte-check`: 0 errors.
- **Сделано (2026-07-10):** `get_graph` больше не выполняет синхронный N+1
  Modrinth-enrich. Сначала возвращается локальный или валидный cached graph,
  сеть обновляет `.tuffbox/cache/dependency-graph.json` в фоне. Resolve plan и
  diagnostics используют тот же snapshot. Missing dependency теперь
  полноценный core-узел, поэтому все edge endpoints существуют и d3 layout не
  падает.

## 6. Вкладка History — надписи не помещаются в шкалу изменений
- **Симптом:** текст описания изменений выходит за границы таймлайна/шкалы.
- **Статус:** исправлено
- **Что сделано:** в таймлайне `ChangeHistory.svelte` полоса `.file-title`
  имеет `overflow:hidden; text-overflow:ellipsis; white-space:nowrap`
  (ChangeHistory.svelte:385), колонка дерева `minmax(0,1fr)` (Graph сетка
  `.file-strip` grid-template-columns: `18px minmax(0,1fr)`), так что длинные
  пути файлов не вылезают за карточку. `preview-header p` (дата/причина)
  — muted, переносится. Команды `list_project_change_history` /
  `read_project_history_file` работают.
- **Примечание:** если ранее текст выезжал — это было до добавления `minmax(0,1fr)`
  в grid-колонки. Сейчас ограничен.

## 7. Export не работает полноценно
- **Симптом:** экспорт (mrpack / server pack / prism / curseforge / release)
  работает не полностью.
- **Статус:** исправлено (покрыто smoke-тестами; рантайм-проверка в GUI опциональна)
- **Что сделано:** все экспорт-команды присутствуют и бэкенд проходит
  `cargo check` без ошибок. Добавлены unit-тесты в `exporter.rs`
  (`export_modrinth_pack_smoke`, `export_server_pack_skips_client_mods`) —
  проверяют, что упаковка не падает и корректно отбирает моды по side
  (server-pack пропускает client-side моды). Тесты проходят (`cargo test
  --lib exporter` → 2 passed). Функции экспорта возвращают `Result`
  (обработка ошибок, без паник), UI — `ExportBuilder.svelte` + `ReleaseRoom.svelte`.
- **Осталось (опционально):** прогнать каждый формат на реальном проекте в GUI
  для финальной проверки специфичных веток (curseforge/prism/ mrpack с
  overrides). Базовая логика подтверждена тестами.

## 8. Обновления модов / смена версии (Content)
- **Симптом:** Update all и точечное обновление пропускали моды без sha1 в
  манифесте; отдельная панель «updates ready» дублировала кнопки на карточках;
  version picker показывал только server-filtered список без канала/поиска.
- **Статус:** исправлено (2026-07-14)
- **Что сделано:**
  1. `check_mod_updates` / `update_all_mods` — общий `resolve_pending_mod_updates`:
     sha1 из манифеста → hash jar на диске → fallback `project/{id}/version`
     под текущие MC+loader (Quilt также пробует Fabric). Сравнение по
     `file_id`/hash файла, не по строке version_number.
  2. Убрана секция update-panel; в тулбаре **Update all**, на карточке —
     иконка Update + Change version; бейджи/dots остаются.
  3. `get_mod_versions` грузит все версии, помечает `compatible` /
     `versionType`; UI: поиск, hide incompatible, changelog, confirm switch.
  4. `validate_project.loaderKind` → `loader_kind_slug`; soft MC-check при
     refresh metadata (явный switch на другую MC-версию после confirm).
- **Референс:** Prism `ModrinthCheckUpdate`, Modrinth App ContentUpdaterModal,
  modrinth-extras version+loader filter.

## 9. CurseForge + установка модпаков + Retry
- **Симптом:** не было CurseForge API; импорт CF zip ставил SourceKind::Modrinth
  и website URL; UI не умел ставить модпаки; download dialog без Retry.
- **Статус:** исправлено (2026-07-14)
- **Что сделано:**
  1. `provider/curseforge.rs` — Flame API (`x-api-key`, search modpacks,
     `/mods/files`, Modrinth SHA1 fallback для blocked downloads).
  2. Исправлен импорт CF: `projectID`/`fileID`, resolve URLs, overrides,
     stash `curseforge/manifest.json`.
  3. `install_modpack` + вкладки Add Instance: Blank / Import / CurseForge.
  4. Download dialog: **Retry** на failed row + **Retry failed (N)**.
- **Референс:** Prism FlameCreationTask / FileResolvingTask / NetworkJobFailedDialog.

---
## 10. MSA-сессия протухает раз в сутки без гарантированного продления
- **Симптом:** Microsoft-токен живёт ~24 часа; `expires_at` ставился один раз
  при логине и не сдвигался при тихом обновлении в `mc_get_auth_status`,
  а launch-путь вообще не проверял протухание — первый запуск дня мог
  упасть на невалидном токене (онлайн-серверы отклоняют сессию).
- **Статус:** исправлено (2026-09-16).
- **Что сделано:**
  1. `mc_get_auth_status` теперь сдвигает `expires_at` после успешного тихого
     обновления токенов (auth.rs).
  2. Выделен общий `refresh_active_microsoft_session()` (обновляет токены в
     keyring, сдвигает expiry, сохраняет состояние); `mc_refresh_profile`
     переведён на него.
  3. Новая команда `mc_refresh_token` — принудительное продление без
     30-секундного троттлинга статуса; зарегистрирована в invoke_handler.
  4. Launch-путь (`launch.ts`): при `expiresAt` ближе 10 минут — тихое
     продление до запуска игры; при неудаче — тост «Sign in / Launch anyway»
     с 60-секундным бэкоффом (повтор не долбит OAuth). Оффлайн/Yggdrasil
     (expiry null) не трогаются.
  5. `App.svelte`: при старте гидратирует сессию и проактивно продлевает
     протухший токен — первый Play дня больше не падает.
  6. Тесты: `needsTokenRefresh` (маржа/граница/null) в launchState.test.ts.

## 11. Java 8/16 не скачивалась; старые версии запускались на новом Java
- **Симптом:** MC ≤1.16.4 требует Java 8, 1.17 — Java 16, но managed-загрузка
  умела только GraalVM CE (у которой нет релизов jdk-8/jdk-16) — установка
  падала ошибкой. Хуже: наличие более нового Java считалось совместимым, и
  старый Forge ≤1.16 запускался на Java 17/21 с классическими крашами
  bootstrap-загрузчика.
- **Статус:** исправлено (2026-09-16).
- **Что сделано (jre.rs):**
  1. `managed_jdk_needed`: legacy-мажоры (≤16) требуют ТОЧНОГО совпадения
     мажора — новый Java больше не подменяет старый; современные мажоры (17/21)
     как раньше принимают более новый рантайм.
  2. Маршрутизация `install_managed_jdk_major`: 8/16 → Adoptium Temurin
     (API `/v3/assets/latest/{major}/hotspot`, sha256-проверка, zip/tar.gz),
     17/21 → GraalVM CE как раньше; переиспользование через `.tuffbox-java-ok`.
  3. Тесты: exact-match для legacy, >= для современных, маршрутизация,
     URL-билдер и имена платформ Adoptium.
- **Эффект:** паки 1.7.10–1.16.5 (старый Forge) получают корректный Temurin 8,
  1.17 — Temurin 16, данные Forge-профилей берутся с launcher-meta Modrinth
  (покрытие до 1.10).

## 12. Простой лаунчера дорогой: ~14% ЦП и 200 МБ ОЗУ на главной в фоне
- **Симптом (замер пользователя, Ryzen 7 5700X3D):** запущенный лаунчер на
  главной странице в фоне занимает 14.2% ЦП (внутри процесса WebView2
  dispatcher — 9%) и ~200 МБ ОЗУ.
- **Причины (по коду):**
  1. Два полноэкранных ambient-пятна `.glow-a/.glow-b` (42vw, `filter:
     blur(90px)`) с бесконечной анимацией transform — каждый кадр дорогая
     растеризация гигантского размытия (главный источник работы dispatcher).
  2. `TaskProgressPanel` опрашивал `list_background_tasks` каждые **800 мс
     бесконечно**, даже когда панель пуста и окно скрыто.
  3. `Header` ходил в Supabase (`get_launcher_online`) каждые 15 с без
     проверки видимости окна.
  4. `PackHealthBadge` сканировал проект (`get_pack_health`) каждые 60 с —
     комментарий «while visible» не выполнялся.
  5. `startRunningInstancesWatch` продолжал опрос (12 с idle) у скрытого
     окна; visibilitychange-listener не снимался при очистке.
- **Статус:** исправлено (2026-09-16).
- **Что сделано:**
  1. `.glow`: предварительное «размытие» radial-gradient вместо рантайм
     `blur(90px)` — визуально то же, растеризация больше не нужна.
  2. Новая политика простоя `src/lib/idlePerf.ts`: `body.app-hidden`
     ставится при `document.visibilityState !== "visible"` (WebView2
     отдаёт hidden при минимизации И полном перекрытии окна), а styles.css
     глушит `animation-play-state: paused` для ВСЕХ анимаций этого класса —
     фоновой лаунчер перестаёт композитить кадры целиком.
  3. `TaskProgressPanel`: адаптивный опрос (2 с пока есть видимые задачи /
     15 с когда пусто), скрытое окно пропускает IPC, идентичные ответы не
     реассайнят стейт (нет лишнего ре-рендера).
  4. `Header`: 30 с + пропуск в фоне (нет Supabase-трафика у свёрнутого окна).
  5. `PackHealthBadge`: скрытое окно пропускает 60-секундный скан.
  6. `startRunningInstancesWatch`: интервал снимается у скрытого окна,
     восстанавливается по видимости; listener корректно снимается в cleanup.
  7. Тесты: `idlePerf.test.ts` (каденс опроса, класс видимости, cleanup) —
     happy-dom проект vitest.
- **Ожидание по метрикам:** скрытое окно — ~0% на рендер (все анимации на
  паузе, опросы остановлены); видимое окно главной — блобы больше не
  перерисовывают blur (десятки раз дешевле кадр), IPC падает с ~1.25/с до
  ≤1/15 с. ОЗУ: ~200 МБ — это почти целиком пол WebView2 (Chromium), а не
  JS-хип лаунчера; заметного снижения ждать не стоит.
- **Важно при замере:** dev-сборка (vite dev + HMR) всегда дороже
  production-сборки — мерить нужно packaged exe.

_Легенда статуса: не исправлено / в работе / исправлено._
_Аудит 2026-07-09: большинство проблем уже реализованы в коде (проверено по
источникам: CREATE_NO_WINDOW везде, Mods/Graph/Recipes/History имеют загрузку и
overflow, `cargo check` бэкенда проходит). Оставшиеся пункты — долгая загрузка
Setup (асинхронность onMount) и рантайм-проверка Export/world-вкладки._
