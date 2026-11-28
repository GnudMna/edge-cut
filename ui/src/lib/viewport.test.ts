import { describe, expect, it } from "vitest";

import { FIT_MARGIN_X, FIT_VIEW, MAX_ZOOM, clampView, effectiveZoom, fitZoom, panBy, paneToContent, zoomAt } from "./viewport";

const big = { contentW: 2000, contentH: 1000, paneW: 500, paneH: 500, dpr: 1 };
const small = { contentW: 100, contentH: 50, paneW: 500, paneH: 500, dpr: 1 };

describe("fitZoom", () => {
  it("大きな画像はペインに収まる倍率にすること", () => {
    expect(fitZoom(big)).toBe((500 - FIT_MARGIN_X * 2) / 2000);
  });

  it("小さな画像は等倍より拡大しないこと", () => {
    expect(fitZoom(small)).toBe(1);
  });

  it("表示スケールを物理 px で考えること", () => {
    expect(fitZoom({ ...big, dpr: 2 })).toBe(((500 - FIT_MARGIN_X * 2) * 2) / 2000);
  });
});

describe("zoomAt", () => {
  it("収まる倍率より小さくしないこと", () => {
    expect(zoomAt(FIT_VIEW, big, 0.01, 0, 0)).toEqual(FIT_VIEW);
  });

  it("最大倍率を超えないこと", () => {
    expect(zoomAt(FIT_VIEW, big, 100, 0, 0).zoom).toBe(MAX_ZOOM);
  });

  it("カーソル下の点を動かさないこと", () => {
    const before = paneToContent(FIT_VIEW, big, 300, 250);
    const view = zoomAt(FIT_VIEW, big, 1, 50, 0);
    const after = paneToContent(view, big, 300, 250);
    expect(after[0]).toBeCloseTo(before[0]);
    expect(after[1]).toBeCloseTo(before[1]);
  });
});

describe("clampView / panBy", () => {
  it("ペインに収まる内容は動かさないこと", () => {
    expect(panBy(FIT_VIEW, small, 100, 100)).toEqual({ zoom: null, offsetX: 0, offsetY: 0 });
  });

  it("はみ出す内容は端までしか動かさないこと", () => {
    const view = clampView({ zoom: 1, offsetX: 10_000, offsetY: -10_000 }, big);
    // 2000px の内容を 500px のペインに表示すると左右に 750px ずつ動かせる
    expect(view.offsetX).toBe(750);
    expect(view.offsetY).toBe(-250);
    expect(effectiveZoom(view, big)).toBe(1);
  });
});
