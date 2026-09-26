# Glasscraft Home Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Ð¡Ð´ÐµÐ»Ð°Ñ‚ÑŒ Ð³Ð»Ð°Ð²Ð½Ñ‹Ð¹ ÑÐºÑ€Ð°Ð½ ÑÐ¾Ð»Ð¸Ð´Ð½Ñ‹Ð¼ modern-glassy: Ð¿Ñ€Ð¾Ñ†ÐµÐ´ÑƒÑ€Ð½Ñ‹Ðµ Minecraft-Ñ‚ÐµÐºÑÑ‚ÑƒÑ€Ñ‹ (CSS-only), glass-Ð¿Ð°Ð½ÐµÐ»Ð¸ Ð½Ð° Home, Ð³Ð»Ð¾Ð±Ð°Ð»ÑŒÐ½Ñ‹Ð¹ Ñ‚ÐµÐºÑÑ‚ÑƒÑ€Ð½Ñ‹Ð¹ ÑÐ»Ð¾Ð¹ app-shell Ð¸ 4 Ð½Ð¾Ð²Ñ‹Ðµ Ð±Ð¸Ð¾Ð¼-Ñ‚ÐµÐ¼Ñ‹.

**Architecture:** Ð’ÑÑ‘ Ñ‡ÐµÑ€ÐµÐ· ÑÑƒÑ‰ÐµÑÑ‚Ð²ÑƒÑŽÑ‰ÑƒÑŽ Ñ‚Ð¾ÐºÐµÐ½-ÑÐ¸ÑÑ‚ÐµÐ¼Ñƒ: Ð½Ð¾Ð²Ñ‹Ð¹ ÑÐ»Ð¾Ð¹ `textures.css` (utility-ÐºÐ»Ð°ÑÑÑ‹ `.tex-*` + Ð´ÐµÑ„Ð¾Ð»Ñ‚Ð½Ñ‹Ð¹ `--app-shell-bg` ÑÐ¾ speckle), Ð°Ð¿Ð³Ñ€ÐµÐ¹Ð´ glass-Ñ‚Ð¾ÐºÐµÐ½Ð¾Ð² Ð² `styles.css`, Ñ‚Ð¾Ñ‡ÐµÑ‡Ð½Ñ‹Ðµ Ð¿Ñ€Ð°Ð²ÐºÐ¸ `HomeHero.svelte`/`Dashboard.svelte`, 4 Ð½Ð¾Ð²Ñ‹Ñ… `[data-theme]` Ð±Ð»Ð¾ÐºÐ° + shared Glasscraft-Ð±Ð»Ð¾Ðº Ð² `themes.css`, Ð·Ð°Ð¿Ð¸ÑÐ¸ Ð² ÐºÐ°Ñ‚Ð°Ð»Ð¾Ð³Ðµ `themes.ts`. ÐÐ¸ Ð¾Ð´Ð½Ð¾Ð³Ð¾ PNG-Ð°ÑÑÐµÑ‚Ð°.

**Tech Stack:** Svelte 5 (runes), Ñ‡Ð¸ÑÑ‚Ñ‹Ð¹ CSS (gradient layers, color-mix, backdrop-filter), Tauri.

**Spec:** `docs/superpowers/specs/2026-08-13-home-glasscraft-redesign-design.md` (commit 8632d99f)

## Global Constraints

- Svelte 5 only: `$state`/`$derived`/`$effect`, `onclick` (Ð½Ðµ `on:click`), `{#snippet}` (Ð½Ðµ `<slot>`).
- Border-radius Ñ‚Ð¾Ð»ÑŒÐºÐ¾ `var(--border-radius-sm|md|lg|xl)`; Ð¼ÐµÐ»ÐºÐ¸Ð¹ Ð´ÐµÐºÐ¾Ñ€ â‰¤4px Ð¸ 999px Ð´Ð¾Ð¿ÑƒÑÑ‚Ð¸Ð¼. ÐŸÑ€Ð¾Ð²ÐµÑ€ÐºÐ°: `npm run lint:tokens` (Ð¸Ð· `apps/tuffbox-desktop`).
- `html.potato-pc` Ð¸ `prefers-reduced-motion`: Ñ‚ÐµÐºÑÑ‚ÑƒÑ€Ñ‹ Ð¸ blur Ð¾Ñ‚ÐºÐ»ÑŽÑ‡Ð°ÑŽÑ‚ÑÑ (ÑÐ¼. Task 2).
- ÐÐ¸ÐºÐ°ÐºÐ¸Ñ… PNG/JPG Ð°ÑÑÐµÑ‚Ð¾Ð² â€” Ñ‚Ð¾Ð»ÑŒÐºÐ¾ CSS-Ð³Ñ€Ð°Ð´Ð¸ÐµÐ½Ñ‚Ñ‹. `image-rendering: pixelated` Ð½Ð° Ñ‚ÐµÐºÑÑ‚ÑƒÑ€Ð½Ñ‹Ñ… ÑÐ»Ð¾ÑÑ….
- ÐšÐ°Ð¶Ð´Ñ‹Ð¹ ÐºÐ¾Ð¼Ð¼Ð¸Ñ‚ Ð´Ð¾Ð±Ð°Ð²Ð»ÑÐµÑ‚ Ð¢ÐžÐ›Ð¬ÐšÐž Ñ„Ð°Ð¹Ð»Ñ‹ ÑÐ²Ð¾ÐµÐ¹ Ð·Ð°Ð´Ð°Ñ‡Ð¸ (`git add <file...>`) â€” Ð² Ñ€ÐµÐ¿Ð¾ Ð¼Ð½Ð¾Ð³Ð¾ Ð½ÐµÐ·Ð°ÐºÐ¾Ð¼Ð¼Ð¸Ñ‡ÐµÐ½Ð½Ð¾Ð¹ Ñ‡ÑƒÐ¶Ð¾Ð¹ Ñ€Ð°Ð±Ð¾Ñ‚Ñ‹, Ð½Ðµ Ð¿Ð¾Ð´Ð¼ÐµÑ‚Ð°Ñ‚ÑŒ ÐµÑ‘.
- Ð Ð°Ð±Ð¾Ñ‡Ð°Ñ Ð´Ð¸Ñ€ÐµÐºÑ‚Ð¾Ñ€Ð¸Ñ Ð´Ð»Ñ npm-ÐºÐ¾Ð¼Ð°Ð½Ð´: `apps/tuffbox-desktop`.
- ÐÐµ Ð»Ð¾Ð¼Ð°Ñ‚ÑŒ Ñ‚ÐµÐ¼Ñƒ `minecraft` (ÐµÑ‘ overrides Ð¸Ð·Ð¾Ð»Ð¸Ñ€Ð¾Ð²Ð°Ð½Ñ‹ Ð¿Ð¾Ð´ `html[data-theme="minecraft"]`).

---

### Task 1: Glass-Ñ‚Ð¾ÐºÐµÐ½Ñ‹ + ÑƒÑ‚Ð¸Ð»Ð¸Ñ‚Ð° `.glass-panel`

**Files:**
- Modify: `apps/tuffbox-desktop/src/styles.css` (:root glass-Ð±Ð»Ð¾Ðº ~L61-72; potato-Ð±Ð»Ð¾Ðº ~L436-441; reduced-motion ~L341-347; utilities Ñ€ÑÐ´Ð¾Ð¼ Ñ `.tb-truncate` ~L414)

