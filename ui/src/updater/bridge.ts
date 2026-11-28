// 更新プログラムの Rust 側とのやり取り
//
// コマンドは `window.ipc.postMessage` で送り、画面状態は Rust が `window.__edgecutUpdater.receive` を呼んで届ける

import { useSyncExternalStore } from "react";

import type { UpdaterCommand, UpdaterSnapshot } from "./types";

declare global {
  interface Window {
    __edgecutUpdater?: { receive(snapshot: UpdaterSnapshot): void };
  }
}

let current: UpdaterSnapshot | null = null;
const listeners = new Set<() => void>();

/** Rust から画面状態を受け取れるようにする (更新画面を表示するときだけ呼ぶ) */
export function installUpdaterBridge(): void {
  window.__edgecutUpdater = {
    receive(snapshot) {
      current = snapshot;
      for (const listener of listeners) listener();
    },
  };
}

/** Rust 側へコマンドを送る (WebView 外で開いた場合は何もしない) */
export function sendUpdater(command: UpdaterCommand): void {
  window.ipc?.postMessage(JSON.stringify(command));
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** 最新の画面状態を購読する (最初の状態が届くまでは `null`) */
export function useUpdaterSnapshot(): UpdaterSnapshot | null {
  return useSyncExternalStore(subscribe, () => current);
}
