import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";

const storage = new Map<string, string>();

let fixPrefs: typeof import("./fixPreferences");

beforeAll(async () => {
  storage.clear();
  vi.stubGlobal("localStorage", {
    getItem: (k: string) => storage.get(k) ?? null,
    setItem: (k: string, v: string) => {
      storage.set(k, v);
    },
    removeItem: (k: string) => {
      storage.delete(k);
    },
  });
  vi.resetModules();
  fixPrefs = await import("./fixPreferences");
});

afterAll(() => {
  vi.unstubAllGlobals();
});

describe("changeOptionKey", () => {
  it("maps a backend ChangeOption to a stable key", () => {
    expect(
      fixPrefs.changeOptionKey({
        label: "Disable Sodium",
        keepMod: "spb-revamped",
        actions: [{ action: "DisableMod", nodeId: "mod:sodium" }],
      }),
    ).toBe("disablemod:sodium");
  });

  it("falls back to label when no machine action is present", () => {
    expect(
      fixPrefs.changeOptionKey({ label: "Update something" }),
    ).toBe("fix:Update something");
  });
});

describe("fix preference persistence", () => {
  it("remembers a chosen option per fingerprint", () => {
    fixPrefs.setFixPreference("fp-abc", "disablemod:sodium");
    expect(fixPrefs.getFixPreference("fp-abc")).toBe("disablemod:sodium");
    // A different crash keeps its own (unset) preference.
    expect(fixPrefs.getFixPreference("fp-other")).toBeNull();
  });

  it("persists to localStorage and survives a module reload", async () => {
    fixPrefs.setFixPreference("fp-crash-2", "keep:spb-revamped");
    expect(storage.get("tuffbox.fix-prefs")).toContain("fp-crash-2");

    // Simulate app restart: fresh module state re-reads localStorage.
    vi.resetModules();
    const reloaded = await import("./fixPreferences");
    expect(reloaded.getFixPreference("fp-crash-2")).toBe("keep:spb-revamped");
    expect(reloaded.getFixPreference("fp-abc")).toBe("disablemod:sodium");
  });

  it("ignores empty keys", () => {
    fixPrefs.setFixPreference(null, "disable:whatever");
    fixPrefs.setFixPreference("", "disable:whatever");
    fixPrefs.setFixPreference("fp-x", "");
    expect(fixPrefs.getFixPreference("fp-x")).toBeNull();
  });
});