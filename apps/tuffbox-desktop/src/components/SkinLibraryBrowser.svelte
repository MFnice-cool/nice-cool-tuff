<script lang="ts">
  import { MoreHorizontal, Plus, Trash2 } from "@lucide/svelte";
  import { api } from "../lib/api";
  import {
    duplicateSavedSkin,
    listSavedSkins,
    removeSavedSkin,
    updateSavedSkinVariant,
    upsertSavedSkin,
    type SavedSkin,
    type SkinModelVariant,
  } from "../lib/skinLibrary";
  import {
    authState,
    skinPath,
    type CapeCatalog,
    type CapeProvider,
  } from "../lib/store";
  import { toasts } from "../lib/toast";
  import SkinFrontPreview from "./SkinFrontPreview.svelte";
  import SkinPreview3D from "./SkinPreview3D.svelte";

  let {
    capeCatalog = null,
    revision = 0,
    onaddnew,
    onapplied,
    onlibrarychange,
  }: {
    capeCatalog?: CapeCatalog | null;
    /** Bump from parent after AddSkinModal saves. */
    revision?: number;
    onaddnew?: () => void;
    onapplied?: () => void;
    onlibrarychange?: () => void;
  } = $props();

  let skins = $state<SavedSkin[]>([]);
  let busyId = $state<string | null>(null);
  let menuId = $state<string | null>(null);
  let addingCurrent = $state(false);

  const canApplyMojang = $derived($authState.loginType === "microsoft" && $authState.loggedIn);
  const skinUrl = $derived($authState.profile?.skinUrl ?? null);
  const capeUrl = $derived($authState.profile?.capeUrl ?? null);
  const accountKey = $derived($authState.activeAccountUuid ?? $authState.profile?.uuid ?? "");
  const currentPath = $derived($skinPath);
  const alreadyInLibrary = $derived.by(() => {
    if (!currentPath) return false;
    const norm = currentPath.replace(/\\/g, "/").toLowerCase();
    return skins.some((s) => s.filePath.replace(/\\/g, "/").toLowerCase() === norm);
  });

  function refresh(notify = true) {
    skins = listSavedSkins();
    if (notify) onlibrarychange?.();
  }

  $effect(() => {
    void $authState.activeAccountUuid;
    void revision;
    refresh(false);
  });

  function closeMenu() {
    menuId = null;
  }

  function openMenu(id: string, e: MouseEvent) {
    e.stopPropagation();
    menuId = menuId === id ? null : id;
  }

  async function applyAuthState(state: Awaited<ReturnType<typeof api.mcAuth.getAuthStatus>>) {
    authState.set(state);
    if (state.profile?.uuid) {
      try {
        skinPath.set(await api.mcAuth.getSkinPath(state.profile.uuid));
      } catch {
        skinPath.set(null);
      }
    }
  }

  async function applySkinEntry(entry: SavedSkin) {
    if (!canApplyMojang) {
      toasts.error("Applying a skin needs a Microsoft account");
      return;
    }
    busyId = entry.id;
    try {
      await applyAuthState(await api.mcAuth.uploadSkinFile(entry.filePath, entry.variant));
      if (!entry.cape) {
        await applyAuthState(await api.mcAuth.setCapeProvider("none"));
      } else if (entry.cape.provider === "mojang") {
        await applyAuthState(await api.mcAuth.applyCape(entry.cape.id));
      } else {
        await applyAuthState(await api.mcAuth.setCapeProvider(entry.cape.provider as CapeProvider));
      }
      toasts.success(`Applied "${entry.name}"`);
      closeMenu();
      onapplied?.();
    } catch (e) {
      toasts.error(String(e));
    } finally {
      busyId = null;
    }
  }

  function copySkin(entry: SavedSkin) {
    const dup = duplicateSavedSkin(entry.id);
    if (!dup) {
      toasts.error("Could not copy skin");
      return;
    }
    refresh();
    menuId = dup.id;
    toasts.success(`Copied "${entry.name}"`);
  }

  function toggleModel(entry: SavedSkin) {
    const next: SkinModelVariant = entry.variant === "classic" ? "slim" : "classic";
    updateSavedSkinVariant(entry.id, next);
    refresh();
  }

  function deleteSkin(entry: SavedSkin) {
    removeSavedSkin(entry.id);
    if (menuId === entry.id) closeMenu();
    refresh();
    toasts.info(`Removed "${entry.name}"`);
  }

  async function addCurrentToLibrary() {
    if (!currentPath) {
      toasts.error("No cached skin file for this account yet");
      return;
    }
    if (alreadyInLibrary) {
      toasts.info("Current skin is already in the library");
      return;
    }
    addingCurrent = true;
    try {
      const provider = ($authState.capeProvider ?? "none") as CapeProvider;
      let cape = null as SavedSkin["cape"];
      if (provider !== "none" && capeUrl) {
        const offer =
          capeCatalog?.offers.find((o) => o.provider === provider && o.active) ??
          capeCatalog?.offers.find((o) => o.provider === provider);
        cape = {
          provider,
          id: offer?.id ?? provider,
          label: offer?.label ?? provider,
          url: offer?.url || capeUrl,
        };
      }
      upsertSavedSkin({
        name: $authState.profile?.name ?? "Current",
        variant: "classic",
        filePath: currentPath,
        cape,
      });
      refresh();
      toasts.success("Added current skin to library");
    } catch (e) {
      toasts.error(String(e));
    } finally {
      addingCurrent = false;
    }
  }
