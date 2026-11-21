//! 画像ファイルの出力パス生成

use std::path::{Path, PathBuf};

// ======================================================================
// 定数
// ======================================================================
/// 一括トリミングで対応する画像拡張子 (小文字)
const BATCH_OUTPUT_EXTENSIONS: &[&str] =
    &["png", "jpg", "jpeg", "bmp", "gif", "tiff", "tif", "webp"];

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
pub fn batch_trim_output_path(source: &Path, out_dir: &Path) -> PathBuf {
    let stem = source
        .file_stem()
        .map_or_else(|| "image".into(), |s| s.to_string_lossy().into_owned());
    let ext = batch_output_extension(source);
    out_dir.join(format!("{stem}_trimmed.{ext}"))
}

/// 元ファイルから一括トリミング用の出力拡張子を決定する
fn batch_output_extension(source: &Path) -> String {
    source
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|ext| BATCH_OUTPUT_EXTENSIONS.contains(&ext.as_str()))
        .unwrap_or_else(|| "png".into())
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
}
