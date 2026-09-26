import { describe, expect, it } from "vitest";
import { get } from "svelte/store";
import { locale, t } from "./i18n";

describe("i18n", () => {
  it("defaults to English and translates", () => {
    locale.set("en");
    expect(get(t)("nav.library")).toBe("Library");
  });

  it("translates Russian keys", () => {
    locale.set("ru");
    expect(get(t)("nav.library")).toBe("Библиотека");
    expect(get(t)("common.play")).toBe("Играть");
  });

  it("fills {token} placeholders", () => {
    locale.set("en");
    expect(get(t)("library.playAria", { name: "ATM9" })).toBe("Play ATM9");
    locale.set("ru");
    expect(get(t)("library.playAria", { name: "ATM9" })).toBe("Играть: ATM9");
  });

  it("falls back to the key for unknown entries and keeps unknown tokens", () => {
    expect(get(t)("no.such.key")).toBe("no.such.key");
    expect(get(t)("library.playAria", { other: 1 })).toBe("Играть: {name}");
  });
});