**Interfaces:**
- Produces: CSS-Ñ‚Ð¾ÐºÐµÐ½ `--glass-saturate` (default `140%`), ÑƒÑ‚Ð¸Ð»Ð¸Ñ‚Ð° `.glass-panel`. ÐŸÐ¾Ñ‚Ñ€ÐµÐ±Ð»ÑÐµÑ‚ÑÑ Ð² Task 3/4 Ð¸ ÑÑƒÑ‰ÐµÑÑ‚Ð²ÑƒÑŽÑ‰Ð¸Ð¼Ð¸ glass-Ð¿Ð¾Ð²ÐµÑ€Ñ…Ð½Ð¾ÑÑ‚ÑÐ¼Ð¸ (hero toolbar, crash banner).

- [x] **Step 1: ÐžÐ±Ð½Ð¾Ð²Ð¸Ñ‚ÑŒ :root glass-Ñ‚Ð¾ÐºÐµÐ½Ñ‹**

Ð’ `styles.css`, Ð² `:root` Ð·Ð°Ð¼ÐµÐ½Ð¸Ñ‚ÑŒ Ð±Ð»Ð¾Ðº:

```css
  --glass-bg: rgba(11, 11, 13, 0.42);
  --glass-border: color-mix(in srgb, #fff 14%, var(--border-color));
  --glass-blur: 10px;
  --glass-highlight: color-mix(in srgb, #fff 14%, transparent);
```

Ð½Ð°:

```css
  --glass-bg: rgba(11, 11, 13, 0.42);
  --glass-border: color-mix(in srgb, #fff 14%, var(--border-color));
  --glass-blur: 14px;
  --glass-saturate: 140%;
  --glass-highlight: color-mix(in srgb, #fff 14%, transparent);
```

- [x] **Step 2: Ð”Ð¾Ð±Ð°Ð²Ð¸Ñ‚ÑŒ ÑƒÑ‚Ð¸Ð»Ð¸Ñ‚Ñƒ `.glass-panel`**

Ð¡Ñ€Ð°Ð·Ñƒ Ð¿Ð¾ÑÐ»Ðµ Ð±Ð»Ð¾ÐºÐ° `.tb-truncate-2` Ð² `styles.css`:

```css
/* Frosted glass surface â€” panels and cards over textured/ambient shell. */
.glass-panel {
  background: var(--glass-bg);
  border: 1px solid var(--glass-border);
  -webkit-backdrop-filter: blur(var(--glass-blur)) saturate(var(--glass-saturate));
  backdrop-filter: blur(var(--glass-blur)) saturate(var(--glass-saturate));
  box-shadow:
    var(--shadow-md),
    inset 0 1px 0 var(--glass-highlight);
}
```

- [x] **Step 3: Ð“Ð°Ñ€Ð´Ñ‹ potato-pc / reduced-motion**

Ð’ `html.potato-pc { ... }` (Ñ‚Ð°Ð¼ ÑƒÐ¶Ðµ `--glass-blur: 0;`) Ð´Ð¾Ð±Ð°Ð²Ð¸Ñ‚ÑŒ `--glass-saturate: 100%;`.
Ð’ `@media (prefers-reduced-motion: reduce) { html { ... } }` Ñ€ÑÐ´Ð¾Ð¼ Ñ `--glass-blur: 0;` Ð´Ð¾Ð±Ð°Ð²Ð¸Ñ‚ÑŒ `--glass-saturate: 100%;`.

- [x] **Step 4: ÐŸÑ€Ð¾Ð²ÐµÑ€ÐºÐ°**

Run: `npm run lint:tokens` (cwd `apps/tuffbox-desktop`)
Expected: PASS (Ð±ÐµÐ· Ð¾ÑˆÐ¸Ð±Ð¾Ðº; Ð½Ð¾Ð²Ñ‹Ñ… radius-Ð»Ð¸Ñ‚ÐµÑ€Ð°Ð»Ð¾Ð² Ð½ÐµÑ‚).

- [x] **Step 5: Commit**

```bash
git add apps/tuffbox-desktop/src/styles.css
git commit -m "feat(ui): glass tokens v2 (blur 14px + saturate) and .glass-panel utility"
```

---

### Task 2: ÐŸÑ€Ð¾Ñ†ÐµÐ´ÑƒÑ€Ð½Ñ‹Ðµ Ñ‚ÐµÐºÑÑ‚ÑƒÑ€Ñ‹ â€” `textures.css`

**Files:**
- Create: `apps/tuffbox-desktop/src/styles/textures.css`
- Modify: `apps/tuffbox-desktop/src/main.ts` (Ð´Ð¾Ð±Ð°Ð²Ð¸Ñ‚ÑŒ import Ð¿Ð¾ÑÐ»Ðµ `./styles/themes.css`)

**Interfaces:**
- Produces: ÐºÐ»Ð°ÑÑÑ‹ `.tex-stone`, `.tex-dirt`, `.tex-grass`, `.tex-deepslate`, `.tex-netherrack`, `.tex-sculk`, `.tex-amethyst`; Ñ‚Ð¾ÐºÐµÐ½Ñ‹ `--tex-size` (16px), `--tex-speck-a/b/c` (per-class overrides); Ð´ÐµÑ„Ð¾Ð»Ñ‚Ð½Ñ‹Ð¹ `--app-shell-bg` ÑÐ¾ stone-speckle (Ð½Ð°ÑÐ»ÐµÐ´ÑƒÑŽÑ‚ Ñ‚ÐµÐ¼Ñ‹ Ð±ÐµÐ· ÑÐ²Ð¾ÐµÐ³Ð¾ `--app-shell-bg`). ÐŸÐ¾Ñ‚Ñ€ÐµÐ±Ð»ÑÐµÑ‚ÑÑ Ð² Task 3 (hero fallback), Task 4 (skin vitrine), Task 5 (Ð±Ð¸Ð¾Ð¼-Ñ‚ÐµÐ¼Ñ‹ Ð¿ÐµÑ€ÐµÐ¾Ð¿Ñ€ÐµÐ´ÐµÐ»ÑÑŽÑ‚ speckle-Ñ†Ð²ÐµÑ‚Ð° Ð¸ `--app-shell-bg`).

**Ð¢ÐµÑ…Ð½Ð¸ÐºÐ°:** ÐºÐ°Ð¶Ð´Ð°Ñ Ñ‚ÐµÐºÑÑ‚ÑƒÑ€Ð° â€” Ð¾Ð´Ð¸Ð½ Ð¾Ð±Ñ‰Ð¸Ð¹ ÑÐ»Ð¾Ñ‘Ð½Ñ‹Ð¹ ÑÑ‚ÐµÐº Ð½Ð° Ð±Ð°Ð·Ð¾Ð²Ð¾Ð¼ ÑÐµÐ»ÐµÐºÑ‚Ð¾Ñ€Ðµ; Ð²Ð°Ñ€Ð¸Ð°Ð½Ñ‚ Ð·Ð°Ð´Ð°Ñ‘Ñ‚ Ñ‚Ð¾Ð»ÑŒÐºÐ¾ 3 Ñ†Ð²ÐµÑ‚Ð°-Â«ÐºÑ€Ð¾ÑˆÐºÐ¸Â» Ñ‡ÐµÑ€ÐµÐ· `--tex-speck-*`. Ð¢Ð°Ð¹Ð» 16px Ñ ÐºÐ»Ð°ÑÑ‚ÐµÑ€Ð°Ð¼Ð¸ ~2px + checker 8px = Ð¿Ð¸ÐºÑÐµÐ»ÑŒ-ÑˆÑƒÐ¼ Ð² Ð´ÑƒÑ…Ðµ MC.

