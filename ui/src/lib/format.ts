// 表示用の文字列の組み立て

import type { OutputFormat, Prefs, Rgba } from "../types";

/** 出力形式の表示名 */
export const FORMAT_LABELS: Record<OutputFormat, string> = {
  Original: "元の形式",
  Png: "PNG",
  Jpeg: "JPEG",
  Webp: "WebP",
  Bmp: "BMP",
  Tiff: "TIFF",
  Gif: "GIF",
};

/** 出力形式の並び順 */
export const OUTPUT_FORMATS: OutputFormat[] = ["Original", "Png", "Jpeg", "Webp", "Bmp", "Tiff", "Gif"];

/** アスペクト比のプリセット (幅, 高さ) */
export const ASPECT_PRESETS: [number, number][] = [
  [1, 1],
  [4, 3],
  [3, 2],
  [16, 9],
  [3, 4],
  [2, 3],
  [9, 16],
];

/**
 * 元画像に対する面積の増減 (%) を四捨五入して返す
 *
 * 元画像の面積が 0 のときは `null`
 */
export function areaChangePercent(orig: [number, number], trim: [number, number]): number | null {
  const origArea = orig[0] * orig[1];
  if (origArea === 0) return null;
  const trimArea = trim[0] * trim[1];
  return Math.round((trimArea / origArea) * 100) - 100;
}

/** 面積の増減の表示 (例: `−81%`) */
export function formatChange(change: number): string {
  if (change === 0) return "±0%";
  return change < 0 ? `−${Math.abs(change)}%` : `+${change}%`;
}

/** バイト数を `B` / `KB` / `MB` 単位で表示する (Rust の `format_bytes` と揃える) */
export function formatBytes(n: number): string {
  const KIB = 1024;
  const MIB = 1024 * 1024;
  if (n >= MIB) return `${(n / MIB).toFixed(1)} MB`;
  if (n >= KIB) return `${(n / KIB).toFixed(1)} KB`;
  return `${n} B`;
}

/** RGBA を `#rrggbb` にする (アルファは含めない) */
export function rgbHex([r, g, b]: Rgba): string {
  return `#${[r, g, b].map((v) => v.toString(16).padStart(2, "0")).join("")}`;
}

/** `#rrggbb` または `#rrggbbaa` を RGBA にする。解釈できなければ `null` */
export function parseHex(text: string, alpha: number): Rgba | null {
  const m = /^#?([0-9a-f]{6})([0-9a-f]{2})?$/i.exec(text.trim());
  if (!m || m[1] === undefined) return null;
  const v = Number.parseInt(m[1], 16);
  const a = m[2] === undefined ? alpha : Number.parseInt(m[2], 16);
  return [(v >> 16) & 0xff, (v >> 8) & 0xff, v & 0xff, a];
}

/** CSS の色 */
export function cssColor([r, g, b, a]: Rgba): string {
  return `rgba(${r}, ${g}, ${b}, ${(a / 255).toFixed(3)})`;
}

/** 詳細設定のうち既定から変えている項目の要約 (閉じていても効いている設定に気づけるようにする) */
export function trimDetailSummary(p: Prefs): string {
  const items: string[] = [];
  if (p.ignorePixels > 0) items.push(`ノイズ ${p.ignorePixels} px`);
  const s = p.sides;
  if (!(s.top && s.right && s.bottom && s.left)) items.push("一部の辺を維持");
  if (p.aspectEnabled) items.push(`${p.aspectW}:${p.aspectH}`);
  return items.join(" · ");
}

/** 出力設定の要約 */
export function outputSummary(p: Prefs): string {
  const format = FORMAT_LABELS[p.outputFormat];
  return p.autoSave ? `${format} · 自動保存` : format;
}

/** ファイルパスからファイル名を取り出す */
export function baseName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}
