<script lang="ts">
  import type { Snippet } from "svelte";
  import { Loader2 } from "@lucide/svelte";

  let {
    loading = false,
    disabled = false,
    variant = "primary",
    type = "button",
    onclick,
    children,
  }: {
    loading?: boolean;
    disabled?: boolean;
    variant?: "primary" | "secondary" | "ghost" | "danger";
    type?: "button" | "submit";
    onclick?: (e: MouseEvent) => void;
    children?: Snippet;
  } = $props();

  let isDisabled = $derived(disabled || loading);
</script>

<button
  {type}
  class={variant}
  disabled={isDisabled}
  class:loading
  {onclick}
>
  {#if loading}
    <Loader2 size={16} class="spin-icon" />
  {/if}
  {@render children?.()}
</button>

<style>
  button {
    position: relative;
    min-width: 80px;
  }

  button.loading {
    pointer-events: none;
  }

  button :global(.spin-icon) {
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  button:disabled:hover {
    transform: none;
  }
</style>
