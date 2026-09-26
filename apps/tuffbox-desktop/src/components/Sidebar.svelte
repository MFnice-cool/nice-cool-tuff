<script lang="ts">
  import { SvelteSet } from "svelte/reactivity";
  import { Home, Library, Workflow, Plus, Settings, User, Play, Square, Terminal } from "@lucide/svelte";
  import {
    openAddInstance,
    projectPath,
    projectInfo,
    recentProjects,
    runningInstances,
    isProjectRunning,
    ideStageRequest,
    ideSuggestedStage,
    openLaunchLog,
    isLaunching,
    launchProgress,
    launchSessions,
    isProjectLaunching,
    brandIcon,
    BRAND_ICON_CREEPER_SRC_SM,
    authState,
    skinPath,
    loginModalOpen,
    loginTypeLabel,
  } from "../lib/store";
  import { api } from "../lib/api";
  import { locale, LOCALES, t } from "../lib/i18n";
  import { homeIcons } from "../lib/homeBootstrap";
  import { killWithFeedback, launchWithFeedback } from "../lib/launch";
  import HeadAvatar from "./HeadAvatar.svelte";

  import type { View } from "../lib/types";
  let { currentView = $bindable() }: { currentView: View } = $props();

  /** Real pack icon (data URL from the instance listing) keyed by project path. */
  const instanceIcons = $derived($homeIcons);
  const currentRunning = $derived(isProjectRunning($projectPath, $runningInstances));
  const currentLaunching = $derived(isProjectLaunching($projectPath, $launchSessions));
  const selectedRunning = $derived(isProjectRunning($projectPath, $runningInstances));
  const playTitle = $derived.by(() => {
    if ($isLaunching) {
      const msg = $launchProgress?.message ?? "Launching…";
      const pct = $launchProgress?.percent;
      return pct != null ? `${msg} (${pct}%)` : msg;
    }
    if (selectedRunning) return "Stop";
    return "Play";
  });
  const profileName = $derived($authState.profile?.name ?? "Sign in");
  const profileKind = $derived.by(() => {
    if (!$authState.loggedIn) return "Minecraft account";
    if ($authState.loginType === "microsoft") return "Microsoft Account";
    if ($authState.loginType === "offline") return "Offline account";
    return loginTypeLabel(
      $authState.loginType,
      $authState.accounts.find((a) => a.uuid === $authState.activeAccountUuid)?.authority,
    );
  });

  function openProfile() {
    if ($authState.loggedIn) currentView = "me";
    else loginModalOpen.set(true);
  }
  const iconRequested = new SvelteSet<string>();

  async function loadInstanceIcon(path: string) {
    try {
      const listing = await api.project.getListing(path);
      const rel = listing.iconPath;
      if (!rel) {
        homeIcons.update((m) => ({ ...m, [path]: null }));
        return;
      }
      const data = await api.project.readListingAsset(rel, path);
      homeIcons.update((m) => ({ ...m, [path]: data }));
    } catch {
      homeIcons.update((m) => ({ ...m, [path]: null }));
    }
  }

  $effect(() => {
    const missing = $recentProjects
      .map((p) => p.path)
      .filter((path) => !iconRequested.has(path) && instanceIcons[path] === undefined);
    if (!missing.length) return;
    // Prefer one batch invoke when several icons are cold.
    for (const path of missing) iconRequested.add(path);
    void api.home
      .projectBriefs(missing)
      .then((briefs) => {
        const icons: Record<string, string | null> = {};
        for (const b of briefs) {
          icons[b.path] = b.iconDataUrl ?? null;
        }
        homeIcons.update((prev) => ({ ...prev, ...icons }));
        // Fill any paths the batch skipped.
        for (const path of missing) {
          if (icons[path] === undefined) void loadInstanceIcon(path);
        }
      })
      .catch(() => {
        for (const path of missing) void loadInstanceIcon(path);
      });
  });

  /**
   * Fallback identity when a pack has no icon: theme-token gradients
   * (accent + surfaces), hashed by name so each instance stays stable and
   * follows the active data-theme without a JS subscription.
   */
  function themeGradient(name: string): [string, string] {
    const pairs: [string, string][] = [
      [
        "color-mix(in srgb, var(--accent-primary) 32%, var(--bg-primary))",
        "color-mix(in srgb, var(--accent-primary) 82%, var(--bg-secondary))",
      ],
      [
        "color-mix(in srgb, var(--accent-secondary) 28%, var(--bg-primary))",
        "color-mix(in srgb, var(--accent-primary) 72%, var(--bg-tertiary))",
      ],
      [
        "color-mix(in srgb, var(--accent-primary) 18%, var(--bg-secondary))",
        "color-mix(in srgb, var(--accent-hover) 78%, var(--bg-primary))",
      ],
      [
        "color-mix(in srgb, var(--bg-tertiary) 45%, var(--accent-primary))",
        "color-mix(in srgb, var(--accent-primary) 88%, var(--accent-secondary))",
      ],
      [
        "color-mix(in srgb, var(--accent-secondary) 22%, var(--bg-primary))",
        "color-mix(in srgb, var(--accent-secondary) 65%, var(--accent-primary))",
      ],
    ];
    let hash = 0;
    for (let i = 0; i < name.length; i++) hash = name.charCodeAt(i) + ((hash << 5) - hash);
    return pairs[Math.abs(hash) % pairs.length];
  }

  function openHome() {
    currentView = "dashboard";
  }

  function openIde() {
    ideStageRequest.set($ideSuggestedStage || "content");
    currentView = "ide";
  }

  function openNewProject() {
    // Dashboard owns the modal, so make sure we're on that view before
    // raising the flag — otherwise the modal component wouldn't be mounted.
    currentView = "dashboard";
    openAddInstance("blank");
  }

  async function playClient() {
    if (!$projectPath || currentLaunching) return;
    if (currentRunning) {
      await killWithFeedback($projectPath);
      return;
    }
    await launchWithFeedback({ path: $projectPath, profile: "client" });
  }

  async function stopClient() {
    if (!$projectPath || $isLaunching) return;
    await killWithFeedback($projectPath);
  }

  function openLogs() {
    if ($projectPath) openLaunchLog($projectPath);
  }

  async function selectInstance(path: string) {
    if ($projectPath === path) {
      currentView = "dashboard";
      return;
    }
    try {
      const info = await api.project.validate(path);
      const manifestPath = info.manifestPath || path;
      // reorder:false — rail icons must not jump around on every click.
      recentProjects.add({ path: manifestPath, info: info as any }, { reorder: false });
      projectPath.set(manifestPath);
      projectInfo.set(info as any);
      void api.session.setLastOpened(manifestPath).catch(() => {});
    } catch {
      const cached = $recentProjects.find((p) => p.path === path);
      if (!cached) return;
      projectPath.set(cached.path);
      projectInfo.set(cached.info);
    }
    currentView = "dashboard";
  }
