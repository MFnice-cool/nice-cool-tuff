import { describe, expect, it } from "vitest";
import {
  collectFlatEntries,
  collectJsonKeys,
  flatCompletionsAt,
  jsonCompletionsAt,
  kubejsCompletionsAt,
} from "./editorAutocomplete";
import { editorThemeFor, isDarkEditorTheme, LIGHT_EDITOR_THEMES } from "./editorTheme";
import { THEMES } from "./themes";

describe("json completions", () => {
  const doc = `{
  "enableShader": true,
  "renderDistance": 12,
  "sodium": {
    "enableShader": false,
    "useChunkFaces": true
  }
}`;

  it("collects document keys without duplicates", () => {
    expect(collectJsonKeys(doc)).toEqual(["enableShader", "renderDistance", "sodium", "useChunkFaces"]);
  });

  it("suggests keys while typing inside a key position", () => {
    // Cursor after `"ena` on a fresh key line inside sodium block.
    const pos = doc.indexOf('"useChunkFaces"');
    const line = '    "ena';
    const edited = doc.slice(0, pos) + line + '"' + doc.slice(pos);
    const res = jsonCompletionsAt(edited, pos + line.length);
    expect(res).not.toBeNull();
    expect(res!.labels).toContain("enableShader");
    expect(res!.from).toBe(pos + 5); // right after the opening quote
  });

  it("suggests literals after a colon outside strings", () => {
    const pos = doc.indexOf('"renderDistance"') + '"renderDistance"'.length + 1; // right after ':'
    const res = jsonCompletionsAt(doc, pos);
    expect(res).not.toBeNull();
    expect(res!.labels).toEqual(["true", "false", "null"]);
  });

  it("suggests existing values inside a value string", () => {
    const pos = doc.indexOf("false");
    const res = jsonCompletionsAt(doc, pos);
    expect(res).not.toBeNull();
    expect(res!.labels).toContain("false");
  });

  it("returns nothing at a random non-string position", () => {
    expect(jsonCompletionsAt(doc, 2)).toBeNull();
  });
});

describe("flat (toml/properties) completions", () => {
  const doc = [
    "# comment = not an entry",
    "maxThreads = 8",
    "useShader = false",
    "",
    "[graphics]",
    "useShader = true",
    "maxThreads = 16",
  ].join("\n");

  it("collects entries per key, deduplicated per value", () => {
    const entries = collectFlatEntries(doc);
    expect(entries.get("useShader")).toEqual(["false", "true"]);
    expect(entries.get("maxThreads")).toEqual(["8", "16"]);
  });

  it("suggests document keys while typing a key", () => {
    const edited = doc + "\nmax";
    const res = flatCompletionsAt(edited, edited.length);
    expect(res).not.toBeNull();
    expect(res!.labels).toContain("maxThreads");
    expect(res!.from).toBe(edited.length - 3);
  });

  it("suggests same-key values after '='", () => {
    const edited = doc + "\nuseShader = ";
    const res = flatCompletionsAt(edited, edited.length);
    expect(res).not.toBeNull();
    expect(res!.labels).toEqual(["false", "true"]);
  });

  it("never completes inside comments", () => {
    const res = flatCompletionsAt(doc, 5);
    expect(res).toBeNull();
  });
});

describe("kubejs completions", () => {
  it("suggests recipe-event methods after 'event.'", () => {
    const doc = "ServerEvents.recipes(event => {\n  event.";
    const res = kubejsCompletionsAt(doc, doc.length);
    expect(res).not.toBeNull();
    expect(res!.labels).toContain("shaped");
    expect(res!.labels).toContain("remove");
    const shaped = res!.extras?.find((e) => e.label === "shaped");
    expect(shaped?.apply).toContain("shaped(");
  });

  it("suggests global buses at a statement start", () => {
    const doc = "// tweak recipes\n\n  ";
    const res = kubejsCompletionsAt(doc, doc.length);
    expect(res).not.toBeNull();
    expect(res!.labels).toContain("ServerEvents");
  });

  it("suggests nothing mid-word", () => {
    const doc = "const x = foo";
    expect(kubejsCompletionsAt(doc, doc.length)).toBeNull();
  });
});

describe("editor theme", () => {
  it("flags the light app themes", () => {
    for (const meta of THEMES) {
      const dark = isDarkEditorTheme(meta.id);
      if (LIGHT_EDITOR_THEMES.has(meta.id)) expect(dark).toBe(false);
      else expect(dark).toBe(true);
    }
  });

  it("defaults to dark for unknown/missing theme ids", () => {
    expect(isDarkEditorTheme(undefined)).toBe(true);
    expect(isDarkEditorTheme("some-future-theme")).toBe(true);
  });

  it("builds a themed extension set without throwing", () => {
    expect(() => editorThemeFor("tuffbox")).not.toThrow();
    expect(() => editorThemeFor("win95")).not.toThrow();
    expect(editorThemeFor("tuffbox").length).toBeGreaterThan(0);
  });
});
