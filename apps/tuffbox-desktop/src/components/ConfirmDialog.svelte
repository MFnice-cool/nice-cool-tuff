<script lang="ts">
  import { AlertDialog as BitsAlert } from "bits-ui";
  import { AlertTriangle } from "@lucide/svelte";
  import { fly, fade } from "svelte/transition";
  import { quintOut } from "svelte/easing";

  let {
    title = "Confirm",
    message = "Are you sure?",
    danger = false,
    confirmLabel = "Confirm",
    cancelLabel = "Cancel",
    onconfirm,
    oncancel,
  }: {
    title?: string;
    message?: string;
    danger?: boolean;
    confirmLabel?: string;
    cancelLabel?: string;
    onconfirm?: () => void;
    oncancel?: () => void;
  } = $props();

  // Bits UI owns open state; the component is always mounted with open=true
  // (callers conditionally render it), so closing routes to cancel/confirm.
  function close(action: "confirm" | "cancel") {
    if (action === "confirm") onconfirm?.();
    else oncancel?.();
  }
</script>

<BitsAlert.Root
  open={true}
  onOpenChange={(open) => { if (!open) close("cancel"); }}
>
  <BitsAlert.Portal>
    <div transition:fly={{ y: 14, duration: 200, opacity: 0, easing: quintOut }} class="cd-transition-wrap">
      <BitsAlert.Overlay class="cd-backdrop" />
      <BitsAlert.Content
        class="cd-dialog"
      >
      <BitsAlert.Title class="cd-title">
        <span class="cd-icon" aria-hidden="true">
          <AlertTriangle size={22} color={danger ? "#f87171" : "#fbbf24"} />
        </span>
        {title}
      </BitsAlert.Title>
      <BitsAlert.Description class="cd-message">{message}</BitsAlert.Description>
      <div class="cd-actions">
        <BitsAlert.Cancel class="ghost" onclick={() => close("cancel")}>{cancelLabel}</BitsAlert.Cancel>
        <BitsAlert.Action class={danger ? "danger" : ""} onclick={() => close("confirm")}>
          {confirmLabel}
        </BitsAlert.Action>
      </div>
      </BitsAlert.Content>
    </div>
  </BitsAlert.Portal>
</BitsAlert.Root>

<style>
  :global(.cd-backdrop) {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.55);
    backdrop-filter: blur(8px);
    z-index: 200;
  }
  :global(.cd-dialog) {
    position: fixed;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    background: var(--bg-secondary);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-xl);
    padding: 26px 28px;
    width: min(420px, 92vw);
    box-shadow: var(--shadow-lg);
    z-index: 201;
  }
  :global(.cd-title) {
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: 17px;
    font-weight: 700;
    color: var(--text-primary);
    margin-bottom: 10px;
  }
  .cd-icon { display: inline-flex; flex-shrink: 0; margin-bottom: 12px; }
  :global(.cd-message) {
    color: var(--text-muted);
    font-size: 13px;
    line-height: 1.5;
    margin-bottom: 20px;
    white-space: pre-wrap;
  }
  .cd-actions { display: flex; gap: 10px; justify-content: flex-end; }
  /* bits-ui renders the button, so the element part must be :global; the
     .cd-actions ancestor keeps the rule scoped to this component. */
  .cd-actions :global(button.danger) { background: var(--accent-danger); color: var(--on-accent, #fff); }
  .cd-actions :global(button.danger:hover) { background: color-mix(in srgb, var(--accent-danger) 85%, #000 15%); }
</style>
