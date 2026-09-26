# IDE — UI/UX аудит через dev browser (vite 1420)

**Дата:** 2026-09-22 (Europe/Minsk)  
**Dev browser:** `vite v5.4.21 --host 0.0.0.0 --port 1420` · `http://localhost:1420/` · Preview: `https://1420-ib7vx48horywtu3ftb3cq.e2b.app`  
**Ветка:** `arena/01a0c7ef-tuffbox-build-test` (от `9521c32 feat/github-pack-transport`)  
**Mock:** `src/lib/browserMock.ts` + `src/main.ts: void installBrowserMockIfNeeded()` (без top-level await, без duplicate case)  
**Build:** `✓ built in 39.12s` · `main 699kB gz 192kB` · `IdeWorkspace 392kB gz 121kB` · `lint:perf OK` · `vitest 134 passed`  
**Выбранный модпак для превью:** **Create: Aeronautics 0.4.2 / 1.20.1 fabric 0.15.7** (9 модов, граф 9 нод/5 рёбер, `DUPLICATE_MOD` Sodium↔Embeddium, 2 мира, 2 снапшота, квест-бук 2 главы/3 квеста, рецепты `create:cutting/stone`). Бейдж `#browser-mock-badge` внизу-справа.

**Метод проверки:** dev browser (живой Vite HMR), ручная прокрутка каждой вкладки IDE через `currentView="ide"` + `activeStage`, проверка `mockIPC` на каждый `invoke` стадии, код-ревью `IdeWorkspace.svelte` (1137 строк, 15 стейджей) + `IdeNextBar.svelte` + `BriefEditor` + `Sidebar` + responsive брейкпоинты, `curl http://localhost:1420/src/main.ts` (содержит `void installBrowserMockIfNeeded();`), `npm run lint:lazy-views` (16 lazy views), `npm test`, inspection `localStorage` `tuffbox.ide.hiddenStages` и `autoHideWorkflowRail`.

---

## 1. Архитектура IDE

`IdeWorkspace` — полноэкранный `grid: 1fr / auto` с `stage-shell` (flex column: `IdeNextBar` + `stage-content`) и `workflow-rail` (bottom). 15 стейджей:

| # | id | label | short | icon | цель (из `stages[].goal`) |
|---|---|---|---|---|---|
| 0 | `brief` | Brief | Idea | ClipboardList | Storefront: icon/summary/markdown + Modrinth/CurseForge preview |
| 1 | `setup` | Setup | Project | SlidersHorizontal | MC/loader/Java, memory, profiles |
| 2 | `content` | Content | Mods | Package | Моды как managed deps (9 модов в мок) |
| 3 | `quests` | Quests | Lore | ScrollText | FTB Quests + AI QuestPlan (20+ квестов) |
| 4 | `recipes` | Recipes | Craft | PackageOpen | JEI-style + KubeJS remove |
| 5 | `world-map` | World | Map | MapIcon | MCA Selector chunk map |
| 6 | `ore-gen` | Ores | Heights | Mountain | Ore heights / worldgen |
| 7 | `resolve` | Resolve | Graph | GitGraph | Dependency graph / conflicts |
| 8 | `configs` | Tune | Configs | FileCode2 | CodeMirror configs + rollback |
| 9 | `history` | History | Changes | History | Timeline pack activity + delta scan |
| 10 | `test` | Test | Runs | PlayCircle | Launch client/server + logs |
| 11 | `diagnose` | Diagnose | Health | Stethoscope | Crash → fix hypotheses |
| 12 | `snapshots` | Snapshots | Checkpoints | Camera | Checkpoint / diff / rollback |
| 13 | `export` | Export | Build | UploadCloud | .mrpack / Prism / server |
| 14 | `release` | Release | Ship | Rocket | Notes / publish draft |

`visibleStages = stages.filter(!hidden)`; `hiddenStages` персист в `localStorage:tuffbox.ide.hiddenStages` (минимум 1 таб остаётся). Контекстное меню (right-click) — `Close / Reopen / Reopen all`. Dirty-guard только для `configs(tune) / brief / quests` (ConfirmDialog). `autoHideWorkflowRail` — rail `position:absolute; translateY(100%)` + 14px `rail-hotzone` внизу. Шорты: `Ctrl+Enter`→Next, `Ctrl+Shift+P`→Play, `Ctrl+1…0`→chords, `[ / ]`→соседние, `Ctrl/Cmd+click` не перехватывается.

`IdeNextBar` — статус-пилл (`issueCount`/`needsHealth`), `detail`, `launchDetail`, `next-cta` (suggested `computeIdeNextAction`), `Health check` ghost, `workTrail` (контекстный tip → escalate через 5 мин в pack problems).

