<script lang="ts">
  import { onMount } from "svelte";
  import {
    User,
    LogIn,
    LogOut,
    Clock,
    Plus,
    ArrowLeftRight,
    Trash2,
    Globe,
    Monitor,
    ArrowLeft,
    Upload,
    Link2,
    Sparkles,
    Library,
  } from "@lucide/svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { api } from "../lib/api";
  import {
    authState,
    skinPath,
    projectPath,
    loginTypeLabel,
    formatPlaytime,
    type CapeProvider,
    type CapeCatalog,
    type CapeOffer,
  } from "../lib/store";
  import { listSavedSkins } from "../lib/skinLibrary";
  import { toasts } from "../lib/toast";
  import SkinPreview3D from "./SkinPreview3D.svelte";
  import AccountManager from "./AccountManager.svelte";
  import MinecraftLogin from "./MinecraftLogin.svelte";
  import SkinLibraryBrowser from "./SkinLibraryBrowser.svelte";
  import AddSkinModal from "./AddSkinModal.svelte";

  let { onBack = () => {} }: { onBack?: () => void } = $props();

  let showAccountManager = $state(false);
  let showLogin = $state(false);
  let playtimeSeconds = $state(0);
  let busy = $state(false);
  let capeCatalog = $state<CapeCatalog | null>(null);
  let capeLoading = $state(false);
  let capeTab = $state<"all" | "mojang" | "tlauncher" | "optifine">("all");
  let applyingCapeId = $state<string | null>(null);
  let brokenCapeIds = $state<Record<string, boolean>>({});

  let skinUrlInput = $state("");
  let skinVariant = $state<"classic" | "slim">("classic");
  let skinBusy = $state(false);
  let showSecondLayer = $state(true);
  let showAddSkin = $state(false);
  let skinLibRevision = $state(0);

  const savedSkinCount = $derived.by(() => {
    void skinLibRevision;
    return listSavedSkins().length;
  });

  const skinUrl = $derived($authState.profile?.skinUrl ?? null);
  const capeUrl = $derived($authState.profile?.capeUrl ?? null);
  const accountKey = $derived($authState.activeAccountUuid ?? $authState.profile?.uuid ?? "");
  const activeAuthority = $derived(
    $authState.accounts.find((a) => a.uuid === $authState.activeAccountUuid)?.authority ?? null,
  );
  const mojangCapeOffers = $derived((capeCatalog?.offers ?? []).filter((o) => o.provider === "mojang"));
  const tlauncherCapeOffers = $derived(
    (capeCatalog?.offers ?? []).filter((o) => o.provider === "tlauncher"),
  );
  const optifineCapeOffers = $derived(
    (capeCatalog?.offers ?? []).filter((o) => o.provider === "optifine"),
  );
  const selectedCapeProvider = $derived($authState.capeProvider ?? "mojang");
  const displayedCapeKey = $derived.by(() => {
    if (selectedCapeProvider === "none") return "none:none";
    if (selectedCapeProvider !== "mojang") {
      // Backend offers carry the provider key as their id for non-Mojang
      // capes (e.g. "tlauncher:tlauncher"), so the provider key matches.
      return `${selectedCapeProvider}:${selectedCapeProvider}`;
    }
    // Only mark a Mojang cape as equipped when the backend reports it as
    // ACTIVE — never fall back to the first offer (that would show an
    // unequipped cape with an "On" badge).
    const active = mojangCapeOffers.find((o) => o.active);
    return active ? `mojang:${active.id}` : "none:none";
  });
  const noneOffer = $derived<CapeOffer>({
    provider: "none",
    id: "none",
    label: "No cape",
    url: "",
    canActivate: true,
    active: selectedCapeProvider === "none",
  });
  const visibleCapeOffers = $derived.by(() => {
    const offers =
      capeTab === "mojang"
        ? mojangCapeOffers
        : capeTab === "tlauncher"
          ? tlauncherCapeOffers
          : capeTab === "optifine"
            ? optifineCapeOffers
            : (capeCatalog?.offers ?? []).filter((o) => o.provider !== "none");
    return [noneOffer, ...offers];
  });
  const canChangeMojangSkin = $derived($authState.loginType === "microsoft" && $authState.loggedIn);

  const capeTabs: { id: typeof capeTab; label: string }[] = [
    { id: "all", label: "All" },
    { id: "mojang", label: "Mojang" },
    { id: "tlauncher", label: "TLauncher" },
    { id: "optifine", label: "OptiFine" },
  ];

  async function applyAuthState(state: Awaited<ReturnType<typeof api.mcAuth.getAuthStatus>>) {
    authState.set(state);
    if (state.profile?.uuid) {
      try {
        skinPath.set(await api.mcAuth.getSkinPath(state.profile.uuid));
      } catch {
        skinPath.set(null);
      }
    } else {
      skinPath.set(null);
    }
  }

  async function refreshAuth() {
    try {
      await applyAuthState(await api.mcAuth.getAuthStatus());
    } catch {}
  }

  async function refreshPlaytime() {
    const path = $projectPath;
    if (!path) {
      playtimeSeconds = 0;
      return;
    }
    try {
      const stats = await api.stats.get(path);
      playtimeSeconds = stats.totalPlaytimeSeconds ?? 0;
    } catch {
      playtimeSeconds = 0;
    }
  }

  function defaultCapeTab(loginType: string): typeof capeTab {
    return loginType === "microsoft" ? "mojang" : "tlauncher";
  }

  async function refreshCapes() {
    // Rust resolves the active profile itself, so don't gate on the (possibly
    // stale) store snapshot — this lets capes load in parallel with auth.
    capeLoading = true;
    try {
      capeCatalog = await api.mcAuth.listCapes();
    } catch {
      capeCatalog = null;
    } finally {
      capeLoading = false;
    }
  }

  async function switchAccount(uuid: string) {
    if (uuid === $authState.activeAccountUuid) return;
    busy = true;
    try {
      await applyAuthState(await api.mcAuth.switchAccount(uuid));
      capeTab = defaultCapeTab($authState.loginType);
      await refreshCapes();
      toasts.success(`Switched to ${$authState.profile?.name ?? "account"}`);
    } catch (e) {
      toasts.error(String(e));
    } finally {
      busy = false;
    }
  }

  async function removeAccount(uuid: string) {
    busy = true;
    try {
      await applyAuthState(await api.mcAuth.removeAccount(uuid));
      await refreshCapes();
      toasts.info("Account removed");
    } catch (e) {
      toasts.error(String(e));
    } finally {
      busy = false;
    }
  }

  async function logout() {
    busy = true;
    try {
      await applyAuthState(await api.mcAuth.logout());
      await refreshCapes();
      toasts.info("Signed out");
    } catch (e) {
      toasts.error(String(e));
    } finally {
      busy = false;
    }
  }

  async function selectCape(offer: CapeOffer) {
    if (applyingCapeId) return;
    if (`${offer.provider}:${offer.id}` === displayedCapeKey) return;
    applyingCapeId = offer.id;
    try {
      if (offer.provider === "none") {
        await applyAuthState(await api.mcAuth.setCapeProvider("none"));
      } else if (offer.canActivate && offer.provider === "mojang") {
        await applyAuthState(await api.mcAuth.applyCape(offer.id));
      } else {
        await applyAuthState(await api.mcAuth.setCapeProvider(offer.provider));
      }
      await refreshCapes();
      toasts.success(offer.provider === "none" ? "Cape hidden" : `${offer.label} equipped`);
    } catch (e) {
      toasts.error(String(e));
    } finally {
      applyingCapeId = null;
    }
  }

  function capeSourceLabel(provider: CapeProvider) {
    if (provider === "tlauncher") return "TLauncher";
    if (provider === "optifine") return "OptiFine";
    if (provider === "mojang") return "Mojang";
    return "Off";
  }

  function markCapeBroken(id: string) {
    brokenCapeIds = { ...brokenCapeIds, [id]: true };
  }

  async function applySkinFromUrl() {
    const url = skinUrlInput.trim();
    if (!url) {
      toasts.error("Enter a skin PNG URL");
      return;
    }
    skinBusy = true;
    try {
      await applyAuthState(await api.mcAuth.applySkin(url, skinVariant));
      toasts.success("Skin updated");
      skinUrlInput = "";
    } catch (e) {
      toasts.error(String(e));
    } finally {
      skinBusy = false;
    }
  }

  async function uploadSkinFile() {
    skinBusy = true;
    try {
      const selected = await open({
        multiple: false,
        filters: [{ name: "Skin PNG", extensions: ["png"] }],
      });
      if (!selected || Array.isArray(selected)) {
        skinBusy = false;
        return;
      }
      await applyAuthState(await api.mcAuth.uploadSkinFile(selected, skinVariant));
      toasts.success("Skin uploaded");
    } catch (e) {
      toasts.error(String(e));
    } finally {
      skinBusy = false;
    }
  }

  onMount(() => {
    void (async () => {
      capeTab = defaultCapeTab($authState.loginType);
      void refreshPlaytime();
      // Auth and capes are independent (Rust reads the profile from disk) —
      // run them concurrently instead of serially.
      await Promise.allSettled([refreshAuth(), refreshCapes()]);
    })();
  });