</script>

<aside class="rail">
  <button
    type="button"
    class="rail-profile"
    onclick={openProfile}
    title={profileName}
  >
    <span class="rail-profile-avatar">
      {#if $authState.loggedIn && $skinPath}
        <HeadAvatar skinSrc={$skinPath} size={36} alt="" />
      {:else}
        <User size={18} />
      {/if}
    </span>
    <span class="rail-profile-text">
      <span class="rail-profile-name">{profileName}</span>
      <span class="rail-profile-kind">{profileKind}</span>
    </span>
  </button>

  <!-- Brand mark — constant identity, not a nav button. -->
  <div class="rail-brand" title="TuffBox">
    {#if $brandIcon === "creeper"}
      <img
        class="brand-logo brand-logo-img"
        src={BRAND_ICON_CREEPER_SRC_SM}
        alt=""
        draggable="false"
        aria-hidden="true"
      />
    {:else}
      <span class="brand-logo" aria-hidden="true">T</span>
    {/if}
  </div>

  <nav class="rail-zone" aria-label={$t("nav.app")}>
    <div class="rail-item">
      <button
        type="button"
        class="rail-btn ghost"
        class:active={currentView === "dashboard"}
        title={$t("nav.home")}
        aria-label={$t("nav.home")}
        onclick={openHome}
      >
        <Home size={21} />
        <span class="rail-label">Java Edition</span>
      </button>
    </div>
    <div class="rail-item">
      <button
        type="button"
        class="rail-btn ghost"
        class:active={currentView === "library"}
        title={$t("nav.library")}
        aria-label={$t("nav.library")}
        onclick={() => (currentView = "library")}
      >
        <Library size={21} />
        <span class="rail-label">{$t("nav.library")}</span>
      </button>
    </div>
    <div class="rail-item">
      <button
        type="button"
        class="rail-btn ghost"
        class:active={currentView === "ide"}
        title={$t("nav.ide")}
        aria-label={$t("nav.ide")}
        onclick={openIde}
      >
        <Workflow size={21} />
        <span class="rail-label">IDE</span>
      </button>
    </div>
    <div class="rail-item">
      <button
        type="button"
        class="rail-btn add"
        title={$t("nav.addInstance")}
        aria-label={$t("nav.addInstance")}
        onclick={openNewProject}
      >
        <Plus size={22} />
        <span class="rail-label">New instance</span>
      </button>
    </div>
  </nav>

  <div class="rail-divider" aria-hidden="true"></div>

  <nav class="rail-zone rail-instances" aria-label={$t("nav.library")}>
    {#each $recentProjects as instance (instance.path)}
      {#if instance.info}
      {@const icon = instanceIcons[instance.path]}
      {@const running = isProjectRunning(instance.path, $runningInstances)}
      {@const [g0, g1] = themeGradient(instance.info.name)}
      <div class="rail-item">
        <button
          type="button"
          class="rail-btn instance"
          class:active={$projectPath === instance.path}
          class:has-icon={!!icon}
          title={instance.info.name}
          aria-label={instance.info.name}
          style={icon ? undefined : `background: linear-gradient(135deg, ${g0}, ${g1})`}
          onclick={() => selectInstance(instance.path)}
        >
          {#if icon}
            <img class="instance-img" src={icon} alt="" draggable="false" />
          {:else}
            <span class="instance-letter">{instance.info.name[0]}</span>
          {/if}
          {#if running}
            <span class="running-dot" title="Running"></span>
          {/if}
          <span class="rail-label">{instance.info.name}</span>
        </button>
      </div>
      {/if}
    {/each}
    {#if $recentProjects.length === 0}
      <div class="rail-item">
        <button
          type="button"
          class="rail-btn add rail-empty-add"
          title={$t("nav.addInstance")}
          aria-label={$t("nav.addInstance")}
          onclick={openNewProject}
        >
          <Plus size={18} />
          <span class="rail-label">New instance</span>
        </button>
      </div>
    {/if}
  </nav>

  <nav class="rail-zone rail-bottom" aria-label={$t("nav.app")}>
    <div class="rail-item rail-compact">
      <button
        type="button"
        class="rail-btn ghost"
        class:launching={$isLaunching}
        class:stop={selectedRunning && !$isLaunching}
        title={playTitle}
        aria-label={playTitle}
        aria-busy={$isLaunching}
        disabled={!$projectPath || $isLaunching}
        onclick={selectedRunning && !$isLaunching ? stopClient : playClient}
      >
        {#if $isLaunching}
          <span class="rail-spinner" aria-hidden="true"></span>
        {:else if selectedRunning}
          <Square size={18} fill="currentColor" />
        {:else}
          <Play size={21} />
        {/if}
        <span class="rail-label">{playTitle}</span>
      </button>
    </div>
    <div class="rail-item rail-compact">
      <button
        type="button"
        class="rail-btn ghost"
        title={$t("nav.logs")}
        aria-label={$t("nav.logs")}
        disabled={!$projectPath}
        onclick={openLogs}
      >
        <Terminal size={21} />
        <span class="rail-label">{$t("nav.logs")}</span>
      </button>
    </div>
    <div class="rail-item">
      <button
        type="button"
        class="rail-btn ghost"
        class:active={currentView === "settings"}
        title={$t("nav.settings")}
        aria-label={$t("nav.settings")}
        onclick={() => (currentView = "settings")}
      >
        <Settings size={21} />
        <span class="rail-label">{$t("nav.settings")}</span>
      </button>
    </div>
    <div class="rail-item rail-compact">
      <button
        type="button"
        class="rail-btn ghost"
        class:active={currentView === "me"}
        title={$t("nav.profile")}
        aria-label={$t("nav.profile")}
        onclick={() => (currentView = "me")}
      >
        <User size={21} />
        <span class="rail-label">{$t("nav.profile")}</span>
      </button>
    </div>
    <div class="rail-item rail-compact">
      <div class="lang-switch" role="group" aria-label={$t("nav.language")}>
        {#each LOCALES as l (l.id)}
          <button
            type="button"
            class="lang-btn"
            class:active={$locale === l.id}
            aria-pressed={$locale === l.id}
            title={l.label}
            onclick={() => locale.set(l.id)}
          >
            {l.label}
          </button>
        {/each}
      </div>
    </div>
  </nav>
</aside>

<style>

  .rail {
    width: 72px;
    flex-shrink: 0;
    height: 100%;
    min-height: 0;
    box-sizing: border-box;
    /* Themes (esp. Minimal) override via --rail-* tokens. */
    background: var(--rail-bg, color-mix(in srgb, var(--bg-secondary) 92%, var(--bg-tertiary)));
    border-right: 1px solid var(--rail-border, var(--border-color));
    -webkit-backdrop-filter: var(--rail-backdrop, none);
    backdrop-filter: var(--rail-backdrop, none);
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 12px 0;
    position: relative;
    z-index: 30;
  }

  /* Brand mark — constant amber identity, not a nav button. */
  .rail-brand {
    padding: 2px 0 12px;
    flex-shrink: 0;
    user-select: none;
  }

  .brand-logo {
    width: 32px;
    height: 32px;
    border-radius: var(--border-radius-lg);
    background: var(--brand-mark-gradient, linear-gradient(135deg, #ffc500, #ff9500));
    color: var(--brand-mark-fg, #241703);
    font-weight: 900;
    font-size: 16px;
    display: flex;
    align-items: center;
    justify-content: center;
    box-shadow: var(--brand-mark-shadow, 0 3px 10px rgba(255, 197, 0, 0.24));
    animation: tb-logo-reveal 1.15s cubic-bezier(0.22, 1, 0.36, 1) both;
    overflow: hidden;
  }

  .brand-logo-img {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: contain;
    padding: 2px;
    box-sizing: border-box;
    /* Task #55: PNG transparency must survive — no own background, no square
       borders visible, glyph centered and never clipped by the rounded box. */
    background: transparent;
    box-shadow: none;
    color: transparent;
    font-size: 0;
  }

  .lang-switch {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 3px;
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
    background: transparent;
  }
  .lang-btn {
    padding: 2px 8px;
    border-radius: var(--border-radius-sm);
    color: var(--text-muted);
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0.04em;
    cursor: pointer;
    transition:
      background var(--motion-fast) var(--ease-out),
      color var(--motion-fast) var(--ease-out);
  }
  .lang-btn:hover {
    color: var(--text-primary);
    background: color-mix(in srgb, var(--text-primary) 8%, transparent);
  }
  .lang-btn.active {
    color: var(--text-primary);
    background: color-mix(in srgb, var(--accent-primary) 22%, transparent);
  }

  .rail-zone {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 14px;
    width: 100%;
    flex-shrink: 0;
  }

  .rail-divider {
    width: 32px;
    height: 2px;
    border-radius: 1px;
    background: color-mix(in srgb, var(--text-primary) 10%, transparent);
    margin: 10px 0;
    flex-shrink: 0;
  }

  /* Middle zone owns the scroll; scrollbar hidden like server rails. */
  .rail-instances {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overflow-x: hidden;
    scrollbar-width: none;
    padding: 2px 0;
    flex-shrink: 1;
  }

  .rail-instances::-webkit-scrollbar {
    display: none;
  }

  .rail-bottom {
    padding-top: 10px;
  }

  .rail-item {
    position: relative;
    width: 100%;
    display: flex;
    justify-content: center;
    flex-shrink: 0;
  }

  /* Active/hover pill on the rail's left edge (height animates, Discord-style). */
  .rail-item::before {
    content: "";
    position: absolute;
    left: 0;
    top: 50%;
    transform: translateY(-50%);
    width: 4px;
    height: 0;
    border-radius: var(--rail-indicator-radius, 0 4px 4px 0);
    background: var(--accent-primary);
    box-shadow: 0 0 10px color-mix(in srgb, var(--accent-primary) 55%, transparent);
    opacity: 0;
    transition:
      height var(--motion-fast, 160ms) var(--ease-hover-in, ease),
      opacity var(--motion-fast, 160ms) var(--ease-hover-in, ease);
    pointer-events: none;
  }

  .rail-item:hover::before {
    height: 20px;
    opacity: 1;
  }

  .rail-item:has(.rail-btn.active)::before {
    height: 36px;
    opacity: 1;
  }

  /* `.rail` prefix lifts specificity above the global themed button radius
     (html[data-rounded-corners] :where(button)) so the shape holds.
     Default: circle at rest → squircle on hover/active.
     Sharp themes override via --rail-btn-radius* → hard squares. */
  .rail .rail-btn {
    position: relative;
    width: 48px;
    height: 48px;
    padding: 0;
    gap: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: var(--rail-btn-radius, 50%);
    background: var(--bg-tertiary);
    color: var(--text-secondary);
    font-size: 14px;
    cursor: pointer;
    overflow: hidden;
    /* Kill the global button hover translate — it would desync the edge pill. */
    transform: none !important;
    /* Kill the global button hover/active filters too: brightness(1.18)
       washes the pale hover pill out on light themes (#d5ddd1 → ~white on a
       near-white rail — the invisible-hover bug), and the :active brightness
       dip + margin shift break the rail's own press look and geometry. The
       explicit background swaps below already encode hover/press. */
    filter: none;
    transition:
      border-radius var(--motion-med, 240ms) var(--ease-hover-in, ease),
      background-color var(--motion-fast, 160ms) var(--ease-hover-in, ease),
      color var(--motion-fast, 160ms) var(--ease-hover-in, ease);
  }

  /* The bare `filter: none` above only covers the rest state (the global
     filter rules only apply on :hover/:active). Re-assert with the same
     pseudo-class shape — (0,4,1) here beats the global (0,2,1) on
     specificity, so this holds no matter the stylesheet order. */
  .rail .rail-btn:hover:not(:disabled),
  .rail .rail-btn:active:not(:disabled) {
    filter: none;
  }

  /* Global :active adds `margin-top: 2px` — that shift would visually detach
     the button from the edge pill. Rail keeps its grid on press. */
  .rail .rail-btn:active:not(:disabled) {
    margin-top: 0;
    border-bottom-width: 0;
  }

  .rail .rail-btn:hover,
  .rail .rail-btn.active {
    border-radius: var(--rail-btn-radius-active, var(--border-radius-lg));
  }

  /* Ghost nav (Home / IDE / Settings / Profile): quiet until touched. */
  .rail .rail-btn.ghost {
    background: transparent;
    color: var(--text-muted);
  }

  .rail .rail-btn.ghost:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .rail .rail-btn.ghost.active {
    background: color-mix(in srgb, var(--accent-primary) 14%, transparent);
    color: var(--accent-primary);
  }

  .rail .rail-btn:disabled {
    opacity: 0.35;
    cursor: not-allowed;
  }

  .rail .rail-btn:disabled:hover {
    background: transparent;
    color: var(--text-muted);
    border-radius: var(--rail-btn-radius, 50%);
  }

  /* Play → launching / stop: share Home's phase store, keep rail compact. */
  .rail .rail-btn.ghost.launching,
  .rail .rail-btn.ghost.launching:disabled,
  .rail .rail-btn.ghost.launching:disabled:hover {
    opacity: 1;
    cursor: wait;
    color: var(--accent-primary);
    background: color-mix(in srgb, var(--accent-primary) 12%, transparent);
  }

  .rail .rail-btn.ghost.stop {
    color: var(--accent-danger, #ef4444);
    background: color-mix(in srgb, var(--accent-danger, #ef4444) 14%, transparent);
  }

  .rail .rail-btn.ghost.stop:hover {
    background: color-mix(in srgb, var(--accent-danger, #ef4444) 22%, transparent);
    color: var(--accent-danger, #ef4444);
  }

  .rail-spinner {
    width: 18px;
    height: 18px;
    box-sizing: border-box;
    border: 2px solid color-mix(in srgb, var(--accent-primary) 25%, transparent);
    border-top-color: var(--accent-primary);
    border-radius: 50%;
    animation: rail-spin 0.7s linear infinite;
  }

  @keyframes rail-spin {
    to {
      transform: rotate(360deg);
    }
  }

  /* Add instance: quiet amber plus. */
  .rail .rail-btn.add {
    background: transparent;
    color: var(--accent-primary);
  }

  .rail .rail-btn.add:hover {
    background: color-mix(in srgb, var(--accent-primary) 14%, transparent);
    color: var(--accent-primary);
  }

  .rail .rail-btn.rail-empty-add {
    border: 1px dashed color-mix(in srgb, var(--accent-primary) 40%, var(--border-color));
    opacity: 0.85;
  }

  /* Instance avatars: theme-token gradient (inline) or the real pack icon.
     `position: relative` on `.rail-btn` makes the abspos icon/running-dot
     clip to the same circle → squircle mask as the letter fallback. */
  .rail .rail-btn.instance {
    color: var(--text-primary);
  }

  .rail .rail-btn.instance.has-icon {
    background: transparent;
  }

  .rail .rail-btn.instance:hover,
  .rail .rail-btn.instance.active {
    background-color: transparent;
    color: var(--text-primary);
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--accent-primary) 45%, transparent);
  }

  .instance-img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    border-radius: inherit;
    pointer-events: none;
  }

  .instance-letter {
    font-weight: 900;
    font-size: 18px;
    line-height: 1;
    text-transform: uppercase;
    color: color-mix(in srgb, var(--accent-primary) 18%, var(--text-primary));
    text-shadow: 0 1px 2px color-mix(in srgb, var(--bg-primary) 55%, transparent);
    pointer-events: none;
  }

  /* Running indicator — green dot pinned to the avatar's lower right. */
  .running-dot {
    position: absolute;
    right: -1px;
    bottom: -1px;
    width: 13px;
    height: 13px;
    border-radius: var(--rail-btn-radius, 50%);
    background: var(--accent-primary);
    border: 3px solid var(--bg-primary);
    box-shadow: 0 0 8px color-mix(in srgb, var(--accent-primary) 60%, transparent);
    pointer-events: none;
  }

  .rail-label,
  .rail-profile {
    display: none;
  }
</style>
