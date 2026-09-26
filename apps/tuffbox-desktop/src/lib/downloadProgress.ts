import { writable, get } from "svelte/store";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type DownloadItemStatus = "queued" | "downloading" | "done" | "failed" | "skipped" | string;

export interface DownloadItem {
  id: string;
  name: string;
  downloaded: number;
  total: number;
  percent: number;
  status: DownloadItemStatus;
  error?: string | null;
}

export interface ModUpdateProgress {
  phase: string;
  message: string;
  current: number;
  total: number;
  percent: number;
  modId?: string | null;
}

export interface DownloadBatch {
  phase: string;
  items?: DownloadItem[];
  downloaded?: string[];
  failed?: { modId: string; error: string }[];
  alreadyPresent?: string[];
  skipped?: string[];
  scopeModIds?: string[];
  batchComplete?: boolean;
}

// ─── Shared download / update progress ──────────────────────────────
//
// Single source of truth for `mod-download-batch`, `mod-download-progress`
// and `mod-update-progress`. Both the Mods pane and the dependency-install
// flow (Graph) read from here instead of each registering its own listeners
// and duplicating the download UX (spec: единый progress с TaskProgressPanel).

export const downloadItems = writable<DownloadItem[]>([]);
export const downloadOpen = writable(false);
export const downloadDone = writable(false);
export const downloadTitle = writable("Downloading content");
export const downloadStageMessage = writable("Preparing downloads…");
export const downloadStagePercent = writable(0);
export const downloadError = writable<string | null>(null);
/** Stable scope: when set, only these mod ids update the progress overlay. */
export const downloadScopeModIds = writable<Set<string> | null>(null);

/** A single batch "job" id — frontend retry scopes requests by it. */
export const downloadJobId = writable<string | null>(null);

let registered: Promise<UnlistenFn[]> | null = null;

export function upsertDownloadItem(item: DownloadItem) {
  downloadItems.update((list) => {
    const idx = list.findIndex((i) => i.id === item.id);
    if (idx >= 0) {
      const next = [...list];
      next[idx] = { ...next[idx], ...item };
      return next;
    }
    return [...list, item];
  });
}

/** Open the progress overlay for a new batch, optionally scoped to ids. */
export function openDownloadOverlay(title: string, scopeIds?: string[] | null) {
  downloadTitle.set(title);
  downloadOpen.set(true);
  downloadDone.set(false);
  downloadStageMessage.set("Preparing downloads…");
  downloadStagePercent.set(0);
  downloadError.set(null);
  downloadScopeModIds.set(scopeIds?.length ? new Set(scopeIds) : null);
  downloadItems.set([]);
  downloadJobId.set(`job-${Date.now()}`);
}

export function closeDownloadOverlay() {
  downloadOpen.set(false);
  downloadScopeModIds.set(null);
  downloadJobId.set(null);
}

/**
 * Register the global download/update listeners exactly once. Returns an
 * unlisten function that tears them all down (idempotent).
 */
export function registerDownloadListeners(): Promise<UnlistenFn[]> {
  if (registered) return registered;
  registered = (async () => {
    const scope = () => get(downloadScopeModIds);
    const openIfNeeded = () => {
      if (!get(downloadOpen)) {
        downloadOpen.set(true);
        downloadDone.set(false);
      }
    };

    const batch = await listen<DownloadBatch>("mod-download-batch", (event) => {
      const payload = event.payload;
      if (payload.phase === "start") {
        downloadOpen.set(true);
        downloadDone.set(false);
        downloadStageMessage.set("Preparing downloads…");
        downloadStagePercent.set(0);
        const scoped = payload.scopeModIds?.length ? new Set(payload.scopeModIds) : null;
        if (scoped) downloadScopeModIds.set(scoped);
        downloadItems.set((payload.items ?? []).map((item) => ({
          id: item.id,
          name: item.name,
          downloaded: item.downloaded ?? 0,
          total: item.total ?? 0,
          percent: item.percent ?? 0,
          status: item.status ?? "queued",
        })));
      } else if (payload.phase === "done") {
        const downloadedIds = new Set(payload.downloaded ?? []);
        const alreadyPresentIds = new Set(payload.alreadyPresent ?? []);
        const skippedIds = new Set(payload.skipped ?? []);
        const failedById = new Map((payload.failed ?? []).map((failure) => [failure.modId, failure.error]));
        const failedIds = new Set(failedById.keys());
        const successfulIds = new Set([...downloadedIds, ...alreadyPresentIds, ...skippedIds]);

        downloadItems.update((items) =>
          items.map((item) => {
            if (skippedIds.has(item.id)) return { ...item, status: "skipped", percent: 100 };
            if (downloadedIds.has(item.id) || alreadyPresentIds.has(item.id)) {
              return { ...item, status: "done", percent: 100 };
            }
            if (
              failedIds.has(item.id) ||
              ((item.status === "queued" || item.status === "downloading") && !successfulIds.has(item.id))
            ) {
              return { ...item, status: "failed", percent: 0, error: failedById.get(item.id) ?? "The download did not complete." };
            }
            return item;
          }),
        );

        if (payload.batchComplete !== false) {
          downloadDone.set(true);
          downloadStagePercent.set(100);
          const failed = get(downloadItems).filter((item) => item.status === "failed").length;
          downloadStageMessage.set(
            failed > 0 ? `Downloads finished with ${failed} failure${failed > 1 ? "s" : ""}.` : "Downloads complete.",
          );
          downloadError.set(
            failed > 0
              ? (payload.failed ?? []).map((failure) => `${failure.modId}: ${failure.error}`).join("\n")
              : null,
          );
          if (failed === 0) {
            setTimeout(() => {
              if (get(downloadDone)) closeDownloadOverlay();
            }, 900);
          }
        }
      }
    });

    const progress = await listen<DownloadItem>("mod-download-progress", (event) => {
      const scoped = scope();
      if (scoped && !scoped.has(event.payload.id)) return;
      openIfNeeded();
      upsertDownloadItem(event.payload);
    });

    const updateProgress = await listen<ModUpdateProgress>("mod-update-progress", (event) => {
      const payload = event.payload;
      downloadStageMessage.set(payload.message);
      downloadStagePercent.set(Math.max(0, Math.min(100, payload.percent)));
      if (!get(downloadOpen)) {
        downloadOpen.set(true);
        downloadDone.set(payload.phase === "done");
      }
    });

    return [batch, progress, updateProgress];
  })();
  return registered;
}

/** Drop the shared registration (e.g. on app teardown). */
export function unregisterDownloadListeners() {
  if (!registered) return;
  void registered.then((unlisteners) => {
    for (const u of unlisteners) u();
  });
  registered = null;
}