---

## 2. Что проверено через dev browser

### Живой рендер (mock)
- `curl http://localhost:1420/` → `<!doctype html><div id="app">` + `VITE ready in 524ms`.
- `curl http://localhost:1420/src/main.ts` → `void installBrowserMockIfNeeded();` (нет `await`, нет `safari13`).
- После `mount(App)` в консоли: `[browserMock] installed — chosen modpack: Create: Aeronautics (1.20.1 · fabric 0.15.7)` + бейдж `PREVIEW · Create: Aeronautics · 1.20.1 · fabric 0.15.7 · 9 модов`.
- Переключение `currentView` → `ide`, затем `activeStage` по кругу через devtools `ideStageRequest.set("brief")` … `"release"` — каждая стадия маунтится без `IPC unavailable`. Проверены `invoke` для каждой:
  - **brief** — `getListing`/`getBrief`/`listCategories` → 2 колонки (Pack Identity + Listing preview), sticky header `Synced/Unsaved`, icon dropzone 112px, category chips с чекбоксами, split markdown (CodeMirror + preview), gallery, author notes.
  - **setup** — `validate_project`/`list_profiles`/`find_java_runtimes` → ProjectSettings (MC/loader/Java, memory 4096, profiles Client/Server). При `$projectPath==null` — `skeleton-page` “No project opened”.
  - **content** — `list_mods` 9 модов, `detect_duplicate_mod_jars` (1 группа sodium), `check_mod_updates`, `get_mod_versions` → Mods (292k строк, cards, search, batch bar). В мок виден бейдж duplicate у Embeddium.
  - **quests** — `load_quest_book` 2 главы/3 квеста, `validate_quest_book`, `quest_chat_turn` → QuestEditor (pane layout, AI sidebar).
  - **recipes** — `scan_mod_recipes` → `create:cutting/stone`, `list_item_tags` → RecipeBrowser (JEI grid, search, Uses/Recipes, KubeJS remove).
  - **world-map** — `list_worlds` 2 мира, `read_world_map` 1 регион 140 чанков → World + WorldMap (canvas, select/delete/export/NBT).
  - **ore-gen** — `scan_ore_generation` [] → OreGenVisualizer (слои высот, не крашится на пусто).
  - **resolve** — `get_graph` 9 нод/5 рёбер, `get_diagnostics` `DUPLICATE_MOD`, `export_graph_dot` → Graph (d3-force, pan/zoom, legend, list).
  - **configs** — `list_config_files` 4 файла, `read_config_file`, `lint_config`, `format_toml` → ConfigEditor (CodeMirror, search, rollback).
  - **history** — `list_project_change_history` (1 событие + `ep_1`), `scan_project_changes` → ChangeHistory (timeline, delta).
  - **test** — `list_test_runs` (run_1 passed 42s), `get_launch_log`, `list_running_instances` → TestRuns (launch, logs, perf).
  - **diagnose** — `get_diagnostics`, `get_pack_health` `overall:warnings`, `has_crashed false` → Diagnostics (1320 cap, suspected mods, fix plan).
  - **snapshots** — `list_snapshots` 2, `get_snapshot_detail`, `diff_snapshots` → Snapshots (diff panes).
  - **export** — `export_modrinth_pack`, `batch_export_all` → ExportBuilder.
  - **release** — `generate_release_changelog`, `create_release_snapshot` → ReleaseRoom.
- Ни одна стадия не показала пустой “IPC error” — все `default:` branch мока логирует `warn` но возвращает `[]`/`null` безопасно.

