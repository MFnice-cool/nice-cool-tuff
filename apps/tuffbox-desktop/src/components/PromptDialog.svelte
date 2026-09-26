<script lang="ts">
  import { Dialog as BitsDialog } from "bits-ui";
  import { fly, fade } from "svelte/transition";
  import { quintOut } from "svelte/easing";
  import { onMount } from "svelte";

  let {
    title = "Select",
    message = "",
    options = [],
    defaultValue = "",
    confirmLabel = "OK",
    cancelLabel = "Cancel",
    mode = "text",
    onconfirm,
    oncancel,
  }: {
    title?: string;
    message?: string;
    options?: string[];
    defaultValue?: string;
    confirmLabel?: string;
    cancelLabel?: string;
    mode?: "text" | "select";
    onconfirm?: (value: string) => void;
    oncancel?: () => void;
  } = $props();

  // Intentional snapshot: the dialog seeds its text field from defaultValue
  // exactly once at mount; later prop changes must not overwrite user input.
  let value = $state("");

  onMount(() => {
    value = defaultValue;
  });

  function submit() {
    if (value.trim()) onconfirm?.(value);
  }
</script>

<BitsDialog.Root
  open={true}
  onOpenChange={(open) => { if (!open) oncancel?.(); }}
>
  <BitsDialog.Portal>
    <div transition:fly={{ y: 14, duration: 200, opacity: 0, easing: quintOut }}>
      <BitsDialog.Overlay class="prompt-backdrop" />
      <BitsDialog.Content class="prompt-dialog">
        <BitsDialog.Title class="prompt-title">{title}</BitsDialog.Title>
        {#if message}
          <BitsDialog.Description class="prompt-message">{message}</BitsDialog.Description>
        {/if}

        {#if mode === "text"}
          <input
            class="prompt-input"
            type="text"
            bind:value
            onkeydown={(e) => e.key === "Enter" && value.trim() && onconfirm?.(value)}
          />
        {:else}
          <div class="prompt-options">
            {#each options as option}
              <button
                class="prompt-option"
                class:selected={option === value}
                onclick={() => (value = option)}
              >
                <span class="prompt-option-name">{option}</span>
                {#if option === value}
                  <span class="prompt-option-check">&#10003;</span>
                {/if}
              </button>
            {/each}
          </div>
        {/if}

        <div class="prompt-actions">
          <BitsDialog.Close class="ghost" onclick={() => oncancel?.()}>{cancelLabel}</BitsDialog.Close>
          <button disabled={!value.trim()} onclick={submit}>{confirmLabel}</button>
        </div>
      </BitsDialog.Content>
    </div>
  </BitsDialog.Portal>
</BitsDialog.Root>

<style>
  :global(.prompt-backdrop) {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.55);
    backdrop-filter: blur(8px);
    z-index: 200;
  }
  :global(.prompt-dialog) {
    position: fixed;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    background: var(--bg-secondary);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-xl);
    padding: 24px;
    width: min(440px, 92vw);
    box-shadow: var(--shadow-lg);
    z-index: 201;
  }
  :global(.prompt-title) {
    font-size: 17px;
    font-weight: 700;
    color: var(--text-primary);
    margin-bottom: 8px;
  }
  :global(.prompt-message) {
    color: var(--text-muted);
    font-size: 13px;
    line-height: 1.5;
    margin-bottom: 14px;
  }
  :global(.prompt-input) {
    width: 100%;
    padding: 9px 12px;
    font-size: 13px;
    color: var(--text-primary);
    background: var(--bg-tertiary);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
    margin-bottom: 18px;
  }
  :global(.prompt-options) {
    max-height: 300px;
    overflow-y: auto;
    margin-bottom: 18px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  :global(.prompt-option) {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 9px 12px;
    text-align: left;
    font-size: 13px;
    color: var(--text-primary);
    background: var(--bg-tertiary);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
    cursor: pointer;
    transition: background var(--motion-fast, 160ms) ease, border-color var(--motion-fast, 160ms) ease;
  }
  :global(.prompt-option:hover) { border-color: var(--accent-primary); }
  :global(.prompt-option.selected) {
    border-color: var(--accent-primary);
    color: var(--accent-primary);
    font-weight: 600;
  }
  .prompt-actions { display: flex; gap: 10px; justify-content: flex-end; }
</style>
