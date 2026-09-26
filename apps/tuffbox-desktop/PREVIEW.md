# TuffBox Desktop · Browser Preview

**Dev server:** `vite --host 0.0.0.0 --port 1420` · PID `frontend-56887b23`
**Preview URL:** `https://1420-ib7vx48horywtu3ftb3cq.e2b.app` (также `http://localhost:1420/`)
**Build:** `✓ built in 33.71s` · `main-DvEpbduC.js 699.94 kB gzip 192.68 kB` · нет `safari13`/`duplicate case` ошибок
**Mock:** `src/lib/browserMock.ts` · активируется в `src/main.ts` через `void installBrowserMockIfNeeded()` до `mount(App)` (без top-level await)

## Выбранный модпак (для превью)

| Поле | Значение |
|---|---|
| **ID** | `create-aeronautics` |
| **Имя** | **Create: Aeronautics** |
| **Версия** | `0.4.2` |
| **MC** | `1.20.1` |
| **Loader** | `fabric 0.15.7` |
| **Путь** | `/home/user/TuffBox/instances/Create-Aeronautics/.tuffbox.json` |
| **Описание** | Полёты на блоках Create — дирижабли, воздушные корабли и автоматика в Fabric 1.20.1. |
| **Игрок** | Aviator (offline, uuid `mock-uuid-aviator`) |

Две дополнительные фиктивные сборки в `recentProjects` для проверки Home: `TuffCraft RPG` (forge 47.1.3) и `All The Mods 9` (neoforge 47.1.106).

## Тестовые данные (что видно в UI без Tauri)

Все `invoke()` за-мокированы; любой неизвестный `plugin:*` возвращает безопасную заглушку, новый неохваченный `cmd` логируется как `warn` но не крашит вкладку.

- **Моды (9 шт для выбранного пака):** Create 0.5.1-f, Create: Aeronautics 0.1.1, JEI 15.2.0.27, Sodium 0.5.8, Iris 1.6.17, FerriteCore 6.0.1, Architectury 9.2.14, Cloth Config 11.1.118, Embeddium 0.3.31 (duplicate) — разность `sodium ↔ embeddium_dup` специально для диагностики `DUPLICATE_MOD`.
- **Граф:** `get_graph`/`refresh_graph` → 9 нод (`minecraft`, `loader`, 7 модов) + 5 ребер (`aeronautics→create`, `create→fabric`, `create→1.20.1`, `iris→sodium`, `embeddium_dup↔sodium conflicts`) + `export_graph_dot`.
- **Диагностика:** `get_diagnostics` → `DUPLICATE_MOD` (warning, `mod:embeddium_dup`+`mod:sodium`) + `PERF_SUGGESTION`; `get_pack_health`/`get_diagnostic_counts`/`get_health_report` с `overall: warnings`.
- **Конфиги:** 4 файла (`create-common.toml`, `create-client.toml`, `jei/jei.toml`, `sodium-options.json`) + `read_config_file`/`write_config_file`/`search_in_configs`/`lint_config`/`format_toml`.
- **Tune:** `tune_config_advise`/`preview_diffs` + `list_tune_chat_sessions`/`tune_chat_turn` (мок-ответ “конфиги выглядят корректно”).
- **Миры:** 2 мира (`Aeronautics Test World` 48 MB survival, `Skyblock Create` 12 MB) + `read_world_info`/`read_world_map` (1 регион, 140 чанков) + `list_world_dimensions`/`list_screenshots`/`list_world_backups`.
- **Снимки/История:** 2 снапшота (`Перед добавлением Aeronautics` manual + `Автоснапшот — оптимизация` auto) + `list_project_change_history` (1 событие `add_mod` + эпизод `ep_1 open`), `get_snapshot_detail`, `diff_snapshots`, `diff_manifest_snapshots`, `prune_auto_snapshots`, `create_project_backup`/`list_backups`.
- **Квесты:** `load_quest_book` → 2 главы (`Введение в Create`: “Кинетическая энергия”→“Первый дирижабль”, `Автоматизация`: “Механический пресс”) + `preview_quest_chapter_snbt`/`validate_quest_book`/`generate_quest_plan_from_prompt`/`quest_chat_turn`/`quest_kubejs_*`.
- **Рецепты:** `scan_mod_recipes` → `create:cutting/stone` (cobble→stone) + `list_item_tags`/`list_item_catalog`/`generate_kubejs_recipe_script`/`write_kubejs_*`.
- **Поиск/Библиотека:** `search_modrinth_mods`/`search_curseforge_mods`/`search_unified_mods`/`search_content` (6 хитов: Create, Sodium, JEI, Iris, FerriteCore, Create: Aeronautics) + `list_modrinth_categories`/`get_modrinth_project`/`get_mod_user_state`.
- **Запуск/Логи:** `launch_profile`/`list_running_instances`/`get_live_debug_stats`/`get_launch_log`/`list_instance_logs`/`list_test_runs` (run_1 passed 42s, 2.8 GB→3.2 GB).
- **Настройки/Система:** `get_launcher_settings`/`save_launcher_settings_cmd`, `get_swarm_settings`, `get_runtime_path_info`/`get_instances_path_info`, `detect_gpus` (Mock GPU 8 GB), `get_minecraft_versions` (1.20.1/1.20.4/1.21.1), `get_loader_versions`, `find_java_runtimes` (17.0.9), `get_app_version` 0.1.0-mock, `check_for_app_update`, `list_templates` (Create Base), `list_content_packs`/`list_mc_servers` (Hypixel), `get_presence_settings`.
- **Auth/Профиль:** `mc_get_auth_status` (loggedIn Aviator offline + 2-й аккаунт Hero microsoft), `mc_list_accounts`/`mc_switch_account`/`mc_apply_skin`/`mc_list_capes`, `cosmetics_get_local_profile`/`cosmetics_*_catalog`, `get_local_kudos_balance` (42, RAC 1.2).
- **Прочее:** `get_home_bootstrap`/`validate_project`/`get_project_schema_status`/`get_project_brief`/`get_project_listing`, `repair_project`, `pin_project`/`is_project_pinned`, `export_modrinth_pack`/`batch_export_all`, `github_pack_*` (ready `tuffbox/example-pack` main 0.4.2), `generate_release_changelog`/`create_release_snapshot`, `create_instance`/`update_project_settings`.

