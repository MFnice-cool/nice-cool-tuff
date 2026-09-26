<script lang="ts">
  // Shown briefly while a lazily-loaded view chunk (see App.svelte's
  // VIEW_LOADERS) is being fetched/parsed. Reuses the existing skeleton
  // utility classes so it looks native to whichever theme is active.
  let { error = null, onRetry = null }: { error?: string | null; onRetry?: (() => void) | null } = $props();
</script>

{#if error}
  <div class="view-load-error">
    <p>Couldn't load this screen.</p>
    <p class="detail">{error}</p>
    {#if onRetry}
      <button type="button" class="retry-btn" onclick={onRetry}>Retry</button>
    {/if}
  </div>
{:else}
  <div class="view-loading" aria-busy="true" aria-label="Loading">
    <div class="skeleton skeleton-block view-loading-title"></div>
    <div class="skeleton skeleton-block view-loading-body"></div>
    <div class="skeleton skeleton-line"></div>
    <div class="skeleton skeleton-line medium view-loading-line"></div>
  </div>
{/if}

<style>
  .view-loading {
    padding: 8px 4px;
    max-width: 640px;
  }

  .view-loading-title {
    height: 32px;
    width: 220px;
    margin-bottom: 20px;
  }

  .view-loading-body {
    height: 120px;
    width: 100%;
    margin-bottom: 16px;
    border-radius: var(--border-radius-lg);
  }

  .view-loading-line {
    margin-top: 10px;
  }

  .view-load-error {
    padding: 64px 24px;
    text-align: center;
    color: var(--text-muted);
  }

  .view-load-error .detail {
    font-size: 12px;
    margin-top: 8px;
    color: var(--accent-danger);
    word-break: break-word;
  }

  .retry-btn {
    margin-top: 16px;
    padding: 6px 16px;
    border: 1px solid var(--border-color, #cfd8cf);
    border-radius: var(--border-radius-sm);
    background: var(--bg-elevated, #e2e8df);
    color: var(--text-primary);
    cursor: pointer;
    font-size: 13px;
  }

  .retry-btn:hover {
    background: var(--bg-hover, #d5ddd1);
  }
</style>
