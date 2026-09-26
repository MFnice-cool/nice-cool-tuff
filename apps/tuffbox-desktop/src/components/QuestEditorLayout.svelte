<script lang="ts">
  /** Reusable glass split-pane shell for the quest editor. Consumers provide
   * presentation snippets so orchestration stays in QuestEditor.svelte. */
  let { toolbar, chapters, canvas, inspector, footer }: {
    toolbar?: any;
    chapters?: any;
    canvas?: any;
    inspector?: any;
    footer?: any;
  } = $props();
</script>

<div class="flex min-h-0 h-full flex-col overflow-hidden rounded-2xl border border-white/[0.08] bg-neutral-950/40 p-3 shadow-2xl backdrop-blur-2xl">
  {#if toolbar}{@render toolbar()}{/if}
  <div class="mt-3 grid min-h-0 flex-1 grid-cols-[minmax(148px,180px)_minmax(0,1fr)_minmax(280px,340px)] gap-3">
    <aside class="min-h-0 overflow-hidden rounded-xl border border-white/[0.08] bg-white/[0.025]">{#if chapters}{@render chapters()}{/if}</aside>
    <main class="relative min-h-0 overflow-hidden rounded-xl border border-white/[0.08] bg-black/20">{#if canvas}{@render canvas()}{/if}</main>
    <aside class="min-h-0 overflow-y-auto rounded-xl border border-white/[0.08] bg-white/[0.025]">{#if inspector}{@render inspector()}{/if}</aside>
  </div>
  {#if footer}{@render footer()}{/if}
</div>
