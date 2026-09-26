# Спринт 4 — IDE: ширина элементов на вкладках (композиция)

**Жалоба:** некоторые элементы интерфейса вкладок IDE имеют «ненужную форму» — растягиваются на всё окно по ширине.

IDE (`IdeWorkspace.svelte`) — 15 стадий-вкладок: Brief, Setup, Content, Quests, Recipes, World, Ores, Resolve, Tune, History, Test, Diagnose, Snapshots, Export, Release. Каждая стадия — отдельный компонент; рабочая область — `.stage-content` (режим `fill-stage` — «полотно» без паддингов, либо обычная страница с паддингами 20×24).

## Диагноз

### D1 (ключевая) — Workspace перебивал собственные капы канвасов

Компоненты-канвасы **спроектированы с центрированной максимальной шириной** для широких окон — с прямыми комментариями в коде:

| Стадия | Компонент | Собственный кап |
|---|---|---|
| Content | `Mods.svelte` | `min(1680px, 100%)` — «…instead of stretching edge-to-edge» |
| Resolve | `Graph.svelte` | `min(1840px, 100%)` — «so node canvas keeps aspect» |
| Diagnose | `Diagnostics.svelte` | `min(1320px, 100%)` — «without edge-to-edge» |
| Snapshots | `Snapshots.svelte` | `max-w-[1440px] mx-auto` |
| World | `World.svelte` | `min(1720px, 100%)` |
| Tune | `ConfigEditor.svelte` | `min(1680px, 100%)` |

Но единственное правило в `IdeWorkspace.svelte`:

```css
.stage-content.fill-stage > :global(.mods), … :global(.snapshots) {
  max-width: none; /* ← перебивало капы (специфичность 0,4,0 против 0,2,0) */
  margin: 0;       /* ← убивало margin: 0 auto */
}
```

**Итог:** в единственном контексте использования этих компонентов — внутри IDE — вся их адаптивная композиция была мёртвым кодом: на окне 1920–2560px каждая стадия растягивалась от края до края. Это и есть «растягивание на всё окно» из жалобы.

### D2 — мёртвый селектор стадии Recipes

Правила стадии целились в `:global(.jei)`, а корень `RecipeBrowser.svelte` называется `.recipe-workspace`. Селектор не соответствовал ничему: стадия Recipes не получала ни сайзинга, ни паддингов стадии — просто растянутый блок.

### D3 — TestRuns: единственный канвас без капа

`TestRuns.svelte` сам объявлял `max-width: none` — ленты статуса, терминал и матрица результатов растягивались на всё окно, расходясь с соседней Diagnose (1320px).

### D4 — ExportBuilder: страница-документ без капа

Export — форма (тулбар, 4 карточки форматов, path-input, сетка «checks», issues, кнопки), а не полотно. Соседние страницы-документы капнуты: History — 1440, OreGen — 1240, Release — 1240. Export объявлял `max-width: none`:

- path-input на широком окне — ~2000+px;
- карточки форматов — по ~500–600px каждая;
- на ноутбуках 901–1280px 4 колонки сжимались до ~210–290px (иконка + текст не помещаются).

### D5 — баннер GitHub-обновления на canvas-стадиях

`GithubPackUpdateBanner` — прямому ребёнку `.stage-content.fill-stage` (padding: 0) — карточка с рамкой прилипала к краям окна; на страницах-документах проблемы нет (там есть паддинги).

## Что не трогал и почему

- `IdeNextBar` — тулбар/статус-бар: полная ширина — корректная форма для бара.
- `QuestEditor` — осознанный full-bleed редактор (сайдбар + SNBT-канвас).
- `BriefEditor` (внутренний кап 72ch) и `ProjectSettings` (внутренний кап 1200px) — композиция обеспечивается внутри.
- History / OreGen / Release — уже капнуты (1440/1240/1240).

## Исправления

| # | Файл | Фикс |
|---|---|---|
| F1 | `IdeWorkspace.svelte` | Из правила canvas-стадий убраны `max-width: none; margin: 0` — канвасы снова сами управляют своей шириной (1680/1840/1320/1440, по центру). Комментарий-предупреждение не повторить регрессию. |
| F2 | `IdeWorkspace.svelte` | Мёртвый `:global(.jei)` → `:global(.recipe-workspace)` (в обоих правилах) — стадия Recipes получает сайзинг и паддинги стадии. |
| F3 | `TestRuns.svelte` | Кап `min(1680px, 100%)` + `margin: 0 auto` — как у Mods/ConfigEditor. |
| F4 | `ExportBuilder.svelte` | Кап `min(1240px, 100%)` + центр — консистентно с Release/OreGen; промежуточный брейкпоинт 901–1280px: сетки форматов и checks — 2×2 вместо сжатых 4-х колонок. |
| F5 | `IdeWorkspace.svelte` | `.fill-stage > :global(.update-card) { margin: 12px 16px 0 }` — баннер insets на canvas-стадиях, совпадает с паддингами канваса. |
| F6 | `RecipeBrowser.svelte` | Собственный кап `min(1840px, 100%)` + центр — как у Graph (действует и вне IDE). |

## Верификация (прогнано в песочнице)

| Проверка | Результат |
|---|---|
| svelte-compile 4 изменённых компонентов | OK, warnings = baseline (8/17/0/1, все — предсуществующие `css_unused_selector`, список сверен с HEAD) |
| Скомпилированный CSS IdeWorkspace | Правило canvas без `max-width`/`margin`; `.recipe-workspace` таргетится; `.update-card` inset есть; `.jei` исчез |
| dist-чанк IdeWorkspace | `min(1240px,100%)` ×2, `min(1440px,100%)`, `min(1680px,100%)`, брейкпоинт 1280 → 2 колонки; клоббера `max-width: none` больше нет |
| `svelte-check` | **0 errors** (138 warnings — без изменений) |
| `vite build` | OK (~33s) |
| Гейты: border-radius / lazy-views / bundle-budget / bridge-parity | OK (193.7/230 KB js, 44.5/46 KB css) |
| `vitest` | **35/35** |

## QA-скрипт (ручная проверка, 5 минут)

1. Откройте окно ≥1920px → вкладка IDE → Content, Resolve, Diagnose, Snapshots, Test: рабочая область центрирована с капом (по бокам виден фон оболочки), не прилипает к краям.
2. Stage Recipes: сетка рецептов центрирована (≤1840), паддинги стадии присутствуют.
3. Stage Export: панель по центру ≤1240px; path-input разумной ширины.
4. Сузьте окно до ~1100px на Export: карточки форматов и checks — 2×2; ниже 900px — в одну колонку.
5. При доступном GitHub-обновлении пакета откройте любую canvas-стадию: карточка баннера с боковыми отступами 16px, не прилипает к краям.
6. Tune (ConfigEditor) и World — без изменений (их капы и раньше работали).

## Бэклог

- Настройка «канвасы во всю ширину» (opt-out от капов) для ультрашироких мониторов.
- Тот же аудит ширины для InstanceHome/Library-панелей вне IDE.
- `data-testid` на стадии/панели IDE (принцип спринта 3).