</script>

<div class="me-page">
  <div class="me-top">
    <button class="back-btn" onclick={onBack} title="Back">
      <ArrowLeft size={18} />
      <span>Back</span>
    </button>
    <h1 class="me-title">Me</h1>
    <div class="me-top-actions">
      {#if $authState.loggedIn}
        <button class="ghost-btn danger" disabled={busy} onclick={logout} title="Sign out">
          <LogOut size={16} />
          Sign out
        </button>
      {/if}
      <button class="ghost-btn" onclick={() => (showLogin = true)}>
        <Plus size={16} />
        Add account
      </button>
    </div>
  </div>

  <div class="me-hero">
    <div class="skin-col">
      {#if $authState.loggedIn && $authState.profile}
        <SkinPreview3D
          {skinUrl}
          {capeUrl}
          cachedPath={$skinPath}
          {accountKey}
          playerName={$authState.profile.name}
          showName={false}
          {showSecondLayer}
          width={240}
          height={330}
        />
        <div class="player-name mc-font">{$authState.profile.name}</div>
        <div class="skin-meta-row">
          <span
            class="type-badge"
            class:microsoft={$authState.loginType === "microsoft"}
            class:offline={$authState.loginType === "offline"}
            class:ygg={$authState.loginType === "yggdrasil"}
          >
            {loginTypeLabel($authState.loginType, activeAuthority)}
          </span>
          <label class="layer-toggle" title="Show the skin overlay (hat, jacket, sleeves, pants)">
            <input type="checkbox" bind:checked={showSecondLayer} />
            <span>2nd layer</span>
          </label>
        </div>
      {:else}
        <div class="skin-empty">
          <User size={48} />
          <p>Not signed in</p>
          <button class="accent-btn" onclick={() => (showLogin = true)}>
            <LogIn size={16} /> Sign in
          </button>
        </div>
      {/if}
    </div>

    <div class="info-col">
      <section class="card">
        <div class="card-head">
          <Clock size={16} />
          <h3>Playtime</h3>
        </div>
        {#if $projectPath}
          <div class="playtime-value">{formatPlaytime(playtimeSeconds)}</div>
          <p class="hint">Total time in this instance (all sessions).</p>
        {:else}
          <p class="hint">Open an instance to track playtime.</p>
        {/if}
      </section>

      {#if canChangeMojangSkin}
        <section class="card">
          <div class="card-head">
            <Upload size={16} />
            <h3>Change skin</h3>
          </div>
          <div class="skin-form">
            <div class="variant-row">
              <button
                class="chip"
                class:active={skinVariant === "classic"}
                onclick={() => (skinVariant = "classic")}
              >Classic</button>
              <button
                class="chip"
                class:active={skinVariant === "slim"}
                onclick={() => (skinVariant = "slim")}
              >Slim</button>
            </div>
            <div class="url-row">
              <input
                class="skin-input"
                type="url"
                placeholder="https://…/skin.png"
                bind:value={skinUrlInput}
                disabled={skinBusy}
              />
              <button class="mini" disabled={skinBusy} onclick={applySkinFromUrl} title="Apply URL">
                <Link2 size={14} />
              </button>
            </div>
            <button class="accent-btn wide" disabled={skinBusy} onclick={uploadSkinFile}>
              <Upload size={16} />
              Upload PNG
            </button>
            <p class="hint">Launcher + Mojang account skin. Wings/FX — in-game (Right Shift).</p>
          </div>
        </section>
      {/if}

      <section class="card cape-card">
        <div class="card-head">
          <Sparkles size={16} />
          <h3>Capes</h3>
        </div>
        <p class="hint cape-hint">
          Browse Mojang, TLauncher, and OptiFine capes. Click a tile to preview it on the skin.
          Cracked / offline nicknames use TLauncher &amp; OptiFine.
        </p>
        {#if !$authState.loggedIn}
          <p class="hint">Sign in to browse capes for this account.</p>
        {:else}
          <div class="cape-tabs" role="tablist" aria-label="Cape source">
            {#each capeTabs as tab (tab.id)}
              <button
                type="button"
                class="chip"
                class:active={capeTab === tab.id}
                role="tab"
                aria-selected={capeTab === tab.id}
                onclick={() => (capeTab = tab.id)}
              >
                {tab.label}
                {#if tab.id === "mojang" && mojangCapeOffers.length}
                  <span class="tab-count">{mojangCapeOffers.length}</span>
                {:else if tab.id === "tlauncher" && tlauncherCapeOffers.length}
                  <span class="tab-count">{tlauncherCapeOffers.length}</span>
                {:else if tab.id === "optifine" && optifineCapeOffers.length}
                  <span class="tab-count">{optifineCapeOffers.length}</span>
                {/if}
              </button>
            {/each}
          </div>

          {#if capeLoading && !capeCatalog}
            <div class="cape-grid" aria-busy="true">
              {#each Array(6) as _, i (i)}
                <div class="cape-tile skel"></div>
              {/each}
            </div>
          {:else}
            <div class="cape-grid">
              {#each visibleCapeOffers as offer (offer.provider + offer.id)}
                {@const isNone = offer.provider === "none"}
                {@const broken = brokenCapeIds[offer.id]}
                {@const isOn = `${offer.provider}:${offer.id}` === displayedCapeKey}
                <button
                  type="button"
                  class="cape-tile"
                  class:active={isOn}
                  class:none={isNone}
                  disabled={!!applyingCapeId || isOn}
                  title={isOn ? `${offer.label} (active)` : `Equip ${offer.label}`}
                  onclick={() => void selectCape(offer)}
                >
                  <span class="cape-tile-art">
                    {#if isNone}
                      <span class="cape-none-mark">×</span>
                    {:else if offer.url && !broken}
                      <img
                        src={offer.url}
                        alt=""
                        referrerpolicy="no-referrer"
                        draggable="false"
                        loading="lazy"
                        decoding="async"
                        onerror={() => markCapeBroken(offer.id)}
                      />
                    {:else}
                      <Sparkles size={18} />
                    {/if}
                  </span>
                  <span class="cape-tile-label">{isNone ? "None" : offer.label}</span>
                  <span class="cape-tile-src">{capeSourceLabel(offer.provider)}</span>
                  {#if isOn}
                    <span class="cape-tile-badge">On</span>
                  {/if}
                </button>
              {/each}
            </div>
            {#if capeTab === "mojang" && mojangCapeOffers.length === 0}
              <p class="hint">No Mojang capes on this Microsoft account. Migrator / Minecon / personal capes show up here after you own them.</p>
            {:else if capeTab === "tlauncher" && tlauncherCapeOffers.length === 0}
              <p class="hint">No TLauncher cape for this nickname. Upload or equip one on tlauncher.org, then refresh Me.</p>
            {:else if capeTab === "optifine" && optifineCapeOffers.length === 0}
              <p class="hint">No OptiFine cape for this nickname.</p>
            {/if}
          {/if}
        {/if}
      </section>

      <section class="card">
        <div class="card-head">
          <User size={16} />
          <h3>Accounts</h3>
          <button class="ghost-icon" title="Add account" onclick={() => (showLogin = true)}>
            <Plus size={16} />
          </button>
          <button class="ghost-icon" title="Manage accounts" onclick={() => (showAccountManager = true)}>
            <ArrowLeftRight size={16} />
          </button>
        </div>

        {#if $authState.accounts.length === 0}
          <p class="hint">No saved accounts. Sign in with Microsoft, Offline, Ely.by, LittleSkin, or custom Yggdrasil.</p>
          <button class="accent-btn" onclick={() => (showLogin = true)}>
            <LogIn size={16} /> Add account
          </button>
        {:else}
          <div class="account-list">
            {#each $authState.accounts as account (account.uuid)}
              <div class="account-item" class:active={account.uuid === $authState.activeAccountUuid}>
                <button
                  class="account-main"
                  disabled={busy || account.uuid === $authState.activeAccountUuid}
                  onclick={() => switchAccount(account.uuid)}
                  title={account.uuid === $authState.activeAccountUuid ? "Active" : "Switch"}
                >
                  <div
                    class="account-ico"
                    class:ms={account.loginType === "microsoft"}
                    class:off={account.loginType === "offline"}
                    class:ygg={account.loginType === "yggdrasil"}
                  >
                    {#if account.loginType === "microsoft"}
                      <Globe size={14} />
                    {:else if account.loginType === "yggdrasil"}
                      <Monitor size={14} />
                    {:else}
                      <User size={14} />
                    {/if}
                  </div>
                  <div class="account-text">
                    <span class="mc-font name">{account.name}</span>
                    <span class="meta">{loginTypeLabel(account.loginType, account.authority)}</span>
                  </div>
                </button>
                <div class="actions">
                  {#if account.uuid !== $authState.activeAccountUuid}
                    <button
                      class="ghost-icon"
                      title="Switch"
                      disabled={busy}
                      onclick={() => switchAccount(account.uuid)}
                    >
                      <ArrowLeftRight size={14} />
                    </button>
                  {/if}
                  <button
                    class="ghost-icon danger"
                    title="Remove"
                    disabled={busy}
                    onclick={() => removeAccount(account.uuid)}
                  >
                    <Trash2 size={14} />
                  </button>
                </div>
              </div>
            {/each}
          </div>
        {/if}
      </section>
    </div>
  </div>

  <section class="card skin-lib-card">
    <div class="card-head">
      <Library size={16} />
      <h3>Skin library</h3>
      {#if savedSkinCount > 0}
        <span class="tab-count">{savedSkinCount}</span>
      {/if}
      <button class="ghost-btn lib-add-btn" onclick={() => (showAddSkin = true)}>
        <Plus size={14} />
        Add skin
      </button>
    </div>
    <SkinLibraryBrowser
      capeCatalog={capeCatalog}
      revision={skinLibRevision}
      onaddnew={() => (showAddSkin = true)}
      onapplied={() => {
        void refreshPlaytime();
        void refreshCapes();
      }}
      onlibrarychange={() => skinLibRevision++}
    />
  </section>
</div>

{#if showLogin}
  <MinecraftLogin
    onclose={() => {
      showLogin = false;
      void refreshAuth();
      void refreshCapes();
    }}
  />
{/if}
{#if showAccountManager}
  <AccountManager
    onclose={() => {
      showAccountManager = false;
      void refreshAuth();
      void refreshCapes();
    }}
  />
{/if}
{#if showAddSkin}
  <AddSkinModal
    capeOffers={capeCatalog?.offers ?? []}
    onclose={() => (showAddSkin = false)}
    onsaved={() => {
      skinLibRevision++;
      void refreshAuth();
      void refreshCapes();
    }}
  />
{/if}

<style>
  .me-page {
    /* Responsive content cap: full width on laptops, centered column on
       1080p/1440p instead of stretching edge-to-edge. */
    max-width: 1320px;
    margin: 0 auto;
    padding: 4px 8px 24px;
    width: 100%;
  }

  .me-top {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 12px;
  }

  .back-btn {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    border-radius: var(--border-radius-md);
    border: 1px solid var(--border-color);
    background: var(--bg-secondary);
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }

  .back-btn:hover {
    color: var(--text-primary);
    border-color: var(--accent-primary);
  }

  .me-title {
    flex: 1;
    margin: 0;
    font-size: 17px;
    font-weight: 800;
    color: var(--text-primary);
  }

  .me-top-actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }

  .ghost-btn {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    border-radius: var(--border-radius-md);
    border: 1px solid var(--border-color);
    background: transparent;
    color: var(--text-secondary);
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
  }

  .ghost-btn:hover {
    color: var(--text-primary);
    background: var(--bg-hover);
  }

  .ghost-btn.danger:hover {
    color: var(--accent-danger);
    border-color: color-mix(in srgb, var(--accent-danger) 35%, transparent);
    background: color-mix(in srgb, var(--accent-danger) 8%, transparent);
  }

  .me-hero {
    display: grid;
    grid-template-columns: 260px 1fr;
    gap: 18px;
    align-items: start;
  }

  @media (max-width: 820px) {
    .me-hero {
      grid-template-columns: 1fr;
    }
  }

  .skin-col {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
  }

  .player-name {
    font-family: var(--font-minecraft);
    font-size: 12px;
    letter-spacing: 0.5px;
    color: var(--mc-nick-color, var(--text-primary));
    text-shadow: var(--mc-nick-shadow-soft, 1px 1px 0 #3f3f3f);
  }

  /* Badge + layer toggle share one compact pill row under the player name. */
  .skin-meta-row {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 4px;
    background: color-mix(in srgb, var(--bg-tertiary) 72%, transparent);
    border: 1px solid var(--border-color);
    border-radius: 999px;
  }

  .skin-meta-row .type-badge {
    padding: 2px 7px;
  }

  .skin-meta-row .layer-toggle {
    margin-top: 0;
    padding: 2px 8px;
    border: none;
    background: transparent;
  }

  .skin-empty {
    width: 240px;
    height: 330px;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    background: var(--bg-secondary);
    border: 1px dashed var(--border-color);
    border-radius: var(--border-radius-lg);
    color: var(--text-muted);
  }

  .info-col {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .card {
    background: var(--bg-secondary);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-lg);
    padding: 12px 14px;
  }

  .card-head {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 10px;
    color: var(--text-secondary);
  }

  .card-head h3 {
    flex: 1;
    font-size: 13px;
    font-weight: 700;
    color: var(--text-primary);
    margin: 0;
  }

  .playtime-value {
    font-size: 22px;
    font-weight: 800;
    color: var(--accent-primary);
    letter-spacing: -0.5px;
  }

  .hint {
    font-size: 12px;
    color: var(--text-muted);
    margin: 5px 0 0;
  }

  .cape-hint {
    margin: 0 0 10px;
  }

  .cape-tabs {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-bottom: 10px;
  }

  .tab-count {
    display: inline-flex;
    min-width: 15px;
    height: 15px;
    padding: 0 5px;
    margin-left: 4px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent-primary) 18%, transparent);
    color: var(--accent-primary);
    font-size: 12px;
    font-weight: 800;
    align-items: center;
    justify-content: center;
  }

  .cape-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(118px, 1fr));
    gap: 8px;
  }

  .cape-tile {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 8px 6px 10px;
    border-radius: var(--border-radius-md);
    border: 1px solid var(--border-color);
    background: var(--bg-primary);
    color: var(--text-secondary);
    cursor: pointer;
    min-height: 150px;
    transition:
      border-color var(--motion-fast) var(--ease-out),
      background var(--motion-fast) var(--ease-out);
  }

  .cape-tile:hover:not(:disabled) {
    border-color: var(--accent-primary);
    color: var(--text-primary);
    background: color-mix(in srgb, var(--accent-primary) 8%, transparent);
    transform: none;
  }

  .cape-tile.active {
    border-color: var(--accent-primary);
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--accent-primary) 45%, transparent);
  }

  .cape-tile:disabled {
    cursor: default;
  }

  .cape-tile.skel {
    min-height: 150px;
    background: var(--bg-primary);
    opacity: 0.55;
    pointer-events: none;
  }

  .cape-tile-art {
    width: 76px;
    height: 94px;
    border-radius: var(--border-radius-sm);
    background: var(--bg-elevated);
    border: 1px solid color-mix(in srgb, var(--border-color) 70%, transparent);
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
    color: var(--text-muted);
  }

  .cape-tile-art img {
    width: 100%;
    height: 100%;
    object-fit: contain;
    image-rendering: pixelated;
  }

  .cape-none-mark {
    font-size: 28px;
    font-weight: 300;
    line-height: 1;
    color: var(--text-muted);
  }

  .cape-tile-label {
    font-size: 12px;
    font-weight: 700;
    text-align: center;
    line-height: 1.2;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .cape-tile-src {
    font-size: 12px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.4px;
    color: var(--text-muted);
  }

  .cape-tile-badge {
    position: absolute;
    top: 6px;
    right: 6px;
    font-size: 12px;
    font-weight: 800;
    text-transform: uppercase;
    padding: 1px 5px;
    border-radius: 4px;
    background: var(--accent-primary);
    color: var(--on-accent, #000);
  }

  .skin-lib-card {
    margin-top: 12px;
    padding-bottom: 16px;
  }

  .lib-add-btn {
    padding: 6px 10px;
    font-size: 12px;
  }

  .skin-form {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .variant-row {
    display: flex;
    gap: 8px;
  }

  .url-row {
    display: flex;
    gap: 8px;
  }

  .skin-input {
    flex: 1;
    min-width: 0;
    padding: 7px 9px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: var(--bg-primary);
    color: var(--text-primary);
    font-size: 12px;
  }

  .accent-btn.wide {
    justify-content: center;
    width: 100%;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    padding: 4px 9px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: var(--bg-primary);
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }

  .chip.active {
    border-color: var(--accent-primary);
    color: var(--accent-primary);
    background: color-mix(in srgb, var(--accent-primary) 8%, transparent);
  }

  .chip:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .mini {
    padding: 4px 8px;
    font-size: 12px;
    font-weight: 700;
    border-radius: 6px;
    border: none;
    background: var(--accent-primary);
    color: var(--on-accent, #000);
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }

  .mini:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .account-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .account-item {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 6px 4px 4px;
    border-radius: var(--border-radius-md);
    background: var(--bg-primary);
    border: 1px solid var(--border-color);
  }

  .account-item.active {
    border-color: var(--accent-primary);
    background: color-mix(in srgb, var(--accent-primary) 4%, transparent);
  }

  .account-main {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 8px;
    border: none;
    border-radius: var(--border-radius-sm);
    background: transparent;
    color: inherit;
    cursor: pointer;
    text-align: left;
  }

  .account-main:disabled {
    cursor: default;
  }

  .account-main:not(:disabled):hover {
    background: var(--bg-hover);
  }

  .account-ico {
    width: 32px;
    height: 32px;
    border-radius: var(--border-radius-sm);
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-elevated);
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .account-ico.ms {
    background: linear-gradient(135deg, #0078d4, #00a4ef);
    color: #fff;
  }

  .account-ico.off {
    border: 1px solid var(--badge-offline-border, rgba(245, 158, 11, 0.35));
    color: var(--badge-offline-fg, #fde68a);
  }

  .account-ico.ygg {
    background: var(--badge-ygg-bg, rgba(168, 85, 247, 0.18));
    color: var(--badge-ygg-fg, #e9d5ff);
  }

  .account-text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .account-text .name {
    font-family: var(--font-minecraft);
    font-size: 12px;
    color: var(--mc-nick-color, var(--text-primary));
    text-shadow: var(--mc-nick-shadow-soft, 1px 1px 0 #3f3f3f);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .account-text .meta {
    font-size: 12px;
    color: var(--text-muted);
  }

  .actions {
    display: flex;
    gap: 8px;
  }

  .ghost-icon {
    width: 28px;
    height: 28px;
    min-width: 28px;
    min-height: 28px;
    padding: 0 !important;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: none !important;
    border-radius: 6px;
    background: transparent !important;
    color: var(--text-muted) !important;
    box-shadow: none !important;
    cursor: pointer;
    flex-shrink: 0;
    transform: none !important;
  }

  .ghost-icon :global(svg) {
    width: 14px;
    height: 14px;
    stroke: currentColor;
    flex-shrink: 0;
  }

  .ghost-icon:hover {
    background: var(--bg-hover) !important;
    color: var(--text-primary) !important;
    transform: none !important;
  }

  .ghost-icon.danger:hover {
    background: color-mix(in srgb, var(--accent-danger) 12%, transparent) !important;
    color: var(--accent-danger) !important;
  }

  .back-btn {
    padding: 8px 12px !important;
    background: var(--bg-secondary) !important;
    color: var(--text-secondary) !important;
  }

  .back-btn:hover {
    transform: none !important;
    background: var(--bg-hover) !important;
  }

  .type-badge {
    font-size: 12px;
    font-weight: 800;
    text-transform: uppercase;
    padding: 2px 8px;
    border-radius: 4px;
    letter-spacing: 0.4px;
  }

  .type-badge.microsoft {
    color: var(--badge-ms-fg, #93c5fd);
    background: var(--badge-ms-bg, rgba(59, 130, 246, 0.15));
  }

  .type-badge.offline {
    color: var(--badge-offline-fg, #fde68a);
    background: var(--badge-offline-bg, rgba(245, 158, 11, 0.12));
  }

  .type-badge.ygg {
    color: var(--badge-ygg-fg, #e9d5ff);
    background: var(--badge-ygg-bg, rgba(168, 85, 247, 0.15));
  }

  .layer-toggle {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    margin-top: 4px;
    padding: 4px 8px;
    border-radius: var(--border-radius-sm);
    border: 1px solid var(--border-color);
    background: var(--bg-tertiary);
    color: var(--text-muted);
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    user-select: none;
    transition:
      color var(--motion-fast) var(--ease-out),
      border-color var(--motion-fast) var(--ease-out),
      background var(--motion-fast) var(--ease-out);
  }
  .layer-toggle:hover {
    color: var(--text-primary);
    border-color: color-mix(in srgb, var(--accent-primary) 35%, transparent);
  }
  .layer-toggle input {
    accent-color: var(--accent-primary);
    width: 14px;
    height: 14px;
    margin: 0;
    cursor: pointer;
  }

  .accent-btn {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    border-radius: var(--border-radius-md);
    background: var(--accent-primary);
    color: var(--on-accent, #000);
    border: none;
    font-size: 13px;
    font-weight: 700;
    cursor: pointer;
  }

  .accent-btn:hover {
    background: var(--accent-hover);
  }

  .accent-btn:disabled {
    opacity: 0.55;
    cursor: not-allowed;
  }

  .mc-font {
    font-family: var(--font-minecraft);
  }
</style>