</script>

<svelte:window
  onclick={() => closeMenu()}
  onkeydown={(e) => {
    if (e.key === "Escape") closeMenu();
  }}
/>

<div class="skin-browser">
  <section class="current-col" aria-labelledby="skin-current-title">
    <h2 id="skin-current-title" class="col-title">Current</h2>
    <div class="current-preview">
      {#if $authState.loggedIn && $authState.profile}
        <SkinPreview3D
          {skinUrl}
          {capeUrl}
          cachedPath={currentPath}
          {accountKey}
          showName={false}
          width={240}
          height={340}
        />
      {:else}
        <div class="current-empty">Sign in to see your skin</div>
      {/if}
    </div>
    <button
      type="button"
      class="add-lib-btn"
      disabled={!currentPath || alreadyInLibrary || addingCurrent || !$authState.loggedIn}
      onclick={addCurrentToLibrary}
    >
      {alreadyInLibrary ? "Already in library" : "Add to library"}
    </button>
  </section>

  <div class="divider" aria-hidden="true"></div>

  <section class="library-col" aria-labelledby="skin-library-title">
    <h2 id="skin-library-title" class="col-title">Library</h2>
    <div class="library-grid">
      <button type="button" class="lib-tile new-tile" onclick={() => onaddnew?.()}>
        <span class="new-circle" aria-hidden="true">
          <Plus size={28} strokeWidth={2.25} />
        </span>
        <span class="tile-name">New skin</span>
      </button>

      {#each skins as entry (entry.id)}
        {@const open = menuId === entry.id}
        {@const busy = busyId === entry.id}
        <div class="lib-tile skin-tile" class:menu-open={open}>
          <button
            type="button"
            class="tile-hit"
            aria-label={canApplyMojang ? `Apply ${entry.name}` : `${entry.name} options`}
            disabled={busy}
            title={canApplyMojang ? "Apply skin" : "Microsoft account required to apply"}
            onclick={(e) => (canApplyMojang ? void applySkinEntry(entry) : openMenu(entry.id, e))}
          >
            <span class="tile-name">{entry.name}</span>
            <span class="tile-art">
              <SkinFrontPreview
                cachedPath={entry.filePath}
                variant={entry.variant}
                width={70}
                height={140}
              />
            </span>
            {#if busy}
              <span class="tile-progress">Applying…</span>
            {/if}
          </button>

          <button
            type="button"
            class="tile-more"
            class:open
            aria-label="{entry.name} options"
            aria-expanded={open}
            disabled={busy}
            onclick={(e) => openMenu(entry.id, e)}
          >
            <MoreHorizontal size={14} />
          </button>

          {#if open}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div
              class="tile-menu"
              onpointerdown={(e) => e.stopPropagation()}
              onclick={(e) => e.stopPropagation()}
            >
              <div class="menu-head">{entry.name}</div>
              <div class="menu-thumb">
                <SkinFrontPreview
                  cachedPath={entry.filePath}
                  variant={entry.variant}
                  width={48}
                  height={96}
                />
              </div>
              {#if !canApplyMojang}
                <p class="menu-hint">Applying needs a Microsoft account.</p>
              {/if}
              <button type="button" class="menu-btn" disabled={busy} onclick={() => copySkin(entry)}>
                Copy
              </button>
              <button
                type="button"
                class="menu-btn"
                disabled={busy}
                onclick={() => toggleModel(entry)}
                title="Toggle arm model"
              >
                {entry.variant === "slim" ? "Slim" : "Wide"}
              </button>
              <button
                type="button"
                class="menu-btn danger"
                disabled={busy}
                onclick={() => deleteSkin(entry)}
                title="Remove from library"
              >
                <Trash2 size={14} />
                Remove
              </button>
            </div>
          {/if}
        </div>
      {/each}
    </div>
  </section>
</div>

<style>
  .skin-browser {
    display: grid;
    grid-template-columns: minmax(220px, 280px) 1px 1fr;
    gap: 0 20px;
    align-items: stretch;
    min-height: 420px;
    padding: 4px 0 8px;
  }

  @media (max-width: 820px) {
    .skin-browser {
      grid-template-columns: 1fr;
      gap: 16px;
    }

    .divider {
      display: none;
    }

    .current-col {
      max-width: 320px;
      margin: 0 auto;
    }
  }

  .col-title {
    margin: 0 0 14px;
    text-align: center;
    font-size: 15px;
    font-weight: 700;
    color: var(--text-primary);
  }

  .current-col {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 14px;
  }

  .current-preview {
    display: flex;
    align-items: center;
    justify-content: center;
    min-height: 340px;
  }

  .current-empty {
    width: 240px;
    height: 340px;
    display: flex;
    align-items: center;
    justify-content: center;
    text-align: center;
    color: var(--text-muted);
    font-size: 13px;
    border: 1px dashed var(--border-color);
    border-radius: var(--border-radius-md);
  }

  .add-lib-btn {
    width: 100%;
    max-width: 240px;
    padding: 11px 16px;
    border: none;
    border-radius: var(--border-radius-sm);
    background: var(--accent-primary);
    color: var(--on-accent);
    font-size: 13px;
    font-weight: 800;
    cursor: pointer;
  }

  .add-lib-btn:hover:not(:disabled) {
    filter: brightness(1.06);
  }

  .add-lib-btn:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .divider {
    background: color-mix(in srgb, var(--border-color) 80%, transparent);
    width: 1px;
    align-self: stretch;
  }

  .library-col {
    min-width: 0;
    display: flex;
    flex-direction: column;
  }

  .library-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(110px, 1fr));
    gap: 14px 12px;
    align-content: start;
  }

  .lib-tile {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: center;
    min-height: 180px;
  }

  .new-tile {
    justify-content: center;
    gap: 12px;
    padding: 12px 8px;
    border: none;
    border-radius: var(--border-radius-md);
    background: transparent;
    color: var(--text-primary);
    cursor: pointer;
  }

  .new-tile:hover .new-circle {
    border-color: var(--accent-primary);
    color: var(--accent-primary);
  }

  .new-circle {
    width: 72px;
    height: 72px;
    border-radius: 999px;
    border: 2px solid var(--text-secondary);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    color: var(--text-secondary);
  }

  .tile-hit {
    width: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 6px 4px 10px;
    border: none;
    border-radius: var(--border-radius-md);
    background: transparent;
    color: inherit;
    cursor: pointer;
  }

  .tile-hit:hover {
    background: color-mix(in srgb, var(--bg-hover, rgba(255, 255, 255, 0.05)) 80%, transparent);
  }

  .skin-tile.menu-open .tile-hit {
    background: color-mix(in srgb, var(--bg-secondary, #1a1a1e) 90%, transparent);
  }

  .tile-more {
    position: absolute;
    top: 6px;
    right: 6px;
    z-index: 2;
    width: 26px;
    height: 26px;
    padding: 0;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: 999px;
    background: color-mix(in srgb, var(--bg-secondary) 85%, transparent);
    color: var(--text-muted);
    cursor: pointer;
    opacity: 0.6;
    transition:
      opacity var(--motion-fast) var(--ease-out),
      color var(--motion-fast) var(--ease-out),
      background var(--motion-fast) var(--ease-out);
  }

  .skin-tile:hover .tile-more,
  .tile-more.open {
    opacity: 1;
  }

  .tile-more:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .tile-more:disabled {
    opacity: 0.35;
    cursor: default;
  }

  .tile-progress {
    position: absolute;
    bottom: 8px;
    left: 50%;
    transform: translateX(-50%);
    padding: 3px 8px;
    border-radius: 999px;
    background: var(--accent-primary);
    color: var(--on-accent);
    font-size: 12px;
    font-weight: 800;
    white-space: nowrap;
    pointer-events: none;
  }

  .tile-name {
    font-size: 12px;
    font-weight: 700;
    color: var(--text-primary);
    text-align: center;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .tile-art {
    display: flex;
    align-items: flex-end;
    justify-content: center;
    min-height: 140px;
  }

  .tile-menu {
    position: absolute;
    z-index: 5;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: 8px;
    padding: 10px 10px 12px;
    border-radius: var(--border-radius-md);
    background: color-mix(in srgb, var(--bg-primary, #121214) 92%, transparent);
    border: 1px solid var(--border-color);
    box-shadow: 0 12px 28px rgba(0, 0, 0, 0.45);
    backdrop-filter: blur(8px);
  }

  .menu-head {
    font-size: 12px;
    font-weight: 800;
    text-align: center;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .menu-thumb {
    display: flex;
    justify-content: center;
    margin: 2px 0 4px;
  }

  .menu-hint {
    margin: 0;
    padding: 6px 8px;
    border-radius: var(--border-radius-sm);
    background: color-mix(in srgb, var(--badge-offline-bg, rgba(245, 158, 11, 0.12)) 60%, transparent);
    color: var(--badge-offline-fg, #fde68a);
    font-size: 12px;
    line-height: 1.35;
    text-align: center;
  }

  .menu-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    width: 100%;
    padding: 7px 8px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: transparent;
    color: var(--text-primary);
    font-size: 12px;
    font-weight: 700;
    cursor: pointer;
  }

  .menu-btn:hover:not(:disabled) {
    border-color: color-mix(in srgb, var(--text-primary) 40%, transparent);
  }

  .menu-btn.danger {
    color: #f87171;
    border-color: color-mix(in srgb, #ef4444 35%, transparent);
  }

  .menu-btn:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
</style>
