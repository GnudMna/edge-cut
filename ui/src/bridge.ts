// Rust 側とのやり取り
//
// コマンドは `window.ipc.postMessage` で送り、画面状態は Rust が `window.__edgecut.receive` を呼んで届ける

import { useSyncExternalStore } from "react";

import type { Command, Snapshot } from "./types";

declare global {
  interface Window {
    ipc?: { postMessage(message: string): void };
    __edgecut?: { receive(snapshot: Snapshot): void };
  }
}

let current: Snapshot | null = null;
const listeners = new Set<() => void>();

window.__edgecut = {
  receive(snapshot) {
    current = snapshot;
    for (const listener of listeners) listener();
  },
};

/** Rust 側へコマンドを送る (WebView 外で開いた場合は何もしない) */
export function send(command: Command): void {
  window.ipc?.postMessage(JSON.stringify(command));
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** 最新の画面状態を購読する (最初の状態が届くまでは `null`) */
export function useSnapshot(): Snapshot | null {
  return useSyncExternalStore(subscribe, () => current);
}
