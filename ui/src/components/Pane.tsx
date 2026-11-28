// プレビューの 1 ペイン (元画像にトリミング範囲を重ねる表示、またはトリミング後の表示)
//
// 元画像のプレビュー画像 (大きな画像は縮小済み) を canvas に描き、トリミング後の表示も同じ画像から切り出す

import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";

import { cssColor, rgbHex } from "../lib/format";
import {
  type Frame,
  MAX_ZOOM,
  type PaneView,
  WHEEL_STEP,
  effectiveZoom,
  fitZoom,
  panBy,
  paneToContent,
  zoomAt,
} from "../lib/viewport";
import type { Rect, Rgba, TrimView } from "../types";
import { Icon } from "./Icon";

/** 市松模様の 1 マス (CSS px) */
const CHECKER = 8;

/** ドラッグとみなす移動量 (CSS px) */
const DRAG_THRESHOLD = 3;

export interface PaneProps {
  kind: "range" | "result";
  bitmap: ImageBitmap | null;
  /** 元画像の寸法 */
  width: number;
  height: number;
  trim: TrimView | null;
  view: PaneView;
  onViewChange: (view: PaneView) => void;
  picking: boolean;
  onPick?: (x: number, y: number) => void;
  label?: string;
}

/** 表示する範囲 (元画像の座標) */
function contentBounds(kind: PaneProps["kind"], width: number, height: number, trim: TrimView | null): Rect {
  const image: Rect = { x: 0, y: 0, w: width, h: height };
  if (!trim) return image;
  if (kind === "result") return trim.rect;
  // 範囲外へ広げた余白も見えるよう、元画像とトリミング範囲を合わせた範囲を表示する
  const x0 = Math.min(0, trim.rect.x);
  const y0 = Math.min(0, trim.rect.y);
  const x1 = Math.max(width, trim.rect.x + trim.rect.w);
  const y1 = Math.max(height, trim.rect.y + trim.rect.h);
  return { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
}

/** 市松模様のパターンを作る (配色はテーマに合わせて CSS 変数から取る) */
function checkerPattern(ctx: CanvasRenderingContext2D, dpr: number, el: HTMLElement): CanvasPattern | null {
  const size = Math.max(1, Math.round(CHECKER * dpr));
  const tile = document.createElement("canvas");
  tile.width = tile.height = size * 2;
  const t = tile.getContext("2d");
  if (!t) return null;
  const style = getComputedStyle(el);
  t.fillStyle = style.getPropertyValue("--checker-a").trim() || "#2a2c31";
  t.fillRect(0, 0, size * 2, size * 2);
  t.fillStyle = style.getPropertyValue("--checker-b").trim() || "#34373d";
  t.fillRect(0, 0, size, size);
  t.fillRect(size, size, size, size);
  return ctx.createPattern(tile, "repeat");
}

export function Pane({ kind, bitmap, width, height, trim, view, onViewChange, picking, onPick, label }: PaneProps) {
  const wrapRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [size, setSize] = useState({ w: 0, h: 0, dpr: window.devicePixelRatio || 1 });
  const [hover, setHover] = useState<{ x: number; y: number; ix: number; iy: number; color: Rgba | null } | null>(null);
  const drag = useRef<{ id: number; x: number; y: number; moved: boolean } | null>(null);
  const [dragging, setDragging] = useState(false);
  const sampler = useRef<CanvasRenderingContext2D | null>(null);

  const bounds = contentBounds(kind, width, height, trim);
  const frame = useMemo<Frame>(
    () => ({ contentW: bounds.w, contentH: bounds.h, paneW: size.w, paneH: size.h, dpr: size.dpr }),
    [bounds.w, bounds.h, size.w, size.h, size.dpr],
  );
  const zoom = effectiveZoom(view, frame);
  const canPan = (bounds.w * zoom) / size.dpr > size.w + 0.5 || (bounds.h * zoom) / size.dpr > size.h + 0.5;

  // ペインの大きさを追う (表示スケールの変更でも大きさの通知が届く)
  useLayoutEffect(() => {
    const el = wrapRef.current;
    if (!el) return;
    const update = () => {
      const r = el.getBoundingClientRect();
      setSize({ w: r.width, h: r.height, dpr: window.devicePixelRatio || 1 });
    };
    update();
    const ro = new ResizeObserver(update);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // スポイトで色を読むため、プレビュー画像を読み取り用の canvas に写しておく
  useEffect(() => {
    sampler.current = null;
    if (!bitmap || kind !== "range") return;
    const c = new OffscreenCanvas(bitmap.width, bitmap.height);
    const ctx = c.getContext("2d", { willReadFrequently: true });
    if (!ctx) return;
    ctx.drawImage(bitmap, 0, 0);
    sampler.current = ctx as unknown as CanvasRenderingContext2D;
  }, [bitmap, kind]);

  // 描画
  useEffect(() => {
    const canvas = canvasRef.current;
    const wrap = wrapRef.current;
    if (!canvas || !wrap || size.w === 0) return;
    const pw = Math.round(size.w * size.dpr);
    const ph = Math.round(size.h * size.dpr);
    if (canvas.width !== pw || canvas.height !== ph) {
      canvas.width = pw;
      canvas.height = ph;
    }
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, pw, ph);

    const z = zoom;
    // 等倍以上では原点を物理 px にそろえ、画素の境界をにじませない
    let ox = (size.w / 2 + view.offsetX) * size.dpr - (bounds.w * z) / 2;
    let oy = (size.h / 2 + view.offsetY) * size.dpr - (bounds.h * z) / 2;
    if (z >= 1) {
      ox = Math.round(ox);
      oy = Math.round(oy);
    }
    const px = (x: number) => ox + (x - bounds.x) * z;
    const py = (y: number) => oy + (y - bounds.y) * z;
    const rectPath = (r: Rect) => {
      ctx.rect(px(r.x), py(r.y), r.w * z, r.h * z);
    };

    const image: Rect = { x: 0, y: 0, w: width, h: height };
    const pattern = checkerPattern(ctx, size.dpr, wrap);
    const sx = bitmap ? bitmap.width / width : 1;
    const sy = bitmap ? bitmap.height / height : 1;
    ctx.imageSmoothingEnabled = z * sx < 1;
    ctx.imageSmoothingQuality = "high";

    // 市松模様 (透明部分)
    const shown = kind === "result" && trim ? trim.rect : image;
    if (pattern) {
      ctx.save();
      ctx.translate(ox, oy);
      ctx.fillStyle = pattern;
      ctx.beginPath();
      ctx.rect(px(shown.x) - ox, py(shown.y) - oy, shown.w * z, shown.h * z);
      if (trim && kind === "range") ctx.rect(px(trim.rect.x) - ox, py(trim.rect.y) - oy, trim.rect.w * z, trim.rect.h * z);
      ctx.fill("nonzero");
      ctx.restore();
    }

    // 元画像の範囲外へ広げた余白 (背景色で塗る)
    if (trim) {
      ctx.save();
      ctx.beginPath();
      rectPath(trim.rect);
      rectPath(image);
      ctx.clip("evenodd");
      ctx.fillStyle = cssColor(trim.previewFill);
      ctx.beginPath();
      rectPath(trim.rect);
      ctx.fill();
      ctx.restore();
    }

    // 画像
    if (bitmap) {
      const src = kind === "result" && trim ? trim.crop : image;
      if (src.w > 0 && src.h > 0) {
        ctx.drawImage(bitmap, src.x * sx, src.y * sy, src.w * sx, src.h * sy, px(src.x), py(src.y), src.w * z, src.h * z);
      }
    }

    if (kind === "range" && trim) {
      // 削られる領域を暗くする
      ctx.save();
      ctx.beginPath();
      rectPath(bounds);
      rectPath(trim.rect);
      ctx.fillStyle = "rgba(8, 10, 14, 0.62)";
      ctx.fill("evenodd");
      ctx.restore();

      // トリミング範囲の枠と四隅の印
      const style = getComputedStyle(wrap);
      const accent = style.getPropertyValue("--accent").trim() || "#38bdf8";
      const x0 = px(trim.rect.x);
      const y0 = py(trim.rect.y);
      const w = trim.rect.w * z;
      const h = trim.rect.h * z;
      const lw = Math.max(1, Math.round(1.5 * size.dpr));
      ctx.lineWidth = lw;
      ctx.strokeStyle = "rgba(0, 0, 0, 0.55)";
      ctx.strokeRect(x0 - lw, y0 - lw, w + lw * 2, h + lw * 2);
      ctx.strokeStyle = accent;
      ctx.strokeRect(x0 - lw / 2, y0 - lw / 2, w + lw, h + lw);
      const mark = Math.min(18 * size.dpr, w / 3, h / 3);
      ctx.lineWidth = lw * 2;
      ctx.beginPath();
      for (const [cx, cy, dx, dy] of [
        [x0, y0, 1, 1],
        [x0 + w, y0, -1, 1],
        [x0, y0 + h, 1, -1],
        [x0 + w, y0 + h, -1, -1],
      ] as const) {
        ctx.moveTo(cx - (lw * dx) / 2, cy + dy * mark);
        ctx.lineTo(cx - (lw * dx) / 2, cy - (lw * dy) / 2);
        ctx.lineTo(cx + dx * mark, cy - (lw * dy) / 2);
      }
      ctx.stroke();
    }
  });

  /** ペイン上の位置 (CSS px) を元画像の座標にする */
  const toImage = useCallback(
    (clientX: number, clientY: number) => {
      const r = wrapRef.current?.getBoundingClientRect();
      if (!r) return null;
      const [cx, cy] = paneToContent(view, frame, clientX - r.left, clientY - r.top);
      return { x: clientX - r.left, y: clientY - r.top, ix: Math.floor(cx + bounds.x), iy: Math.floor(cy + bounds.y) };
    },
    [view, frame, bounds.x, bounds.y],
  );

  const sample = (ix: number, iy: number): Rgba | null => {
    const ctx = sampler.current;
    if (!ctx || !bitmap || ix < 0 || iy < 0 || ix >= width || iy >= height) return null;
    const d = ctx.getImageData(Math.floor((ix * bitmap.width) / width), Math.floor((iy * bitmap.height) / height), 1, 1).data;
    return [d[0] ?? 0, d[1] ?? 0, d[2] ?? 0, d[3] ?? 0];
  };

  const setZoom = (next: number) => onViewChange(zoomAt(view, frame, next, 0, 0));
  const pickable = picking && kind === "range";

  return (
    <div
      ref={wrapRef}
      className={`pane${pickable ? " is-picking" : canPan ? (dragging ? " is-grabbing" : " can-pan") : ""}`}
      onWheel={(e) => {
        const r = wrapRef.current?.getBoundingClientRect();
        if (!r) return;
        const factor = e.deltaY < 0 ? WHEEL_STEP : 1 / WHEEL_STEP;
        onViewChange(zoomAt(view, frame, zoom * factor, e.clientX - r.left - r.width / 2, e.clientY - r.top - r.height / 2));
      }}
      onPointerDown={(e) => {
        if (e.button !== 0 && e.button !== 1) return;
        e.currentTarget.setPointerCapture(e.pointerId);
        drag.current = { id: e.pointerId, x: e.clientX, y: e.clientY, moved: false };
      }}
      onPointerMove={(e) => {
        const d = drag.current;
        if (d && d.id === e.pointerId) {
          const dx = e.clientX - d.x;
          const dy = e.clientY - d.y;
          if (!d.moved && Math.hypot(dx, dy) < DRAG_THRESHOLD) return;
          if (canPan) {
            d.moved = true;
            setDragging(true);
            d.x = e.clientX;
            d.y = e.clientY;
            onViewChange(panBy(view, frame, dx, dy));
          }
          return;
        }
        if (pickable) {
          const p = toImage(e.clientX, e.clientY);
          setHover(p ? { ...p, color: sample(p.ix, p.iy) } : null);
        }
      }}
      onPointerUp={(e) => {
        const d = drag.current;
        drag.current = null;
        setDragging(false);
        if (!d || d.moved || !pickable || e.button !== 0) return;
        const p = toImage(e.clientX, e.clientY);
        if (p && p.ix >= 0 && p.iy >= 0 && p.ix < width && p.iy < height) onPick?.(p.ix, p.iy);
      }}
      onPointerLeave={() => setHover(null)}
      onDoubleClick={() => onViewChange(view.zoom === null ? zoomAt(view, frame, 1, 0, 0) : { zoom: null, offsetX: 0, offsetY: 0 })}
    >
      <canvas ref={canvasRef} className="pane-canvas" style={{ width: size.w, height: size.h }} />
      {label && <span className="pane-label">{label}</span>}
      {pickable && hover && hover.color && (
        <div className="pick-readout" style={{ left: hover.x + 16, top: hover.y + 16 }}>
          <span className="swatch" style={{ background: cssColor(hover.color) }} />
          <span>{rgbHex(hover.color)}</span>
          <span className="muted">
            {hover.ix}, {hover.iy}
          </span>
        </div>
      )}
      <div className="zoom-controls" data-no-drag onPointerDown={(e) => e.stopPropagation()} onDoubleClick={(e) => e.stopPropagation()}>
        <button type="button" onClick={() => setZoom(zoom / WHEEL_STEP)} disabled={zoom <= fitZoom(frame) + 1e-6} title="縮小" aria-label="縮小">
          <Icon name="zoomOut" size={14} />
        </button>
        <button
          type="button"
          className="zoom-value"
          onClick={() => onViewChange({ zoom: null, offsetX: 0, offsetY: 0 })}
          title="ペインに収まる倍率に戻す"
        >
          {Math.round(zoom * 100)}%
        </button>
        <button type="button" onClick={() => setZoom(zoom * WHEEL_STEP)} disabled={zoom >= MAX_ZOOM} title="拡大" aria-label="拡大">
          <Icon name="zoomIn" size={14} />
        </button>
        <span className="zoom-sep" />
        <button type="button" onClick={() => onViewChange({ zoom: null, offsetX: 0, offsetY: 0 })} title="フィット" aria-label="フィット" className={view.zoom === null ? "is-active" : ""}>
          <Icon name="fit" size={14} />
        </button>
        <button type="button" onClick={() => setZoom(1)} title="等倍 (画像 1 px を画面の 1 px で表示)" className={`zoom-1x${Math.abs(zoom - 1) < 1e-6 ? " is-active" : ""}`}>
          1:1
        </button>
      </div>
    </div>
  );
}
