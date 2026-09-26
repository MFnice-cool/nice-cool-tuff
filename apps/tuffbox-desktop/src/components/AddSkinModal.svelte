<script lang="ts">
  import { HelpCircle, X } from "@lucide/svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { api } from "../lib/api";
  import { trapFocus } from "../lib/focusTrap";
  import { upsertSavedSkin, type SkinModelVariant } from "../lib/skinLibrary";
  import {
    authState,
    skinPath,
    type CapeOffer,
    type CapeProvider,
  } from "../lib/store";
  import { toasts } from "../lib/toast";
  import SkinPreview3D from "./SkinPreview3D.svelte";

  let {
    capeOffers = [],
    onclose,
    onsaved,
  }: {
    capeOffers?: CapeOffer[];
    onclose?: () => void;
    onsaved?: (applied: boolean) => void;
  } = $props();

  let name = $state("");
  let variant = $state<SkinModelVariant>("classic");
  let filePath = $state<string | null>(null);
  let selectedCapeKey = $state("none:none");
  let busy = $state(false);
  let brokenCapeIds = $state<Record<string, boolean>>({});
  let fileHintOpen = $state(false);
  let capeHintOpen = $state(false);

  const canApplyMojang = $derived($authState.loginType === "microsoft" && $authState.loggedIn);
  const previewSkinUrl = $derived(filePath ? null : ($authState.profile?.skinUrl ?? null));
  const previewCache = $derived(filePath ?? $skinPath);
  const accountKey = $derived($authState.activeAccountUuid ?? $authState.profile?.uuid ?? "add-skin");

  const noneOffer = $derived<CapeOffer>({
    provider: "none",
    id: "none",
    label: "No cape",
    url: "",
    canActivate: true,
    active: false,
  });

  const capeGrid = $derived<CapeOffer[]>([noneOffer, ...capeOffers.filter((o) => o.provider !== "none")]);

  const selectedCape = $derived(
    capeGrid.find((o) => `${o.provider}:${o.id}` === selectedCapeKey) ?? noneOffer,
  );
  const previewCapeUrl = $derived(selectedCape.provider === "none" ? null : selectedCape.url || null);
  const fileLabel = $derived(filePath ? filePath.split(/[/\\]/).pop() ?? filePath : null);

  function markCapeBroken(id: string) {
    brokenCapeIds = { ...brokenCapeIds, [id]: true };
  }

  function capeSourceLabel(provider: CapeProvider) {
    if (provider === "mojang") return "Mojang";
    if (provider === "tlauncher") return "TL";
    if (provider === "optifine") return "OF";
    return "";
  }

  async function browseSkin() {
    try {
      const selected = await open({
        multiple: false,
        filters: [{ name: "Skin PNG", extensions: ["png"] }],
      });
      if (!selected || Array.isArray(selected)) return;
      filePath = selected;
      if (!name.trim()) {
        const base = selected.split(/[/\\]/).pop()?.replace(/\.png$/i, "") ?? "";
        if (base) name = base;
      }
    } catch (e) {
      toasts.error(String(e));
    }
  }

  function persistLibraryEntry() {
    if (!filePath) {
      toasts.error("Choose a skin PNG file");
      return null;
    }
    return upsertSavedSkin({
      name: name.trim() || "Untitled",
      variant,
      filePath,
      cape:
        selectedCape.provider === "none"
          ? null
          : {
              provider: selectedCape.provider,
              id: selectedCape.id,
              label: selectedCape.label,
              url: selectedCape.url,
            },
    });
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

  async function applyToAccount() {
    if (!filePath) {
      toasts.error("Choose a skin PNG file");
      return false;
    }
    if (!canApplyMojang) {
      toasts.error("Save and apply needs a Microsoft account skin upload");
      return false;
    }
    await applyAuthState(await api.mcAuth.uploadSkinFile(filePath, variant));
    if (selectedCape.provider === "none") {
      await applyAuthState(await api.mcAuth.setCapeProvider("none"));
    } else if (selectedCape.provider === "mojang") {
      await applyAuthState(await api.mcAuth.applyCape(selectedCape.id));
    } else {
      await applyAuthState(await api.mcAuth.setCapeProvider(selectedCape.provider));
    }
    return true;
  }

  async function saveOnly() {
    if (busy) return;
    busy = true;
    try {
      const entry = persistLibraryEntry();
      if (!entry) return;
      toasts.success(`Saved "${entry.name}"`);
      onsaved?.(false);
      onclose?.();
    } catch (e) {
      toasts.error(String(e));
    } finally {
      busy = false;
    }
  }

  async function saveAndApply() {
    if (busy) return;
    busy = true;
    try {
      const entry = persistLibraryEntry();
      if (!entry) return;
      const ok = await applyToAccount();
      if (!ok) return;
      toasts.success(`Applied "${entry.name}"`);
      onsaved?.(true);
      onclose?.();
    } catch (e) {
      toasts.error(String(e));
    } finally {
      busy = false;
    }
  }
</script>

<div
  class="overlay"
  role="presentation"
  onclick={(e) => {
    if (e.target === e.currentTarget && !busy) onclose?.();
  }}
>
  <div
    class="modal"
    role="dialog"
    aria-modal="true"
    aria-labelledby="add-skin-title"
    use:trapFocus={{ onEscape: () => { if (!busy) onclose?.(); } }}
  >
    <header class="modal-head">
      <h2 id="add-skin-title">Add skin</h2>
      <button type="button" class="icon-btn" disabled={busy} onclick={() => onclose?.()} title="Close" aria-label="Close">
        <X size={18} />
      </button>
    </header>

    <div class="modal-body">
      <div class="preview-col">
        <SkinPreview3D
          skinUrl={previewSkinUrl}
          capeUrl={previewCapeUrl}
          cachedPath={previewCache}
          {accountKey}
          model={variant}
          showName={false}
          width={260}
          height={360}
        />
      </div>

      <div class="form-col">
        <label class="field">
          <span class="field-label">Name</span>
          <input
            class="text-input"
            type="text"
            placeholder="Untitled"
            bind:value={name}
            disabled={busy}
          />
        </label>

        <fieldset class="field" disabled={busy}>
          <legend class="field-label">Player model</legend>
          <div class="radio-row" role="radiogroup" aria-label="Player model">
            <label class="radio">
              <input type="radio" name="skin-model" value="classic" bind:group={variant} />
              <span class="radio-dot" aria-hidden="true"></span>
              <span>Wide</span>
            </label>
            <label class="radio">
              <input type="radio" name="skin-model" value="slim" bind:group={variant} />
              <span class="radio-dot" aria-hidden="true"></span>
              <span>Slim</span>
            </label>
          </div>
        </fieldset>

        <div class="field">
          <div class="field-label-row">
            <span class="field-label">Skin file</span>
            <button
              type="button"
              class="help-btn"
              aria-label="Skin file help"
              aria-expanded={fileHintOpen}
              onclick={() => (fileHintOpen = !fileHintOpen)}
            >
              <HelpCircle size={14} />
            </button>
          </div>
          {#if fileHintOpen}
            <p class="hint-pop">PNG skin texture (64×64 or 64×32). HD skins are supported for preview.</p>
          {/if}
          <div class="file-row">
            <button type="button" class="browse-btn" disabled={busy} onclick={browseSkin}>Browse</button>
            {#if fileLabel}
              <span class="file-name" title={filePath ?? undefined}>{fileLabel}</span>
            {/if}
          </div>
        </div>

        <div class="field cape-field">
          <div class="field-label-row">
            <span class="field-label">Cape</span>
            <button
              type="button"
              class="help-btn"
              aria-label="Cape help"
              aria-expanded={capeHintOpen}
              onclick={() => (capeHintOpen = !capeHintOpen)}
            >
              <HelpCircle size={14} />
            </button>
          </div>
          {#if capeHintOpen}
            <p class="hint-pop">Pick a cape for the library entry. Save and apply equips it on the account.</p>
          {/if}

          <div class="cape-grid" role="radiogroup" aria-label="Cape">
            {#each capeGrid as offer (`${offer.provider}:${offer.id}`)}
              {@const key = `${offer.provider}:${offer.id}`}
              {@const isNone = offer.provider === "none"}
              {@const broken = brokenCapeIds[offer.id]}
              {@const on = key === selectedCapeKey}
              <button
                type="button"
                class="cape-tile"
                class:active={on}
                class:none={isNone}
                role="radio"
                aria-checked={on}
                disabled={busy}
                title={offer.label}
                onclick={() => (selectedCapeKey = key)}
              >
                <span class="cape-art">
                  {#if isNone}
                    <span class="cape-none"></span>
                  {:else if offer.url && !broken}
                    <img
                      src={offer.url}
                      alt=""
                      referrerpolicy="no-referrer"
                      draggable="false"
                      onerror={() => markCapeBroken(offer.id)}
                    />
                  {/if}
                  <span class="cape-radio" class:on aria-hidden="true"></span>
                </span>
                <span class="cape-name">{isNone ? "No cape" : offer.label}</span>
                {#if !isNone}
                  <span class="cape-src">{capeSourceLabel(offer.provider)}</span>
                {/if}
              </button>
            {/each}
          </div>
        </div>
      </div>
    </div>

    <footer class="modal-foot">
      <button type="button" class="btn ghost" disabled={busy} onclick={() => onclose?.()}>Cancel</button>
      <button type="button" class="btn secondary" disabled={busy || !filePath} onclick={saveOnly}>Save</button>
      <button
        type="button"
        class="btn primary"
        disabled={busy || !filePath || !canApplyMojang}
        title={canApplyMojang ? "Save to library and upload to Mojang" : "Microsoft account required to apply"}
        onclick={saveAndApply}
      >
        Save and apply
      </button>
    </footer>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    /* Was 80 — below other overlays (AccountManager/Login use 200) and, more
       importantly, could lose to transformed ancestors' contexts. Task:
       'New skin button does nothing'. Align with the rest of the modals. */
    z-index: 200;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
    background: rgba(0, 0, 0, 0.72);
    backdrop-filter: blur(6px);
  }

  .modal {
    width: min(860px, 100%);
    max-height: min(92vh, 720px);
    display: flex;
    flex-direction: column;
    border-radius: var(--border-radius-lg);
    border: 1px solid var(--border-color);
    background: var(--bg-primary, #141416);
    box-shadow: 0 24px 64px rgba(0, 0, 0, 0.55);
    overflow: hidden;
  }

  .modal-head {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 18px 48px 10px;
    flex-shrink: 0;
  }

  .modal-head h2 {
    margin: 0;
    font-size: 18px;
    font-weight: 700;
    color: var(--text-primary);
    letter-spacing: 0.01em;
  }

  .icon-btn {
    position: absolute;
    top: 14px;
    right: 14px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border: none;
    border-radius: var(--border-radius-sm);
    background: transparent;
    color: var(--text-secondary);
    cursor: pointer;
  }

  .icon-btn:hover:not(:disabled) {
    color: var(--text-primary);
    background: var(--bg-hover, rgba(255, 255, 255, 0.06));
  }

  .modal-body {
    display: grid;
    grid-template-columns: 280px 1fr;
    gap: 8px 28px;
    padding: 8px 28px 12px;
    overflow: auto;
    min-height: 0;
  }

  @media (max-width: 720px) {
    .modal-body {
      grid-template-columns: 1fr;
    }

    .preview-col {
      justify-content: center;
    }
  }

  .preview-col {
    display: flex;
    align-items: center;
    justify-content: center;
    min-height: 360px;
  }

  .form-col {
    display: flex;
    flex-direction: column;
    gap: 18px;
    min-width: 0;
    padding-top: 8px;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin: 0;
    padding: 0;
    border: none;
    min-width: 0;
  }

  .field-label,
  legend.field-label {
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--text-muted, #9a9aa3);
    padding: 0;
  }

  .field-label-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .help-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    padding: 0;
    border: none;
    border-radius: 999px;
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
  }

  .help-btn:hover {
    color: var(--text-primary);
  }

  .hint-pop {
    margin: 0;
    font-size: 12px;
    line-height: 1.4;
    color: var(--text-secondary);
  }

  .text-input {
    width: 100%;
    box-sizing: border-box;
    padding: 10px 12px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: transparent;
    color: var(--text-primary);
    font-size: 14px;
    outline: none;
  }

  .text-input::placeholder {
    color: var(--text-muted);
  }

  .text-input:focus {
    border-color: var(--accent-primary);
  }

  .radio-row {
    display: flex;
    flex-wrap: wrap;
    gap: 18px;
  }

  .radio {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 14px;
    color: var(--text-primary);
    cursor: pointer;
    user-select: none;
  }

  .radio input {
    position: absolute;
    opacity: 0;
    pointer-events: none;
  }

  .radio-dot {
    width: 16px;
    height: 16px;
    border-radius: 999px;
    border: 2px solid color-mix(in srgb, var(--text-muted) 70%, transparent);
    box-sizing: border-box;
    position: relative;
  }

  .radio input:checked + .radio-dot {
    border-color: var(--accent-primary);
  }

  .radio input:checked + .radio-dot::after {
    content: "";
    position: absolute;
    inset: 3px;
    border-radius: 999px;
    background: var(--accent-primary);
  }

  .file-row {
    display: flex;
    align-items: center;
    gap: 12px;
    min-width: 0;
  }

  .browse-btn {
    padding: 8px 18px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: transparent;
    color: var(--text-primary);
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
  }

  .browse-btn:hover:not(:disabled) {
    border-color: var(--accent-primary);
  }

  .file-name {
    font-size: 12px;
    color: var(--text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .cape-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(72px, 1fr));
    gap: 10px 8px;
    max-height: 220px;
    overflow: auto;
    padding: 2px;
  }

  .cape-tile {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 0;
    border: none;
    background: transparent;
    color: var(--text-secondary);
    cursor: pointer;
    min-width: 0;
  }

  .cape-tile:hover .cape-art {
    border-color: var(--accent-primary);
  }

  .cape-tile.active {
    color: var(--text-primary);
  }

  .cape-art {
    position: relative;
    width: 56px;
    height: 84px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: #0e0e12;
    overflow: hidden;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .cape-tile.active .cape-art {
    border-color: var(--accent-primary);
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--accent-primary) 40%, transparent);
  }

  .cape-art img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    image-rendering: pixelated;
  }

  .cape-none {
    width: 100%;
    height: 100%;
    background: #1a1a1f;
  }

  .cape-radio {
    position: absolute;
    left: 6px;
    top: 6px;
    width: 12px;
    height: 12px;
    border-radius: 999px;
    border: 2px solid rgba(255, 255, 255, 0.55);
    box-sizing: border-box;
    background: rgba(0, 0, 0, 0.35);
  }

  .cape-radio.on {
    border-color: var(--accent-primary);
    background: radial-gradient(circle at center, var(--accent-primary) 0 45%, transparent 48%);
  }

  .cape-name {
    font-size: 12px;
    font-weight: 600;
    text-align: center;
    line-height: 1.2;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .cape-src {
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--text-muted);
  }

  .modal-foot {
    display: flex;
    justify-content: flex-end;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
    padding: 14px 24px 20px;
    flex-shrink: 0;
  }

  .btn {
    padding: 9px 16px;
    border-radius: var(--border-radius-sm);
    font-size: 13px;
    font-weight: 700;
    cursor: pointer;
  }

  .btn:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .btn.ghost {
    border: none;
    background: transparent;
    color: var(--text-secondary);
  }

  .btn.ghost:hover:not(:disabled) {
    color: var(--text-primary);
  }

  .btn.secondary {
    border: 1px solid var(--border-color);
    background: color-mix(in srgb, var(--bg-secondary, #1c1c20) 90%, transparent);
    color: var(--text-primary);
  }

  .btn.secondary:hover:not(:disabled) {
    border-color: color-mix(in srgb, var(--text-primary) 35%, transparent);
  }

  .btn.primary {
    border: 1px solid transparent;
    background: var(--accent-primary);
    color: var(--on-accent);
  }

  .btn.primary:hover:not(:disabled) {
    filter: brightness(1.06);
  }
</style>
