<script lang="ts">
  import type { Snippet } from "svelte";

  /**
   * Grid — responsive 12-column grid system (Tailwind grid utilities).
   * All candidate classes are written out statically below so the Tailwind
   * v4 scanner can see them — dynamic `grid-cols-${n}` strings are NOT picked up.
   *
   * Auto-fill mode: pass `autoMin` (px) instead of cols to get
   * `repeat(auto-fill, minmax(autoMin, 1fr))` — columns flow to fit the width.
   */
  interface Props {
    /** Base column count (1..12), applied at all breakpoints */
    cols?: number;
    /** Column count at ≥640px */
    sm?: number;
    /** Column count at ≥768px */
    md?: number;
    /** Column count at ≥1024px */
    lg?: number;
    /** Auto-fill mode: minimum column width in px (overrides cols/sm/md/lg). */
    autoMin?: number;
    gap?: string;
    class?: string;
    children: Snippet;
  }

  const { cols = 12, sm, md, lg, autoMin, gap = "4", class: cls = "", children }: Props = $props();

  const GRID_COLS: Record<number, string> = {
    1: "grid-cols-1",
    2: "grid-cols-2",
    3: "grid-cols-3",
    4: "grid-cols-4",
    5: "grid-cols-5",
    6: "grid-cols-6",
    7: "grid-cols-7",
    8: "grid-cols-8",
    9: "grid-cols-9",
    10: "grid-cols-10",
    11: "grid-cols-11",
    12: "grid-cols-12",
  };
  const SM_COLS: Record<number, string> = {
    1: "sm:grid-cols-1",
    2: "sm:grid-cols-2",
    3: "sm:grid-cols-3",
    4: "sm:grid-cols-4",
    5: "sm:grid-cols-5",
    6: "sm:grid-cols-6",
    7: "sm:grid-cols-7",
    8: "sm:grid-cols-8",
    9: "sm:grid-cols-9",
    10: "sm:grid-cols-10",
    11: "sm:grid-cols-11",
    12: "sm:grid-cols-12",
  };
  const MD_COLS: Record<number, string> = {
    1: "md:grid-cols-1",
    2: "md:grid-cols-2",
    3: "md:grid-cols-3",
    4: "md:grid-cols-4",
    5: "md:grid-cols-5",
    6: "md:grid-cols-6",
    7: "md:grid-cols-7",
    8: "md:grid-cols-8",
    9: "md:grid-cols-9",
    10: "md:grid-cols-10",
    11: "md:grid-cols-11",
    12: "md:grid-cols-12",
  };
  const LG_COLS: Record<number, string> = {
    1: "lg:grid-cols-1",
    2: "lg:grid-cols-2",
    3: "lg:grid-cols-3",
    4: "lg:grid-cols-4",
    5: "lg:grid-cols-5",
    6: "lg:grid-cols-6",
    7: "lg:grid-cols-7",
    8: "lg:grid-cols-8",
    9: "lg:grid-cols-9",
    10: "lg:grid-cols-10",
    11: "lg:grid-cols-11",
    12: "lg:grid-cols-12",
  };
  const GAPS: Record<string, string> = {
    "0": "gap-0",
    "1": "gap-1",
    "2": "gap-2",
    "3": "gap-3",
    "4": "gap-4",
    "5": "gap-5",
    "6": "gap-6",
    "8": "gap-8",
  };

  const clamp = (n?: number) => (n === undefined ? undefined : Math.min(12, Math.max(1, n)));

  const colsCls = $derived(
    autoMin
      ? ""
      : `${GRID_COLS[clamp(cols) ?? 12] ?? "grid-cols-12"} ${sm !== undefined ? (SM_COLS[clamp(sm) ?? 1] ?? "") : ""} ${md !== undefined ? (MD_COLS[clamp(md) ?? 1] ?? "") : ""} ${lg !== undefined ? (LG_COLS[clamp(lg) ?? 1] ?? "") : ""}`,
  );
  const gapCls = $derived(autoMin ? "" : (GAPS[gap] ?? "gap-4"));
  const autoStyle = $derived(autoMin ? `--auto-min:${autoMin}px` : undefined);
</script>

<div
  class="grid {colsCls} {gapCls} {cls}"
  class:auto-grid={autoMin !== undefined}
  style={autoStyle}
>
  {@render children()}
</div>

<style>
  /* Auto-fill grid: column count adapts to container width. The px value comes
     from the autoMin prop via --auto-min; gap uses the passed Tailwind gap via
     the regular gap-* classes above when not in auto mode. */
  .auto-grid {
    grid-template-columns: repeat(auto-fill, minmax(var(--auto-min, 220px), 1fr));
    gap: calc(var(--spacing, 0.25rem) * 4);
  }
</style>
