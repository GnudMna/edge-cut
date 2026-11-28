// Rust 側 (src/app/ipc) とやり取りする型
//
// 画面状態 (Snapshot) は src/app/ipc/snapshot.rs、コマンド (Command) は src/app/ipc/command.rs と揃える

export type Rgba = [number, number, number, number];

export type PreviewMode = "Range" | "Result" | "SideBySide";
export type ThemeMode = "System" | "Light" | "Dark";
export type AspectFit = "Expand" | "Crop";
export type OutputFormat = "Original" | "Png" | "Jpeg" | "Webp" | "Bmp" | "Tiff" | "Gif";

export interface Sides<T> {
  top: T;
  right: T;
  bottom: T;
  left: T;
}

export interface Prefs {
  tolerance: number;
  ignorePixels: number;
  padding: number;
  linkMargins: boolean;
  margins: Sides<number>;
  sides: Sides<boolean>;
  aspectEnabled: boolean;
  aspectW: number;
  aspectH: number;
  aspectFit: AspectFit;
  theme: ThemeMode;
  previewMode: PreviewMode;
  outputFormat: OutputFormat;
  jpegQuality: number;
  webpQuality: number;
  overwriteOutput: boolean;
  autoSave: boolean;
  autoSaveDir: string;
  panelOpen: boolean;
  panelWidth: number;
}

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface TrimView {
  /** トリミング後の範囲 (元画像の座標。範囲外へ広げた余白を含む) */
  rect: Rect;
  /** 元画像から切り出す範囲 */
  crop: Rect;
  previewFill: Rgba;
  fill: Rgba;
}

export interface PreviewView {
  id: number;
  width: number;
  height: number;
}

export interface ImageView {
  name: string;
  width: number;
  height: number;
  preview: PreviewView | null;
  trim: TrimView | null;
  outputExt: string;
}

export type UpdatePhase =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "upToDate"; current: string }
  | { kind: "available"; current: string; latest: string; size: number }
  | { kind: "error"; message: string }
  | { kind: "updaterRunning"; latest: string }
  | { kind: "exiting" };

export interface Snapshot {
  status: string;
  version: string;
  prefs: Prefs;
  image: ImageView | null;
  background: { color: Rgba | null; manual: boolean; picking: boolean };
  busy: { trimming: boolean; saving: boolean; loading: boolean; dialog: boolean; canOutput: boolean };
  batch: { running: boolean; processed: number; total: number; errors: string[]; errorsOpen: boolean };
  update: { open: boolean; applyBlocked: boolean; phase: UpdatePhase };
  closeConfirm: boolean;
  bundled: { docs: boolean; license: boolean; thirdPartyNotices: boolean };
  window: { maximized: boolean; dragHover: boolean };
  limits: {
    tolerance: [number, number];
    padding: [number, number];
    ignorePixels: [number, number];
    aspectPart: [number, number];
    jpegQuality: [number, number];
    webpQuality: [number, number];
    panelWidth: [number, number];
  };
}

export type TrimPatch = Partial<
  Pick<
    Prefs,
    | "tolerance"
    | "ignorePixels"
    | "padding"
    | "linkMargins"
    | "margins"
    | "sides"
    | "aspectEnabled"
    | "aspectW"
    | "aspectH"
    | "aspectFit"
  >
>;

export interface OutputPatch {
  format?: OutputFormat;
  jpegQuality?: number;
  webpQuality?: number;
  overwriteOutput?: boolean;
}

export type BundledItem = "docs" | "license" | "thirdPartyNotices";
export type ResizeEdge = "north" | "northEast" | "northWest";

export type WindowAction =
  | { kind: "minimize" }
  | { kind: "toggleMaximize" }
  | { kind: "close" }
  | { kind: "drag" }
  | { kind: "resize"; edge: ResizeEdge };

export type Command =
  | { type: "ready" }
  | { type: "openImage" }
  | { type: "paste" }
  | { type: "batchTrim" }
  | { type: "cancelBatch" }
  | { type: "clear" }
  | { type: "save" }
  | { type: "copy" }
  | { type: "setTrim"; patch: TrimPatch; live: boolean }
  | { type: "setOutput"; patch: OutputPatch }
  | { type: "setAutoSave"; enabled: boolean }
  | { type: "pickAutoSaveDir" }
  | { type: "setPreviewMode"; mode: PreviewMode }
  | { type: "setTheme"; mode: ThemeMode }
  | { type: "setPanel"; open?: boolean; width?: number }
  | { type: "setPickMode"; enabled: boolean }
  | { type: "pickPixel"; x: number; y: number }
  | { type: "setBackground"; rgba: Rgba }
  | { type: "resetBackground" }
  | { type: "openBundled"; item: BundledItem }
  | { type: "openUpdate" }
  | { type: "checkUpdate" }
  | { type: "applyUpdate" }
  | { type: "closeUpdate" }
  | { type: "showBatchErrors" }
  | { type: "closeBatchErrors" }
  | { type: "confirmClose" }
  | { type: "cancelClose" }
  | { type: "window"; action: WindowAction };
