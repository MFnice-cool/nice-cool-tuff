<script lang="ts">
  import type { Snippet } from "svelte";

  /**
   * ShellRail — pinned bottom bar of the <Shell>.
   * flex-shrink: 0 — NOT position: sticky (sticky never triggers inside a
   * height:100% flex container; see tauri-svelte-layout pitfalls).
   */
  interface Props {
    /** Height token key (Tailwind spacing scale), default "16" (4rem). */
    height?: "12" | "14" | "16" | "18" | "20";
    class?: string;
    children: Snippet;
  }
  const { height = "16", class: cls = "", children }: Props = $props();

  const HEIGHTS: Record<string, string> = {
    "12": "h-12",
    "14": "h-14",
    "16": "h-16",
    "18": "h-18",
    "20": "h-20",
  };
</script>

<nav class="flex shrink-0 items-center gap-4 border-t border-[var(--border-color)] px-4 {HEIGHTS[height] ?? 'h-16'} {cls}">
  {@render children()}
</nav>
