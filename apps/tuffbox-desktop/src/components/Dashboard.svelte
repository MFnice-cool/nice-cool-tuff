<script lang="ts">
  import { onMount } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import {
    LogIn,
    User,
  } from "@lucide/svelte";
  import HeadAvatar from "./HeadAvatar.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { confirm } from "@tauri-apps/plugin-dialog";
  import {
    recentProjects,
    projectPath,
    projectInfo,
    authState,
    skinPath,
    newProjectOpen,
    loginModalOpen,
    isLaunching,
    launchProgress,
    launchSessions,
    isProjectLaunching,
    runningInstances,
    isProjectRunning,
    normalizeInstancePath,
    formatPlaytime,
    libraryTabRequest,
    addInstanceMode,
    openAddInstance,
    launcherSettingsLive,
    homeYoutubePlacement,
    ideStageRequest,
    optimizeAfterCreate,
    type RecentProject,
  } from "../lib/store";
  import { toasts } from "../lib/toast";
  import { copyText } from "../lib/clipboard";
  import { api } from "../lib/api";
  import { launchWithFeedback, killWithFeedback } from "../lib/launch";
  import { fetchCrashFixBanner, rollbackLastCrashFix } from "../lib/softVerify";
  import {
    homeCrashFixBanner,
    homeIcons,
    homeSizes,
    homeSkinPaths,
    homeStats,
  } from "../lib/homeBootstrap";
  import AddInstanceModal from "./AddInstanceModal.svelte";
  import SkinPreview3D from "./SkinPreview3D.svelte";
  import HomeHero, { type PosterCoverKind } from "./HomeHero.svelte";
  import GithubPackUpdateBanner from "./GithubPackUpdateBanner.svelte";
  import GithubPackUpdateGate from "./GithubPackUpdateGate.svelte";
  import HomeInstanceShelf from "./HomeInstanceShelf.svelte";
  import YoutubeFeed from "./YoutubeFeed.svelte";
  import PromptDialog from "./PromptDialog.svelte";
  import { Stack } from "@tuffbox/layout-lib";

  import type { View } from "../lib/types";

  let { currentView = $bindable() }: { currentView: View } = $props();

  let authReady = $state(false);

  function browseLibrary() {
    libraryTabRequest.set("discover");
    currentView = "library";
  }

  const projectStats = $derived($homeStats);
  const instanceSizes = $derived($homeSizes);
  const accountSkinPaths = $derived($homeSkinPaths);
  const crashFixBanner = $derived($homeCrashFixBanner);

  let selectedPath = $state<string | null>($projectPath);
  let potatoPc = $state(
    typeof document !== "undefined" && document.documentElement.classList.contains("potato-pc"),
  );
  let accountSwitchBusy = $state(false);
  let heroOverflowOpen = $state(false);
  let updateGateOpen = $state(false);
  let launchAfterGate = false;
  // Honest spinner: launchWithFeedback resets the global isLaunching flag as
  // soon as the spawn invoke returns; hold our own flag until the backend
  // reports this instance actually exited ("process-exited" event).
  let launchHoldPath = $state<string | null>(null);
  let heroActionBusy = $state(false);
  let showRenamePrompt = $state(false);
  let showClonePrompt = $state(false);
  let renameDefault = $state("");
  let cloneDefault = $state("");

  const selectedProject = $derived($recentProjects.find((p) => p.path === selectedPath));
  const selectedRunning = $derived(isProjectRunning(selectedPath, $runningInstances));
  const selectedLaunching = $derived(isProjectLaunching(selectedPath, $launchSessions));
  const selectedLaunchMessage = $derived(
    selectedPath ? $launchSessions[selectedPath]?.message ?? "Launching…" : "Launching…",
  );
  // Spinner covers spawn + play session, not just the invoke round-trip.
  const launchingHeld = $derived($isLaunching || launchHoldPath !== null);
  const selectedInstanceMeta = $derived.by(() => {
    const info = selectedProject?.info;
    if (!info) return "";
    const loader = info.loaderVersion?.trim()
      ? `${info.loaderKind} ${info.loaderVersion.trim()}`
      : info.loaderKind;
    const bits = [`${info.minecraftVersion} · ${loader}`];
    const path = selectedProject?.path;
    if (path && projectStats[path]?.playtime) {
      bits.push(formatPlaytime(projectStats[path].playtime));
    }
    if (path && instanceSizes[path] && instanceSizes[path] !== "?") {
      bits.push(instanceSizes[path]);
    }
    return bits.join(" · ");
  });
  const youtubeOnHome = $derived($launcherSettingsLive?.showYoutubeOnHome === true);
  const youtubeBesideSkin = $derived(youtubeOnHome && $homeYoutubePlacement === "right");
  const youtubeFullOnHome = $derived(youtubeOnHome && !youtubeBesideSkin);
  const homeBackdropOn = $derived($launcherSettingsLive?.homeBackdrop !== false);
  const skinPreviewHeight = $derived(youtubeBesideSkin ? 340 : 400);
  const skinAvatarSize = $derived(youtubeBesideSkin ? 96 : 120);

  type CoverState = { url: string | null; kind: PosterCoverKind };
  let coverByPath = $state<Record<string, CoverState>>({});
  const listingFetched = new SvelteSet<string>();

  const posterCover = $derived.by((): CoverState => {
    if (!selectedPath) return { url: null, kind: "none" };
    return coverByPath[selectedPath] ?? { url: null, kind: "none" };
  });

  $effect(() => {
    const path = selectedPath;
    if (!path || listingFetched.has(path)) return;
    listingFetched.add(path);
    let cancelled = false;
    void (async () => {
      try {
        const listing = await api.project.getListing(path);
        if (cancelled) return;
        const first = listing.gallery?.[0];
        if (first?.url) {
          coverByPath = { ...coverByPath, [path]: { url: first.url, kind: "gallery" } };
          return;
        }
        if (first?.path) {
          const data = await api.project.readListingAsset(first.path, path);
          if (cancelled) return;
          coverByPath = { ...coverByPath, [path]: { url: data, kind: "gallery" } };
          return;
        }
        const icon = $homeIcons[path];
        if (icon && !potatoPc) {
          coverByPath = { ...coverByPath, [path]: { url: icon, kind: "icon" } };
          return;
        }
        coverByPath = { ...coverByPath, [path]: { url: null, kind: "none" } };
      } catch {
        if (cancelled) return;
        const icon = $homeIcons[path];
        coverByPath = {
          ...coverByPath,
          [path]: icon && !potatoPc ? { url: icon, kind: "icon" } : { url: null, kind: "none" },
        };
      }
    })();
    return () => {
      cancelled = true;
    };
  });

  $effect(() => {
    const path = selectedPath;
    if (!path || potatoPc) return;
    const existing = coverByPath[path];
    if (existing?.kind === "gallery") return;
    const icon = $homeIcons[path];
    if (icon && (!existing || existing.kind === "none")) {
      coverByPath = { ...coverByPath, [path]: { url: icon, kind: "icon" } };
    }
  });

  // The sidebar rail switches instances through the global store — mirror it here.
  $effect(() => {
    const p = $projectPath;
    if (p && p !== selectedPath) {
      selectedPath = p;
      heroOverflowOpen = false;
    }
  });
  const skinUrl = $derived($authState.profile?.skinUrl ?? null);
  const capeUrl = $derived($authState.profile?.capeUrl ?? null);
  const accountKey = $derived($authState.activeAccountUuid ?? $authState.profile?.uuid ?? "");
  /** Shrink Minecraft nick under the skin preview so long names fit the 320px rail. */
  const skinNameFontPx = $derived.by(() => {
    const n = ($authState.profile?.name ?? "").length;
    if (n <= 8) return 12;
    if (n <= 12) return 11;
    if (n <= 16) return 10;
    if (n <= 20) return 9;
    return 8;
  });

  let crashFixBusy = $state(false);
  let softVerifyNowUnix = $state(Math.floor(Date.now() / 1000));

  const softVerifyRemainingSecs = $derived.by(() => {
    const b = crashFixBanner;
    if (!b?.softVerifyStartedUnix) return null;
    const min = Number(b.minPlaytimeSecs ?? 180);
    const started = Number(b.softVerifyStartedUnix);
    const elapsed = Math.max(0, softVerifyNowUnix - started);
    return Math.max(0, min - elapsed);
  });

  async function refreshCrashFixBanner(path: string | null) {
    if (!path) {
      homeCrashFixBanner.set(null);
      return;
    }
    try {
      homeCrashFixBanner.set(await fetchCrashFixBanner(path));
    } catch {
      homeCrashFixBanner.set(null);
    }
  }

  $effect(() => {
    void refreshCrashFixBanner(selectedPath);
  });

  $effect(() => {
    // Selected size only — never walk every recent instance on home.
    const path = selectedPath;
    if (!path) return;
    if ($homeSizes[path]) return;
    let cancelled = false;
    void (async () => {
      try {
        const label = await api.instance.getSize(path);
        if (!cancelled) {
          homeSizes.update((m) => ({ ...m, [path]: label }));
        }
      } catch {
        if (!cancelled) {
          homeSizes.update((m) => ({ ...m, [path]: "?" }));
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  });

  $effect(() => {
    const started = crashFixBanner?.softVerifyStartedUnix;
    if (!started) return;
    softVerifyNowUnix = Math.floor(Date.now() / 1000);
    const id = setInterval(() => {
      softVerifyNowUnix = Math.floor(Date.now() / 1000);
    }, 1000);
    return () => clearInterval(id);
  });

  async function onRollbackCrashFix() {
    if (!selectedPath || crashFixBusy) return;
    crashFixBusy = true;
    try {
      const ok = await rollbackLastCrashFix(selectedPath);
      if (ok) await refreshCrashFixBanner(selectedPath);
    } finally {
      crashFixBusy = false;
    }
  }

  $effect(() => {
    const uuids = $authState.accounts.map((a) => a.uuid).filter(Boolean);
    const missing = uuids.filter((u) => !$homeSkinPaths[u]);
    if (!missing.length) return;
    void api.home
      .accountSkinPaths(missing)
      .then((map) => {
        if (map && Object.keys(map).length) {
          homeSkinPaths.update((prev) => ({ ...prev, ...map }));
        }
      })
      .catch(() => {});
  });

  async function switchHomeAccount(uuid: string) {
    if (accountSwitchBusy || uuid === $authState.activeAccountUuid) return;
    accountSwitchBusy = true;
    try {
      const state = await api.mcAuth.switchAccount(uuid);
      authState.set(state);
      if (state.profile) {
        try {
          const path = await api.mcAuth.getSkinPath(state.profile.uuid);
          skinPath.set(path);
          homeSkinPaths.update((prev) => ({ ...prev, [state.profile!.uuid]: path }));
        } catch {
          skinPath.set(null);
        }
      } else {
        skinPath.set(null);
      }
      toasts.success(`Switched to ${state.profile?.name ?? "account"}`);
    } catch (e) {
      toasts.error(String(e));
    } finally {
      accountSwitchBusy = false;
    }
  }

  onMount(() => {
    let cleanup: (() => void) | undefined;
    void (async () => {
      potatoPc = document.documentElement.classList.contains("potato-pc");
      // Auth usually arrives via home bootstrap; refresh only if still empty.
      if (!$authState.loggedIn && !$authState.profile) {
        try {
          const status = await api.mcAuth.getAuthStatus();
          authState.set(status);
          if (status.loggedIn && status.profile) {
            try {
              const path = await api.mcAuth.getSkinPath(status.profile.uuid);
              skinPath.set(path);
              homeSkinPaths.update((prev) => ({
                ...prev,
                [status.profile!.uuid]: path,
              }));
            } catch {}
          }
        } catch {
        } finally {
          authReady = true;
        }
      } else {
        authReady = true;
      }

      if (selectedPath && !selectedProject && $recentProjects.length > 0) {
        selectProject($recentProjects[0].path);
      }

      // JVM crash toast handler is registered once in App.svelte (it is
      // idempotent and must live for the whole app lifetime, not per-mount).

      // Refresh playtime when a session ends.
      const { listen } = await import("@tauri-apps/api/event");
      const unlistenExit = await listen<{ id: string }>("process-exited", (event) => {
        const id = event.payload?.id;
        if (id) {
          if (
            launchHoldPath &&
            normalizeInstancePath(id) === normalizeInstancePath(launchHoldPath)
          ) {
            launchHoldPath = null;
          }
          void api.stats.get(id).then((s) => {
            homeStats.update((prev) => ({
              ...prev,
              [id]: {
                playtime: s.totalPlaytimeSeconds ?? 0,
                lastLaunch: s.lastLaunch ?? null,
              },
            }));
          }).catch(() => {});
          void api.home.invalidateCache(id).catch(() => {});
        }
      });
      const unlistenSoft = await listen("tuffbox:soft-verify-outcome", () => {
        void refreshCrashFixBanner(selectedPath);
      });
      const onCrashFixApplied = () => {
        void refreshCrashFixBanner(selectedPath);
      };
      window.addEventListener("tuffbox:crash-fix-applied", onCrashFixApplied);
      cleanup = () => {
        unlistenExit();
        unlistenSoft();
        window.removeEventListener("tuffbox:crash-fix-applied", onCrashFixApplied);
      };
    })();
    return () => cleanup?.();
  });

  async function loadProject(path: string) {
    const info = await invoke("validate_project", { path }) as import("../lib/api").ProjectSummary;
    const manifestPath = info.manifestPath || path;
    const project: RecentProject = { path: manifestPath, info: info as any };
    recentProjects.add(project);
    projectPath.set(manifestPath);
    projectInfo.set(project.info);
    selectedPath = manifestPath;
  }

  async function selectProject(path: string) {
    try {
      await loadProject(path);
    } catch {
      const project = $recentProjects.find((p) => p.path === path);
      if (project) {
        selectedPath = path;
        projectPath.set(path);
        projectInfo.set(project.info);
      }
    }
  }

  async function launch() {
    if (!selectedPath) return;
    const launchTarget = selectedPath;
    // Update-to-play gate: if this pack came from a GitHub repo and has a
    // pending update, block the launch until the user reviews and applies it.
    try {
      const status = await api.transport.github.checkUpdate(launchTarget);
      if (status.updateAvailable) {
        updateGateOpen = true;
        launchAfterGate = true;
        return;
      }
    } catch {
      // Not a GitHub-transported pack — launch normally.
    }
    await doLaunch(launchTarget);
  }

  async function doLaunch(path: string) {
    const result = await launchWithFeedback({ path, profile: "client" });
    // null = launch failed (toast shown); otherwise hold spinner until exit.
    if (result) launchHoldPath = path;
    const project = $recentProjects.find((p) => p.path === path);
    if (project) recentProjects.add(project);
    void api.stats.get(path).then((s) => {
      homeStats.update((prev) => ({
        ...prev,
        [path]: {
          playtime: s.totalPlaytimeSeconds ?? 0,
          lastLaunch: s.lastLaunch ?? null,
        },
      }));
    }).catch(() => {});
  }

  async function stopGame() {
    if (!selectedPath) return;
    await killWithFeedback(selectedPath);
  }

  function openSettings() {
    currentView = "project-settings";
  }

  function closeHeroOverflow() {
    heroOverflowOpen = false;
  }

  function toggleHeroOverflow() {
    if (heroActionBusy) return;
    heroOverflowOpen = !heroOverflowOpen;
  }

  function openRenamePrompt() {
    if (!selectedProject || heroActionBusy) return;
    closeHeroOverflow();
    renameDefault = selectedProject.info.name;
    showRenamePrompt = true;
  }

  function openClonePrompt() {
    if (!selectedProject || heroActionBusy) return;
    closeHeroOverflow();
    cloneDefault = `${selectedProject.info.name} copy`;
    showClonePrompt = true;
  }

  async function confirmRename(newName: string) {
    showRenamePrompt = false;
    const project = selectedProject;
    const name = newName.trim();
    if (!project || !name) return;
    if (name === project.info.name) return;
    heroActionBusy = true;
    try {
      const listing = await api.project.getListing(project.path);
      await api.project.updateListing({ ...listing, name }, project.path);
      const info = await api.project.validate(project.path);
      const updated: RecentProject = {
        path: project.path,
        info: info as RecentProject["info"],
      };
      recentProjects.add(updated, { reorder: false });
      projectInfo.set(updated.info);
      toasts.success(`Renamed to “${name}”`);
    } catch (e) {
      toasts.error(String(e));
    } finally {
      heroActionBusy = false;
    }
  }

  async function confirmClone(newName: string) {
    showClonePrompt = false;
    const project = selectedProject;
    const name = newName.trim();
    if (!project || !name) return;
    heroActionBusy = true;
    try {
      const clonedPath = await api.files.cloneProject(name, project.path);
      await loadProject(clonedPath);
      toasts.success(`Cloned to “${name}”`);
    } catch (e) {
      toasts.error(String(e));
    } finally {
      heroActionBusy = false;
    }
  }

  async function exportSelected(kind: "mrpack" | "server") {
    const project = selectedProject;
    if (!project || heroActionBusy) return;
    closeHeroOverflow();
    heroActionBusy = true;
    try {
      const exported =
        kind === "server"
          ? await api.export.serverPack(null, project.path)
          : await api.export.modrinthPack(null, project.path);
      const label = kind === "server" ? "server pack" : ".mrpack";
      try {
        await copyText(exported.path);
        toasts.success(`Exported ${label} — path copied: ${exported.path}`);
      } catch {
        toasts.success(`Exported ${label}: ${exported.path}`);
      }
    } catch (e) {
      toasts.error(String(e));
    } finally {
      heroActionBusy = false;
    }
  }

  async function repairSelected() {
    const project = selectedProject;
    if (!project || heroActionBusy) return;
    closeHeroOverflow();
    heroActionBusy = true;
    try {
      const report = await api.project.repair(project.path);
      const downloaded = report.downloaded?.length ?? 0;
      const failed = report.failed?.length ?? 0;
      if (failed > 0) {
        toasts.warning(`Repair: ${downloaded} re-downloaded, ${failed} failed — see Content.`, 8000);
      } else if (downloaded > 0) {
        toasts.success(`Repair: ${downloaded} file${downloaded === 1 ? "" : "s"} re-downloaded.`);
      } else {
        toasts.success("All mod files present and valid.");
      }
    } catch (e) {
      toasts.error(String(e));
    } finally {
      heroActionBusy = false;
    }
  }

  async function logsZipSelected() {
    const project = selectedProject;
    if (!project || heroActionBusy) return;
    closeHeroOverflow();
    heroActionBusy = true;
    try {
      const zipPath = await api.logs.createZip(project.path);
      try {
        await copyText(zipPath);
        toasts.success(`logs.zip created — path copied: ${zipPath}`);
      } catch {
        toasts.success(`logs.zip created: ${zipPath}`);
      }
    } catch (e) {
      toasts.error(String(e));
    } finally {
      heroActionBusy = false;
    }
  }

  // Post-create flow: AddInstanceModal sets optimizeAfterCreate when the user
  // asked for performance mods — open IDE → Content. Mods is the sole
  // consumer (opens the wizard once Content mounts, then clears the request),
  // so this effect only navigates and never clears.
  $effect(() => {
    const pending = $optimizeAfterCreate;
    if (!pending || !selectedPath) return;
    if (normalizeInstancePath(pending) !== normalizeInstancePath(selectedPath)) return;
    ideStageRequest.set("content");
    currentView = "ide";
  });

  async function deleteSelectedInstance() {
    const project = selectedProject;
    if (!project || heroActionBusy) return;
    closeHeroOverflow();
    const ok = await confirm(`Delete "${project.info.name}" from disk?`, {
      title: "Delete instance",
      kind: "warning",
    });
    if (!ok) return;
    heroActionBusy = true;
    try {
      await api.files.deleteProject(project.path);
      const next = $recentProjects.find((p) => p.path !== project.path) ?? null;
      recentProjects.remove(project.path);
      selectedPath = next?.path ?? null;
      projectPath.set(selectedPath);
      projectInfo.set(next?.info ?? null);
      toasts.success(`Deleted “${project.info.name}”`);
    } catch (e) {
      toasts.error(String(e));
    } finally {
      heroActionBusy = false;
    }
  }
</script>

<div class="home fade-slide-in" data-home-backdrop={homeBackdropOn ? "on" : "off"}>
  <!-- Tailwind layout: 2-column grid (fluid + 320px side rail), collapses on narrow windows.
       Arbitrary value classes are written statically so the Tailwind v4 scanner sees them. -->
  <div class="grid grid-cols-1 home-2col gap-6 items-start">
    <Stack direction="col" gap="5" class="home-main min-w-0 overflow-visible {youtubeFullOnHome ? 'min-h-[calc(100dvh-6.5rem)]' : ''}">
      {#if $projectPath}
        <GithubPackUpdateBanner />
      {/if}
      <HomeHero
        hasSelection={!!selectedProject}
        emptyZero={$recentProjects.length === 0}
        title={selectedProject?.info.name ?? ""}
        meta={selectedInstanceMeta}
        launching={launchingHeld}
        launchMessage={$launchProgress?.message ?? ""}
        launchPercent={$launchProgress?.percent ?? null}
        running={selectedRunning}
        playDisabled={!selectedPath}
        coverUrl={posterCover.url}
        coverKind={posterCover.kind}
        potato={potatoPc}
        actionBusy={heroActionBusy}
        overflowOpen={heroOverflowOpen}
        signedIn={$authState.loggedIn}
        accounts={$authState.accounts}
        accountSkins={accountSkinPaths}
        activeAccountUuid={$authState.activeAccountUuid ?? null}
        accountSwitchBusy={accountSwitchBusy}
        onSwitchAccount={(uuid) => void switchHomeAccount(uuid)}
        crashBanner={crashFixBanner}
        crashFixBusy={crashFixBusy}
        softVerifyRemainingSecs={softVerifyRemainingSecs}
        onPlay={launch}
        onStop={stopGame}
        onEditIn={() => {
          ideStageRequest.set("content");
          currentView = "ide";
        }}
        onSettings={openSettings}
        onFolder={() => {
          if (selectedProject) void invoke("open_project_folder", { path: selectedProject.path });
        }}
        onToggleOverflow={toggleHeroOverflow}
        onRename={openRenamePrompt}
        onClone={openClonePrompt}
        onExportMrpack={() => void exportSelected("mrpack")}
        onExportServer={() => void exportSelected("server")}
        onRepair={() => void repairSelected()}
        onLogsZip={() => void logsZipSelected()}
        onDelete={() => void deleteSelectedInstance()}
        onCreate={() => openAddInstance("blank")}
        onImport={() => openAddInstance("import")}
        onBrowse={browseLibrary}
        onRollback={() => void onRollbackCrashFix()}
        onDiagnostics={() => (currentView = "diagnostics")}
        onSignIn={() => loginModalOpen.set(true)}
      />

      <HomeInstanceShelf
        selectedPath={selectedPath}
        potato={potatoPc}
        showPlacementToggle={!youtubeOnHome}
        onselect={(path) => void selectProject(path)}
        onlibrary={() => (currentView = "library")}
      />

      {#if youtubeOnHome && !youtubeBesideSkin}
        <div class="home-feed">
          <YoutubeFeed variant="row" />
        </div>
      {/if}
    </Stack>

    <aside class="home-side flex flex-col gap-4 w-full max-w-full lg:sticky lg:top-5 self-start" class:has-feed={youtubeBesideSkin}>
      <div class="skin-panel" aria-busy={!authReady}>
        {#if !authReady}
          <div class="skin-skel" aria-hidden="true">
            <div class="skin-skel-canvas skeleton skeleton-block skeleton-card"></div>
            <div class="skin-skel-footer">
              <span class="skeleton skeleton-block skeleton-round" style="width: 72px; height: 22px;"></span>
              <span class="skeleton skeleton-block skeleton-round" style="width: 88px; height: 28px;"></span>
            </div>
            <div class="skin-skel-name skeleton skeleton-block skeleton-line medium" style="width: 40%; height: 14px; margin: 0 auto 12px;"></div>
            <div class="skin-skel-cape">
              <span class="skeleton skeleton-block skeleton-line short" style="width: 90px; height: 10px; margin-bottom: 10px;"></span>
              <div class="skin-skel-cape-row home-skel-stagger">
                {#each Array(3) as _, i (i)}
                  <span class="skeleton skeleton-block skeleton-round" style={`--i: ${i}; width: 100%; height: 36px;`}></span>
                {/each}
              </div>
            </div>
          </div>
        {:else if $authState.loggedIn && $authState.profile}
          {#if potatoPc}
            <div class="flex flex-col items-center justify-center gap-3 px-6 py-8 min-h-[400px] bg-[var(--bg-primary)]">
              <HeadAvatar skinSrc={$skinPath} size={skinAvatarSize} alt={$authState.profile.name} />
              <span class="skin-static-name" style={`font-size: ${skinNameFontPx}px`}>{$authState.profile.name}</span>
            </div>
          {:else}
          <SkinPreview3D
            skinUrl={skinUrl}
            capeUrl={capeUrl}
            cachedPath={$skinPath}
            accountKey={accountKey}
            playerName={$authState.profile.name}
            showName={false}
            width={318}
            height={skinPreviewHeight}
          />
          {/if}
          <div
            class="skin-player-name text-center overflow-hidden whitespace-nowrap text-ellipsis box-border max-w-full -mt-1 pb-3 px-2.5"
            title={$authState.profile.name}
            style={`font-size: ${skinNameFontPx}px`}
          >
            {$authState.profile.name}
          </div>
        {:else}
          <Stack direction="col" gap="3" align="center" class="skin-panel-empty text-center px-6 py-[60px]">
            <User size={48} aria-hidden="true" />
            <h2 class="m-0 text-[15px] font-semibold text-[var(--text-primary)]">Not signed in</h2>
            <p class="m-0 max-w-[220px] text-[13px] leading-normal text-[var(--text-muted)]">
              Sign in with Microsoft or an offline account to play.
            </p>
            <button class="action-btn accent" onclick={() => loginModalOpen.set(true)}>
              <LogIn size={16} />
              Sign In
            </button>
            <button
              type="button"
              class="mt-1 p-0 border-none bg-transparent text-[var(--text-muted)] text-xs font-medium cursor-pointer underline underline-offset-2 hover:text-[var(--text-secondary)]"
              onclick={() => loginModalOpen.set(true)}
            >
              More sign-in options
            </button>
          </Stack>
        {/if}
      </div>

      {#if youtubeBesideSkin}
        <div class="home-feed-rail">
          <YoutubeFeed variant="rail" />
        </div>
      {/if}
    </aside>
  </div>
</div>

{#if updateGateOpen && selectedPath}
  <GithubPackUpdateGate
    targetPath={selectedPath}
    oncontinue={() => {
      updateGateOpen = false;
      const path = selectedPath;
      if (path && launchAfterGate) {
        launchAfterGate = false;
        void doLaunch(path);
      }
    }}
    oncancel={() => {
      updateGateOpen = false;
      launchAfterGate = false;
    }}
  />
{/if}

{#if $newProjectOpen}
  <AddInstanceModal
    initialMode={$addInstanceMode}
    onclose={() => (newProjectOpen.set(false))}
    oncreated={(path) => loadProject(path)}
  />
{/if}

{#if showRenamePrompt && selectedProject}
  <PromptDialog
    title="Rename instance"
    message={`Rename “${selectedProject.info.name}”`}
    mode="text"
    defaultValue={renameDefault}
    confirmLabel="Rename"
    onconfirm={(v) => void confirmRename(v)}
    oncancel={() => (showRenamePrompt = false)}
  />
{/if}

{#if showClonePrompt && selectedProject}
  <PromptDialog
    title="Clone instance"
    message={`Create a copy of “${selectedProject.info.name}”`}
    mode="text"
    defaultValue={cloneDefault}
    confirmLabel="Clone"
    onconfirm={(v) => void confirmClone(v)}
    oncancel={() => (showClonePrompt = false)}
  />
{/if}

<style>
  .home {
    /* Full-bleed on laptops, centered capped column on 1440p+. */
    max-width: 1520px;
    margin: 0 auto;
  }

  /* ─── Main layout: Tailwind grid + arbitrary template (see markup) ───
     grid-cols-[minmax(0,1fr)_320px] at ≥1024px, single column below. */
  .home-2col {
    grid-template-columns: minmax(0, 1fr);
  }
  @media (min-width: 1024px) {
    .home-2col {
      grid-template-columns: minmax(0, 1fr) 320px;
    }
  }

  /* .home-side.has-feed and the .home-main min-height rail are expressed with
     Tailwind/conditional classes in markup. */

  .home-feed:has(:global(.youtube-feed.is-full)) {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .home-feed :global(.youtube-feed) {
    min-width: 0;
    width: 100%;
    background: var(--glass-bg);
    border: 1px solid var(--glass-border);
    -webkit-backdrop-filter: blur(var(--glass-blur)) saturate(var(--glass-saturate));
    backdrop-filter: blur(var(--glass-blur)) saturate(var(--glass-saturate));
    box-shadow:
      var(--shadow-md),
      inset 0 1px 0 var(--glass-highlight);
    border-radius: var(--border-radius-xl);
    padding: 10px 14px;
  }

  .home-feed :global(.youtube-feed.is-full) {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    height: calc(100dvh - 6.5rem);
    overflow: hidden;
  }

  .home-feed :global(.youtube-feed.is-collapsed) {
    padding: 8px 14px;
  }

  .home-feed :global(.youtube-feed.is-collapsed .section-header) {
    min-height: 36px;
  }

  .home-feed :global(.youtube-feed.is-collapsed .section-header-main h2) {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-secondary);
  }

  .home-feed :global(.youtube-feed.is-collapsed .section-header-main svg) {
    width: 16px;
    height: 16px;
  }

  .home-feed :global(.youtube-feed .feed-row) {
    max-width: 100%;
  }

  /* .home-side layout (flex-col, gap, sticky) moved to Tailwind classes in markup;
     .has-feed override stays below. */

  .home-feed-rail {
    min-width: 0;
    width: 100%;
  }

  .home-feed-rail :global(.youtube-feed) {
    min-width: 0;
    width: 100%;
    background: var(--glass-bg);
    border: 1px solid var(--glass-border);
    -webkit-backdrop-filter: blur(var(--glass-blur)) saturate(var(--glass-saturate));
    backdrop-filter: blur(var(--glass-blur)) saturate(var(--glass-saturate));
    box-shadow:
      var(--shadow-md),
      inset 0 1px 0 var(--glass-highlight);
    border-radius: var(--border-radius-xl);
    padding: 10px 14px;
  }

  .skin-panel {
    background: var(--glass-bg);
    border: 1px solid var(--glass-border);
    -webkit-backdrop-filter: blur(var(--glass-blur)) saturate(var(--glass-saturate));
    backdrop-filter: blur(var(--glass-blur)) saturate(var(--glass-saturate));
    border-radius: var(--border-radius-xl);
    box-shadow:
      var(--shadow-md),
      inset 0 1px 0 var(--glass-highlight);
    overflow: hidden;
    display: flex;
    flex-direction: column;
    /* Don't let yt-under-skin rail compress the 3D preview (overflow clips the model). */
    flex-shrink: 0;
  }

  /* Keep the canvas frame from being flexed/squashed inside the panel;
     fill the panel width and drop nested rounding (panel already clips). */
  .skin-panel :global(.skin-3d-wrap),
  .skin-panel :global(.skin-3d-container) {
    flex-shrink: 0;
    width: 100% !important;
    max-width: 100%;
    border-radius: 0;
    border: none !important;
    box-shadow: none !important;
  }

  .skin-panel :global(.skin-3d-wrap) {
    align-items: stretch;
  }

  .skin-player-name {
    font-family: var(--font-minecraft);
    font-weight: 400;
    font-size: 12px;
    line-height: 1.4;
    letter-spacing: 0.5px;
    color: var(--mc-nick-color, var(--text-primary));
    text-shadow: var(
      --mc-nick-shadow,
      2px 2px 0 color-mix(in srgb, var(--text-primary) 18%, #3f3f3f),
      -1px 0 0 color-mix(in srgb, var(--bg-primary) 70%, #000),
      1px 0 0 color-mix(in srgb, var(--bg-primary) 70%, #000),
      0 -1px 0 color-mix(in srgb, var(--bg-primary) 70%, #000),
      0 1px 0 color-mix(in srgb, var(--bg-primary) 70%, #000)
    );
    text-align: center;
    padding: 0 10px 12px;
    margin-top: -4px;
    max-width: 100%;
    box-sizing: border-box;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  /* skin-panel-empty* layouts moved to Tailwind classes in markup. */

  .skin-static-name {
    font-family: var(--font-minecraft);
    font-size: 12px;
    letter-spacing: 0.5px;
    color: var(--text-primary);
  }

  .skin-skel {
    display: flex;
    flex-direction: column;
    width: 100%;
  }

  .skin-skel-canvas {
    width: 100%;
    height: 400px;
    border-radius: 0;
  }

  .skin-skel-footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 16px;
    border-top: 1px solid var(--border-color);
    gap: 8px;
  }

  .skin-skel-cape {
    padding: 12px 16px 16px;
    border-top: 1px solid var(--border-color);
  }

  .skin-skel-cape-row {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .action-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 14px;
    border-radius: var(--border-radius-md);
    background: var(--bg-secondary);
    border: 1px solid var(--border-color);
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 600;
    transition: all 0.15s ease;
  }

  .action-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .action-btn.accent {
    background: var(--accent-primary);
    color: var(--on-accent, #000);
    border-color: transparent;
  }

  .action-btn.accent:hover {
    background: var(--accent-hover);
  }
</style>
