<script lang="ts">
  import type { Snippet } from "svelte";

  /**
   * Split — two-pane layout: fixed-size pane + fluid pane.
   * Class maps are static so the Tailwind v4 scanner can see every candidate.
   */
  interface Props {
    orientation?: "horizontal" | "vertical";
    /** Size token key (Tailwind spacing scale) for the fixed pane, default "72". */
    size?: "40" | "48" | "56" | "64" | "72" | "80" | "96";
    reverse?: boolean; // put the fixed pane on the right / bottom
    class?: string;
    fixed: Snippet;
    fluid: Snippet;
  }
  const { orientation = "horizontal", size = "72", reverse = false, class: cls = "", fixed, fluid }: Props = $props();

  const WIDTHS: Record<string, string> = {
    "40": "w-40",
    "48": "w-48",
    "56": "w-56",
    "64": "w-64",
    "72": "w-72",
    "80": "w-80",
    "96": "w-96",
  };
  const HEIGHTS: Record<string, string> = {
    "40": "h-40",
    "48": "h-48",
    "56": "h-56",
    "64": "h-64",
    "72": "h-72",
    "80": "h-80",
    "96": "h-96",
  };

  const flex = $derived(orientation === "horizontal" ? "flex-row" : "flex-col");
  const fixedCls = $derived(orientation === "horizontal" ? (WIDTHS[size] ?? "w-72") : (HEIGHTS[size] ?? "h-72"));
  const order = $derived(reverse ? "order-2" : "");
</script>

<div class="flex {flex} h-full w-full {cls}">
  <div class="{fixedCls} shrink-0 min-h-0 min-w-0 {order}">
    {@render fixed()}
  </div>
  <div class="min-h-0 min-w-0 flex-1">
    {@render fluid()}
  </div>
</div>
