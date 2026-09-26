<script lang="ts">
  import { onMount } from "svelte";
  import { X, Bot } from "@lucide/svelte";
  import AiSettingsPanel from "./AiSettingsPanel.svelte";

  let {
    open = $bindable(false),
    onsaved,
    onclose,
  }: {
    open?: boolean;
    onsaved?: () => void;
    onclose?: () => void;
  } = $props();

  function close() {
    open = false;
    onclose?.();
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") close();
  }

  onMount(() => {
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });
</script>

{#if open}
  <div class="backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && close()}>
    <div class="modal" role="dialog" aria-modal="true" aria-label="AI settings">
      <header>
        <div class="title">
          <Bot size={18} />
          <div>
            <strong>AI settings</strong>
            <small>Local models or cloud API</small>
          </div>
        </div>
        <button class="icon" onclick={close} aria-label="Close"><X size={16} /></button>
      </header>
      <AiSettingsPanel {onsaved} />
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 80;
    background: rgba(0, 0, 0, 0.55);
    display: grid;
    place-items: center;
    padding: 16px;
  }
  .modal {
    width: min(640px, 100%);
    max-height: min(92vh, 880px);
    overflow: auto;
    background: var(--bg-secondary, #16161a);
    border: 1px solid var(--border-color, #2a2a32);
    border-radius: var(--border-radius-lg);
    padding: 16px 18px 14px;
    box-shadow: 0 20px 60px rgba(0, 0, 0, 0.45);
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 10px;
  }
  .title {
    display: flex;
    gap: 10px;
    align-items: center;
  }
  .title strong {
    display: block;
    font-size: 15px;
  }
  .title small {
    color: var(--text-muted);
    font-size: 12px;
  }
  .icon {
    border: none;
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
    padding: 4px;
  }
</style>
