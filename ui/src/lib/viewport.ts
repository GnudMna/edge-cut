// プレビューのズームとパンの計算
//
// 倍率は「元画像 1 px あたりの物理 px 数」で表す (1 のとき等倍。表示スケールが 125% でも画素がそろう)。
// 位置は表示内容の中心がペインの中心からずれている量 (CSS px)

/** ペイン 1 つ分のズームとパン */
export interface PaneView {
  /** 倍率。`null` のときはペインに収まる倍率に追従する */
  zoom: number | null;
  offsetX: number;
  offsetY: number;
}

export const FIT_VIEW: PaneView = { zoom: null, offsetX: 0, offsetY: 0 };

/** 最大倍率 (1600%) */
export const MAX_ZOOM = 16;

/** ペインに収めるときに左右に空ける余白 (CSS px) */
export const FIT_MARGIN_X = 24;

/** ペインに収めるときに上下に空ける余白 (CSS px。上の表示切り替えと下のズーム操作に重ならないようにする) */
export const FIT_MARGIN_Y = 56;

/** ホイール 1 段あたりの倍率 */
export const WHEEL_STEP = 1.2;

/** 表示する内容とペインの大きさ */
export interface Frame {
  /** 内容の幅 (元画像の px) */
  contentW: number;
  contentH: number;
  /** ペインの幅 (CSS px) */
  paneW: number;
  paneH: number;
  /** `devicePixelRatio` */
  dpr: number;
}

/** ペインに収まる倍率 (余白を空け、等倍より大きくはしない) */
export function fitZoom(f: Frame): number {
  const w = Math.max(1, f.paneW - FIT_MARGIN_X * 2);
  const h = Math.max(1, f.paneH - FIT_MARGIN_Y * 2);
  if (f.contentW <= 0 || f.contentH <= 0 || f.paneW <= 0 || f.paneH <= 0) return 1;
  return Math.min(1, (w * f.dpr) / f.contentW, (h * f.dpr) / f.contentH);
}

/** 実際の倍率 */
export function effectiveZoom(view: PaneView, f: Frame): number {
  const fit = fitZoom(f);
  return Math.max(fit, view.zoom ?? fit);
}

/** 内容がペインからはみ出さない範囲に位置を収める */
export function clampView(view: PaneView, f: Frame): PaneView {
  const z = effectiveZoom(view, f);
  const limit = (content: number, pane: number) => Math.max(0, ((content * z) / f.dpr - pane) / 2);
  const lx = limit(f.contentW, f.paneW);
  const ly = limit(f.contentH, f.paneH);
  return {
    zoom: view.zoom,
    offsetX: Math.min(lx, Math.max(-lx, view.offsetX)),
    offsetY: Math.min(ly, Math.max(-ly, view.offsetY)),
  };
}

/**
 * 指定位置を中心に倍率を変える
 *
 * @param px カーソルの位置 (ペインの中心からの CSS px)
 * @param py カーソルの位置 (ペインの中心からの CSS px)
 */
export function zoomAt(view: PaneView, f: Frame, nextZoom: number, px: number, py: number): PaneView {
  const fit = fitZoom(f);
  const z = effectiveZoom(view, f);
  const next = Math.min(MAX_ZOOM, Math.max(fit, nextZoom));
  if (next <= fit + 1e-6) return FIT_VIEW;
  // カーソル下の画像上の点が動かないよう位置を補正する
  const ratio = next / z;
  return clampView(
    {
      zoom: next,
      offsetX: px - (px - view.offsetX) * ratio,
      offsetY: py - (py - view.offsetY) * ratio,
    },
    f,
  );
}

/** ドラッグで位置を動かす */
export function panBy(view: PaneView, f: Frame, dx: number, dy: number): PaneView {
  return clampView({ ...view, offsetX: view.offsetX + dx, offsetY: view.offsetY + dy }, f);
}

/**
 * ペイン上の位置 (左上からの CSS px) を内容の座標 (内容の左上からの元画像 px) にする
 */
export function paneToContent(view: PaneView, f: Frame, x: number, y: number): [number, number] {
  const z = effectiveZoom(view, f);
  const cx = f.paneW / 2 + view.offsetX;
  const cy = f.paneH / 2 + view.offsetY;
  return [((x - cx) * f.dpr) / z + f.contentW / 2, ((y - cy) * f.dpr) / z + f.contentH / 2];
}
