<script lang="ts">
  import type { Snippet } from "svelte";

  /**
   * ShellContent — scrollable main area of the <Shell>.
   * min-h-0 (not height:100%) so children can grow and the container scrolls;
   * overflow-y-auto keeps scrollbars visible per layout contract.
   */
  interface Props {
    padding?: "0" | "2" | "3" | "4" | "5" | "6" | "8";
    class?: string;
    children: Snippet;
  }
  const { padding = "6", class: cls = "", children }: Props = $props();

  const PADDINGS: Record<string, string> = {
    "0": "p-0",
    "2": "p-2",
    "3": "p-3",
    "4": "p-4",
    "5": "p-5",
    "6": "p-6",
    "8": "p-8",
  };
</script>

<main class="min-h-0 flex-1 overflow-y-auto {PADDINGS[padding] ?? 'p-6'} {cls}">
  {@render children()}
</main>
