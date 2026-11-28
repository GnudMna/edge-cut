//! `EdgeCut` のコアライブラリ
//!
//! 画像の背景余白検出・トリミング、安全な読み込み、一括処理を提供する。
//! デスクトップ GUI は [`app`] モジュール (UI 本体は `ui/` の TypeScript)、バイナリは `EdgeCut` (本体) と
//! `EdgeCutUpdater` (更新プログラム) で配布する。
//!
//! # クイックスタート
//!
//! ```no_run
//! use std::path::Path;
//!
//! use edge_cut::{open_image, trim};
//!
//! let img = open_image(Path::new("input.png")).expect("読み込み");
//! let result = trim(&img, 0, 0, None);
//! result.image.save("output.png").expect("保存");
//! ```
//!
//! # モジュール
//!
//! | モジュール | 責務 |
//! | ---------- | ---- |
//! | [`mod@trim`] | 背景色検出・余白の矩形検出・アスペクト比の固定・切り出し |
//! | [`image_load`] | サイズ上限付きの画像読み込み (ICC プロファイル・解像度の取得、複数フレームの検出を含む) |
//! | [`image_output`] | 出力形式の決定・画像保存 (ICC プロファイル・解像度の再埋め込み)・出力パス生成 |
//! | [`dpi`] | 解像度 (DPI) の型と、PNG / JPEG / TIFF からの読み取り・書き込み |
//! | [`batch_trim`] | 複数ファイルの一括トリミング |
//! | [`consts`] | 上限値・対応拡張子などの定数 |
//! | [`app`] | wry / tao と React (TypeScript) による GUI (本体 `app::run`、更新プログラム `app::run_updater`) |
//! | `panic_hook` | panic のログ記録と UI スレッドでのダイアログ通知 (クレート内部用) |
//! | `panic_message` | panic の捕捉とメッセージ取り出し (クレート内部用) |
//! | `update` | GitHub Releases による更新確認・署名検証、更新プログラムの進行・無人インストール・ポータブル版の差し替え |
//! | `install_layout` | 配布形態 (MSI / ポータブル) ごとの実行ファイル・データの配置 (クレート内部用) |

// doctest は Clippy の対象外で警告も表示されないため、警告をエラーにして検出する
#![doc(test(attr(deny(warnings))))]

pub mod batch_trim;
pub mod consts;
pub mod dpi;
pub mod image_load;
pub mod image_output;
pub mod trim;

pub mod app;

mod install_layout;
mod panic_hook;
mod panic_message;

mod update;

#[cfg(test)]
pub mod test_helpers;

// ======================================================================
// 公開 API の再エクスポート
// ======================================================================
pub use batch_trim::{
    BatchFileError, BatchProgress, BatchTrimParams, BatchTrimResult, run_batch_trim,
};
pub use dpi::Dpi;
pub use image_load::{
    DimensionLimitKind, LoadError, LoadedImage, check_dimensions, check_file_size, open_image,
    open_image_with_icc, validate_clipboard_rgba,
};
pub use image_output::{
    OutputFormat, SaveError, SaveOptions, save_image, save_image_unique, save_image_unique_with,
    save_image_with, save_trimmed_output, save_trimmed_output_with, trimmed_file_name,
    trimmed_file_stem, trimmed_output_path,
};
pub use trim::{
    AspectFit, AspectRatio, AspectSkip, Margins, TrimBounds, TrimInfo, TrimOptions, TrimResult,
    TrimSides, apply_trim, detect_background_rgba, detect_trim, detect_trim_with_options,
    sample_pixel, trim, trim_with_options,
};
