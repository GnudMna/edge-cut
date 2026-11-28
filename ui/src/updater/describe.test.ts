import { describe, expect, it } from "vitest";

import { describePage } from "./describe";

describe("describePage", () => {
  it("更新ありでは「後で」と「更新する」を出すこと", () => {
    const d = describePage({ kind: "available", current: "1.0.0", latest: "1.1.0", size: 2048 }, false);
    expect(d.buttons.map((b) => b.action)).toEqual(["close", "update"]);
    expect(d.lines[0]).toBe("v1.0.0 → v1.1.0 (2.0 KB)");
  });

  it("ポータブル版では差し替えと data フォルダの扱いを説明すること", () => {
    const d = describePage({ kind: "available", current: "1.0.0", latest: "1.1.0", size: 1 }, true);
    expect(d.lines[1]).toContain("data フォルダ");
  });

  it("ダウンロード中は進捗と中止ボタンを出すこと", () => {
    const d = describePage({ kind: "downloading", latest: "1.1.0", received: 512, total: 1024 }, false);
    expect(d.progress).toBe(0.5);
    expect(d.buttons).toEqual([{ label: "中止", action: "cancel" }]);
  });

  it("全体の大きさが 0 でも進捗を 1 以下に収めること", () => {
    const d = describePage({ kind: "downloading", latest: "1.1.0", received: 10, total: 0 }, false);
    expect(d.progress).toBe(1);
  });

  it("終了待ち・インストール中はボタンを出さずに処理中を示すこと", () => {
    const waiting = describePage({ kind: "waitingExit" }, false);
    const installing = describePage({ kind: "installing", latest: "1.1.0", restart: true }, false);
    for (const d of [waiting, installing]) {
      expect(d.buttons).toEqual([]);
      expect(d.busy).toBe(true);
    }
  });

  it("本体が閉じられていた場合は起動せず、起動ボタンを出すこと", () => {
    const installing = describePage({ kind: "installing", latest: "1.1.0", restart: false }, false);
    expect(installing.lines[0]).toContain("起動しない");
    const done = describePage({ kind: "installed", latest: "1.1.0" }, false);
    expect(done.buttons.map((b) => b.action)).toEqual(["launchApp", "close"]);
  });

  it("本体を終了させた後の失敗では起動ボタンとログを出すこと", () => {
    const d = describePage({ kind: "failed", reason: "失敗", log: "C:\\logs\\updater-msi.log", canLaunch: true }, false);
    expect(d.buttons.map((b) => b.action)).toEqual(["close", "launchApp"]);
    expect(d.log).toBe("C:\\logs\\updater-msi.log");
  });

  it("本体が動作したままの失敗では閉じるだけにすること", () => {
    const d = describePage({ kind: "failed", reason: "失敗", log: null, canLaunch: false }, false);
    expect(d.buttons.map((b) => b.action)).toEqual(["close"]);
  });
});
