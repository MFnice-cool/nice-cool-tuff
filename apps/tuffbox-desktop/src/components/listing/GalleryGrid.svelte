<script lang="ts">
  import {
    ImagePlus,
    Plus,
    X,
    MoveLeft,
    MoveRight,
    Link2,
    FileImage,
    FilePlus2,
  } from "@lucide/svelte";
  import type { ListingGalleryItem } from "../../lib/api";

  let {
    items = [],
    urls = {},
    onAddFile = () => {},
    onAddUrl = () => {},
    onDropFiles = () => {},
    onRemove = () => {},
    onMove = () => {},
    onInsert = () => {},
  }: {
    items: ListingGalleryItem[];
    urls: Record<string, string>;
    onAddFile?: () => void;
    onAddUrl?: () => void;
    onDropFiles?: (e: DragEvent) => void;
    onRemove?: (index: number) => void;
    onMove?: (from: number, to: number) => void;
    onInsert?: (item: ListingGalleryItem) => void;
  } = $props();

  let isDraggingOver = $state(false);

  function src(item: ListingGalleryItem): string | null {
    if (item.url) return item.url;
    if (item.path && urls[item.path]) return urls[item.path];
    return null;
  }

  function onDragEnter(e: DragEvent) {
    e.preventDefault();
    isDraggingOver = true;
  }

  function onDragLeave(e: DragEvent) {
    e.preventDefault();
    const related = e.relatedTarget as Node | null;
    if (!related || !(e.currentTarget as Node)?.contains(related)) {
      isDraggingOver = false;
    }
  }

  function onDragOver(e: DragEvent) {
    e.preventDefault();
    isDraggingOver = true;
  }

  function onDrop(e: DragEvent) {
    e.preventDefault();
    isDraggingOver = false;
    if (e.dataTransfer?.files?.length) {
      onDropFiles(e);
    }
  }
</script>

