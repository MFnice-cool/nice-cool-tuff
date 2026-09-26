<!-- Per-instance content panel hosted in the Library side rail (selected pack).

Moved out of the home dashboard's InstanceHome: same mods/packs/shaders
coverage minus Worlds (worlds live in the World view), with a servers-first
focus — online status and player counts. The host owns the surrounding
collapse; this component always renders expanded. -->
<script lang="ts">
  import {
    Package,
    Image,
    Sparkles,
    Server,
    RefreshCw,
    Plus,
    Trash2,
    Power,
    FolderOpen,
    ExternalLink,
    Play,
    Search,
    SlidersHorizontal,
    Users,
  } from "@lucide/svelte";
  import { api, type ContentPackEntry, type McServerEntry } from "../lib/api";
  import {
    libraryTabRequest,
    isProjectLaunching,
    isProjectRunning,
    launchSessions,
    runningInstances,
  } from "../lib/store";
  import { toasts } from "../lib/toast";
  import { get } from "svelte/store";
  import { t } from "../lib/i18n";
  import { launchWithFeedback } from "../lib/launch";
  import { openModsBrowserWindow, type BrowserContentType } from "../lib/modsBrowserWindow";

  type Tab = "mods" | "resourcepacks" | "shaderpacks" | "servers";
  type PingResult = {
    address: string;
    online: boolean;
    latencyMs: number | null;
    error: string | null;
    playersOnline?: number | null;
    playersMax?: number | null;
  };

  const {
    projectPath,
    onOpenMods,
    onManage,
  }: {
    projectPath: string;
    onOpenMods: () => void;
    /** Opens the Prism-style instance manager (mods / backups / health). */
    onManage?: () => void;
  } = $props();

  const TAB_KEY = "tuffbox-instance-home-tab";

  function readInitialTab(): Tab {
    try {
      const raw = typeof localStorage === "undefined" ? null : localStorage.getItem(TAB_KEY);
      // "worlds" no longer exists here — fall through to the servers-first default.
      if (raw === "mods" || raw === "resourcepacks" || raw === "shaderpacks" || raw === "servers") {
        return raw;
      }
    } catch {
      /* ignore */
    }
    return "servers";
  }

  let tab = $state<Tab>(readInitialTab());

  $effect(() => {
    try {
      localStorage.setItem(TAB_KEY, tab);
    } catch {
      /* ignore */
    }
  });
  let loading = $state(false);
  let resourcePacks = $state<ContentPackEntry[]>([]);
  let shaderPacks = $state<ContentPackEntry[]>([]);
  let servers = $state<McServerEntry[]>([]);
  let pings = $state<Record<string, PingResult>>({});
  let modCount = $state<number | null>(null);
  let busyKey = $state<string | null>(null);
  /** Tabs that already have a successful load — avoid skeleton/collapse on revisit. */
  let primed = $state<Partial<Record<Tab, boolean>>>({});
  let loadGen = 0;

  let newServerName = $state("");
  let newServerAddress = $state("");

  const packs = $derived(tab === "shaderpacks" ? shaderPacks : resourcePacks);
  const projectLaunching = $derived(isProjectLaunching(projectPath, $launchSessions));
  const projectRunning = $derived(isProjectRunning(projectPath, $runningInstances));
  const tabPrimed = $derived(!!primed[tab]);
  const showSkeleton = $derived(
    loading &&
      !tabPrimed &&
      tab !== "mods" &&
      (tab === "resourcepacks" || tab === "shaderpacks"
        ? packs.length === 0
        : servers.length === 0),
  );
  /** Online-first, unknown last, then by name — the servers tab is a status board. */
  const sortedServers = $derived(
    [...servers].sort((a, b) => {
      const pa = pings[a.address];
      const pb = pings[b.address];
      const rank = (p: PingResult | undefined) => (p ? (p.online ? 0 : 1) : 2);
      return rank(pa) - rank(pb) || a.name.localeCompare(b.name);
    }),
  );
  const serversPinged = $derived(servers.filter((s) => pings[s.address]).length);
  const serversOnline = $derived(servers.filter((s) => pings[s.address]?.online).length);

  async function load(opts?: { force?: boolean }) {
    if (!projectPath) return;
    const t = tab;
    const force = !!opts?.force;
    const first = !primed[t];
    const gen = ++loadGen;
    // Only flash loading UI on cold tab — keeps panel height stable when switching.
    if (first || force) loading = true;
    try {
      if (t === "resourcepacks" || t === "shaderpacks") {
        const next = await api.content.listPacks(t, projectPath);
        if (gen !== loadGen) return;
        if (t === "shaderpacks") shaderPacks = next;
        else resourcePacks = next;
      } else if (t === "servers") {
        const next = await api.servers.list(projectPath);
        if (gen !== loadGen) return;
        servers = next;
      } else if (t === "mods") {
        try {
          const mods = await api.mods.list(projectPath);
          if (gen !== loadGen) return;
          modCount = Array.isArray(mods)
            ? mods.filter((m: any) => !m.contentType || m.contentType === "mod").length
            : 0;
        } catch {
          if (gen !== loadGen) return;
          modCount = null;
        }
      }
      if (gen !== loadGen) return;
      primed = { ...primed, [t]: true };
      // Servers-first: refresh the status board on first open and on manual refresh.
      if (t === "servers" && (first || force)) void pingAll(servers);
    } catch (e) {
      if (gen === loadGen) toasts.error(String(e));
    } finally {
      if (gen === loadGen) loading = false;
    }
  }

  let lastProjectPath = "";

  $effect(() => {
    if (!projectPath) return;
    if (projectPath !== lastProjectPath) {
      lastProjectPath = projectPath;
      resourcePacks = [];
      shaderPacks = [];
      servers = [];
      pings = {};
      modCount = null;
      primed = {};
    }
    // Depend on tab so switching tabs reloads (from cache when primed).
    void tab;
    void load();
  });

  async function togglePack(pack: ContentPackEntry) {
    busyKey = pack.fileName;
    try {
      await api.content.setEnabled(
        tab === "shaderpacks" ? "shaderpacks" : "resourcepacks",
        pack.fileName,
        !pack.enabled,
        projectPath,
      );
      await load({ force: true });
    } catch (e) {
      toasts.error(String(e));
    } finally {
      busyKey = null;
    }
  }

  async function openFolder() {
    const folder =
      tab === "shaderpacks"
        ? "shaderpacks"
        : tab === "resourcepacks"
          ? "resourcepacks"
          : tab === "mods"
            ? "mods"
            : null;
    try {
      await api.files.openFolder(projectPath, folder);
    } catch (e) {
      toasts.error(String(e));
    }
  }

  function openCatalog() {
    libraryTabRequest.set("discover");
    window.dispatchEvent(new CustomEvent("tuffbox:open-library"));
  }

  /** Standalone content browser window; Discover tab as the fallback. */
  async function addContent(type: BrowserContentType) {
    if (await openModsBrowserWindow(projectPath, type)) return;
    openCatalog();
  }

  async function addServer() {
    if (!newServerName.trim() || !newServerAddress.trim()) return;
    busyKey = "add-server";
    try {
      servers = await api.servers.add(newServerName.trim(), newServerAddress.trim(), projectPath);
      newServerName = "";
      newServerAddress = "";
      toasts.success(get(t)("content.toastAdded"));
    } catch (e) {
      toasts.error(String(e));
    } finally {
      busyKey = null;
    }
  }

  async function removeServer(address: string) {
    busyKey = address;
    try {
      servers = await api.servers.remove(address, projectPath);
      const next = { ...pings };
      delete next[address];
      pings = next;
    } catch (e) {
      toasts.error(String(e));
    } finally {
      busyKey = null;
    }
  }

  async function pingServer(address: string) {
    busyKey = `ping:${address}`;
    try {
      const result = await api.servers.ping(address);
      pings = { ...pings, [address]: result };
    } catch (e) {
      toasts.error(String(e));
    } finally {
      busyKey = null;
    }
  }

  async function pingAll(list: McServerEntry[]) {
    if (list.length === 0 || busyKey === "ping-all") return;
    busyKey = "ping-all";
    try {
      const results = await Promise.all(
        list.map((s) =>
          api.servers.ping(s.address).catch(
            (e): PingResult => ({
              address: s.address,
              online: false,
              latencyMs: null,
              error: String(e),
            }),
          ),
        ),
      );
      const next = { ...pings };
      for (const r of results) next[r.address] = r;
      pings = next;
    } finally {
      busyKey = null;
    }
  }

  async function joinServer(address: string) {
    if (projectLaunching || projectRunning) return;
    busyKey = `join:${address}`;
    try {
      await launchWithFeedback({
        path: projectPath,
        profile: "client",
        quickPlayType: "multiplayer",
        quickPlayValue: address,
      });
    } finally {
      busyKey = null;
    }
  }