- [x] **Step 1: Ð¡Ð¾Ð·Ð´Ð°Ñ‚ÑŒ `textures.css`**

```css
/* Procedural Minecraft-flavoured pixel textures â€” pure CSS, zero assets.
   Layer stack is shared; each variant only re-colors the three speckle
   tones via --tex-speck-a (light fleck) / --tex-speck-b (dark fleck) /
   --tex-speck-c (checker). Themes may override the vars per data-theme. */

:root {
  --tex-size: 16px;
  --tex-speck-a: rgba(255, 255, 255, 0.05);
  --tex-speck-b: rgba(0, 0, 0, 0.16);
  --tex-speck-c: rgba(0, 0, 0, 0.06);

  /* Default app shell: faint stone speckle over the base color. Themes
     that set their own --app-shell-bg are untouched (higher specificity). */
  --app-shell-bg:
    radial-gradient(circle at 22% 30%, rgba(255, 255, 255, 0.035) 0 10%, transparent 11%),
    radial-gradient(circle at 68% 12%, rgba(0, 0, 0, 0.12) 0 12%, transparent 13%),
    radial-gradient(circle at 82% 64%, rgba(255, 255, 255, 0.03) 0 9%, transparent 10%),
    radial-gradient(circle at 38% 78%, rgba(0, 0, 0, 0.10) 0 13%, transparent 14%),
    repeating-conic-gradient(rgba(0, 0, 0, 0.045) 0% 25%, transparent 0% 50%),
    var(--bg-primary);
}

.app-shell {
  background-size:
    var(--tex-size) var(--tex-size),
    var(--tex-size) var(--tex-size),
    var(--tex-size) var(--tex-size),
    var(--tex-size) var(--tex-size),
    calc(var(--tex-size) / 2) calc(var(--tex-size) / 2),
    auto;
}

/* Shared speckle stack for component surfaces (hero fallback, vitrineâ€¦). */
:where(.tex-stone, .tex-dirt, .tex-grass, .tex-deepslate, .tex-netherrack, .tex-sculk, .tex-amethyst) {
  background-image:
    radial-gradient(circle at 22% 30%, var(--tex-speck-a) 0 10%, transparent 11%),
    radial-gradient(circle at 68% 12%, var(--tex-speck-b) 0 12%, transparent 13%),
    radial-gradient(circle at 82% 64%, var(--tex-speck-a) 0 9%, transparent 10%),
    radial-gradient(circle at 38% 78%, var(--tex-speck-b) 0 13%, transparent 14%),
    radial-gradient(circle at 8% 88%, var(--tex-speck-c) 0 8%, transparent 9%),
    repeating-conic-gradient(var(--tex-speck-c) 0% 25%, transparent 0% 50%);
  background-size:
    var(--tex-size) var(--tex-size),
    var(--tex-size) var(--tex-size),
    var(--tex-size) var(--tex-size),
    var(--tex-size) var(--tex-size),
    var(--tex-size) var(--tex-size),
    calc(var(--tex-size) / 2) calc(var(--tex-size) / 2);
  image-rendering: pixelated;
}

.tex-stone {
  --tex-speck-a: rgba(255, 255, 255, 0.05);
  --tex-speck-b: rgba(0, 0, 0, 0.16);
  --tex-speck-c: rgba(0, 0, 0, 0.06);
}

.tex-dirt {
  --tex-speck-a: rgba(196, 146, 98, 0.10);
  --tex-speck-b: rgba(28, 17, 9, 0.22);
  --tex-speck-c: rgba(64, 42, 24, 0.10);
}

.tex-grass {
  --tex-speck-a: rgba(122, 220, 110, 0.10);
  --tex-speck-b: rgba(24, 34, 14, 0.20);
  --tex-speck-c: rgba(70, 120, 50, 0.10);
}

.tex-deepslate {
  --tex-speck-a: rgba(160, 190, 210, 0.045);
  --tex-speck-b: rgba(0, 0, 0, 0.26);
  --tex-speck-c: rgba(10, 14, 18, 0.10);
}

.tex-netherrack {
  --tex-speck-a: rgba(255, 122, 82, 0.08);
  --tex-speck-b: rgba(22, 5, 7, 0.26);
  --tex-speck-c: rgba(120, 32, 28, 0.10);
}

.tex-sculk {
  --tex-speck-a: rgba(47, 214, 200, 0.10);
  --tex-speck-b: rgba(0, 0, 0, 0.28);
  --tex-speck-c: rgba(16, 60, 64, 0.12);
}

.tex-amethyst {
  --tex-speck-a: rgba(198, 156, 255, 0.10);
  --tex-speck-b: rgba(10, 5, 20, 0.24);
  --tex-speck-c: rgba(96, 62, 150, 0.10);
}

/* Potato PC: no texture repaint cost anywhere. */
html.potato-pc :where(.tex-stone, .tex-dirt, .tex-grass, .tex-deepslate, .tex-netherrack, .tex-sculk, .tex-amethyst) {
  background-image: none;
}

html.potato-pc {
  --app-shell-bg: var(--bg-primary);
}
```

- [x] **Step 2: ÐŸÐ¾Ð´ÐºÐ»ÑŽÑ‡Ð¸Ñ‚ÑŒ Ð² `main.ts`**

```ts
import "./styles.css";
import "./styles/themes.css";
import "./styles/textures.css";
```

- [x] **Step 3: ÐŸÑ€Ð¾Ð²ÐµÑ€ÐºÐ°**

Run: `npm run lint:tokens; npm run build`
Expected: Ð¾Ð±Ð° PASS (vite build ÑÐ¾Ð±Ð¸Ñ€Ð°ÐµÑ‚ CSS Ð±ÐµÐ· Ð¾ÑˆÐ¸Ð±Ð¾Ðº).

- [x] **Step 4: Commit**

```bash
git add apps/tuffbox-desktop/src/styles/textures.css apps/tuffbox-desktop/src/main.ts
git commit -m "feat(ui): procedural CSS pixel-texture layer (.tex-*) and textured default app shell"
```

---

### Task 3: HomeHero â€” Â«cave sceneÂ» fallback + grass-ÐºÑ€Ð¾Ð¼ÐºÐ° + glass-Ð²Ñ‹Ñ€Ð°Ð²Ð½Ð¸Ð²Ð°Ð½Ð¸Ðµ

**Files:**
- Modify: `apps/tuffbox-desktop/src/components/HomeHero.svelte` (style: `.poster-procedural` ~L366-373; `.poster-toolbar-bar` ~L409-418; `.storefront-ghost` ~L686-692; `.crash-fix-banner` ~L694-706)

