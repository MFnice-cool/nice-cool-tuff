import { describe, expect, it } from "vitest";
import {
  canStartLaunch,
  createLaunchSession,
  normalizeInstanceId,
  needsTokenRefresh,
  reduceLaunchSession,
  TOKEN_REFRESH_MARGIN_S,
} from "./launchState";

describe("normalizeInstanceId", () => {
  it("treats Windows path spelling variants as the same instance", () => {
    expect(normalizeInstanceId("C:\\Dev\\Pack\\tuffbox.json\\")).toBe(
      normalizeInstanceId("c:/dev/pack/tuffbox.json"),
    );
  });
});

describe("launch session transitions", () => {
  it("blocks a second launch while another session is active", () => {
    const active = createLaunchSession({
      instanceId: "C:\\packs\\alpha\\tuffbox.json",
      profileId: "client",
      retry: { path: "C:\\packs\\alpha\\tuffbox.json", profile: "client" },
    });

    expect(canStartLaunch(active, "C:/packs/alpha/tuffbox.json")).toBe(false);
    expect(canStartLaunch(active, "C:/packs/beta/tuffbox.json")).toBe(false);
  });

  it("ignores progress from another instance and stale progress after start", () => {
    const session = createLaunchSession({
      instanceId: "C:/packs/alpha/tuffbox.json",
      profileId: "client",
      retry: { path: "C:/packs/alpha/tuffbox.json", profile: "client" },
    });
    const foreign = reduceLaunchSession(session, {
      type: "progress",
      instanceId: "C:/packs/beta/tuffbox.json",
      profileId: "client",
      phase: "install",
      message: "Installing…",
      percent: 55,
    });
    const running = reduceLaunchSession(foreign, {
      type: "started",
      instanceId: "C:/packs/alpha/tuffbox.json",
      profileId: "client",
      pid: 42,
    });
    const stale = reduceLaunchSession(running, {
      type: "progress",
      instanceId: "C:/packs/alpha/tuffbox.json",
      profileId: "client",
      phase: "starting",
      message: "Starting…",
      percent: 95,
    });

    expect(foreign).toEqual(session);
    expect(running?.phase).toBe("running");
    expect(stale).toEqual(running);
  });

  it("ignores an exit for an older pid and clears the matching session", () => {
    const started = reduceLaunchSession(
      createLaunchSession({
        instanceId: "C:/packs/alpha/tuffbox.json",
        profileId: "client",
        retry: { path: "C:/packs/alpha/tuffbox.json", profile: "client" },
      }),
      {
        type: "started",
        instanceId: "C:/packs/alpha/tuffbox.json",
        profileId: "client",
        pid: 42,
      },
    );

    expect(
      reduceLaunchSession(started, {
        type: "exited",
        instanceId: "C:/packs/alpha/tuffbox.json",
        profileId: "client",
        pid: 41,
      }),
    ).toEqual(started);
    expect(
      reduceLaunchSession(started, {
        type: "exited",
        instanceId: "C:/packs/alpha/tuffbox.json",
        profileId: "client",
        pid: 42,
      }),
    ).toBeNull();
  });

  it("keeps retry ownership on the failed instance", () => {
    const session = createLaunchSession({
      instanceId: "C:/packs/alpha/tuffbox.json",
      profileId: "client",
      retry: {
        path: "C:/packs/alpha/tuffbox.json",
        profile: "client",
        quickPlayType: "multiplayer",
        quickPlayValue: "mc.example.test",
      },
    });
    const failed = reduceLaunchSession(session, {
      type: "failed",
      instanceId: "C:/packs/alpha/tuffbox.json",
      profileId: "client",
      error: { kind: "offline", message: "Network unavailable" },
    });

    expect(failed?.phase).toBe("failed");
    expect(failed?.retry).toEqual({
      path: "C:/packs/alpha/tuffbox.json",
      profile: "client",
      quickPlayType: "multiplayer",
      quickPlayValue: "mc.example.test",
    });
    expect(canStartLaunch(failed, "C:/packs/beta/tuffbox.json")).toBe(true);
  });
});

describe("needsTokenRefresh", () => {
  it("never refreshes sessions without an expiry (offline / yggdrasil)", () => {
    expect(needsTokenRefresh(null)).toBe(false);
    expect(needsTokenRefresh(undefined)).toBe(false);
  });

  it("refreshes when the remaining validity is under the margin", () => {
    const now = 1_000_000;
    expect(needsTokenRefresh(now + 60, now)).toBe(true);
    expect(needsTokenRefresh(now + TOKEN_REFRESH_MARGIN_S - 1, now)).toBe(true);
    expect(needsTokenRefresh(now - 3600, now)).toBe(true);
  });

  it("keeps a fresh session untouched", () => {
    const now = 1_000_000;
    expect(needsTokenRefresh(now + TOKEN_REFRESH_MARGIN_S, now)).toBe(false);
    expect(needsTokenRefresh(now + TOKEN_REFRESH_MARGIN_S + 1, now)).toBe(false);
    expect(needsTokenRefresh(now + 86400, now)).toBe(false);
  });
});
