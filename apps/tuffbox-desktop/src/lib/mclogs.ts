import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-shell";
import { toasts } from "./toast";

export interface McloShareResult {
  id: string;
  url: string;
  rawUrl?: string | null;
  lines?: number | null;
  size?: number | null;
  fileName?: string | null;
}

/** Upload crash/latest log to mclo.gs. Optional `logName` selects a specific file (`__live__` = Live tab source). */
export async function shareLogToMclogs(
  projectPath: string,
  logName?: string | null,
): Promise<McloShareResult> {
  return invoke<McloShareResult>("share_log_mclogs", {
    path: projectPath,
    logName: logName ?? null,
  });
}

/** Share + toast with Open / Copy URL actions (non-blocking UX). */
export async function shareCrashLogWithFeedback(
  projectPath: string,
  logName?: string | null,
): Promise<McloShareResult | null> {
  toasts.info("Uploading log to mclo.gs…", 4000);
  try {
    const result = await shareLogToMclogs(projectPath, logName);
    toasts.success(`Log shared: ${result.url}`, 16000, [
      {
        label: "Open",
        run: () => {
          open(result.url).catch(() => {});
        },
      },
      {
        label: "Copy URL",
        run: () => {
          void navigator.clipboard.writeText(result.url);
        },
      },
    ]);
    return result;
  } catch (e) {
    toasts.error(`Could not share log: ${e}`);
    return null;
  }
}
