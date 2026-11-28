// 画面全体の組み立て・テーマ・キーボードショートカット

import { useEffect, useRef } from "react";

import { send, useSnapshot } from "./bridge";
import { Dialogs } from "./components/Dialogs";
import { Inspector } from "./components/Inspector";
import { Preview } from "./components/Preview";
import { StatusBar } from "./components/StatusBar";
import { TitleBar } from "./components/TitleBar";
import { isTextInput, shortcutFor } from "./lib/shortcuts";
import type { ResizeEdge, Snapshot } from "./types";

export function App() {
  const s = useSnapshot();

  // 準備ができたことを伝える (Rust はこれを受けてウィンドウを表示し、状態を送る)
  useEffect(() => {
    send({ type: "ready" });
  }, []);

  useTheme(s);
  useShortcuts(s);

  if (!s) return <div className="boot" />;

  return (
    <div className={`app${s.window.maximized ? " is-maximized" : ""}`}>
      {!s.window.maximized && <ResizeEdges />}
      <TitleBar s={s} />
      <div className="workspace">
        <Preview s={s} />
        {s.prefs.panelOpen && <Inspector s={s} />}
      </div>
      <StatusBar s={s} />
      <Dialogs s={s} />
    </div>
  );
}

/** 配色テーマを `<html data-theme>` に反映する (`System` のときは OS の設定に任せる) */
function useTheme(s: Snapshot | null) {
  const theme = s?.prefs.theme;
  useEffect(() => {
    const root = document.documentElement;
    if (!theme || theme === "System") delete root.dataset.theme;
    else root.dataset.theme = theme === "Dark" ? "dark" : "light";
  }, [theme]);
}

/** キーボードショートカット */
function useShortcuts(s: Snapshot | null) {
  const latest = useRef(s);
  // 描画中は ref を書き換えず、描画のあとに最新の画面状態へ更新する
  useEffect(() => {
    latest.current = s;
  });

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const s = latest.current;
      if (!s || e.defaultPrevented || e.repeat) return;
      const shortcut = shortcutFor(e);
      if (!shortcut) return;
      // ダイアログの表示中は受け付けない
      if (s.update.open || s.closeConfirm || s.batch.errorsOpen) return;
      const typing = isTextInput(e.target);
      const hasSelection = (window.getSelection()?.toString() ?? "") !== "";

      switch (shortcut) {
        case "open":
          send({ type: "openImage" });
          break;
        case "batchTrim":
          if (!s.batch.running) send({ type: "batchTrim" });
          break;
        case "paste":
          if (typing) return;
          send({ type: "paste" });
          break;
        case "save":
          send({ type: "save" });
          break;
        case "copy":
          // 入力欄や文字列の選択中は文字列のコピーを優先する
          if (typing || hasSelection) return;
          send({ type: "copy" });
          break;
        case "clear":
          if (typing) return;
          send({ type: "clear" });
          break;
        case "cancel":
          if (s.background.picking) send({ type: "setPickMode", enabled: false });
          else if (s.batch.running) send({ type: "cancelBatch" });
          else return;
          break;
      }
      e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
}

/**
 * ウィンドウ上端の大きさ変更 (枠なしのウィンドウでも上端と上の角をドラッグできるようにする)
 *
 * 左右と下端は OS が影の部分で受け付ける
 */
function ResizeEdges() {
  const start = (edge: ResizeEdge) => (e: React.PointerEvent) => {
    if (e.button !== 0) return;
    e.preventDefault();
    send({ type: "window", action: { kind: "resize", edge } });
  };
  return (
    <>
      <div className="resize-edge resize-n" onPointerDown={start("north")} />
      <div className="resize-edge resize-nw" onPointerDown={start("northWest")} />
      <div className="resize-edge resize-ne" onPointerDown={start("northEast")} />
    </>
  );
}
