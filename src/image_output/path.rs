//! 出力ファイル名・パスの生成

use std::path::{Path, PathBuf};

use super::format::OutputFormat;

// ======================================================================
// パス生成
// ======================================================================
/// トリミング結果の出力ファイル名を生成する
///
/// `{元ファイル名}_trimmed.{拡張子}` 形式。拡張子は `format` から決める
/// 実際の保存は衝突時に連番を付ける `save_trimmed_output` で行う
///
/// # Arguments
/// * `source` - 元画像ファイルのパス
/// * `format` - 出力形式
///
/// # Returns
/// * `String` - 出力ファイル名 (ディレクトリを含まない)
#[must_use]
pub fn trimmed_file_name(source: &Path, format: OutputFormat) -> String {
    format!(
        "{}.{}",
        trimmed_file_stem(source),
        format.output_extension(source)
    )
}

/// トリミング結果の出力ファイルパスを生成する
///
/// `out_dir` 配下の `trimmed_file_name` を返す
///
/// # Arguments
/// * `source`  - 元画像ファイルのパス
/// * `out_dir` - 出力先ディレクトリ
/// * `format`  - 出力形式
///
/// # Returns
/// * `PathBuf` - 出力ファイルのパス (衝突の回避はしない)
#[must_use]
pub fn trimmed_output_path(source: &Path, out_dir: &Path, format: OutputFormat) -> PathBuf {
    out_dir.join(trimmed_file_name(source, format))
}

/// 出力ファイル名の拡張子を除く部分を返す
///
/// 元の名前が欠落している場合は `image` を使う
///
/// # Arguments
/// * `source` - 元画像ファイルのパス
///
/// # Returns
/// * `String` - `{元ファイル名}_trimmed`
#[must_use]
pub fn trimmed_file_stem(source: &Path) -> String {
    let stem = source
        .file_stem()
        .map_or_else(|| "image".into(), |s| s.to_string_lossy().into_owned());
    format!("{stem}_trimmed")
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;

    /// PNG 拡張子が維持されること
    #[test]
    fn preserves_png_extension() {
        let path = trimmed_output_path(
            Path::new("photo.png"),
            Path::new("/out"),
            OutputFormat::Original,
        );
        assert_eq!(path, Path::new("/out/photo_trimmed.png"));
    }

    /// JPEG 拡張子が維持されること
    #[test]
    fn preserves_jpeg_extension() {
        let path = trimmed_output_path(
            Path::new("a.jpeg"),
            Path::new("C:\\out"),
            OutputFormat::Original,
        );
        assert_eq!(path, Path::new("C:\\out/a_trimmed.jpeg"));
    }

    /// 大文字 `.JPG` が小文字 `.jpg` に正規化されること
    #[test]
    fn preserves_jpg_extension() {
        let path = trimmed_output_path(
            Path::new("a.JPG"),
            Path::new("/out"),
            OutputFormat::Original,
        );
        assert_eq!(path, Path::new("/out/a_trimmed.jpg"));
    }

    /// `.tif` 拡張子が維持されること
    #[test]
    fn preserves_tif_extension() {
        let path = trimmed_output_path(
            Path::new("scan.TIF"),
            Path::new("/out"),
            OutputFormat::Original,
        );
        assert_eq!(path, Path::new("/out/scan_trimmed.tif"));
    }

    /// WebP と BMP 拡張子がそれぞれ維持されること
    #[test]
    fn preserves_webp_and_bmp() {
        assert_eq!(
            trimmed_output_path(Path::new("x.webp"), Path::new("/o"), OutputFormat::Original),
            Path::new("/o/x_trimmed.webp")
        );
        assert_eq!(
            trimmed_output_path(Path::new("x.bmp"), Path::new("/o"), OutputFormat::Original),
            Path::new("/o/x_trimmed.bmp")
        );
    }

    /// 未対応拡張子のとき PNG にフォールバックすること
    #[test]
    fn unknown_extension_falls_back_to_png() {
        let path = trimmed_output_path(
            Path::new("doc.pdf"),
            Path::new("/out"),
            OutputFormat::Original,
        );
        assert_eq!(path, Path::new("/out/doc_trimmed.png"));
    }

    /// 拡張子なしのファイル名でも PNG にフォールバックすること
    #[test]
    fn missing_extension_falls_back_to_png() {
        let path = trimmed_output_path(
            Path::new("noext"),
            Path::new("/out"),
            OutputFormat::Original,
        );
        assert_eq!(path, Path::new("/out/noext_trimmed.png"));
    }

    /// 形式指定時は元の拡張子に関係なく形式の拡張子になること
    #[test]
    fn explicit_format_overrides_source_extension() {
        let src = Path::new("photo.png");
        assert_eq!(
            trimmed_file_name(src, OutputFormat::Jpeg),
            "photo_trimmed.jpg"
        );
        assert_eq!(
            trimmed_file_name(src, OutputFormat::Webp),
            "photo_trimmed.webp"
        );
        assert_eq!(
            trimmed_file_name(Path::new("scan.tif"), OutputFormat::Png),
            "scan_trimmed.png"
        );
        // 拡張子なしの元画像 (クリップボード由来など) も形式の拡張子を付ける
        assert_eq!(
            trimmed_file_name(Path::new("clipboard"), OutputFormat::Gif),
            "clipboard_trimmed.gif"
        );
    }

    /// 元の名前がない場合は `image_trimmed` を使うこと
    #[test]
    fn empty_source_uses_fallback_stem() {
        assert_eq!(
            trimmed_file_name(Path::new(""), OutputFormat::Original),
            "image_trimmed.png"
        );
    }
}
