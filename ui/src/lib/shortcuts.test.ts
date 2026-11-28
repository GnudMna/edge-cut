import { describe, expect, it } from "vitest";

import { shortcutFor } from "./shortcuts";

const key = (k: string, mods: Partial<{ ctrl: boolean; shift: boolean; alt: boolean }> = {}) => ({
  key: k,
  ctrlKey: mods.ctrl ?? false,
  shiftKey: mods.shift ?? false,
  altKey: mods.alt ?? false,
  metaKey: false,
});

describe("shortcutFor", () => {
  it("Ctrl の組み合わせを判定すること", () => {
    expect(shortcutFor(key("o", { ctrl: true }))).toBe("open");
    expect(shortcutFor(key("O", { ctrl: true, shift: true }))).toBe("batchTrim");
    expect(shortcutFor(key("v", { ctrl: true }))).toBe("paste");
    expect(shortcutFor(key("s", { ctrl: true }))).toBe("save");
    expect(shortcutFor(key("c", { ctrl: true }))).toBe("copy");
  });

  it("単独のキーを判定すること", () => {
    expect(shortcutFor(key("Delete"))).toBe("clear");
    expect(shortcutFor(key("Escape"))).toBe("cancel");
  });

  it("対応しない組み合わせは無視すること", () => {
    expect(shortcutFor(key("s"))).toBeNull();
    expect(shortcutFor(key("s", { ctrl: true, shift: true }))).toBeNull();
    expect(shortcutFor(key("o", { ctrl: true, alt: true }))).toBeNull();
    expect(shortcutFor(key("Delete", { shift: true }))).toBeNull();
  });
});
