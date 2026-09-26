import { describe, expect, it } from "vitest";
import {
  formatBytes,
  formatDayStamp,
  formatStamp,
  matchesModFilter,
  mergeUpdates,
  sortMods,
  sourceChip,
  updateLabel,
  versionOptions,
  type ModRow,
} from "./instanceMods";

function row(over: Partial<ModRow> = {}): ModRow {
  return { id: "a", name: "Alpha", version: "1.0.0", ...over };
}

describe("sourceChip", () => {
  it("maps known sources and capitalizes unknown ones", () => {
    expect(sourceChip("modrinth")).toBe("Modrinth");
    expect(sourceChip("CurseForge")).toBe("CurseForge");
    expect(sourceChip("local")).toBe("Local");
    expect(sourceChip(null)).toBe("Local");
    expect(sourceChip("weird")).toBe("Weird");
  });
});

describe("matchesModFilter", () => {
  const mod = { name: "Sodium Extra", fileName: "sodium-extra-0.5.4.jar", version: "0.5.4" };
  it("matches name, file name and version case-insensitively", () => {
    expect(matchesModFilter(mod, "sodium")).toBe(true);
    expect(matchesModFilter(mod, "EXTRA")).toBe(true);
    expect(matchesModFilter(mod, "0.5.4.jar")).toBe(true);
    expect(matchesModFilter(mod, "optifine")).toBe(false);
  });
  it("empty query matches everything", () => {
    expect(matchesModFilter(mod, "")).toBe(true);
    expect(matchesModFilter(mod, "   ")).toBe(true);
  });
});

describe("sortMods", () => {
  const rows = [
    { ...row({ id: "1", name: "Charlie" }) },
    { ...row({ id: "2", name: "alpha", source: "modrinth" }) },
    { ...row({ id: "3", name: "Beta", source: "curseforge" }), hasUpdate: true },
  ];
  it("sorts by name (locale compare)", () => {
    expect(sortMods(rows, "name").map((r) => r.id)).toEqual(["2", "3", "1"]);
  });
  it("sorts by source chip then name", () => {
    // CurseForge < Local < Modrinth in locale order.
    expect(sortMods(rows, "source").map((r) => r.id)).toEqual(["3", "1", "2"]);
  });
  it("floats updated rows to the top", () => {
    expect(sortMods(rows, "updates").map((r) => r.id)).toEqual(["3", "2", "1"]);
  });
  it("does not mutate the input", () => {
    const input = [...rows];
    sortMods(rows, "name");
    expect(rows).toEqual(input);
  });
});

describe("mergeUpdates", () => {
  it("normalizes and indexes updates by modId, skipping junk entries", () => {
    const junk = [{ modId: "" }, null, 5, { latestVersion: "no-id" }] as unknown as Array<
      Record<string, unknown>
    >;
    const map = mergeUpdates([{ modId: "a", latestVersion: "2.0", breakingLoader: true }, ...junk]);
    expect(Object.keys(map)).toEqual(["a"]);
    expect(map.a).toEqual({
      modId: "a",
      latestVersion: "2.0",
      versionId: null,
      fileName: null,
      breakingLoader: true,
      breakingMinecraft: false,
      changelog: null,
    });
  });
  it("tolerates non-array input", () => {
    expect(mergeUpdates(undefined as never)).toEqual({});
  });
});

describe("updateLabel", () => {
  it("appends a breaking marker", () => {
    expect(updateLabel({ modId: "a", latestVersion: "2.0" })).toBe("→ 2.0");
    expect(updateLabel({ modId: "a", latestVersion: "2.0", breakingLoader: true })).toBe(
      "→ 2.0 (breaking)",
    );
    expect(updateLabel(null)).toBe("");
  });
});

describe("formatBytes", () => {
  it("formats the usual ladder", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(2048)).toBe("2.0 KB");
    expect(formatBytes(5 * 1024 * 1024)).toBe("5.0 MB");
    expect(formatBytes(3.5 * 1024 ** 3)).toBe("3.5 GB");
  });
  it("handles missing values", () => {
    expect(formatBytes(null)).toBe("—");
    expect(formatBytes(undefined)).toBe("—");
    expect(formatBytes(-5)).toBe("—");
  });
});

describe("formatDayStamp", () => {
  it("returns never for absent/invalid stamps", () => {
    expect(formatDayStamp(null)).toBe("never");
    expect(formatDayStamp(0)).toBe("never");
    expect(formatDayStamp(Number.NaN)).toBe("never");
  });
  it("formats a valid millisecond stamp", () => {
    expect(formatDayStamp(new Date("2026-03-04T12:00:00Z").getTime())).toMatch(/2026/);
  });
});

describe("formatStamp", () => {
  it("falls back to a dash for missing/invalid input", () => {
    expect(formatStamp(null)).toBe("—");
    expect(formatStamp("not-a-date")).toBe("—");
  });
  it("formats a valid date without throwing", () => {
    expect(formatStamp("2026-01-02T03:04:00Z")).toMatch(/2026/);
  });
});

describe("versionOptions", () => {
  it("normalizes the loose payload and marks the current version", () => {
    const opts = versionOptions(
      [
        { id: "v2", version_number: "1.2.3", version_type: "beta", date_published: "2026-03-04T00:00:00Z" },
        { id: "v1", version_number: "1.2.2" },
        { version_number: "no-id" },
      ],
      "1.2.2",
    );
    expect(opts).toHaveLength(2);
    expect(opts[0]).toEqual({ id: "v2", label: "1.2.3 · beta · 2026-03-04" });
    expect(opts[1].label).toBe("1.2.2 (current)");
  });
  it("tolerates non-array input", () => {
    expect(versionOptions(null as never, "1")).toEqual([]);
  });
});
