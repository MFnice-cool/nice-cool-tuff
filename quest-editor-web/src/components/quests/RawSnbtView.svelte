<script lang="ts">
  import type { QuestChapter } from "../../lib/store";
  import { exportChapterSnbt } from "../../lib/store";

  interface Props {
    chapter: QuestChapter;
  }

  let { chapter }: Props = $props();

  let copied = $state(false);

  let rawSnbt = $derived(exportChapterSnbt(chapter));

  function copyToClipboard() {
    navigator.clipboard.writeText(rawSnbt).then(() => {
      copied = true;
      setTimeout(() => (copied = false), 1500);
    });
  }
</script>

<div class="raw-view">
  <div class="raw-header">
    <span class="raw-title">{chapter.title} — Raw SNBT</span>
    <button type="button" class="btn ghost small" onclick={copyToClipboard}>
      {copied ? "Copied!" : "Copy"}
    </button>
  </div>
  <pre class="raw-code"><code>{rawSnbt}</code></pre>
</div>

<style>
  .raw-view {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }
  .raw-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }
  .raw-title {
    font-size: 11px;
    color: var(--text-muted);
    font-weight: 600;
  }
  .raw-code {
    flex: 1;
    overflow: auto;
    padding: 12px;
    margin: 0;
    font-size: 11px;
    line-height: 1.5;
    font-family: 'SF Mono', 'Fira Code', 'Cascadia Code', monospace;
    background: var(--bg-primary);
    color: var(--text-primary);
    white-space: pre;
    tab-size: 2;
  }
  .btn {
    padding: 4px 8px;
    border: 1px solid var(--border);
    background: rgba(0,0,0,0.25);
    color: var(--text-primary);
    font-size: 11px;
    font-weight: 600;
    border-radius: 2px;
  }
  .btn:hover { border-color: var(--accent); }
  .btn.ghost { background: transparent; }
  .btn.small { padding: 2px 6px; font-size: 10px; }
</style>
