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
//! | [`trim`] | 背景色検出・余白除去 |
//! | [`image_load`] | サイズ上限付きの画像読み込み |
//! | [`image_output`] | 画像保存・出力パス生成 |
//! | [`batch_trim`] | 複数ファイルの一括トリミング |
//! | [`consts`] | 上限値・対応拡張子などの定数 |
//! | [`app`] | egui ベースの GUI (`app::run`) |
//! | `update` | gh-release-bridge 経由の更新確認・MSI 適用 (Windows) |

pub mod batch_trim;
pub mod consts;
pub mod image_load;
pub mod image_output;
pub mod trim;

pub mod app;

#[cfg(windows)]
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
    DimensionLimitKind, LoadError, check_dimensions, check_file_size, open_image,
    validate_clipboard_rgba,
};
pub use image_output::{
    SaveError, batch_trim_output_path, save_batch_output, save_image, save_image_unique,
};
pub use trim::{TrimInfo, TrimResult, detect_background_rgba, sample_pixel, trim};
