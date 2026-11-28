// タイトルバー (ウィンドウ枠の代わり) とツールバー
//
// 空いている部分のドラッグでウィンドウを動かし、ダブルクリックで最大化を切り替える

import { useRef, useState } from "react";

import { send } from "../bridge";
import { baseName } from "../lib/format";
import type { Snapshot, ThemeMode } from "../types";
import { Button } from "./controls";
import { Icon } from "./Icon";
import { type MenuItem, MenuList, Popover } from "./overlays";

/** アプリのロゴ (assets/icons/icon.svg と同じ図柄) */
function Logo() {
  return (
    <svg className="logo" viewBox="0 0 512 512" width="18" height="18" aria-hidden="true">
      <rect width="512" height="512" rx="108" fill="#0b0b0f" />
      <rect x="108" y="108" width="296" height="296" rx="20" fill="#17171f" />
      <rect x="140" y="140" width="232" height="232" rx="14" fill="#f2f2f4" />
      <g fill="none" stroke="#38bdf8" strokeWidth="30" strokeLinecap="round" strokeLinejoin="round">
        <path d="M76 168 L76 76 L168 76" />
        <path d="M436 168 L436 76 L344 76" />
        <path d="M76 344 L76 436 L168 436" />
        <path d="M436 344 L436 436 L344 436" />
      </g>
    </svg>
  );
}

/** クリックした場所がボタンなどの操作部品か */
function isInteractive(target: EventTarget | null): boolean {
  return target instanceof Element && target.closest("button, input, select, a, [data-no-drag]") !== null;
}

const THEMES: { mode: ThemeMode; label: string }[] = [
  { mode: "System", label: "システムに合わせる" },
  { mode: "Light", label: "ライト" },
  { mode: "Dark", label: "ダーク" },
];

export function TitleBar({ s }: { s: Snapshot }) {
  const [menuOpen, setMenuOpen] = useState(false);
  const pressed = useRef(false);
  const busy = s.busy.dialog;

  const menuItems: MenuItem[] = [
    { label: "テーマ" },
    ...THEMES.map((t) => ({
      label: t.label,
      checked: s.prefs.theme === t.mode,
      onSelect: () => send({ type: "setTheme", mode: t.mode }),
    })),
    { separator: true, label: "" },
    {
      icon: "book",
      label: "ドキュメント",
      hint: s.bundled.docs ? "利用者向けドキュメントを既定のブラウザで開く" : "配布物に同梱されたファイルが見つからない",
      disabled: !s.bundled.docs,
      onSelect: () => send({ type: "openBundled", item: "docs" }),
    },
    {
      icon: "file",
      label: "ライセンス",
      disabled: !s.bundled.license,
      onSelect: () => send({ type: "openBundled", item: "license" }),
    },
    {
      icon: "file",
      label: "第三者ライセンス",
      disabled: !s.bundled.thirdPartyNotices,
      onSelect: () => send({ type: "openBundled", item: "thirdPartyNotices" }),
    },
    { separator: true, label: "" },
    {
      icon: "refresh",
      label: "更新を確認…",
      note: `v${s.version}`,
      onSelect: () => send({ type: "openUpdate" }),
    },
  ];

  return (
    <header
      className="titlebar"
      // 押しただけでは移動を始めず、動かしたときに始める (押した直後に始めるとダブルクリックを受け取れない)
      onPointerDown={(e) => {
        pressed.current = e.button === 0 && !isInteractive(e.target);
      }}
      onPointerMove={(e) => {
        if (!pressed.current || (e.buttons & 1) === 0) return;
        pressed.current = false;
        send({ type: "window", action: { kind: "drag" } });
      }}
      onPointerUp={() => {
        pressed.current = false;
      }}
      onDoubleClick={(e) => {
        if (!isInteractive(e.target)) send({ type: "window", action: { kind: "toggleMaximize" } });
      }}
    >
      <div className="titlebar-brand">
        <Logo />
        <span className="titlebar-name">EdgeCut</span>
        {s.image && (
          <>
            <span className="titlebar-sep">/</span>
            <span className="titlebar-file" title={s.image.name}>
              {baseName(s.image.name)}
            </span>
          </>
        )}
      </div>

      <div className="titlebar-tools">
        <Button icon="folder" onClick={() => send({ type: "openImage" })} disabled={busy} title="画像ファイルを開く (Ctrl+O)">
          開く
        </Button>
        <Button icon="paste" onClick={() => send({ type: "paste" })} title="クリップボードの画像・ファイルを使う (Ctrl+V)">
          貼り付け
        </Button>
        {s.batch.running ? (
          <Button icon="stop" variant="danger" onClick={() => send({ type: "cancelBatch" })} title="一括トリミングを中止 (Esc)">
            中止
          </Button>
        ) : (
          <Button
            icon="batch"
            onClick={() => send({ type: "batchTrim" })}
            disabled={busy}
            title="複数の画像をまとめてトリミングして保存 (Ctrl+Shift+O)"
          >
            一括トリミング
          </Button>
        )}
      </div>

      <div className="titlebar-spacer" />

      <div className="titlebar-tools">
        {/* 設定パネルを閉じている間は、保存・コピーをここに出す */}
        {!s.prefs.panelOpen && s.image && (
          <>
            <Button
              icon="save"
              variant="primary"
              disabled={!s.busy.canOutput || busy}
              onClick={() => send({ type: "save" })}
              title="トリミング後の画像を保存 (Ctrl+S)"
            >
              保存
            </Button>
            <Button icon="copy" disabled={!s.busy.canOutput || busy} onClick={() => send({ type: "copy" })} title="トリミング後の画像をクリップボードへコピー (Ctrl+C)">
              コピー
            </Button>
            <span className="titlebar-divider" />
          </>
        )}
        <Button
          icon="panel"
          variant="ghost"
          active={s.prefs.panelOpen}
          onClick={() => send({ type: "setPanel", open: !s.prefs.panelOpen })}
          title="設定パネルを開閉"
        />
        <Popover
          open={menuOpen}
          onClose={() => setMenuOpen(false)}
          align="end"
          anchor={<Button icon="menu" variant="ghost" active={menuOpen} onClick={() => setMenuOpen((v) => !v)} title="メニュー" />}
        >
          <MenuList items={menuItems} onClose={() => setMenuOpen(false)} />
        </Popover>
      </div>

      <div className="window-controls">
        <button type="button" className="wc" onClick={() => send({ type: "window", action: { kind: "minimize" } })} aria-label="最小化">
          <Icon name="minimize" size={20} />
        </button>
        <button
          type="button"
          className="wc"
          onClick={() => send({ type: "window", action: { kind: "toggleMaximize" } })}
          aria-label={s.window.maximized ? "元のサイズに戻す" : "最大化"}
        >
          <Icon name={s.window.maximized ? "restore" : "maximize"} size={20} />
        </button>
        <button type="button" className="wc wc-close" onClick={() => send({ type: "window", action: { kind: "close" } })} aria-label="閉じる">
          <Icon name="close" size={20} />
        </button>
      </div>
    </header>
  );
}