**Interfaces:**
- Consumes: `.tex-deepslate`-Ð¿Ð¾Ð´Ð¾Ð±Ð½Ñ‹Ð¹ speckle (Ð²ÑÑ‚Ñ€Ð¾ÐµÐ½ Ð¿Ñ€ÑÐ¼Ð¾ Ð² `.poster-procedural`, Ñ‚.Ðº. Ñƒ ÑÐ»ÐµÐ¼ÐµÐ½Ñ‚Ð° ÑƒÐ¶Ðµ ÐµÑÑ‚ÑŒ ÑÐ²Ð¾Ð¸ gradient-ÑÐ»Ð¾Ð¸ â€” ÐºÐ»Ð°ÑÑ `.tex-*` Ð½Ðµ ÐºÐ¾Ð¼Ð±Ð¸Ð½Ð¸Ñ€ÑƒÐµÑ‚ÑÑ), `--glass-saturate` Ð¸Ð· Task 1.
- Produces: Ð½Ð¸Ñ‡ÐµÐ³Ð¾ Ð½Ð°Ñ€ÑƒÐ¶Ñƒ.

- [x] **Step 1: Ð¢ÐµÐºÑÑ‚ÑƒÑ€Ð½Ñ‹Ð¹ fallback Ð²Ð¼ÐµÑÑ‚Ð¾ Ð¿Ð»Ð¾ÑÐºÐ¾Ð³Ð¾ Ð³Ñ€Ð°Ð´Ð¸ÐµÐ½Ñ‚Ð°**

Ð—Ð°Ð¼ÐµÐ½Ð¸Ñ‚ÑŒ `.poster-procedural` Ð½Ð°:

```css
  .poster-procedural {
    position: absolute;
    inset: 0;
    background-image:
      radial-gradient(ellipse 70% 80% at 18% 85%, color-mix(in srgb, var(--accent-primary) 28%, transparent), transparent 55%),
      radial-gradient(ellipse 55% 60% at 92% 8%, color-mix(in srgb, var(--accent-secondary) 22%, transparent), transparent 50%),
      radial-gradient(circle at 22% 30%, var(--tex-speck-a, rgba(160, 190, 210, 0.045)) 0 10%, transparent 11%),
      radial-gradient(circle at 68% 12%, var(--tex-speck-b, rgba(0, 0, 0, 0.26)) 0 12%, transparent 13%),
      radial-gradient(circle at 82% 64%, var(--tex-speck-a, rgba(160, 190, 210, 0.045)) 0 9%, transparent 10%),
      radial-gradient(circle at 38% 78%, var(--tex-speck-b, rgba(0, 0, 0, 0.26)) 0 13%, transparent 14%),
      repeating-conic-gradient(var(--tex-speck-c, rgba(10, 14, 18, 0.10)) 0% 25%, transparent 0% 50%);
    background-size:
      auto, auto,
      var(--tex-size, 16px) var(--tex-size, 16px),
      var(--tex-size, 16px) var(--tex-size, 16px),
      var(--tex-size, 16px) var(--tex-size, 16px),
      var(--tex-size, 16px) var(--tex-size, 16px),
      calc(var(--tex-size, 16px) / 2) calc(var(--tex-size, 16px) / 2);
    background-color: var(--bg-primary);
    image-rendering: pixelated;
  }

  /* Grass-block edge along the bottom of the fallback scene. */
  .poster-procedural::after {
    content: "";
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    height: 6px;
    background-image:
      repeating-conic-gradient(color-mix(in srgb, var(--accent-primary) 55%, #2c1f10) 0% 25%, #3a2a16 0% 50%);
    background-size: 8px 6px;
    image-rendering: pixelated;
    opacity: 0.85;
  }
```

- [x] **Step 2: Glass-Ð²Ñ‹Ñ€Ð°Ð²Ð½Ð¸Ð²Ð°Ð½Ð¸Ðµ**

Ð’ `.poster-toolbar-bar`, `.storefront-ghost`, `.crash-fix-banner` Ð·Ð°Ð¼ÐµÐ½Ð¸Ñ‚ÑŒ Ð¾Ð±Ð° backdrop-filter (Ð¸ `-webkit-` Ð²ÐµÑ€ÑÐ¸Ð¸):
`blur(var(--glass-blur, 10px))` â†’ `blur(var(--glass-blur)) saturate(var(--glass-saturate, 100%))`.

- [x] **Step 3: Ð“Ð°Ñ€Ð´ potato-pc**

Ð’ ÑÑƒÑ‰ÐµÑÑ‚Ð²ÑƒÑŽÑ‰ÐµÐ¼ Ð±Ð»Ð¾ÐºÐµ `:global(html.potato-pc) .poster...` (Ñ€ÑÐ´Ð¾Ð¼, ~L327-330) Ð´Ð¾Ð±Ð°Ð²Ð¸Ñ‚ÑŒ:

```css
  :global(html.potato-pc) .poster-procedural {
    background-image: none;
  }
```

(Ð¿Ð¾ÑÐ»Ðµ ÑÑ‚Ð¾Ð³Ð¾ Ð¾ÑÑ‚Ð°Ð½ÐµÑ‚ÑÑ Ñ‚Ð¾Ð»ÑŒÐºÐ¾ `background-color: var(--bg-primary)`).

- [x] **Step 4: ÐŸÑ€Ð¾Ð²ÐµÑ€ÐºÐ°**

Run: `npm run lint:tokens; npm run check`
Expected: PASS, 0 Ð½Ð¾Ð²Ñ‹Ñ… Ð¾ÑˆÐ¸Ð±Ð¾Ðº svelte-check.

- [x] **Step 5: Commit**

```bash
git add apps/tuffbox-desktop/src/components/HomeHero.svelte
git commit -m "feat(home): textured cave fallback with grass edge in hero, glass saturate alignment"
```

---

### Task 4: Dashboard â€” glass-Ð¿Ð°Ð½ÐµÐ»Ð¸ + Ð²Ð¸Ñ‚Ñ€Ð¸Ð½Ð° ÑÐºÐ¸Ð½Ð°

**Files:**
- Modify: `apps/tuffbox-desktop/src/components/Dashboard.svelte` (style: `.skin-panel` ~L785-795, `.home-feed :global(.youtube-feed)` ~L735-742)

**Interfaces:**
- Consumes: `--glass-bg`, `--glass-border`, `--glass-blur`, `--glass-saturate`, `--glass-highlight` (Task 1), speckle-Ñ‚Ð¾ÐºÐµÐ½Ñ‹ (Task 2).
- Produces: Ð½Ð¸Ñ‡ÐµÐ³Ð¾ Ð½Ð°Ñ€ÑƒÐ¶Ñƒ.

- [x] **Step 1: Skin-Ð¿Ð°Ð½ÐµÐ»ÑŒ â†’ glass + ÐºÐ°Ð¼ÐµÐ½Ð½Ð°Ñ Ð²Ð¸Ñ‚Ñ€Ð¸Ð½Ð° Ð¿Ð¾Ð´ 3D-Ð¿Ñ€ÐµÐ²ÑŒÑŽ**

Ð’ `.skin-panel` Ð·Ð°Ð¼ÐµÐ½Ð¸Ñ‚ÑŒ `background: var(--bg-secondary);` Ð½Ð°:

```css
  background: var(--glass-bg);
  -webkit-backdrop-filter: blur(var(--glass-blur)) saturate(var(--glass-saturate));
  backdrop-filter: blur(var(--glass-blur)) saturate(var(--glass-saturate));
```