### Навигация и состояния
- **Rail рендер:** 15 табов `flex:1 1 auto` `min-h 52px` + `gap 4px` в `flex-wrap`. На `>1100px` видны `label`+`short`, иконка 20px, Circle 12px слева. На `<1100px` `strong` скрывается (только `short`), `justify:center`. На `<720px` `flex-direction:column` (иконка сверху, подпись 11px) + `padding-inline 8px` — при 15 табов на 720px rail занимает 2 ряда (~104px высоты) — ожидаемо, но съедает вертикаль; позже рекомендовано уплотнить или виртуализировать.
- **Empty / Loading / Error:** `Setup` имеет `skeleton-page`; остальные стадии внутри себя показывают “Open project” / “Loading listing…” (Brief) или скелет; `App.svelte: ensureViewLoaded` показывает `ViewLoading` + `viewLoadError` с retry.
- **Dirty guard:** переход из `configs/brief/quests` при `dirty` → `ConfirmDialog` “Discard & leave” — проверено вводом в Brief summary и попыткой уйти в Setup → диалог появляется, `Cancel` остаётся, `Confirm` сбрасывает dirty и меняет `activeStage`.
- **Closable tabs:** right-click на таб → menu `Close {label}` (disabled если 1 таб остаётся) + секция `Reopen` (список hidden + `Reopen all`). `+` кнопка появляется только когда `hiddenStages.size>0` (30×30 dashed). Персист через `localStorage` — после reload hidden остаются. Навигация через `IdeNextBar` или `Ctrl+chord` на hidden таб автоматически `reopenStage(id)`.
- **Auto-hide:** когда `autoHideWorkflowRail==true` (Settings → Appearance), rail `absolute` скрыт (`translateY(100%) opacity 0`). `rail-hotzone` 14px внизу — раньше был прозрачный 12px, теперь имеет видимую 2px accent линию (градиент) — проверено hover у нижнего края → rail плавно `translateY(0)` за 200ms. `RAIL_HIDE_MS 280ms` и `RAIL_NAV_HOLD_MS 2800ms` после навигации — rail остаётся видимым 2.8s, затем прячется. При `revealed` hotzone `pointer-events:none`.
- **Фокус:** `stage-tab:focus-visible` → `outline 2px accent`; `onfocusin` reveal, `onfocusout` scheduleHide — клавиатурный Tab по rail корректно открывает его.

---

## 3. Инспекция кода — UI/UX находки (до фикса)

### Критичные / заметные

1. **Active vs Hover неразличимы** — `.stage-tab:hover, .stage-tab.active` имели одинаковый `background: var(--bg-tertiary); border 35%` и `color: primary`; различие только в заливке маленького `Circle` (12px). На светлых темах hover `bg-tertiary` почти сливался с rail, и пользователь не понимал где он находится. Требовался distinct active (accent tint + underline).

2. **Невидимый hotzone при auto-hide** — 12px прозрачная полоса без визуального хинта; новые пользователи не догадывались что rail внизу и как его вызвать. Нужна видимая ручка (2px accent line + faint gradient).

3. **Chord discoverability 0** — `STAGE_CHORD` (`1→content, 2→resolve…0→brief`) не соответствует визуальному порядку, и нигде в UI не показывался. `title` был только `stage.goal`, без hint `Ctrl+1`. Пользователь не знал про шорты; `[ / ]` тоже не документированы в UI.

4. **ARIA семантика** — rail был `<nav aria-label>` без `role=tablist`, табы — `<button>` без `role=tab`/`aria-selected` (только `aria-current="step"`). Скринридеры не видели tab pattern; клавиатурная навигация стрелками не работала (только `[ / ]`). Svelte ругался `a11y_no_noninteractive_element_to_interactive_role` при добавлении `role=tablist` на `<nav>`.

5. **Responsive 720px — rail 2 ряда** — при 15 табов на узком экране rail wrap в 2–3 строки, высота ~100–150px, контент сжимается. Приемлемо, но нет уплотнённого режима (иконка-only) или горизонтального скролла.

6. **Perf: IdeWorkspace bundle 392kB** (все стадии импортируются статически внутри IDE). Ленивая загрузка есть на уровне `App.svelte` (IDE как lazy view), но внутри IDE все 15 стейджей в одном чанке. Первая загрузка IDE на potato-pc (e2b) — 121kB gz, ~600ms parse, приемлемо, но могло бы быть code-split per stage.

### Средние

- **BriefEditor** — отличная 2-колонка (`1.35fr / 380–460px`) + sticky header + sync pill; category chips с чекбоксом — хорошо. Но `MdToolbar` + `CodeMirror` + `md-preview` в `grid 1fr 1fr` на `<1100px` схлопывается в 1 колонку — корректно. Минус: `summary` counter `warn>200 / bad>256` — цветовой hint есть, но нет `aria-live` для скринридера.
- **Setup** — при `stayAfterSave` нет явного “Saved” toast внутри формы (есть глобальный toast, но не видно без скролла).
- **Content (Mods)** — 292k строк, вероятно нет виртуализации списка (9 модов в мок — ок, но 150 модов в реальных сборках — скролл тяжелый). Фильтры/поиск — есть, но не sticky.
- **Diagnostics/Graph** — Graph имеет `d3-force` и `overflow-y:auto` на root (правильно), но Diagnostics `flex column overflow hidden` — внутренние панели скроллят сами, но нет visible scrollbar hint.
- **History vs Snapshots** — различие `History (timeline activity) vs Snapshots (checkpoints)` — в `stages[].goal` это описано, но в UI нет persistent hint; новые пользователи путают.
- **IdeNextBar** — pill `warn/ok` + `detail` + `next-cta` (accent) + `Health check` ghost — иерархия верная, но `next-cta` не показывает `Ctrl+Enter` hint, `Health check` не показывает что это `Diagnose` stage.
- **Sidebar rail 72px** — brand `T` + `Home/Library/IDE` + instances (gradient аватарки) + `Play/Logs/Settings/Me` — хорошо, но `Library` и `IDE` рядом, а `IDE` раскрывается в bottom rail — двойная навигация может смущать (Sidebar → IDE → bottom rail).
- **Empty states** — консистентны, но тексты на английском (“No project opened”) — при `locale ru` должны быть локализованы через `t()` (Sidebar локализован, IdeWorkspace — нет).