Все критичные `invoke` возвращают типизированный объект совпадающий с Rust-стороной, поэтому вкладки не падают в “IPC unavailable”.

## Проверка фронтенд-вкладок (16 lazy views)

`npm run lint:lazy-views` → **16 lazy views are not statically imported.**
`npm run lint:perf` → OK (tokens/fonts/microtext/glass/lazy-views/bundle-budget 192.68 KB < 230 KB).
`npm test` → **134 passed (15 files).**

Каждый `VIEW_LOADERS[id]` (`dashboard`/`ide`/`mods`/`graph`/`world`/`diagnostics`/`crash-votes`/`snapshots`/`configs`/`settings`/`project-settings`/`ore-gen`/`recipes`/`quests`/`library`/`chats`/`me` + спец. `ide` alias) загружается через `import()` и опирается только на за-мокированные `invoke`, поэтому при переключении вкладок в превью нет пустых состояний:

1. **Dashboard** — `get_home_bootstrap` рендерит 3 проекта, `selectedSummary` = Create: Aeronautics, `statsByPath`/`sizesByPath`, `running: []`.
2. **IDE** — `get_graph`/`get_diagnostics`/`list_mods` + `validate_project`.
3. **Mods** — `list_mods` 9 модов с `iconUrl`/`status`, `get_mod_versions`/`check_mod_updates`/`detect_duplicate_mod_jars` (1 группа sodium).
4. **Graph** — 9 нод/5 ребер, `export_graph_dot` отдает DOT.
5. **World** — список 2 миров + карта 1 региона + `list_screenshots`.
6. **Diagnostics** — 1 warning `DUPLICATE_MOD`.
7. **Crash-Votes** — `get_crash_diagnosis` пусто, `has_crashed false`.
8. **Snapshots** — 2 снапшота + деталка `get_snapshot_detail`.
9. **Configs** — 4 файла, чтение TOML/JSON, `format_toml`.
10. **Settings** — `get_launcher_settings`/`get_swarm_settings`.
11. **Project-Settings** — `validate_project`/`list_profiles` (Client Full/Server Pack).
12. **Ore-Gen** — `scan_ore_generation` пусто (визуализатор не крашится).
13. **Recipes** — 1 рецепт `create:cutting/stone` + каталог тегов/предметов.
14. **Quests** — `load_quest_book` 2 главы/3 квеста + чат `quest_chat_turn`.
15. **Library** — поиск (6 результатов, фильтр по `query`) + `get_mod_user_state`.
16. **Chats** — `list_tune_chat_sessions`/`list_quest_chat_sessions` (0 сессий, коррупция 0).
17. **Me** — `mc_get_auth_status` Aviator offline + `mc_list_accounts` + `mc_list_capes`.

> Индикатор превью: после `installBrowserMockIfNeeded()` в DOM инжектируется бейдж `#browser-mock-badge` внизу-справа `PREVIEW · Create: Aeronautics · 1.20.1 · fabric 0.15.7 · 9 модов` (появляется через 200 мс после `mount`, переживает HMR, не перехватывает клики). В консоли: `[browserMock] installed — chosen modpack: Create: Aeronautics (1.20.1 · fabric 0.15.7)`.

## Фикс для dev-превью (почему билд 34s)

- `src/main.ts`: `await installBrowserMockIfNeeded()` → `void installBrowserMockIfNeeded()` — синхронный prelude `mockIPC`/`mockWindows` успевает до первого `invoke`, но `vite:esbuild-transpile` больше не требует `Top-level await` (`safari13`).
- `src/lib/browserMock.ts`: удалены дубли `case "pin_project"/"save_recent_projects"/"is_project_pinned"/"get_last_opened_project"` (~1603) и второй `case "get_download_progress"` (~1810) — `vite build` больше не ворнит `duplicate case`, остается единственный блок `save_recent_projects` (~695, localStorage) и `get_download_progress` (~1600).

Проверка: `curl http://localhost:1420/src/main.ts` содержит `void installBrowserMockIfNeeded();` без `await`; `npm run build` 33.71–34.03s успешно.

## Как проверить вручную

1. Открой превью: `https://1420-ib7vx48horywtu3ftb3cq.e2b.app/` (или нажать Live Preview “Frontend” в Arena).
2. Убедись что внизу-справа висит бейдж `PREVIEW · Create: Aeronautics…` и в консоли нет `[mockIPC] unhandled cmd` для критичных путей.
3. Прокликай слева все 16 вкладок из `VIEW_ORDER` — каждая должна показать мок-контент выше, ни одна не падает в “Desktop IPC unavailable”.
4. В Mods проверь цифру “9 модов” и бейдж “duplicate” у Embeddium; в Graph 9 нод/5 ребер; в Snapshots 2 записи; в Quests 2 главы.
