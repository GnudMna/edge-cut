// ステータスバー (処理状況・一括トリミングの進捗・エラー・背景色の手動指定)

import { send } from "../bridge";
import { cssColor } from "../lib/format";
import type { Snapshot } from "../types";
import { Icon } from "./Icon";

export function StatusBar({ s }: { s: Snapshot }) {
  const busy = s.busy.trimming || s.busy.saving || s.busy.loading || s.batch.running;
  const errors = s.batch.errors.length;
  const { processed, total } = s.batch;
  const trim = s.image?.trim;

  return (
    <footer className="statusbar">
      <div className="status-main" role="status" aria-live="polite">
        {busy ? <span className="spinner spinner-xs" /> : <span className="status-dot" />}
        <span className="status-text" title={s.status}>
          {s.status}
        </span>
      </div>

      {s.batch.running && total > 0 && (
        <div className="status-progress" title={`一括トリミング ${processed}/${total}`}>
          <div className="progress">
            <div className="progress-bar" style={{ width: `${(processed / total) * 100}%` }} />
          </div>
          <span className="mono">
            {processed}/{total}
          </span>
        </div>
      )}

      {errors > 0 && (
        <button type="button" className="status-chip status-error" onClick={() => send({ type: "showBatchErrors" })}>
          <Icon name="alert" size={13} />
          エラー {errors} 件
        </button>
      )}

      {/* 手動指定の解除忘れを防ぐため、指定中は常に表示する */}
      {s.background.manual && s.background.color && (
        <button
          type="button"
          className="status-chip status-warn"
          onClick={() => send({ type: "resetBackground" })}
          title="クリックで自動検出に戻す"
        >
          <span className="swatch swatch-xs" style={{ background: cssColor(s.background.color) }} />
          背景色: 手動指定
        </button>
      )}

      {!s.prefs.panelOpen && s.image && (
        <span className="status-meta mono">
          元 {s.image.width}×{s.image.height}
          {trim && ` → ${trim.rect.w}×${trim.rect.h}`}
        </span>
      )}
    </footer>
  );
}
