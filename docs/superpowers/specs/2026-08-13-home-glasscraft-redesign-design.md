# Glasscraft — редизайн Home: modern glassy + процедурные MC-текстуры + биом-темы

Дата: 2026-08-13
Статус: утверждено пользователем (2026-08-13)

## Цель

Главный экран лаунчера (Home) должен выглядеть солидно и современно: glassmorphism-поверхности поверх лёгких Minecraft-текстур, плюс новый набор тем по биомам. Без потери читаемости, производительности (potato-pc) и существующей функциональности.

## Зафиксированные решения

| Вопрос | Решение |
|--------|---------|
| Scope | Home + лёгкий глобальный слой (фон app-shell, sidebar, header через токены). Остальные экраны наследуют эффект через токены, без правок разметки |
| Текстуры | Гибрид: процедурные пиксель-текстуры на чистом CSS (gradient-слои), zero ассетов, темизируются токенами |
| Темы | Биом-сет из 4 новых тем: Overworld, Nether, Deep Dark, Amethyst. Существующая `minecraft` (плоская реплика Mojang-launcher) не меняется |

## Текущее состояние (задел)

- Токены `--glass-bg/border/blur/highlight`, `--hero-scrim`, `--play-glow` уже есть в `styles.css`; glass применён только в hero-тулбаре и crash-баннере.
- `themes.css` (~2000 строк): per-theme overrides + shared «Minimal» material block (`:is([data-theme="solar"], …)`) с `--ambient-a/b`, `--app-shell-bg` (layered gradients), `--rail-bg`, `--header-bg`, glass-карточками.
- Каталог тем: `src/lib/themes.ts` (`THEMES`, shades для пикера, `commitTheme()` → `data-theme` на `<html>`).
- Пиксель-шрифт Press Start 2P забандлен (`styles.css` @font-face).
- potato-pc режим глушит blur/glow через `html.potato-pc` overrides.
- Текстурных ассетов в `src/` нет вообще.
- CI: `scripts/check-border-radius.mjs` запрещает литералы 8/12/16/24px — только `var(--border-radius-*)`.

## Дизайн

### 1. Система процедурных текстур — новый `src/styles/textures.css`

Импортируется в `main.ts` после `themes.css`.

**Токены (per-theme переопределяемые):**

```
--tex-opacity   /* 0.0–0.14, интенсивность текстуры */
--tex-size      /* логический тайл, по умолчанию 16px */
```

**Варианты текстур** (utility-классы): `.tex-stone`, `.tex-dirt`, `.tex-grass`, `.tex-deepslate`, `.tex-netherrack`, `.tex-sculk`, `.tex-amethyst`.

Техника: 3–5 слоёв `radial-gradient` / `repeating-conic-gradient` с разными `background-size` (4px/8px/16px) и позициями — дают псевдослучайные пиксель-кластеры двух-трёх тонов поверх базового цвета темы. `image-rendering: pixelated` на случай масштабирования. Текстура всегда полупрозрачная подложка, никогда не самостоятельный фон под текстом.

**Точки применения:**

- **App shell**: каждая биом-тема включает свои текстурные gradient-слои прямо в определение `--app-shell-bg` (он уже поддерживает многослойность у Minimal-тем) — отдельный механизм не нужен. Классы `.tex-*` обслуживают компонентные поверхности (hero fallback, skin-панель, полоса-кромка).
- **Hero fallback** (нет обложки): вместо плоского градиента — «cave scene»: deepslate-текстура + 2 radial-свечения в цвете акцента («руда»).
- **Skin-панель**: текстура stone за 3D-превью («витрина»).
- **Тонкая текстурная полоса** (4–6px) на нижней кромке hero — отсылка к grass-block edge.

### 2. Глобальный glass-апгрейд (`styles.css`)

