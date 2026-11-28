// プレビュー領域 (表示の切り替え・空の状態・ドロップ時の表示)

import { useEffect, useState } from "react";

import { send } from "../bridge";
import { FIT_VIEW, type PaneView } from "../lib/viewport";
import type { PreviewMode, PreviewView, Snapshot } from "../types";
import { Button, Segmented } from "./controls";
import { Icon } from "./Icon";
import { Pane } from "./Pane";

/** プレビュー画像を取得して `ImageBitmap` にする (取得中は前の画像を表示し続ける) */
function usePreviewBitmap(preview: PreviewView | null): ImageBitmap | null {
  const [state, setState] = useState<{ id: number; bitmap: ImageBitmap } | null>(null);
  const id = preview?.id ?? null;

  useEffect(() => {
    if (id === null) return;
    let cancelled = false;
    (async () => {
      const res = await fetch(`/preview/${id}`, { cache: "no-store" });
      if (!res.ok || cancelled) return;
      const w = Number(res.headers.get("X-Width"));
      const h = Number(res.headers.get("X-Height"));
      const data = new Uint8ClampedArray(await res.arrayBuffer());
      if (cancelled || data.length !== w * h * 4) return;
      const bitmap = await createImageBitmap(new ImageData(data, w, h), { premultiplyAlpha: "premultiply" });
      if (cancelled) {
        bitmap.close();
        return;
      }
      setState({ id, bitmap });
    })().catch(() => {
      // 取得に失敗しても次の画像で取り直す
    });
    return () => {
      cancelled = true;
    };
  }, [id]);

  // 画像を閉じたら保持している画像を手放す (描画中に状態を調整する)
  if (id === null && state !== null) setState(null);

  // 状態から外れた (差し替え・破棄・アンマウント) ImageBitmap を解放する
  useEffect(() => () => state?.bitmap.close(), [state]);

  // 画像が無いときは、保持している前の画像を返さない
  return id === null ? null : (state?.bitmap ?? null);
}

const MODES: { value: PreviewMode; label: string; title: string }[] = [
  { value: "Range", label: "範囲", title: "元画像に、削られる領域とトリミング範囲を重ねて表示する" },
  { value: "Result", label: "結果", title: "トリミング後の画像だけを表示する" },
  { value: "SideBySide", label: "並べて", title: "元画像とトリミング後の画像を左右に並べて表示する" },
];

export function Preview({ s }: { s: Snapshot }) {
  const image = s.image;
  const bitmap = usePreviewBitmap(image?.preview ?? null);
  const [views, setViews] = useState<[PaneView, PaneView]>([FIT_VIEW, FIT_VIEW]);
  const previewId = image?.preview?.id ?? null;

  // 新しい画像を開いたら倍率と位置を戻す (描画中に状態を調整する)
  const [viewsPreviewId, setViewsPreviewId] = useState(previewId);
  if (viewsPreviewId !== previewId) {
    setViewsPreviewId(previewId);
    setViews([FIT_VIEW, FIT_VIEW]);
  }

  const mode = s.prefs.previewMode;
  const picking = s.background.picking;
  // スポイト中は元画像が見える表示にする
  useEffect(() => {
    if (picking && mode === "Result") send({ type: "setPreviewMode", mode: "Range" });
  }, [picking, mode]);

  if (!image) {
    return (
      <main className="preview">
        <EmptyState loading={s.busy.loading} />
        {s.window.dragHover && <DropOverlay />}
      </main>
    );
  }

  const pane = (index: 0 | 1, kind: "range" | "result", label?: string) => (
    <Pane
      key={kind}
      kind={kind}
      bitmap={bitmap}
      width={image.width}
      height={image.height}
      trim={image.trim}
      view={views[index]}
      onViewChange={(v) => setViews((prev) => (index === 0 ? [v, prev[1]] : [prev[0], v]))}
      picking={picking}
      onPick={(x, y) => send({ type: "pickPixel", x, y })}
      {...(label ? { label } : {})}
    />
  );

  return (
    <main className="preview">
      <div className="preview-toolbar" data-no-drag>
        <Segmented label="プレビューの表示" value={mode} options={MODES} onChange={(m) => send({ type: "setPreviewMode", mode: m })} size="sm" />
        <Button icon="trash" variant="ghost" size="sm" onClick={() => send({ type: "clear" })} title="画像を閉じる (Delete)">
          閉じる
        </Button>
      </div>

      {picking && (
        <div className="pick-banner">
          <Icon name="eyedropper" size={14} />
          <span>元画像をクリックして背景色を選ぶ</span>
          <kbd>Esc</kbd>
          <span className="muted">で解除</span>
          <button type="button" className="link" onClick={() => send({ type: "setPickMode", enabled: false })}>
            やめる
          </button>
        </div>
      )}

      <div className={`panes panes-${mode === "SideBySide" ? "split" : "single"}`}>
        {mode !== "Result" && pane(0, "range", mode === "SideBySide" ? "元画像" : undefined)}
        {mode !== "Range" && pane(1, "result", mode === "SideBySide" ? "トリミング後" : undefined)}
      </div>

      {(!bitmap || s.busy.loading) && <div className="preview-progress" aria-hidden="true" />}
      {s.window.dragHover && <DropOverlay />}
    </main>
  );
}

function EmptyState({ loading }: { loading: boolean }) {
  return (
    <div className="empty">
      <div className="empty-card">
        <div className="empty-art" aria-hidden="true">
          <Icon name="crop" size={40} />
        </div>
        {loading ? (
          <>
            <h1>読み込み中…</h1>
            <div className="spinner" />
          </>
        ) : (
          <>
            <h1>画像をドロップして余白を取り除く</h1>
            <p className="muted">複数のファイルやフォルダをドロップすると、まとめてトリミングして保存する</p>
            <div className="empty-actions">
              <Button icon="folder" variant="primary" onClick={() => send({ type: "openImage" })}>
                画像を開く
              </Button>
              <Button icon="paste" onClick={() => send({ type: "paste" })}>
                クリップボードから貼り付け
              </Button>
            </div>
            <dl className="empty-keys">
              <div>
                <dt>
                  <kbd>Ctrl</kbd>+<kbd>O</kbd>
                </dt>
                <dd>開く</dd>
              </div>
              <div>
                <dt>
                  <kbd>Ctrl</kbd>+<kbd>V</kbd>
                </dt>
                <dd>貼り付け</dd>
              </div>
              <div>
                <dt>
                  <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>O</kbd>
                </dt>
                <dd>一括トリミング</dd>
              </div>
            </dl>
            <p className="empty-formats">PNG · JPEG · WebP · BMP · TIFF · GIF</p>
          </>
        )}
      </div>
    </div>
  );
}

function DropOverlay() {
  return (
    <div className="drop-overlay">
      <div className="drop-card">
        <Icon name="download" size={28} />
        <strong>ドロップして開く</strong>
        <span>複数のファイル・フォルダは一括トリミング</span>
      </div>
    </div>
  );
}