Ð˜ Ð² Ñ‚Ð¾Ð¼ Ð¶Ðµ `<style>` Ð´Ð¾Ð±Ð°Ð²Ð¸Ñ‚ÑŒ Ð²Ð¸Ñ‚Ñ€Ð¸Ð½Ñƒ (speckle Ð¿Ð¾Ð´ Ð¿Ð¾Ð»ÑƒÐ¿Ñ€Ð¾Ð·Ñ€Ð°Ñ‡Ð½Ñ‹Ð¼ canvas Ð¿Ñ€ÐµÐ²ÑŒÑŽ):

```css
  .skin-panel :global(.skin-3d-container) {
    background-image:
      radial-gradient(circle at 22% 30%, var(--tex-speck-a, rgba(255, 255, 255, 0.05)) 0 10%, transparent 11%),
      radial-gradient(circle at 68% 12%, var(--tex-speck-b, rgba(0, 0, 0, 0.16)) 0 12%, transparent 13%),
      radial-gradient(circle at 82% 64%, var(--tex-speck-a, rgba(255, 255, 255, 0.05)) 0 9%, transparent 10%),
      radial-gradient(circle at 38% 78%, var(--tex-speck-b, rgba(0, 0, 0, 0.16)) 0 13%, transparent 14%),
      repeating-conic-gradient(var(--tex-speck-c, rgba(0, 0, 0, 0.06)) 0% 25%, transparent 0% 50%);
    background-size:
      var(--tex-size, 16px) var(--tex-size, 16px),
      var(--tex-size, 16px) var(--tex-size, 16px),
      var(--tex-size, 16px) var(--tex-size, 16px),
      var(--tex-size, 16px) var(--tex-size, 16px),
      calc(var(--tex-size, 16px) / 2) calc(var(--tex-size, 16px) / 2);
    image-rendering: pixelated;
  }

  :global(html.potato-pc) .skin-panel :global(.skin-3d-container) {
    background-image: none;
  }
```

Ð•ÑÐ»Ð¸ Ð¿Ñ€Ð¸ Ð²Ð¸Ð·ÑƒÐ°Ð»ÑŒÐ½Ð¾Ð¹ Ð¿Ñ€Ð¾Ð²ÐµÑ€ÐºÐµ Ð¾ÐºÐ°Ð¶ÐµÑ‚ÑÑ, Ñ‡Ñ‚Ð¾ `.skin-3d-container` Ñ€Ð¸ÑÑƒÐµÑ‚ ÑÐ¾Ð±ÑÑ‚Ð²ÐµÐ½Ð½Ñ‹Ð¹ Ð½ÐµÐ¿Ñ€Ð¾Ð·Ñ€Ð°Ñ‡Ð½Ñ‹Ð¹ Ñ„Ð¾Ð½ (Ñ‚ÐµÐºÑÑ‚ÑƒÑ€Ñ‹ Ð½Ðµ Ð²Ð¸Ð´Ð½Ð¾) â€” Ð¿Ñ€Ð¸Ð¼ÐµÐ½Ð¸Ñ‚ÑŒ Ñ‚Ð¾Ñ‚ Ð¶Ðµ Ð±Ð»Ð¾Ðº Ðº `:global(.skin-3d-wrap)` Ð²Ð¼ÐµÑÑ‚Ð¾ container (Ð¿Ñ€Ð¾Ð²ÐµÑ€Ð¸Ñ‚ÑŒ Ð² dev, Ð¾ÑÑ‚Ð°Ð²Ð¸Ñ‚ÑŒ Ð¾Ð´Ð¸Ð½ Ð²Ð°Ñ€Ð¸Ð°Ð½Ñ‚).

- [x] **Step 2: YouTube-Ð»ÐµÐ½Ñ‚Ð° â†’ glass**

Ð’ `.home-feed :global(.youtube-feed)` Ð·Ð°Ð¼ÐµÐ½Ð¸Ñ‚ÑŒ `background: var(--bg-secondary);` Ð½Ð° `background: var(--glass-bg);` Ð¸ Ð´Ð¾Ð±Ð°Ð²Ð¸Ñ‚ÑŒ Ñ‚ÑƒÐ´Ð° Ð¶Ðµ Ð¾Ð±Ðµ ÑÑ‚Ñ€Ð¾ÐºÐ¸ `backdrop-filter` (ÐºÐ°Ðº Ð² Step 1).

- [x] **Step 3: ÐŸÑ€Ð¾Ð²ÐµÑ€ÐºÐ°**

Run: `npm run lint:tokens; npm run check`
Expected: PASS.

- [x] **Step 4: Commit**

```bash
git add apps/tuffbox-desktop/src/components/Dashboard.svelte
git commit -m "feat(home): glass skin panel with stone vitrine and glass youtube feed"
```

---

### Task 5: Ð‘Ð¸Ð¾Ð¼-Ñ‚ÐµÐ¼Ñ‹ â€” Overworld / Nether / Deep Dark / Amethyst

**Files:**
- Modify: `apps/tuffbox-desktop/src/styles/themes.css` (Ð½Ð¾Ð²Ñ‹Ðµ Ð±Ð»Ð¾ÐºÐ¸ â€” Ð¿Ð¾ÑÐ»Ðµ `[data-theme="minecraft"]`-ÑÐµÐºÑ†Ð¸Ð¸, Ð¿ÐµÑ€ÐµÐ´ shared Minimal-Ð±Ð»Ð¾ÐºÐ¾Ð¼ ~L533)
- Modify: `apps/tuffbox-desktop/src/lib/themes.ts` (ThemeId + THEMES)

**Interfaces:**
- Consumes: `--tex-*` speckle-Ð¿ÐµÑ€ÐµÐ¾Ð¿Ñ€ÐµÐ´ÐµÐ»ÐµÐ½Ð¸Ñ (Task 2), glass-Ñ‚Ð¾ÐºÐµÐ½Ñ‹ (Task 1).
- Produces: `ThemeId` += `"overworld" | "nether" | "deepdark" | "amethyst"`; 4 Ð·Ð°Ð¿Ð¸ÑÐ¸ `THEMES` Ñ shades; per-theme `--app-shell-bg` ÑÐ¾ ÑÐ²Ð¾ÐµÐ¹ speckle-Ð¿Ð°Ð»Ð¸Ñ‚Ñ€Ð¾Ð¹.

- [x] **Step 1: ÐšÐ°Ñ‚Ð°Ð»Ð¾Ð³ Ñ‚ÐµÐ¼**

Ð’ `themes.ts` Ñ€Ð°ÑÑˆÐ¸Ñ€Ð¸Ñ‚ÑŒ `ThemeId` Ð¸ `THEMES`:

```ts
export type ThemeId =
  | "tuffbox"
  // â€¦existingâ€¦
  | "minecraft"
  | "overworld"
  | "nether"
  | "deepdark"
  | "amethyst";
```

```ts
  { id: "minecraft", label: "Minecraft", shades: ["#212121", "#313131", "#3fad32"] },
  { id: "overworld", label: "Overworld", shades: ["#0d100c", "#1a2416", "#52d465"] },
  { id: "nether", label: "Nether", shades: ["#140608", "#241012", "#ff6a2b"] },
  { id: "deepdark", label: "Deep Dark", shades: ["#05090e", "#0a141c", "#2fd6c8"] },
  { id: "amethyst", label: "Amethyst", shades: ["#0f0a18", "#1c1230", "#b07cff"] },
```

