import { describe, expect, it } from "vitest";

import { areaChangePercent, formatBytes, formatChange, parseHex, rgbHex } from "./format";

describe("areaChangePercent", () => {
  it("面積の比率を四捨五入して増減にすること", () => {
    expect(areaChangePercent([100, 100], [44, 44])).toBe(-81);
    expect(areaChangePercent([100, 100], [110, 110])).toBe(21);
    expect(areaChangePercent([100, 100], [100, 100])).toBe(0);
  });

  it("元画像の面積が 0 のときは null を返すこと", () => {
    expect(areaChangePercent([0, 10], [1, 1])).toBeNull();
  });
});

describe("formatChange", () => {
  it("符号を付けて表示すること", () => {
    expect(formatChange(-81)).toBe("−81%");
    expect(formatChange(21)).toBe("+21%");
    expect(formatChange(0)).toBe("±0%");
  });
});

describe("formatBytes", () => {
  it("単位を切り替えること", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(2048)).toBe("2.0 KB");
    expect(formatBytes(5 * 1024 * 1024)).toBe("5.0 MB");
  });
});

describe("hex", () => {
  it("RGB を往復できること", () => {
    expect(rgbHex([255, 0, 16, 200])).toBe("#ff0010");
    expect(parseHex("#ff0010", 200)).toEqual([255, 0, 16, 200]);
    expect(parseHex("ff001080", 255)).toEqual([255, 0, 16, 128]);
  });

  it("解釈できない文字列は null を返すこと", () => {
    expect(parseHex("#ggg", 255)).toBeNull();
    expect(parseHex("#12345", 255)).toBeNull();
  });
});