<div class="gallery-root">
  <!-- Top dropzone & upload area -->
  <div
    class="dropzone"
    class:dragging={isDraggingOver}
    role="region"
    aria-label="Gallery upload zone"
    ondragenter={onDragEnter}
    ondragover={onDragOver}
    ondragleave={onDragLeave}
    ondrop={onDrop}
  >
    <div class="dz-content">
      <div class="dz-icon-wrap" aria-hidden="true">
        <ImagePlus size={26} />
      </div>
      <div class="dz-text">
        <p class="dz-title">Drag & drop screenshots here</p>
        <p class="dz-sub">PNG, WebP, JPG or GIF. Dragged or pasted images from clipboard land here.</p>
      </div>
      <div class="dz-actions">
        <button type="button" class="action-btn primary-action" onclick={onAddFile}>
          <FilePlus2 size={14} /> Choose files…
        </button>
        <button type="button" class="action-btn secondary-action" onclick={onAddUrl}>
          <Link2 size={14} /> Add from URL
        </button>
      </div>
    </div>
  </div>

  <!-- Gallery grid -->
  {#if items.length > 0}
    <div class="gallery-header-row">
      <span class="gallery-count">
        <FileImage size={14} />
        {items.length} {items.length === 1 ? "screenshot" : "screenshots"}
      </span>
      <span class="gallery-hint">Hover a screenshot to insert into description, reorder, or delete</span>
    </div>

    <div class="gallery-grid">
      {#each items as item, i (item.path || item.url || `g-${i}`)}
        <div class="gal-tile">
          <div class="gal-frame">
            <span class="gal-index">#{i + 1}</span>
            {#if src(item)}
              <img src={src(item)} alt={item.caption || `Screenshot ${i + 1}`} loading="lazy" />
            {:else}
              <div class="gal-ph">
                <FileImage size={24} />
                <span>Loading…</span>
              </div>
            {/if}
            <div class="gal-overlay">
              <button
                type="button"
                class="ov-btn"
                title="Insert markdown tag into description"
                onclick={() => onInsert(item)}
              >
                <Plus size={13} /> Insert
              </button>
              <div class="ov-divider" aria-hidden="true"></div>
              <button
                type="button"
                class="ov-btn icon"
                title="Move left"
                disabled={i === 0}
                onclick={() => onMove(i, i - 1)}
                aria-label="Move left"
              >
                <MoveLeft size={13} />
              </button>
              <button
                type="button"
                class="ov-btn icon"
                title="Move right"
                disabled={i === items.length - 1}
                onclick={() => onMove(i, i + 1)}
                aria-label="Move right"
              >
                <MoveRight size={13} />
              </button>
              <div class="ov-divider" aria-hidden="true"></div>
              <button
                type="button"
                class="ov-btn icon danger"
                title="Remove screenshot"
                onclick={() => onRemove(i)}
                aria-label="Remove screenshot"
              >
                <X size={14} />
              </button>
            </div>
          </div>
          <span class="gal-caption" title={item.caption || item.path || item.url || ""}>
            {item.caption || (item.path ? item.path.split("/").pop() : item.url ? "Web URL" : `Image #${i + 1}`)}
          </span>
        </div>
      {/each}
    </div>
  {:else}
    <div class="gallery-empty">
      <p>No gallery screenshots added yet. Add screenshots to showcase your modpack on CurseForge and Modrinth.</p>
    </div>
  {/if}
</div>

<style>
  .gallery-root {
    display: flex;
    flex-direction: column;
    gap: 14px;
    width: 100%;
  }

  /* Dropzone */
  .dropzone {
    width: 100%;
    border: 1.5px dashed var(--border-color);
    border-radius: var(--border-radius-md);
    background: color-mix(in srgb, var(--bg-secondary) 25%, transparent);
    padding: 20px 24px;
    box-sizing: border-box;
    transition:
      border-color var(--motion-fast, 160ms) ease,
      background-color var(--motion-fast, 160ms) ease,
      box-shadow var(--motion-fast, 160ms) ease;
  }
  .dropzone:hover {
    border-color: color-mix(in srgb, var(--accent-primary) 45%, transparent);
    background: color-mix(in srgb, var(--accent-primary) 4%, transparent);
  }
  .dropzone.dragging {
    border-color: var(--accent-primary);
    background: color-mix(in srgb, var(--accent-primary) 12%, transparent);
    box-shadow: 0 0 16px color-mix(in srgb, var(--accent-primary) 20%, transparent);
  }

  .dz-content {
    display: flex;
    align-items: center;
    gap: 16px;
    flex-wrap: wrap;
  }

  .dz-icon-wrap {
    width: 44px;
    height: 44px;
    border-radius: var(--border-radius-md);
    display: flex;
    align-items: center;
    justify-content: center;
    background: color-mix(in srgb, var(--accent-primary) 12%, transparent);
    color: var(--accent-primary);
    flex-shrink: 0;
  }

  .dz-text {
    flex: 1;
    min-width: 200px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .dz-title {
    margin: 0;
    font-size: 13.5px;
    font-weight: 700;
    color: var(--text-primary);
  }
  .dz-sub {
    margin: 0;
    font-size: 12px;
    color: color-mix(in srgb, var(--text-secondary) 90%, var(--text-primary));
  }

  .dz-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  .action-btn {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    height: 32px;
    padding: 0 12px;
    border-radius: var(--border-radius-sm);
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
    transition: background var(--motion-fast, 160ms) ease, border-color var(--motion-fast, 160ms) ease, color var(--motion-fast, 160ms) ease;
  }
  .action-btn.primary-action {
    border: 1px solid color-mix(in srgb, var(--accent-primary) 40%, transparent);
    background: color-mix(in srgb, var(--accent-primary) 15%, transparent);
    color: var(--accent-primary);
  }
  .action-btn.primary-action:hover {
    background: color-mix(in srgb, var(--accent-primary) 25%, transparent);
    border-color: var(--accent-primary);
  }
  .action-btn.secondary-action {
    border: 1px solid var(--border-color);
    background: var(--bg-secondary);
    color: var(--text-secondary);
  }
  .action-btn.secondary-action:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  /* Gallery info */
  .gallery-header-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    flex-wrap: wrap;
    padding: 0 2px;
  }
  .gallery-count {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 12.5px;
    font-weight: 700;
    color: var(--text-secondary);
  }
  .gallery-hint {
    font-size: 12px;
    color: color-mix(in srgb, var(--text-secondary) 85%, var(--text-primary));
  }

  /* Gallery Grid */
  .gallery-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: 14px;
    width: 100%;
  }

  .gal-tile {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 0;
  }

  .gal-frame {
    position: relative;
    aspect-ratio: 16 / 10;
    border-radius: var(--border-radius-md);
    overflow: hidden;
    border: 1px solid var(--border-color);
    background: color-mix(in srgb, var(--bg-secondary) 40%, transparent);
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.16);
  }
  .gal-frame img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
  .gal-ph {
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    color: var(--text-muted);
    font-size: 12px;
  }

  .gal-index {
    position: absolute;
    top: 6px;
    left: 6px;
    background: rgba(0, 0, 0, 0.72);
    color: #e5e7eb;
    font-size: 12px;
    font-weight: 700;
    padding: 2px 6px;
    border-radius: 4px;
    backdrop-filter: blur(4px);
    z-index: 2;
    pointer-events: none;
  }

  .gal-overlay {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    background: linear-gradient(0deg, rgba(6, 9, 14, 0.88), rgba(6, 9, 14, 0.45) 60%, rgba(6, 9, 14, 0.3));
    opacity: 0;
    transition: opacity var(--motion-fast, 160ms) ease;
    z-index: 3;
    padding: 6px;
  }
  .gal-tile:hover .gal-overlay,
  .gal-frame:focus-within .gal-overlay {
    opacity: 1;
  }

  .ov-divider {
    width: 1px;
    height: 20px;
    background: rgba(255, 255, 255, 0.2);
  }

  .ov-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    height: 28px;
    padding: 0 9px;
    border: 1px solid rgba(255, 255, 255, 0.18);
    border-radius: var(--border-radius-sm);
    background: rgba(18, 22, 28, 0.85);
    color: #f3f4f6;
    font-size: 12px;
    font-weight: 700;
    cursor: pointer;
    backdrop-filter: blur(8px);
    transition: background var(--motion-fast, 160ms) ease, color var(--motion-fast, 160ms) ease;
  }
  .ov-btn:hover:not(:disabled) {
    background: var(--accent-primary);
    color: #000;
    border-color: var(--accent-primary);
  }
  .ov-btn.icon {
    width: 28px;
    padding: 0;
  }
  .ov-btn:disabled {
    opacity: 0.3;
    cursor: not-allowed;
  }
  .ov-btn.danger {
    color: #f87171;
  }
  .ov-btn.danger:hover:not(:disabled) {
    background: rgba(239, 68, 68, 0.85);
    color: #fff;
    border-color: #ef4444;
  }

  .gal-caption {
    font-size: 12px;
    font-weight: 500;
    color: color-mix(in srgb, var(--text-secondary) 85%, var(--text-primary));
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    padding: 0 2px;
  }

  .gallery-empty {
    padding: 12px 14px;
    border-radius: var(--border-radius-md);
    background: color-mix(in srgb, var(--bg-secondary) 20%, transparent);
    color: color-mix(in srgb, var(--text-secondary) 85%, var(--text-primary));
    font-size: 12.5px;
    margin: 0;
  }
  .gallery-empty p {
    margin: 0;
  }
</style>
