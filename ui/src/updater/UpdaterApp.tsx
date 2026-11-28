// 更新プログラムの画面 (EdgeCutUpdater.exe が `?view=updater` で開く)

import { useEffect } from "react";

import { Button } from "../components/controls";
import { Icon } from "../components/Icon";
import { sendUpdater, useUpdaterSnapshot } from "./bridge";
import { describePage, type PageButton } from "./describe";

export function UpdaterApp() {
  const s = useUpdaterSnapshot();

  // 準備ができたことを伝える (Rust はこれを受けてウィンドウを表示し、更新を始める)
  useEffect(() => {
    sendUpdater({ type: "ready" });
  }, []);

  if (!s) return <div className="boot" />;

  const d = describePage(s.page, s.portable);
  const press = (button: PageButton) =>
    sendUpdater(button.action === "cancel" ? { type: "cancel" } : { type: "choose", action: button.action });
  const [lead, ...notes] = d.lines;

  return (
    <div className="updater">
      <main className="dialog updater-panel" aria-label={d.title}>
        <header className="dialog-head">
          <span className={`dialog-icon tone-${d.tone}`}>
            <Icon name={d.icon} size={18} />
          </span>
          <h2>{d.title}</h2>
        </header>
        <div className="dialog-body">
          {d.busy && (
            <div className="dialog-progress">
              <span className="spinner spinner-sm" />
              しばらくお待ちください…
            </div>
          )}
          {lead && <p className={d.tone === "danger" ? "error-text" : undefined}>{lead}</p>}
          {d.progress !== null && (
            <div className="progress updater-progress" role="progressbar" aria-valuenow={Math.round(d.progress * 100)}>
              <div className="progress-bar" style={{ width: `${d.progress * 100}%` }} />
            </div>
          )}
          {notes.map((note) => (
            <p key={note} className="muted small">
              {note}
            </p>
          ))}
          {d.log && <p className="muted small updater-log">詳細ログ: {d.log}</p>}
        </div>
        <footer className="dialog-actions">
          <span className="updater-version muted small">現在 v{s.version}</span>
          {d.buttons.map((button) => (
            <Button key={button.action} variant={button.primary ? "primary" : "default"} onClick={() => press(button)}>
              {button.label}
            </Button>
          ))}
        </footer>
      </main>
    </div>
  );
}