- [x] **Step 2: Ð§ÐµÑ‚Ñ‹Ñ€Ðµ `[data-theme]` Ð±Ð»Ð¾ÐºÐ° Ð² themes.css**

ÐšÐ°Ð¶Ð´Ñ‹Ð¹ Ð±Ð»Ð¾Ðº: Ð¿Ð¾Ð»Ð½Ð°Ñ Ð¿Ð°Ð»Ð¸Ñ‚Ñ€Ð° + `--app-shell-bg` ÑÐ¾ speckle-ÑÐ»Ð¾ÑÐ¼Ð¸ (Ð¿ÐµÑ€Ð²Ñ‹Ðµ 5 ÑÐ»Ð¾Ñ‘Ð² = Ñ‚ÐµÐºÑÑ‚ÑƒÑ€Ð°, Ð·Ð°Ñ‚ÐµÐ¼ ambient, Ð·Ð°Ñ‚ÐµÐ¼ Ñ†Ð²ÐµÑ‚) + companion-Ð¿Ñ€Ð°Ð²Ð¸Ð»Ð¾ `background-size` Ð´Ð»Ñ `.app-shell`. Ð¨Ð°Ð±Ð»Ð¾Ð½ (Ð¿Ð¾ÐºÐ°Ð·Ð°Ð½ Ð´Ð»Ñ overworld; Ð¾ÑÑ‚Ð°Ð»ÑŒÐ½Ñ‹Ðµ â€” Ð¿Ð¾ Ñ‚Ð°Ð±Ð»Ð¸Ñ†Ðµ Ð¿Ð°Ð»Ð¸Ñ‚Ñ€ Ð½Ð¸Ð¶Ðµ, Ñ‚Ð¾Ñ‚ Ð¶Ðµ ÐºÐ°Ñ€ÐºÐ°Ñ):

```css
[data-theme="overworld"] {
  --bg-primary: #0d100c;
  --bg-secondary: #131811;
  --bg-tertiary: #1a2416;
  --bg-elevated: #22301c;
  --bg-hover: #2a3c22;
  --bg-active: #35502a;
  --text-primary: #f2f8ee;
  --text-secondary: #c4d4b8;
  --text-muted: #7f9474;
  --accent-primary: #52d465;
  --accent-hover: #6fe083;
  --accent-secondary: #f0a44a;
  --accent-danger: #ef5350;
  --accent-warning: #f0c14b;
  --border-color: #26341f;
  --shadow-sm: 0 1px 2px rgba(4, 8, 3, 0.5);
  --shadow-md: 0 6px 18px rgba(4, 8, 3, 0.5);
  --shadow-lg: 0 16px 40px rgba(4, 8, 3, 0.55);
  --on-accent: #07130a;
  --ambient-a: rgba(82, 212, 101, 0.07);
  --ambient-b: rgba(240, 164, 74, 0.05);
  --tex-speck-a: rgba(196, 146, 98, 0.10);
  --tex-speck-b: rgba(28, 17, 9, 0.22);
  --tex-speck-c: rgba(70, 120, 50, 0.10);
  --glass-bg: rgba(13, 16, 12, 0.55);
  --glass-highlight: color-mix(in srgb, #fff 12%, transparent);
  --hero-scrim:
    linear-gradient(90deg, rgba(13, 16, 12, 0.72) 0%, transparent 55%),
    linear-gradient(0deg, rgba(13, 16, 12, 0.68) 0%, transparent 52%);
  --app-shell-bg:
    radial-gradient(circle at 22% 30%, var(--tex-speck-a) 0 10%, transparent 11%),
    radial-gradient(circle at 68% 12%, var(--tex-speck-b) 0 12%, transparent 13%),
    radial-gradient(circle at 82% 64%, var(--tex-speck-a) 0 9%, transparent 10%),
    radial-gradient(circle at 38% 78%, var(--tex-speck-b) 0 13%, transparent 14%),
    repeating-conic-gradient(var(--tex-speck-c) 0% 25%, transparent 0% 50%),
    radial-gradient(ellipse 85% 55% at 8% -8%, var(--ambient-a), transparent 58%),
    radial-gradient(ellipse 70% 45% at 96% 4%, var(--ambient-b), transparent 52%),
    var(--bg-primary);
}

html[data-theme="overworld"] .app-shell {
  background-size:
    var(--tex-size) var(--tex-size),
    var(--tex-size) var(--tex-size),
    var(--tex-size) var(--tex-size),
    var(--tex-size) var(--tex-size),
    calc(var(--tex-size) / 2) calc(var(--tex-size) / 2),
    auto, auto, auto;
}
```

ÐŸÐ°Ð»Ð¸Ñ‚Ñ€Ñ‹ Ð¾ÑÑ‚Ð°Ð»ÑŒÐ½Ñ‹Ñ… (Ñ‚Ð¾Ñ‚ Ð¶Ðµ ÐºÐ°Ñ€ÐºÐ°Ñ, Ð¿Ð¾Ð´ÑÑ‚Ð°Ð²Ð¸Ñ‚ÑŒ Ð·Ð½Ð°Ñ‡ÐµÐ½Ð¸Ñ):

| token | nether | deepdark | amethyst |
|-------|--------|----------|----------|
| bg-primary | `#140608` | `#05090e` | `#0f0a18` |
| bg-secondary | `#1c0a0c` | `#081018` | `#150e22` |
| bg-tertiary | `#241012` | `#0a141c` | `#1c1230` |
| bg-elevated | `#2e1618` | `#0e1c26` | `#25183e` |
| bg-hover | `#3a1d20` | `#14262f` | `#2f2050` |
| bg-active | `#482528` | `#1a3240` | `#3b2a62` |
| text-primary | `#fdf1ec` | `#e8f6f6` | `#f4efff` |
| text-secondary | `#e8c4b8` | `#a8ccd0` | `#c8bce8` |
| text-muted | `#a88478` | `#5f8890` | `#8d7fb0` |
| accent-primary | `#ff6a2b` | `#2fd6c8` | `#b07cff` |
| accent-hover | `#ff8547` | `#54e4d6` | `#c495ff` |
| accent-secondary | `#ffc24b` | `#1e9e93` | `#8f6fd0` |
| accent-danger | `#ff4545` | `#f87171` | `#fb7185` |
| accent-warning | `#fcd34d` | `#eab308` | `#fbbf24` |
| border-color | `#33141a` | `#12242e` | `#2a1d44` |
| on-accent | `#1a0803` | `#03201c` | `#170b28` |
| ambient-a | `rgba(255,106,43,0.08)` | `rgba(47,214,200,0.06)` | `rgba(176,124,255,0.08)` |
| ambient-b | `rgba(255,60,60,0.05)` | `rgba(30,100,120,0.05)` | `rgba(124,88,200,0.06)` |
| tex-speck-a | `rgba(255,122,82,0.08)` | `rgba(47,214,200,0.10)` | `rgba(198,156,255,0.10)` |
| tex-speck-b | `rgba(22,5,7,0.26)` | `rgba(0,0,0,0.28)` | `rgba(10,5,20,0.24)` |
| tex-speck-c | `rgba(120,32,28,0.10)` | `rgba(16,60,64,0.12)` | `rgba(96,62,150,0.10)` |
| glass-bg | `rgba(20,6,8,0.55)` | `rgba(5,9,14,0.55)` | `rgba(15,10,24,0.55)` |
| hero-scrim rgba-Ð±Ð°Ð·Ð° | `20,6,8` | `5,9,14` | `15,10,24` |

