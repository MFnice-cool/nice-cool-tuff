<script lang="ts">
  import type { Snippet } from "svelte";

  /**
   * Center — centers its child both axes (useful for empty states, spinners).
   */
  interface Props {
    fill?: boolean; // default true: occupy the parent's full box
    class?: string;
    children: Snippet;
  }
  const { fill = true, class: cls = "", children }: Props = $props();
</script>

<div class="flex items-center justify-center {fill ? 'h-full w-full' : ''} {cls}">
  {@render children()}
</div>
