<script lang="ts">
  import type { Snippet } from "svelte";

  /** ShellSidebar — fixed-width left rail of the <Shell>. */
  interface Props {
    /** Width token key (Tailwind spacing scale), default "18" (4.5rem). */
    width?: "14" | "16" | "18" | "20" | "24" | "32" | "48" | "64" | "72";
    class?: string;
    children: Snippet;
  }
  const { width = "18", class: cls = "", children }: Props = $props();

  const WIDTHS: Record<string, string> = {
    "14": "w-14",
    "16": "w-16",
    "18": "w-18",
    "20": "w-20",
    "24": "w-24",
    "32": "w-32",
    "48": "w-48",
    "64": "w-64",
    "72": "w-72",
  };
</script>

<aside class="flex h-full shrink-0 flex-col overflow-y-auto border-r border-[var(--border-color)] {WIDTHS[width] ?? 'w-18'} {cls}">
  {@render children()}
</aside>
