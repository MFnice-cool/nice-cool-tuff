import { afterEach, describe, expect, it, vi } from "vitest";
import {
  APP_HIDDEN_CLASS,
  installAppHiddenSync,
  TASK_POLL_ACTIVE_MS,
  TASK_POLL_IDLE_MS,
  taskPollIntervalMs,
} from "./idlePerf";

function setVisibility(value: DocumentVisibilityState) {
  Object.defineProperty(document, "visibilityState", {
    configurable: true,
    get: () => value,
  });
}

describe("taskPollIntervalMs", () => {
  it("polls responsively while background work is visible", () => {
    expect(taskPollIntervalMs(1)).toBe(TASK_POLL_ACTIVE_MS);
    expect(taskPollIntervalMs(7)).toBe(TASK_POLL_ACTIVE_MS);
  });

  it("relaxes when the panel has nothing to show", () => {
    expect(taskPollIntervalMs(0)).toBe(TASK_POLL_IDLE_MS);
  });

  it("never returns the old 800ms hotspot cadence", () => {
    expect(TASK_POLL_ACTIVE_MS).toBeGreaterThan(800);
  });
});

describe("installAppHiddenSync", () => {
  afterEach(() => {
    setVisibility("visible");
    document.body.classList.remove(APP_HIDDEN_CLASS);
    vi.restoreAllMocks();
  });

  it("marks the body while the window is hidden and clears it when visible", () => {
    setVisibility("hidden");
    const cleanup = installAppHiddenSync();
    expect(document.body.classList.contains(APP_HIDDEN_CLASS)).toBe(true);

    setVisibility("visible");
    document.dispatchEvent(new Event("visibilitychange"));
    expect(document.body.classList.contains(APP_HIDDEN_CLASS)).toBe(false);

    setVisibility("hidden");
    document.dispatchEvent(new Event("visibilitychange"));
    expect(document.body.classList.contains(APP_HIDDEN_CLASS)).toBe(true);

    cleanup();
    expect(document.body.classList.contains(APP_HIDDEN_CLASS)).toBe(false);
  });

  it("cleanup removes the listener — later changes do not touch the body", () => {
    setVisibility("visible");
    const cleanup = installAppHiddenSync();
    cleanup();
    setVisibility("hidden");
    document.dispatchEvent(new Event("visibilitychange"));
    expect(document.body.classList.contains(APP_HIDDEN_CLASS)).toBe(false);
  });
});
