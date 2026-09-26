/** Home / Dashboard bootstrap snapshot + progressive enrich events. */

import { get, writable } from "svelte/store";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { ProjectSummary } from "./api";
import {
  authState,
  launcherSettingsLive,
  projectInfo,
  projectPath,
  recentProjects,
  runningInstances,
  skinPath,
  type AuthState,
  type LauncherSettings,
  type ProjectInfo,
  type RecentProject,
} from "./store";

export type HomeStatsBrief = { playtime: number; lastLaunch: string | null };
export type HomeRunningInstance = {
  id: string;
  pid: number;
  profile: string;
  startedAt: number;
};

export type CrashFixBannerPayload = {
  snapshotId: string;
  fingerprintKey: string;
  planSource?: string | null;
  humanExplanation: string;
  matchedCaseIds: string[];
  actionsSummary: string[];
  createdAt: string;
  resolved: boolean;
  rolledBack: boolean;
  softVerifyStartedUnix?: number | null;
  minPlaytimeSecs: number;
};

export type HomeSnapshot = {
  recent: Array<{ path: string; info: ProjectInfo }>;
  lastOpened: string | null;
  launcherSettings: LauncherSettings;
  auth: AuthState;
  skinPaths: Record<string, string>;
  running: HomeRunningInstance[];
  statsByPath: Record<string, HomeStatsBrief>;
  iconsByPath: Record<string, string>;
  sizesByPath: Record<string, string>;
  selectedSummary: ProjectSummary | null;
};

export type HomeEnrichPayload = {
  selectedSummary?: ProjectSummary | null;
  crashFixBanner?: CrashFixBannerPayload | null;
  clearCrashFixBanner?: boolean;
  iconsByPath?: Record<string, string>;
  sizesByPath?: Record<string, string>;
  statsByPath?: Record<string, HomeStatsBrief>;
  auth?: AuthState;
  skinPaths?: Record<string, string>;
  phase?: string;
};

export const homeStats = writable<Record<string, HomeStatsBrief>>({});
export const homeSizes = writable<Record<string, string>>({});
export const homeIcons = writable<Record<string, string | null>>({});
export const homeSkinPaths = writable<Record<string, string>>({});
export const homeCrashFixBanner = writable<CrashFixBannerPayload | null>(null);
export const homeBootstrapReady = writable(false);

function summaryToProjectInfo(s: ProjectSummary): ProjectInfo {
  return {
    id: s.id,
    name: s.name,
    version: s.version,
    minecraftVersion: s.minecraftVersion,
    loaderKind: s.loaderKind,
    loaderVersion: s.loaderVersion,
    javaPath: s.javaPath,
    memoryMb: s.memoryMb,
    jvmArgs: s.jvmArgs ?? [],
    playerName: s.playerName,
  };
}

function applyStats(map: Record<string, HomeStatsBrief> | undefined) {
  if (!map) return;
  homeStats.update((prev) => ({ ...prev, ...map }));
}

function applySizes(map: Record<string, string> | undefined) {
  if (!map) return;
  homeSizes.update((prev) => ({ ...prev, ...map }));
}

function applyIcons(map: Record<string, string> | undefined) {
  if (!map) return;
  homeIcons.update((prev) => {
    const next = { ...prev };
    for (const [k, v] of Object.entries(map)) next[k] = v;
    return next;
  });
}

function applySkinPaths(map: Record<string, string> | undefined, auth?: AuthState) {
  if (!map) return;
  homeSkinPaths.update((prev) => ({ ...prev, ...map }));
  const uuid = auth?.activeAccountUuid ?? auth?.profile?.uuid;
  if (uuid && map[uuid]) skinPath.set(map[uuid]);
}

/** Apply P0 snapshot into global stores (restore + home rail). */
export function applyHomeSnapshot(snap: HomeSnapshot) {
  if (snap.recent?.length) {
    const disk: RecentProject[] = snap.recent.map((r) => ({
      path: r.path,
      info: r.info as ProjectInfo,
    }));
    const local = get(recentProjects);
    if (local.length === 0) {
      recentProjects.set(disk.slice(0, 20));
    } else {
      const have = new Set(local.map((p) => p.path));
      const next = [...local];
      for (const p of disk) {
        if (!have.has(p.path)) next.push(p);
      }
      recentProjects.set(next.slice(0, 20));
    }
  }

  if (snap.selectedSummary) {
    const manifestPath = snap.selectedSummary.manifestPath || snap.lastOpened;
    if (manifestPath) {
      projectPath.set(manifestPath);
      projectInfo.set(summaryToProjectInfo(snap.selectedSummary));
      recentProjects.add({
        path: manifestPath,
        info: summaryToProjectInfo(snap.selectedSummary),
      });
    }
  } else if (snap.lastOpened) {
    projectPath.set(snap.lastOpened);
  }

  if (snap.launcherSettings) {
    launcherSettingsLive.set(snap.launcherSettings);
  }

  if (snap.auth) {
    authState.set(snap.auth);
  }

  applySkinPaths(snap.skinPaths, snap.auth);
  applyStats(snap.statsByPath);
  applySizes(snap.sizesByPath);
  applyIcons(snap.iconsByPath);

  if (snap.running) {
    runningInstances.set(
      snap.running.map((r) => ({
        id: r.id,
        pid: r.pid,
        profile: r.profile,
        startedAt: r.startedAt,
      })),
    );
  }

  homeBootstrapReady.set(true);
}

export function applyHomeEnrich(payload: HomeEnrichPayload) {
  if (payload.selectedSummary) {
    const s = payload.selectedSummary;
    const manifestPath = s.manifestPath;
    projectPath.set(manifestPath);
    projectInfo.set(summaryToProjectInfo(s));
    recentProjects.add({ path: manifestPath, info: summaryToProjectInfo(s) });
  }
  if (payload.clearCrashFixBanner) {
    homeCrashFixBanner.set(null);
  } else if (payload.crashFixBanner !== undefined && payload.crashFixBanner !== null) {
    homeCrashFixBanner.set(payload.crashFixBanner);
  } else if (payload.crashFixBanner === null) {
    homeCrashFixBanner.set(null);
  }
  applyIcons(payload.iconsByPath);
  applySizes(payload.sizesByPath);
  applyStats(payload.statsByPath);
  if (payload.auth) authState.set(payload.auth);
  applySkinPaths(payload.skinPaths, payload.auth);
}

let enrichUnlisten: UnlistenFn | null = null;

/** Subscribe once to progressive `home:enrich` events. */
export async function ensureHomeEnrichListener(): Promise<() => void> {
  if (enrichUnlisten) return () => {};
  enrichUnlisten = await listen<HomeEnrichPayload>("home:enrich", (event) => {
    applyHomeEnrich(event.payload ?? {});
  });
  return () => {
    enrichUnlisten?.();
    enrichUnlisten = null;
  };
}
