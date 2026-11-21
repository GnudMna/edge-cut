//! 画像ファイルの保存と出力パス生成
//!
//! 出力形式 (`OutputFormat`) の決定、保存先の形式に合わせた変換 (JPEG の透過合成など)、
//! ICC プロファイルの再埋め込み、
//! 既存ファイルを上書きしない排他作成による連番保存を担う

mod encode;
mod error;
mod format;
mod path;
mod save;
mod temp_file;
mod webp_lossy;

pub use self::error::SaveError;
pub use self::format::OutputFormat;
pub use self::path::{trimmed_file_name, trimmed_file_stem, trimmed_output_path};
pub use self::save::{
    SaveOptions, save_image, save_image_unique, save_image_unique_with, save_image_with,
    save_trimmed_output, save_trimmed_output_with,
};

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests;