- [x] **Step 3: Shared Â«GlasscraftÂ»-Ð±Ð»Ð¾Ðº**

ÐŸÐ¾ÑÐ»Ðµ Ñ‡ÐµÑ‚Ñ‹Ñ€Ñ‘Ñ… Ð±Ð»Ð¾ÐºÐ¾Ð²:

```css
/* â”€â”€ Glasscraft biome themes: glass cards, ambient rail/header, glowing CTAs â”€â”€ */
:is(
  [data-theme="overworld"],
  [data-theme="nether"],
  [data-theme="deepdark"],
  [data-theme="amethyst"]
) {
  --accent-gradient: linear-gradient(145deg, var(--accent-primary) 0%, var(--accent-secondary) 100%);
  --brand-mark-gradient: var(--accent-gradient);
  --brand-mark-fg: var(--on-accent);
  --brand-mark-shadow: 0 4px 12px color-mix(in srgb, var(--accent-primary) 18%, transparent);
  --rail-bg: linear-gradient(
    180deg,
    color-mix(in srgb, var(--accent-primary) 6%, var(--bg-secondary)) 0%,
    color-mix(in srgb, var(--bg-secondary) 88%, transparent) 42%,
    color-mix(in srgb, var(--bg-primary) 78%, transparent) 100%
  );
  --rail-border: color-mix(in srgb, var(--accent-primary) 8%, var(--border-color));
  --rail-backdrop: saturate(140%) blur(22px);
  --header-bg: color-mix(in srgb, var(--bg-secondary) 72%, transparent);
  --header-border: color-mix(in srgb, var(--accent-primary) 6%, var(--border-color));
  --header-backdrop: saturate(140%) blur(18px);
}

/* Quiet glass cards. */
:is(
  [data-theme="overworld"],
  [data-theme="nether"],
  [data-theme="deepdark"],
  [data-theme="amethyst"]
)
  :where(.card, .panel, .tb-card, .projects-section, .settings-grid > .card) {
  background: color-mix(in srgb, var(--bg-secondary) 82%, transparent);
  border-color: var(--border-color);
  box-shadow: var(--shadow-sm);
  -webkit-backdrop-filter: blur(12px) saturate(130%);
  backdrop-filter: blur(12px) saturate(130%);
}

/* Primary CTAs: solid accent with soft biome glow. */
:is(
  [data-theme="overworld"],
  [data-theme="nether"],
  [data-theme="deepdark"],
  [data-theme="amethyst"]
)
  :is(button.primary, button.accent, .play-btn:not(.stop), .primary-action, .tb-btn.primary):not(:disabled) {
  background: var(--accent-gradient);
  color: var(--on-accent);
  border-color: transparent;
  box-shadow: 0 6px 22px color-mix(in srgb, var(--accent-primary) 30%, transparent);
}

:is(
  [data-theme="overworld"],
  [data-theme="nether"],
  [data-theme="deepdark"],
  [data-theme="amethyst"]
)
  :is(button.primary, button.accent, .play-btn:not(.stop), .primary-action, .tb-btn.primary):not(:disabled):hover {
  filter: brightness(1.08);
  box-shadow: 0 8px 28px color-mix(in srgb, var(--accent-primary) 42%, transparent);
}
```

- [x] **Step 4: ÐŸÑ€Ð¾Ð²ÐµÑ€ÐºÐ°**

Run: `npm run lint:tokens; npm run check; npm run build`
Expected: Ð²ÑÐµ PASS.

- [x] **Step 5: Commit**

```bash
git add apps/tuffbox-desktop/src/styles/themes.css apps/tuffbox-desktop/src/lib/themes.ts
git commit -m "feat(themes): Glasscraft biome set â€” Overworld, Nether, Deep Dark, Amethyst"
```

---

### Task 6: Ð¤Ð¸Ð½Ð°Ð»ÑŒÐ½Ð°Ñ Ð¿Ñ€Ð¾Ð²ÐµÑ€ÐºÐ° Ð¸ Ð²Ð¸Ð·ÑƒÐ°Ð»ÑŒÐ½Ñ‹Ð¹ Ð¿Ñ€Ð¾Ñ…Ð¾Ð´

**Files:** â€” (Ñ‚Ð¾Ð»ÑŒÐºÐ¾ Ð¿Ñ€Ð¾Ð²ÐµÑ€ÐºÐ¸)

- [x] **Step 1: Ð¡Ñ‚Ð°Ñ‚Ð¸Ñ‡ÐµÑÐºÐ¸Ðµ Ð¿Ñ€Ð¾Ð²ÐµÑ€ÐºÐ¸**

Run (cwd `apps/tuffbox-desktop`): `npm run lint:perf; npm run check; npm run build`
Expected: Ð²ÑÑ‘ Ð·ÐµÐ»Ñ‘Ð½Ð¾Ðµ; bundle-budget Ð½Ðµ ÑƒÐ¿Ð°Ð» (CSS Ð¿Ñ€Ð¸Ñ€Ð¾ÑÑ‚ ~10KB Ð´Ð¾Ð¿ÑƒÑÑ‚Ð¸Ð¼; ÐµÑÐ»Ð¸ ÑƒÐ¿Ð°Ð» â€” Ð¿Ñ€Ð¾Ð²ÐµÑ€Ð¸Ñ‚ÑŒ Ð»Ð¸Ð¼Ð¸Ñ‚ Ð² `scripts/check-bundle-budget.mjs` Ð¸ Ð¿Ñ€Ð¸ Ð½ÐµÐ¾Ð±Ñ…Ð¾Ð´Ð¸Ð¼Ð¾ÑÑ‚Ð¸ Ð¾Ð±Ð¾ÑÐ½Ð¾Ð²Ð°Ð½Ð½Ð¾ Ð¿Ð¾Ð´Ð½ÑÑ‚ÑŒ Ð¿Ð¾Ñ€Ð¾Ð³ Ð² ÐºÐ¾Ð½Ñ„Ð¸Ð³Ðµ ÑÐºÑ€Ð¸Ð¿Ñ‚Ð°, Ð·Ð°Ñ„Ð¸ÐºÑÐ¸Ñ€Ð¾Ð²Ð°Ð² Ð² ÐºÐ¾Ð¼Ð¼Ð¸Ñ‚Ðµ).

- [x] **Step 2: Ð’Ð¸Ð·ÑƒÐ°Ð»ÑŒÐ½Ñ‹Ð¹ Ñ‡ÐµÐºÐ»Ð¸ÑÑ‚** (Ñ‡ÐµÑ€ÐµÐ· `npm run tauri:dev` Ð¸Ð»Ð¸ Ð¾Ñ‚ÐºÐ»Ð¾Ð½Ð¸Ñ‚ÑŒ ÐºÐ°Ðº Ñ€ÑƒÑ‡Ð½Ð¾Ð¹ ÑˆÐ°Ð³ Ð¿Ð¾Ð»ÑŒÐ·Ð¾Ð²Ð°Ñ‚ÐµÐ»Ñ)

