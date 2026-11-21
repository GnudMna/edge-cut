//! 画像ファイルの保存と出力パス生成

use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::consts::{DEFAULT_IMAGE_EXTENSION, is_supported_image_extension};

use image::{DynamicImage, ImageError};

// ======================================================================
// エラー型
// ======================================================================
/// 画像保存時のエラー
#[derive(Debug)]
pub enum SaveError {
    /// エンコードまたはファイル書き込みエラー
    Encode(ImageError),
}

impl From<ImageError> for SaveError {
    fn from(value: ImageError) -> Self {
        Self::Encode(value)
    }
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Encode(e) => write!(f, "保存エラー: {e}"),
        }
    }
}

impl Error for SaveError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Encode(e) => Some(e),
        }
    }
}

// ======================================================================
// 画像保存
// ======================================================================
/// 画像をファイルへ保存する
///
/// # Errors
/// エンコードまたは書き込みに失敗した場合
pub fn save_image(img: &DynamicImage, path: &Path) -> Result<(), SaveError> {
    img.save(path)?;
    Ok(())
}

/// 連番サフィックス探索の上限 (`{名前}_trimmed_2` 〜 `_{N}`)
const BATCH_OUTPUT_SUFFIX_MAX: u32 = 10_000;

// ======================================================================
// パス生成
// ======================================================================
/// 一括トリミングの出力ファイルパスを生成する
///
/// `{元ファイル名}_trimmed.{拡張子}` 形式で `out_dir` 配下のパスを返す
/// 元ファイルの拡張子を維持する。拡張子がない、または未対応の場合は PNG とする
///
/// # Arguments
/// * `source`  - 元画像ファイルのパス
/// * `out_dir` - 出力先ディレクトリ
#[must_use]
pub fn batch_trim_output_path(source: &Path, out_dir: &Path) -> PathBuf {
    let stem = source_file_stem(source);
    let ext = batch_output_extension(source);
    out_dir.join(format!("{stem}_trimmed.{ext}"))
}

/// 一括トリミング用の出力パスを決定する
///
/// 既定の `{名前}_trimmed.{拡張子}` が既に存在する場合は
/// `{名前}_trimmed_2.{拡張子}`, `_3`, ... と連番を付けて衝突を避ける
///
/// # Returns
/// * `(PathBuf, bool)` - 出力パスと、連番サフィックスで回避したかどうか
#[must_use]
pub fn resolve_batch_output_path(source: &Path, out_dir: &Path) -> (PathBuf, bool) {
    let base = batch_trim_output_path(source, out_dir);
    if !base.exists() {
        return (base, false);
    }

    let stem = source_file_stem(source);
    let ext = batch_output_extension(source);
    for n in 2..=BATCH_OUTPUT_SUFFIX_MAX {
        let candidate = out_dir.join(format!("{stem}_trimmed_{n}.{ext}"));
        if !candidate.exists() {
            return (candidate, true);
        }
    }

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    (out_dir.join(format!("{stem}_trimmed_{ts}.{ext}")), true)
}

/// 元ファイルの stem。欠落時は `image`
fn source_file_stem(source: &Path) -> String {
    source
        .file_stem()
        .map_or_else(|| "image".into(), |s| s.to_string_lossy().into_owned())
}

/// 元ファイルから一括トリミング用の出力拡張子を決定する
fn batch_output_extension(source: &Path) -> String {
    source
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|ext| is_supported_image_extension(ext))
        .unwrap_or_else(|| DEFAULT_IMAGE_EXTENSION.into())
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    /// PNG 拡張子が維持されることを確認する
    #[test]
    fn preserves_png_extension() {
        let path = batch_trim_output_path(Path::new("photo.png"), Path::new("/out"));
        assert_eq!(path, Path::new("/out/photo_trimmed.png"));
    }

    /// JPEG 拡張子が維持されることを確認する
    #[test]
    fn preserves_jpeg_extension() {
        let path = batch_trim_output_path(Path::new("a.jpeg"), Path::new("C:\\out"));
        assert_eq!(path, Path::new("C:\\out/a_trimmed.jpeg"));
    }

    /// 大文字 `.JPG` が小文字 `.jpg` に正規化されることを確認する
    #[test]
    fn preserves_jpg_extension() {
        let path = batch_trim_output_path(Path::new("a.JPG"), Path::new("/out"));
        assert_eq!(path, Path::new("/out/a_trimmed.jpg"));
    }

    /// `.tif` 拡張子が維持されることを確認する
    #[test]
    fn preserves_tif_extension() {
        let path = batch_trim_output_path(Path::new("scan.TIF"), Path::new("/out"));
        assert_eq!(path, Path::new("/out/scan_trimmed.tif"));
    }

    /// WebP と BMP 拡張子がそれぞれ維持されることを確認する
    #[test]
    fn preserves_webp_and_bmp() {
        assert_eq!(
            batch_trim_output_path(Path::new("x.webp"), Path::new("/o")),
            Path::new("/o/x_trimmed.webp")
        );
        assert_eq!(
            batch_trim_output_path(Path::new("x.bmp"), Path::new("/o")),
            Path::new("/o/x_trimmed.bmp")
        );
    }

    /// 未対応拡張子のとき PNG にフォールバックすることを確認する
    #[test]
    fn unknown_extension_falls_back_to_png() {
        let path = batch_trim_output_path(Path::new("doc.pdf"), Path::new("/out"));
        assert_eq!(path, Path::new("/out/doc_trimmed.png"));
    }

    /// 拡張子なしのファイル名でも PNG にフォールバックすることを確認する
    #[test]
    fn missing_extension_falls_back_to_png() {
        let path = batch_trim_output_path(Path::new("noext"), Path::new("/out"));
        assert_eq!(path, Path::new("/out/noext_trimmed.png"));
    }

    /// `SaveError` が元の `ImageError` を `source()` で返すことを確認する
    #[test]
    fn save_error_source_returns_encode_inner() {
        use std::error::Error;

        use image::ImageError;
        use image::error::{ParameterError, ParameterErrorKind};

        let inner = ImageError::Parameter(ParameterError::from_kind(
            ParameterErrorKind::DimensionMismatch,
        ));
        let err = super::SaveError::Encode(inner);
        assert!(Error::source(&err).is_some());
    }

    /// 出力先に同名ファイルが無いときは既定パスをそのまま返すことを確認する
    #[test]
    fn resolve_uses_base_path_when_not_exists() {
        let dir = std::env::temp_dir().join(format!(
            "edge_cut_resolve_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let (path, renamed) = resolve_batch_output_path(Path::new("photo.png"), &dir);
        assert_eq!(path, dir.join("photo_trimmed.png"));
        assert!(!renamed);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 既定パスが既存のとき連番サフィックスで回避することを確認する
    #[test]
    fn resolve_appends_numeric_suffix_on_collision() {
        let dir = std::env::temp_dir().join(format!(
            "edge_cut_resolve_col_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let existing = dir.join("photo_trimmed.png");
        std::fs::write(&existing, b"x").expect("placeholder");
        let (path, renamed) = resolve_batch_output_path(Path::new("photo.png"), &dir);
        assert_eq!(path, dir.join("photo_trimmed_2.png"));
        assert!(renamed);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