---

## 4. Что исправлено в этом PR (проверено через dev browser)

### Fix 1 — Active contrast + chord badge (IdeWorkspace.svelte)
**До:** `hover` и `active` одинаковые (`bg-tertiary`, `border 35%`), только Circle отличался.  
**После:**
```css
.stage-tab:hover { bg: var(--bg-tertiary); border 25%; }
.stage-tab.active {
  bg: color-mix(accent 14%, bg-tertiary);
  border: accent 45%;
  box-shadow: inset 0 -2px 0 accent, 0 0 0 1px accent 18%;
}
```
Теперь active имеет тёплую accent-подсветку + 2px нижнюю линию, hover — просто `bg-tertiary`. Проверено: на `Create: Aeronautics` активный `Content` визуально “приподнят”; hover светлее, не путать. Добавили `position:relative` для бейджа.

**Chord badge:** `span.stage-chord` `absolute top:4px right:6px` `14×14` `9px mono` `bg accent 18%` `border accent 30%` `opacity 0 → 1` на `hover/active/focus-visible` с `scale` transition. В `title` теперь `stageTooltip` → `“{goal} • Ctrl+{chord} • [ / ] соседние • Right-click to hide”`. На 720px бейдж `12×12 8px`. Проверено: hover на `Content` показывает `1` в углу, `Brief` — `0`, `Quests` — `8`; без hover — скрыт, не шумит.

### Fix 2 — Видимая ручка auto-hide (IdeWorkspace.svelte)
**До:** `rail-hotzone` `12px` `transparent` — невидим.  
**После:**
```css
.rail-hotzone {
  height: 14px;
  background: linear-gradient(to top, accent 10% → transparent 60%);
  border-top: 2px solid accent 28%;
  opacity: 0.95;
}
.ide-workspace.auto-hide-rail:has(.workflow-rail.revealed) .rail-hotzone { opacity:0; pointer-events:none; }
```
Теперь внизу, когда rail скрыт, видна тонкая 2px accent линия с faint градиентом — хинт “потяни снизу”. При hover/навигации rail выезжает, линия исчезает. Проверено: при `autoHideWorkflowRail=true` (Settings) внизу видна линия; hover у края (even over CodeMirror scrollbar-safe 8px padding) — rail появляется; `scheduleHide 280ms` не конфликтует со скроллбаром.

### Fix 3 — ARIA tab semantics (IdeWorkspace.svelte)
`nav.workflow-rail` → `role="tablist"` (с `<!-- svelte-ignore a11y_no_noninteractive_element_to_interactive_role -->`), каждый `button.stage-tab` → `role="tab"` `aria-selected={active}` `aria-current="step"` `data-chord`. Теперь скринридер объявляет `tablist 15 tabs`, `selected`. `title` включает хинт, `aria-hidden` на Circle/Chord. Проверено: `svelte-check` без ошибок, `vite build` без a11y ворнинга (игнор оставлен для nav).

### Fix 4 — Dev preview инфраструктура (main.ts + browserMock.ts)
- `main.ts`: `await installBrowserMockIfNeeded()` → `void installBrowserMockIfNeeded();` — синхронный prelude `mockWindows`/`mockIPC` успевает до `mount(App)`, но `vite:esbuild-transpile` больше не требует `Top-level await` (`safari13`). Проверено `curl src/main.ts` без `await`.
- `browserMock.ts`: удалены дубли `case pin_project/save_recent_projects/is_project_pinned/get_last_opened_project` (~1603) и второй `get_download_progress` (~1810); оставлены оригинальные блоки (`save_recent_projects` с `localStorage` ~695, `get_download_progress` ~1600). Build warnings `duplicate case` исчезли.
- Добавлен бейдж `#browser-mock-badge` + `console.info` — видимый индикатор превью.
- Добавлен `@internationalized/date` в `package.json` (peer для `bits-ui` calendar) — фиксит `Rollup failed to resolve @internationalized/date` после `npm i playwright`.

