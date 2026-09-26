<script lang="ts">
  import type { Snippet } from "svelte";

  /**
   * Shell — full-height app shell: sidebar + main column (header / content / rail).
   * Layout contract (tauri-svelte-layout):
   *  - header and rail are flex-shrink-0 (NOT position: sticky)
   *  - content is flex-1 + min-h-0 + overflow-y-auto (internal scroll)
   */
  interface Props {
    /** Sidebar slot; pass a <ShellSidebar>. */
    sidebar: Snippet;
    children: Snippet;
  }

  const { sidebar, children }: Props = $props();
</script>

<div class="flex h-screen w-screen overflow-hidden">
  {@render sidebar()}
  <div class="flex min-w-0 flex-1 flex-col">
    {@render children()}
  </div>
</div>
