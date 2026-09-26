<script lang="ts">
  import type { Snippet } from "svelte";

  /**
   * Stack — vertical or horizontal flex stack with consistent gap.
   * Follows the app flex-column pattern (see tauri-svelte-layout skill):
   * scrollable content should be a child with flex-1 + min-h-0, not height:100%.
   * Class maps are static so the Tailwind v4 scanner can see every candidate.
   */
  interface Props {
    direction?: "col" | "row";
    gap?: "0" | "1" | "2" | "3" | "4" | "5" | "6" | "8";
    align?: "start" | "center" | "end" | "stretch" | "baseline";
    justify?: "start" | "center" | "end" | "between" | "around" | "evenly";
    wrap?: boolean;
    /** Fill the parent (h-full + w-full). */
    fill?: boolean;
    class?: string;
    children: Snippet;
  }

  const {
    direction = "col",
    gap = "4",
    align = "stretch",
    justify = "start",
    wrap = false,
    fill = false,
    class: cls = "",
    children,
  }: Props = $props();

  const ALIGN: Record<string, string> = {
    start: "items-start",
    center: "items-center",
    end: "items-end",
    stretch: "items-stretch",
    baseline: "items-baseline",
  };
  const JUSTIFY: Record<string, string> = {
    start: "justify-start",
    center: "justify-center",
    end: "justify-end",
    between: "justify-between",
    around: "justify-around",
    evenly: "justify-evenly",
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

  const flex = $derived(direction === "col" ? "flex-col" : "flex-row");
</script>

<div class="flex {flex} {ALIGN[align] ?? 'items-stretch'} {JUSTIFY[justify] ?? 'justify-start'} {GAPS[gap] ?? 'gap-4'} {wrap ? 'flex-wrap' : ''} {fill ? 'h-full w-full' : ''} {cls}">
  {@render children()}
</div>
