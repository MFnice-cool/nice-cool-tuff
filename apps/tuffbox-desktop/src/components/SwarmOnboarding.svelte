<script lang="ts">
  import { Network } from "@lucide/svelte";
  import { trapFocus } from "../lib/focusTrap";

  let {
    onEnable,
    onSkip,
  }: {
    onEnable?: () => void;
    onSkip?: () => void;
  } = $props();
</script>

<div
  class="sw-backdrop"
  role="presentation"
  onclick={(e) => {
    if (e.target === e.currentTarget) onSkip?.();
  }}
>
  <div
    class="sw-dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="swarm-onboard-title"
    use:trapFocus={{ onEscape: () => onSkip?.() }}
  >
    <div class="sw-icon"><Network size={28} /></div>
    <h3 id="swarm-onboard-title">Use TuffSwarm network?</h3>
    <p>
      Share crash-fix experience with the community backend (automatic) and unlock Creation mode
      (modpack trends). Without the network, those modes stay unavailable. You can change this anytime in Settings.
    </p>
    <div class="sw-actions">
      <button class="ghost" type="button" onclick={(e) => { e.stopPropagation(); onSkip?.(); }}>Not now</button>
      <button type="button" onclick={(e) => { e.stopPropagation(); onEnable?.(); }}>Use network</button>
    </div>
  </div>
</div>

<style>
  .sw-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.55);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 220;
    /* WebView2: backdrop-filter can eat clicks; keep a plain dim instead. */
  }
  .sw-dialog {
    position: relative;
    z-index: 1;
    background: var(--bg-secondary);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-xl);
    padding: 28px;
    width: min(440px, 92vw);
    text-align: center;
    box-shadow: var(--shadow-lg);
  }
  .sw-icon {
    margin-bottom: 12px;
    color: var(--accent-primary);
  }
  .sw-dialog h3 {
    font-size: 18px;
    margin-bottom: 8px;
    color: var(--text-primary);
  }
  .sw-dialog p {
    color: var(--text-muted);
    font-size: 13px;
    line-height: 1.5;
    margin-bottom: 20px;
  }
  .sw-actions {
    display: flex;
    gap: 10px;
    justify-content: center;
  }
  .sw-actions button {
    position: relative;
    z-index: 2;
    cursor: pointer;
  }
</style>
