<script lang="ts">
  import type { Component, Snippet } from "svelte";

  let {
    icon = null,
    iconSize = 48,
    title = "",
    description = "",
    actionLabel = "",
    compact = false,
    onaction,
    children,
  }: {
    icon?: Component | null;
    iconSize?: number;
    title?: string;
    description?: string;
    actionLabel?: string;
    compact?: boolean;
    onaction?: () => void;
    children?: Snippet;
  } = $props();
</script>

<div class="empty-state" class:compact>
  {#if icon}
    {@const Icon = icon}
    <div class="empty-icon" class:compact-icon={compact}>
      <Icon size={compact ? 28 : iconSize} strokeWidth={1.5} />
    </div>
  {/if}
  {#if title}
    <h3>{title}</h3>
  {/if}
  {#if description}
    <p>{description}</p>
  {/if}
  {#if actionLabel}
    <button class="empty-action" onclick={() => onaction?.()}>{actionLabel}</button>
  {/if}
  {@render children?.()}
</div>

<style>
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 72px 32px;
    text-align: center;
    color: var(--text-muted);
  }

  .empty-state.compact {
    padding: 32px 16px;
    gap: 8px;
  }

  .empty-icon {
    width: 72px;
    height: 72px;
    border-radius: 20px;
    background: var(--bg-elevated);
    border: 1px solid var(--border-color);
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
    margin-bottom: 8px;
  }

  .compact-icon {
    width: 48px;
    height: 48px;
    border-radius: 14px;
    margin-bottom: 4px;
  }

  h3 {
    margin: 0;
    font-size: 16px;
    font-weight: 700;
    color: var(--text-secondary);
  }

  p {
    margin: 0;
    font-size: 13px;
    max-width: 340px;
    line-height: 1.5;
  }

  .empty-action {
    margin-top: 4px;
    padding: 8px 20px;
    background: var(--bg-elevated);
    border: 1px solid var(--border-color);
    color: var(--text-primary);
    font-size: 13px;
    font-weight: 600;
    border-radius: var(--border-radius-md);
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .empty-action:hover {
    background: var(--accent-primary);
    color: #000;
    border-color: var(--accent-primary);
    transform: translateY(-1px);
  }
</style>
