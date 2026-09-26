# @tuffbox/layout-lib

Tailwind CSS v4 layout-библиотека для TuffBox desktop (Svelte 5 runes). Не заменяет bits-ui — компоненты bits-ui вкладываются внутрь layout-компонентов.

## Подключение

1. В `apps/tuffbox-desktop` Tailwind уже настроен через `@tailwindcss/vite`. В `src/styles.css` добавь:

```css
@import "@tuffbox/layout-lib/styles.css";
```

(плагин Tailwind сканирует исходники, классы из пакета подхватятся автоматически; если нет — добавь `@source "../../packages/layout-lib/src";`)

## Компоненты

| Компонент | Назначение |
|-----------|------------|
| `<Grid>` + `<GridItem>` | 12-колоночная grid-система, breakpoints `sm`/`md`/`lg` |
| `<Stack>` | flex-стек (col/row) с gap, align, justify |
| `<Shell>` | каркас приложения: sidebar + main column |
| `<ShellSidebar>` / `<ShellHeader>` / `<ShellContent>` / `<ShellRail>` | части Shell: фиксированные края + скроллируемый центр |
| `<Split>` | два pane: фикс + fluid (horizontal/vertical, reverse) |
| `<Center>` | центрирование по обеим осям |

## Контракт лейаута (tauri-svelte-layout)

- Header/Rail — `shrink-0`, НЕ `position: sticky`.
- Content — `flex-1 min-h-0 overflow-y-auto`.
- Внутри flex-детей не использовать `vh`-min-height.

## Пример

```svelte
<script lang="ts">
  import { Shell, ShellSidebar, ShellHeader, ShellContent, ShellRail, Grid, GridItem } from "@tuffbox/layout-lib";
</script>

<Shell>
  <ShellSidebar slot-less snippet>{#snippet sidebar()}…{/snippet}</ShellSidebar>
</Shell>
```

Проще — сниппеты передаются прямо:

```svelte
<Shell>
  {#snippet sidebar()}
    <ShellSidebar><!-- nav --></ShellSidebar>
  {/snippet}
  <ShellHeader><!-- toolbar --></ShellHeader>
  <ShellContent>
    <Grid cols={1} md={2} lg={3} gap="4">
      <GridItem span={1}>card</GridItem>
      <GridItem spanMd={2}>wide card</GridItem>
    </Grid>
  </ShellContent>
  <ShellRail><!-- status bar --></ShellRail>
</Shell>
```
