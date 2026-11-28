// 更新プログラムの画面状態とコマンド (src/app/updater/view.rs と揃える)

export type UpdaterPage =
  | { kind: "checking" }
  | { kind: "upToDate"; current: string }
  | { kind: "checkFailed"; reason: string }
  | { kind: "available"; current: string; latest: string; size: number }
  | { kind: "downloading"; latest: string; received: number; total: number }
  | { kind: "waitingExit" }
  | { kind: "installing"; latest: string; restart: boolean }
  | { kind: "installed"; latest: string }
  | { kind: "starting" }
  | { kind: "rebootRequired"; latest: string }
  | { kind: "canceled" }
  | { kind: "failed"; reason: string; log: string | null; canLaunch: boolean };

export interface UpdaterSnapshot {
  version: string;
  portable: boolean;
  page: UpdaterPage;
}

export type UpdaterAction = "update" | "close" | "launchApp";

export type UpdaterCommand = { type: "ready" } | { type: "choose"; action: UpdaterAction } | { type: "cancel" };
