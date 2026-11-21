//! `EdgeCut` のコアライブラリ
//!
//! 画像の背景余白検出・トリミング、安全な読み込み、一括処理を提供する。
//! デスクトップ GUI は [`app`] モジュール、バイナリは `EdgeCut` クレート名で配布する。
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
//! | [`trim`] | 背景色検出・余白の矩形検出・アスペクト比の固定・切り出し |
//! | [`image_load`] | サイズ上限付きの画像読み込み (ICC プロファイル取得・複数フレームの検出を含む) |
//! | [`image_output`] | 出力形式の決定・画像保存 (ICC プロファイル再埋め込み)・出力パス生成 |
//! | [`batch_trim`] | 複数ファイルの一括トリミング |
//! | [`consts`] | 上限値・対応拡張子などの定数 |
//! | [`app`] | egui ベースの GUI (`app::run`) |
//! | `panic_hook` | panic のログ記録と UI スレッドでのダイアログ通知 (クレート内部用) |
//! | `panic_message` | panic の捕捉とメッセージ取り出し (クレート内部用) |
//! | `update` | gh-release-bridge 経由の更新確認・署名検証・MSI 適用 |

pub mod batch_trim;
pub mod consts;
pub mod image_load;
pub mod image_output;
pub mod trim;

pub mod app;

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
pub use image_load::{
    DimensionLimitKind, LoadError, LoadedImage, check_dimensions, check_file_size, open_image,
    open_image_with_icc, validate_clipboard_rgba,
};
pub use image_output::{
    OutputFormat, SaveError, SaveOptions, save_image, save_image_unique, save_image_unique_with,
    save_image_with, save_trimmed_output, save_trimmed_output_with, trimmed_file_name,
    trimmed_output_path,
};
pub use trim::{
    AspectFit, AspectRatio, Margins, TrimBounds, TrimInfo, TrimOptions, TrimResult, TrimSides,
    apply_trim, detect_background_rgba, detect_trim, detect_trim_with_options, sample_pixel, trim,
    trim_with_options,
};