---

## 5. Остальные рекомендации (не в scope этого PR, но зафиксированы)

- **Responsive rail:** при `<720px` и 15 табов — ввести `icon-only` compact (скрыть `short` тоже, оставить только иконку 20px + tooltip) или `scroll-snap-x` с `overflow-x:auto` и fade-edges, чтобы не занимать 2 ряда. Альтернативно — grip handle для ручной прокрутки rail.
- **Keyboard:** добавить `ArrowLeft/Right` и `Home/End` на rail (native tablist nav), пока есть только `[ / ]`. Показать `KBD` hints в `KeyboardHelp` (`?`) и в `title` уже есть.
- **Dirty coverage:** расширить guard на `setup` (ProjectSettings) — сейчас только `tune/brief/quests`. ProjectSettings тоже имеет dirty (memory, javaPath) — нужен аналогичный `ConfirmDialog`.
- **IdeNextBar:** добавить `title="Ctrl+Enter"` на `next-cta` и `title="Go to Diagnose (Ctrl+5)"` на Health check; сделать `next.detail` `aria-live="polite"`.
- **Perf:** code-split IdeWorkspace stages: `() => import("./Mods.svelte")` внутри `{#if activeStage==="content"}` → отдельные чанки `Mods-*.js` уже есть на уровне App, но внутри IDE они bundled вместе; вынести каждый stage в lazy import уменьшит `IdeWorkspace 392kB` → ~80kB + on-demand.
- **Brief l10n:** заменить хардкод `“No project opened”` на `t("ide.noProject")`; аналогично `Discard Tune changes?`.
- **History vs Snapshots:** добавить persistent `info` иконку с tooltip “History = activity feed, Snapshots = checkpoints” в header каждой стадии.
- **Mods virtualization:** при 100+ модов — виртуализировать список (`@tanstack/virtual`), sticky search bar.
- **Focus:** после `goToStage` — `focus()` на `stage-content` заголовок для скринридера (сейчас только `pushIdeRecent`).
- **Glass lint:** `lint:glass` уже OK (0 ошибок на 9514 элементов), но `BriefEditor` использует `backdrop-filter: blur(14-20px)` — на potato-pc может дропать fps; уже есть `potato-pc` класс который `prefersReducedMotion` → `fly duration 0`, стоит также отключать blur в potato режиме.

---

## 6. Верификация после фиксов (dev browser + CI)

- `npm run build` — `✓ 4471 modules, built in 39.12s`, без `Top-level await` / `duplicate case`, только `a11y` ignore (ожидаемо).
- `npm run lint:perf` — `tokens OK, fonts OK (221 files), microtext OK (217), glass OK (9514×5774, 0 ignore), lazy-views OK (16), bundle-budget OK (main 192.68kB <230kB)`.
- `npm test` — `134 passed (15 files)` (до фикса тоже 134).
- `curl http://localhost:1420/src/main.ts` — содержит `void installBrowserMockIfNeeded();` без `await`.
- `ps aux | grep vite` — `vite --host 0.0.0.0 --port 1420` `ready in 524ms`, preview URL отвечает `200`.
- Ручная прокрутка IDE в preview: все 15 стейджей рендерят мок (Brief split editor, Content 9 модов, Graph 9/5, World 2 мира, etc.), rail active отличается, chord badge появляется на hover, auto-hide линия видна, dirty guard работает, right-click menu позиционируется clamped (MENU_W 220, MENU_H 260) и не вылезает за `window`.

---

## 7. Как воспроизвести проверку

1. `npm install --legacy-peer-deps` (из-за `workspaces` + `bits-ui` peer).
2. `npm run dev -- --host 0.0.0.0 --port 1420` → открыть `https://1420-ib7vx48horywtu3ftb3cq.e2b.app` или Live Preview “Frontend”.
3. В консоли — `[browserMock] installed … Create: Aeronautics`; внизу-справа — бейдж `PREVIEW … 9 модов`.
4. В Sidebar — `IDE` → внутри IDE кликать bottom rail (`Brief` … `Release`), hover у нижнего края для auto-hide, `Ctrl+1…0` / `[ / ]` / `Ctrl+Enter` / `Ctrl+Shift+P`, right-click → `Close / Reopen`.
5. Проверить Brief dirty: изменить Summary → клик `Setup` → `Discard & leave` диалог.
6. `npm run build && npm run lint:perf && npm test` — все зелёные.

