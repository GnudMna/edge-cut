// UI のエントリポイント

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { App } from "./App";
import "./styles.css";
import { installUpdaterBridge } from "./updater/bridge";
import { UpdaterApp } from "./updater/UpdaterApp";

// 右クリックメニュー・ページ内のドラッグ・ファイルのドロップによる画面遷移を抑える
// (ファイルのドロップは Rust 側が受け取る)
window.addEventListener("contextmenu", (e) => {
  if (!(e.target instanceof HTMLInputElement)) e.preventDefault();
});
window.addEventListener("dragstart", (e) => e.preventDefault());

// 更新プログラム (EdgeCutUpdater.exe) は `?view=updater` で開き、更新画面だけを表示する
const isUpdater = new URLSearchParams(window.location.search).get("view") === "updater";
if (isUpdater) installUpdaterBridge();

const root = document.getElementById("root");
if (root) {
  createRoot(root).render(<StrictMode>{isUpdater ? <UpdaterApp /> : <App />}</StrictMode>);
}