</script>

<div class="lib-content">
  <div class="tabs" role="tablist" aria-label={$t("content.title")}>
    <button
      type="button"
      role="tab"
      aria-selected={tab === "mods"}
      class:active={tab === "mods"}
      onclick={() => (tab = "mods")}
    >
      <Package size={13} /> Mods{#if modCount != null} · {modCount}{/if}
    </button>
    <button
      type="button"
      role="tab"
      aria-selected={tab === "resourcepacks"}
      class:active={tab === "resourcepacks"}
      onclick={() => (tab = "resourcepacks")}
    >
      <Image size={13} /> Packs{#if primed.resourcepacks} · {resourcePacks.length}{/if}
    </button>
    <button
      type="button"
      role="tab"
      aria-selected={tab === "shaderpacks"}
      class:active={tab === "shaderpacks"}
      onclick={() => (tab = "shaderpacks")}
    >
      <Sparkles size={13} /> Shaders{#if primed.shaderpacks} · {shaderPacks.length}{/if}
    </button>
    <button
      type="button"
      role="tab"
      aria-selected={tab === "servers"}
      class:active={tab === "servers"}
      onclick={() => (tab = "servers")}
      title={serversPinged > 0 ? `${serversOnline} of ${servers.length} online` : "Servers"}
    >
      <Server size={13} /> Servers{#if servers.length > 0 && serversPinged > 0} · {serversOnline}/{servers.length}{:else if servers.length > 0} · {servers.length}{/if}
    </button>
    <span class="tabs-spacer"></span>
    <button
      type="button"
      class="mini-btn icon"
      title="Refresh"
      aria-label={$t("common.refresh")}
      disabled={loading}
      onclick={() => void load({ force: true })}
    >
      <RefreshCw size={13} class={loading ? "spin" : ""} />
    </button>
  </div>

  <div class="panel" role="tabpanel">
    {#if showSkeleton}
      <div class="skel-list" aria-hidden="true">
        <div class="skel-row"></div>
        <div class="skel-row"></div>
        <div class="skel-row"></div>
      </div>
    {:else if tab === "mods"}
      <div class="mods-cta">
        <div class="mods-count">
          {#if modCount == null}
            Couldn't read the mod list.
          {:else}
            <strong>{modCount}</strong> {modCount === 1 ? "mod" : "mods"} installed
          {/if}
        </div>
        <div class="mods-actions">
          <button type="button" class="mini-btn primary" onclick={onOpenMods}>
            <ExternalLink size={12} /> Open Mods
          </button>
          {#if onManage}
            <button type="button" class="mini-btn primary" onclick={onManage}>
              <SlidersHorizontal size={12} /> Manage
            </button>
          {/if}
          <button type="button" class="mini-btn primary" title={$t("content.browseCatalogTitle")} onclick={() => void addContent("mod")}>
            <Plus size={12} /> Add mods
          </button>
          <button type="button" class="mini-btn" title="Open instance folder" onclick={() => void openFolder()}>
            <FolderOpen size={12} /> Folder
          </button>
        </div>
      </div>
    {:else if tab === "resourcepacks" || tab === "shaderpacks"}
      {#if packs.length === 0}
        <div class="empty">
          No {tab === "shaderpacks" ? "shaders" : "resource packs"} yet.
          <button
            type="button"
            class="link"
            onclick={() => void addContent(tab === "shaderpacks" ? "shader" : "resourcepack")}
          >{$t("content.browseCatalog")}</button>
        </div>
      {:else}
        <ul class="rows">
          {#each packs as pack (pack.fileName)}
            <li class="row">
              <span class="row-name" title={pack.fileName}>{pack.name}</span>
              <span class="row-state" class:on={pack.enabled} class:off={!pack.enabled}>
                {pack.enabled ? "On" : "Off"}
              </span>
              <button
                type="button"
                class="mini-btn"
                disabled={busyKey === pack.fileName}
                onclick={() => void togglePack(pack)}
              >
                <Power size={12} /> {pack.enabled ? "Disable" : "Enable"}
              </button>
            </li>
          {/each}
        </ul>
      {/if}
      <div class="panel-foot">
        <button type="button" class="mini-btn" title={$t("library.openFolder")} onclick={() => void openFolder()}>
          <FolderOpen size={12} /> Folder
        </button>
      </div>
    {:else if tab === "servers"}
      <form
        class="add-server"
        onsubmit={(e) => {
          e.preventDefault();
          void addServer();
        }}
      >
        <input
          type="text"
          placeholder={$t("content.namePh")}
          aria-label={$t("content.serverName")}
          bind:value={newServerName}
          maxlength={64}
        />
        <input
          type="text"
          placeholder="play.example.com"
          aria-label={$t("content.serverAddr")}
          bind:value={newServerAddress}
          maxlength={128}
        />
        <button
          type="submit"
          class="mini-btn primary"
          disabled={busyKey === "add-server" || !newServerName.trim() || !newServerAddress.trim()}
        >
          <Plus size={12} /> Add
        </button>
      </form>
      {#if servers.length === 0}
        <div class="empty">{$t("content.noServers")}</div>
      {:else}
        <ul class="rows servers">
          {#each sortedServers as srv (srv.address)}
            {@const ping = pings[srv.address]}
            <li class="row server-row">
              <span
                class="dot"
                class:online={ping?.online}
                class:offline={ping && !ping.online}
                title={ping ? (ping.online ? "Online" : (ping.error ?? "Offline")) : "Not pinged yet"}
              ></span>
              <span class="srv-main">
                <span class="row-name" title={srv.address}>{srv.name}</span>
                <span class="srv-sub">
                  {srv.address}
                  {#if ping?.online && ping.latencyMs != null} · {ping.latencyMs} ms{/if}
                </span>
              </span>
              {#if ping?.online && ping.playersOnline != null && ping.playersMax != null}
                <span class="players" title="Players online">
                  <Users size={12} /> {ping.playersOnline}/{ping.playersMax}
                </span>
              {:else if ping && !ping.online}
                <span class="players off">{$t("content.offline")}</span>
              {/if}
              <span class="srv-actions">
                <button
                  type="button"
                  class="mini-btn primary"
                  title={$t("content.join")}
                  disabled={projectLaunching || projectRunning || busyKey === `join:${srv.address}`}
                  onclick={() => void joinServer(srv.address)}
                >
                  <Play size={12} /> Join
                </button>
                <button
                  type="button"
                  class="mini-btn icon"
                  title="Ping"
                  aria-label={`Ping ${srv.name}`}
                  disabled={busyKey === `ping:${srv.address}`}
                  onclick={() => void pingServer(srv.address)}
                >
                  <Search size={12} />
                </button>
                <button
                  type="button"
                  class="mini-btn icon danger"
                  title={$t("common.delete")}
                  aria-label={`Delete ${srv.name}`}
                  disabled={busyKey === srv.address}
                  onclick={() => void removeServer(srv.address)}
                >
                  <Trash2 size={12} />
                </button>
              </span>
            </li>
          {/each}
        </ul>
        <div class="panel-foot">
          <button
            type="button"
            class="mini-btn"
            disabled={busyKey === "ping-all"}
            onclick={() => void pingAll(servers)}
          >
            <RefreshCw size={12} /> Ping all
          </button>
        </div>
      {/if}
    {/if}
  </div>
</div>

<style>
  .lib-content {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 0;
  }

  .tabs {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  .tabs [role="tab"] {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 4px 8px;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-muted);
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--border-radius-sm);
    cursor: pointer;
    white-space: nowrap;
  }

  .tabs [role="tab"]:hover {
    color: var(--text);
    background: var(--surface-hover);
  }

  .tabs [role="tab"].active {
    color: var(--text);
    background: var(--surface-raised);
    border-color: var(--border);
  }

  .tabs-spacer {
    flex: 1 1 auto;
  }

  .panel {
    min-height: 0;
    max-height: 220px;
    overflow-y: auto;
    border: 1px solid var(--border);
    border-radius: var(--border-radius-md);
    background: var(--surface);
    padding: 8px;
  }

  .skel-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .skel-row {
    height: 30px;
    border-radius: var(--border-radius-sm);
    background: var(--surface-hover);
  }

  .empty {
    padding: 10px 4px;
    font-size: 12px;
    color: var(--text-muted);
    text-align: center;
  }

  .link {
    background: none;
    border: none;
    padding: 0;
    color: var(--accent);
    font-size: inherit;
    cursor: pointer;
  }

  .link:hover {
    text-decoration: underline;
  }

  .mods-cta {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 4px;
  }

  .mods-count {
    font-size: 12px;
    color: var(--text-muted);
  }

  .mods-actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }

  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 5px 6px;
    border: 1px solid var(--border);
    border-radius: var(--border-radius-sm);
    background: var(--surface-raised);
    min-width: 0;
  }

  .row-name {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
    font-weight: 600;
  }

  .row-state {
    font-size: 12px;
    font-weight: 700;
  }

  .row-state.on {
    color: var(--success);
  }

  .row-state.off {
    color: var(--text-muted);
  }

  .server-row {
    flex-wrap: wrap;
  }

  .dot {
    flex: 0 0 auto;
    width: 8px;
    height: 8px;
    border-radius: 999px;
    background: var(--text-muted);
    opacity: 0.5;
  }

  .dot.online {
    background: var(--success);
    opacity: 1;
  }

  .dot.offline {
    background: var(--danger);
    opacity: 1;
  }

  .srv-main {
    flex: 1 1 110px;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .srv-sub {
    font-size: 12px;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .players {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    font-weight: 700;
    color: var(--success);
    white-space: nowrap;
  }

  .players.off {
    color: var(--danger);
    font-weight: 600;
  }

  .srv-actions {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    margin-left: auto;
  }

  .add-server {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    margin-bottom: 8px;
  }

  .add-server input {
    flex: 1 1 110px;
    min-width: 0;
    padding: 5px 8px;
    font-size: 12px;
    color: var(--text);
    background: var(--surface-raised);
    border: 1px solid var(--border);
    border-radius: var(--border-radius-sm);
  }

  .add-server input::placeholder {
    color: var(--text-muted);
  }

  .panel-foot {
    display: flex;
    gap: 8px;
    margin-top: 8px;
  }

  .mini-btn {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 4px 8px;
    font-size: 12px;
    font-weight: 600;
    color: var(--text);
    background: var(--surface-raised);
    border: 1px solid var(--border);
    border-radius: var(--border-radius-sm);
    cursor: pointer;
    white-space: nowrap;
  }

  .mini-btn:hover:not(:disabled) {
    background: var(--surface-hover);
  }

  .mini-btn:disabled {
    opacity: 0.55;
    cursor: default;
  }

  .mini-btn.primary {
    background: var(--accent);
    border-color: transparent;
    color: var(--on-accent, #fff);
  }

  .mini-btn.icon {
    padding: 4px 6px;
  }

  .mini-btn.danger {
    color: var(--danger);
  }

  :global(.spin) {
    animation: lib-content-spin 0.9s linear infinite;
  }

  @keyframes lib-content-spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
