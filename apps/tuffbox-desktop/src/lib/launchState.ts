export type LaunchSessionPhase =
  | "authorizing"
  | "preparing"
  | "starting"
  | "running"
  | "failed";

export type RetryLaunchParams = {
  path: string;
  profile?: string;
  quickPlayType?: string | null;
  quickPlayValue?: string | null;
  memoryMbOverride?: number | null;
  serverDir?: string | null;
  levelSeed?: string | null;
  onlineMode?: boolean | null;
};

export type LaunchSessionError = {
  kind: string;
  message: string;
  logPath?: string | null;
};

export type LaunchSession = {
  instanceId: string;
  profileId: string;
  phase: LaunchSessionPhase;
  progress: {
    phase: string;
    message: string;
    percent: number | null;
  } | null;
  pid: number | null;
  retry: RetryLaunchParams;
  error: LaunchSessionError | null;
};

export type LaunchSessionEvent =
  | {
      type: "progress";
      instanceId: string;
      profileId?: string;
      phase: string;
      message: string;
      percent: number | null;
    }
  | {
      type: "started";
      instanceId: string;
      profileId?: string;
      pid: number;
    }
  | {
      type: "failed";
      instanceId: string;
      profileId?: string;
      error: LaunchSessionError;
    }
  | {
      type: "exited";
      instanceId: string;
      profileId?: string;
      pid?: number | null;
    };

export function normalizeInstanceId(path: string): string {
  return path.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
}

export function createLaunchSession(input: {
  instanceId: string;
  profileId: string;
  retry: RetryLaunchParams;
}): LaunchSession {
  return {
    instanceId: input.instanceId,
    profileId: input.profileId,
    phase: "authorizing",
    progress: null,
    pid: null,
    retry: { ...input.retry },
    error: null,
  };
}

export function canStartLaunch(
  active: LaunchSession | null,
  _instanceId: string,
): boolean {
  return active == null || active.phase === "failed";
}

function eventBelongsToSession(
  session: LaunchSession,
  event: LaunchSessionEvent,
): boolean {
  if (normalizeInstanceId(session.instanceId) !== normalizeInstanceId(event.instanceId)) {
    return false;
  }
  return !event.profileId || event.profileId === session.profileId;
}

export function reduceLaunchSession(
  session: LaunchSession | null,
  event: LaunchSessionEvent,
): LaunchSession | null {
  if (!session || !eventBelongsToSession(session, event)) return session;

  switch (event.type) {
    case "progress":
      if (session.phase === "running" || session.phase === "failed") return session;
      return {
        ...session,
        phase: event.phase === "starting" ? "starting" : "preparing",
        progress: {
          phase: event.phase,
          message: event.message,
          percent: event.percent,
        },
      };
    case "started":
      if (session.phase === "failed") return session;
      return {
        ...session,
        phase: "running",
        pid: event.pid,
        progress: {
          phase: "running",
          message: "Running",
          percent: 100,
        },
      };
    case "failed":
      return {
        ...session,
        phase: "failed",
        error: event.error,
        progress: null,
      };
    case "exited":
      if (
        session.pid != null &&
        event.pid != null &&
        session.pid !== event.pid
      ) {
        return session;
      }
      return null;
  }
}

/** Renew the MSA token when less than this remains (tokens live ~24h). */
export const TOKEN_REFRESH_MARGIN_S = 10 * 60;

/**
 * True when a logged-in session's expiry is close enough (or already past)
 * that launching should renew it first. `null` expiry (offline / Yggdrasil
 * accounts) never needs a renewal.
 */
export function needsTokenRefresh(
  expiresAt: number | null | undefined,
  nowSec: number = Math.floor(Date.now() / 1000),
): boolean {
  if (expiresAt == null) return false;
  return expiresAt - nowSec < TOKEN_REFRESH_MARGIN_S;
}
