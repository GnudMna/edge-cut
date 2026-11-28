// 更新プログラムのページを表示内容 (見出し・本文・ボタン) にする

import type { IconName } from "../components/Icon";
import { formatBytes } from "../lib/format";
import type { UpdaterAction, UpdaterPage } from "./types";

export interface PageButton {
  label: string;
  /** 押したときに送る操作 (`cancel` はダウンロードの中止) */
  action: UpdaterAction | "cancel";
  primary?: boolean;
}

export interface PageDescription {
  title: string;
  icon: IconName;
  tone: "default" | "danger" | "success";
  /** 本文の段落 (先頭が主な内容) */
  lines: string[];
  /** 進捗 (0〜1)。進捗バーを出さない場合は `null` */
  progress: number | null;
  /** 処理中の表示 (スピナー) を出すか */
  busy: boolean;
  /** 詳細ログの場所 */
  log: string | null;
  buttons: PageButton[];
}

const CLOSE: PageButton = { label: "閉じる", action: "close", primary: true };

/** 各ページで共通の既定値 */
type Defaults = Pick<PageDescription, "tone" | "lines" | "progress" | "busy" | "log" | "buttons">;
const BASE: Readonly<Defaults> = { tone: "default", lines: [], progress: null, busy: false, log: null, buttons: [] };

/**
 * ページを表示内容にする
 *
 * @param page - 表示するページ
 * @param portable - ポータブル版を更新するか (更新方法の説明に使う)
 */
export function describePage(page: UpdaterPage, portable: boolean): PageDescription {
  switch (page.kind) {
    case "checking":
      return { ...BASE, title: "更新を確認しています", icon: "refresh", busy: true };
    case "upToDate":
      return {
        ...BASE,
        title: "最新版を使用しています",
        icon: "check",
        tone: "success",
        lines: [`現在のバージョン: v${page.current}`],
        buttons: [CLOSE],
      };
    case "checkFailed":
      return {
        ...BASE,
        title: "更新を確認できませんでした",
        icon: "alert",
        tone: "danger",
        lines: [page.reason],
        buttons: [CLOSE],
      };
    case "available":
      return {
        ...BASE,
        title: "新しいバージョンがあります",
        icon: "download",
        lines: [
          `v${page.current} → v${page.latest} (${formatBytes(page.size)})`,
          portable
            ? "ダウンロードして検証した後、EdgeCut を終了してファイルを差し替え、起動し直す。設定 (data フォルダ) はそのまま残る"
            : "ダウンロードして検証した後、EdgeCut を終了してインストールし、起動し直す。インストーラーの操作は要らない",
          "未保存の画像は先に保存しておく",
        ],
        buttons: [
          { label: "後で", action: "close" },
          { label: "更新する", action: "update", primary: true },
        ],
      };
    case "downloading":
      return {
        ...BASE,
        title: `v${page.latest} をダウンロードしています`,
        icon: "download",
        lines: [`${formatBytes(page.received)} / ${formatBytes(page.total)}`, "サイズ・SHA-256・署名を検証してから更新する"],
        progress: Math.min(page.received / Math.max(page.total, 1), 1),
        buttons: [{ label: "中止", action: "cancel" }],
      };
    case "waitingExit":
      return {
        ...BASE,
        title: "EdgeCut の終了を待っています",
        icon: "refresh",
        lines: ["保存中の画像があれば、完了してから終了する"],
        busy: true,
      };
    case "installing":
      return {
        ...BASE,
        title: `v${page.latest} をインストールしています`,
        icon: "download",
        lines: [page.restart ? "完了すると EdgeCut を起動し直す" : "EdgeCut が閉じられたため、完了しても起動しない"],
        busy: true,
      };
    case "installed":
      return {
        ...BASE,
        title: "更新が完了しました",
        icon: "check",
        tone: "success",
        lines: [`v${page.latest} をインストールした`, "更新中に EdgeCut が閉じられたため、起動していない"],
        buttons: [
          { label: "EdgeCut を起動", action: "launchApp" },
          { label: "閉じる", action: "close", primary: true },
        ],
      };
    case "starting":
      return { ...BASE, title: "EdgeCut を起動しています", icon: "refresh", busy: true };
    case "rebootRequired":
      return {
        ...BASE,
        title: "Windows の再起動が必要です",
        icon: "info",
        lines: [`v${page.latest} のインストールを完了するには、Windows を再起動する`],
        buttons: [CLOSE],
      };
    case "canceled":
      return {
        ...BASE,
        title: "更新を中止しました",
        icon: "info",
        lines: ["EdgeCut はそのまま使える"],
        buttons: [CLOSE],
      };
    case "failed":
      return {
        ...BASE,
        title: "更新に失敗しました",
        icon: "alert",
        tone: "danger",
        lines: [page.reason],
        log: page.log,
        buttons: page.canLaunch
          ? [
              { label: "閉じる", action: "close" },
              { label: "EdgeCut を起動", action: "launchApp", primary: true },
            ]
          : [CLOSE],
      };
  }
}