1. Home Ð½Ð° Ñ‚ÐµÐ¼Ðµ `tuffbox`: Ñ„Ð¾Ð½ Ð¾ÐºÐ½Ð° Ñ Ð»Ñ‘Ð³ÐºÐ¸Ð¼ stone-speckle; hero fallback Ñ Ñ‚ÐµÐºÑÑ‚ÑƒÑ€Ð¾Ð¹ Ð¸ grass-ÐºÑ€Ð¾Ð¼ÐºÐ¾Ð¹ (Ð¸Ð½ÑÑ‚Ð°Ð½Ñ Ð±ÐµÐ· Ð¾Ð±Ð»Ð¾Ð¶ÐºÐ¸); skin-Ð¿Ð°Ð½ÐµÐ»ÑŒ Ð¸ Ð»ÐµÐ½Ñ‚Ð° â€” glass.
2. ÐšÐ°Ð¶Ð´Ð°Ñ Ð¸Ð· 4 Ð±Ð¸Ð¾Ð¼-Ñ‚ÐµÐ¼: Settings â†’ Appearance â†’ Ð¿ÐµÑ€ÐµÐºÐ»ÑŽÑ‡Ð¸Ñ‚ÑŒ â€” Home Ñ‡Ð¸Ñ‚Ð°ÐµÐ¼, Ñ‚ÐµÐºÑÑ‚ÑƒÑ€Ð° Ð² Ð¿Ð°Ð»Ð¸Ñ‚Ñ€Ðµ Ñ‚ÐµÐ¼Ñ‹, CTA ÑÐ¾ ÑÐ²ÐµÑ‡ÐµÐ½Ð¸ÐµÐ¼.
3. Ð¢ÐµÐ¼Ð° `minecraft`: Ð±ÐµÐ· Ð¸Ð·Ð¼ÐµÐ½ÐµÐ½Ð¸Ð¹ (Ð¿Ð»Ð¾ÑÐºÐ°Ñ, ÑÐ²Ð¾Ð¹ layout).
4. `potato-pc` (Settings â†’ Performance): Ñ‚ÐµÐºÑÑ‚ÑƒÑ€Ñ‹ Ð¸ blur Ð¾Ñ‚ÐºÐ»ÑŽÑ‡ÐµÐ½Ñ‹.
5. Ð”Ð»Ð¸Ð½Ð½Ñ‹Ð¹ Ð½Ð¸Ðº/Ð¸Ð¼Ñ Ð¸Ð½ÑÑ‚Ð°Ð½ÑÐ°: Ð¾Ð±Ñ€ÐµÐ·ÐºÐ° Ð¿Ð¾ ellipsis Ð½Ðµ ÑÐ»Ð¾Ð¼Ð°Ð»Ð°ÑÑŒ.

- [x] **Step 3: Ð¤Ð¸Ð½Ð°Ð»ÑŒÐ½Ñ‹Ð¹ ÐºÐ¾Ð¼Ð¼Ð¸Ñ‚ (ÐµÑÐ»Ð¸ Ð±Ñ‹Ð»Ð¸ Ð¿Ñ€Ð°Ð²ÐºÐ¸ Ð¿Ð¾ Ð¸Ñ‚Ð¾Ð³Ð°Ð¼ Ð¿Ñ€Ð¾Ð²ÐµÑ€Ð¾Ðº)**

```bash
git add <Ñ‚Ð¾Ð»ÑŒÐºÐ¾ Ð·Ð°Ñ‚Ñ€Ð¾Ð½ÑƒÑ‚Ñ‹Ðµ Ñ„Ð°Ð¹Ð»Ñ‹>
git commit -m "fix(ui): glasscraft review pass"
```

---

## Self-Review Ð²Ñ‹Ð¿Ð¾Ð»Ð½ÐµÐ½ Ð¿Ñ€Ð¸ Ð½Ð°Ð¿Ð¸ÑÐ°Ð½Ð¸Ð¸

- Spec coverage: Ñ‚ÐµÐºÑÑ‚ÑƒÑ€Ñ‹ (Task 2,3,4), glass-Ð°Ð¿Ð³Ñ€ÐµÐ¹Ð´ (Task 1,3,4), Home (Task 3,4), Ð±Ð¸Ð¾Ð¼-Ñ‚ÐµÐ¼Ñ‹ (Task 5), Ð³Ð°Ñ€Ð´Ñ‹ potato/reduced-motion (Task 1,2,3), ÐºÑ€Ð¸Ñ‚ÐµÑ€Ð¸Ð¸ Ð¿Ñ€Ð¸Ñ‘Ð¼ÐºÐ¸ (Task 6). Ð Ð°ÑÑ…Ð¾Ð¶Ð´ÐµÐ½Ð¸Ðµ ÑÐ¾ ÑÐ¿ÐµÐºÐ¾Ð¹ Ð¾ÑÐ¾Ð·Ð½Ð°Ð½Ð½Ð¾Ðµ: potato Ð³Ð»ÑƒÑˆÐ¸Ñ‚ `.tex-*` ÐºÐ»Ð°ÑÑÐ°Ð¼Ð¸ Ð¸ `--app-shell-bg: var(--bg-primary)` Ð²Ð¼ÐµÑÑ‚Ð¾ `--tex-opacity` (alpha Ð²Ð½ÑƒÑ‚Ñ€Ð¸ gradient-ÑÑ‚ÐµÐºÐ° Ñ‚Ð¾ÐºÐµÐ½Ð¾Ð¼ Ð½Ðµ ÑƒÐ¿Ñ€Ð°Ð²Ð»ÑÐµÑ‚ÑÑ) â€” ÑÑ„Ñ„ÐµÐºÑ‚ Ñ‚Ð¾Ñ‚ Ð¶Ðµ.
- Placeholders: Ð½ÐµÑ‚ â€” Ð²ÐµÑÑŒ CSS/TS Ð¿Ñ€Ð¸Ð²ÐµÐ´Ñ‘Ð½ Ð¿Ð¾Ð»Ð½Ð¾ÑÑ‚ÑŒÑŽ, ÐºÑ€Ð¾Ð¼Ðµ Ñ‚Ð°Ð±Ð»Ð¸Ñ‡Ð½Ð¾Ð¹ Ð¿Ð¾Ð´ÑÑ‚Ð°Ð½Ð¾Ð²ÐºÐ¸ Ð¿Ð°Ð»Ð¸Ñ‚Ñ€ Task 5 (Ð²ÑÐµ Ð·Ð½Ð°Ñ‡ÐµÐ½Ð¸Ñ Ð´Ð°Ð½Ñ‹ Ð² Ñ‚Ð°Ð±Ð»Ð¸Ñ†Ðµ).
- Type consistency: `--tex-speck-a/b/c`, `--tex-size`, `--glass-saturate`, `.tex-*`, ThemeId â€” ÑÐ¾Ð³Ð»Ð°ÑÐ¾Ð²Ð°Ð½Ñ‹ Ð¼ÐµÐ¶Ð´Ñƒ Ð·Ð°Ð´Ð°Ñ‡Ð°Ð¼Ð¸.