- `--glass-blur`: 10px → 14px; новый `--glass-saturate: 140%`.
- Новая утилита `.glass-panel`: `var(--glass-bg)` + `backdrop-filter: blur(var(--glass-blur)) saturate(var(--glass-saturate))` + `1px solid var(--glass-border)` + inset highlight (`--glass-highlight`) + `var(--shadow-md)`.
- Home-поверхности переходят на glass: skin-панель, YouTube-лента (`.home-feed`), overflow-меню hero. Sidebar/header получают текстуру под существующие `--rail-backdrop`/`--header-backdrop` (уже поддерживают blur; разметку не трогаем).

### 3. Home (Dashboard.svelte / HomeHero.svelte)

- `HomeHero`: `poster-procedural` → текстурная «cave scene» (класс + токены, potato/reduced-motion отключают).
- Toolbar, storefront CTA, crash-баннер: миграция на `.glass-panel` (поведение и layout без изменений).
- `Dashboard`: `.skin-panel` и `.home-feed` → glass; внутренние отступы/сетка не меняются.
- Play-кнопка не декорируется — солидность за счёт окружения.

### 4. Биом-темы (`themes.css` + `themes.ts`)

Общий `:is([data-theme="overworld"], [data-theme="nether"], [data-theme="deepdark"], [data-theme="amethyst"])`-блок «Glasscraft»: glass-карточки (по образцу Minimal-блока), ambient glows, CTA-язык. Per-theme блоки задают палитру и `--tex-*`.

| id | label | bg deep → mid → accent | Текстура |
|----|-------|------------------------|----------|
| `overworld` | Overworld | `#0d100c` → `#1a2416` → `#52d465` (вторичный: факел `#f0a44a`) | dirt/grass |
| `nether` | Nether | `#140608` → `#241012` → `#ff6a2b` | netherrack + ember glow |
| `deepdark` | Deep Dark | `#05090e` → `#0a141c` → `#2fd6c8` | deepslate + sculk |
| `amethyst` | Amethyst | `#0f0a18` → `#1c1230` → `#b07cff` | calcite/amethyst |

Каждая тема: полный набор базовых токенов (bg/text/accent/border/shadow), `--ambient-a/b`, `--app-shell-bg` + текстурный слой, `--glass-*` overrides, hero-scrim под свою палитру. `themes.ts`: 4 записи с shades. `ThemePicker.svelte` driven by `THEMES` — правок не требует (проверить в ходе реализации).

### 5. Границы и защита

- `html.potato-pc`: `--tex-opacity: 0 !important`, blur 0 (существующий override-блок).
- `prefers-reduced-motion`: анимации текстур не вводятся вообще (только статика).
- Border-radius: только `var(--border-radius-*)`; мелкий декор ≤4px допустим.
- Контраст: текстуры ≤14% opacity; текстовые поверхности со scrim/стеклом.
- Не ломать тему `minecraft` (её layout-overrides специфичны и изолированы под `html[data-theme="minecraft"]`).

## Файлы

- **Новый**: `apps/tuffbox-desktop/src/styles/textures.css`
- **Правки**: `apps/tuffbox-desktop/src/main.ts` (import), `src/styles.css` (glass-токены, `.glass-panel`), `src/styles/themes.css` (4 темы + Glasscraft-блок), `src/lib/themes.ts` (каталог), `src/components/Dashboard.svelte`, `src/components/HomeHero.svelte`

## Критерии приёмки

1. `node apps/tuffbox-desktop/scripts/check-border-radius.mjs` — зелёный.
2. `npx svelte-check` — без новых ошибок.
3. Все 17 тем (13 + 4) переключаются через Settings → Appearance без визуальных поломок Home.
4. На Home: glass-панели читаемы, текстура видна, но не мешает тексту (WCAG AA для основного текста).
5. potato-pc: текстуры и blur отключены, FPS не страдает.
6. Тема `minecraft` выглядит как раньше.

## Out of scope

- Редизайн Library/IDE/Settings разметки (наследуют токены автоматически).
- Анимированные текстуры / parallax.
- PNG-ассеты текстур, кастомные курсоры, звуки.
- Правки темы `minecraft`.
