<script lang="ts">
  import type { Snippet } from "svelte";

  /** ShellHeader — fixed-height top bar of the <Shell>. flex-shrink: 0. */
  interface Props {
    /** Height token key (Tailwind spacing scale), default "18" (4.5rem). */
    height?: "12" | "14" | "16" | "18" | "20" | "24";
    class?: string;
    children: Snippet;
  }
  const { height = "18", class: cls = "", children }: Props = $props();

  const HEIGHTS: Record<string, string> = {
    "12": "h-12",
    "14": "h-14",
    "16": "h-16",
    "18": "h-18",
    "20": "h-20",
    "24": "h-24",
  };
</script>

<header class="flex shrink-0 items-center gap-4 border-b border-[var(--border-color)] px-4 {HEIGHTS[height] ?? 'h-18'} {cls}">
  {@render children()}
</header>
