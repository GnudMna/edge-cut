//! 画像ファイルの保存と出力パス生成
//!
//! 保存先の形式に合わせた変換 (JPEG の透過合成など) と、既存ファイルを
//! 上書きしない排他作成による連番保存を担う

use std::borrow::Cow;
use std::error::Error;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Cursor, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::consts::{
    DEFAULT_IMAGE_EXTENSION, JPEG_FLATTEN_BACKGROUND, JPEG_QUALITY, OUTPUT_SUFFIX_MAX,
    is_supported_image_extension,
};

use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ImageError, ImageFormat, Rgb, RgbImage};

// ======================================================================
// エラー型
// ======================================================================
/// 画像保存時のエラー
#[derive(Debug)]
pub enum SaveError {
    /// 形式の判定またはエンコードのエラー
    Encode(ImageError),
    /// ファイルの作成・書き込みエラー
    Io(io::Error),
}

impl From<ImageError> for SaveError {
    fn from(value: ImageError) -> Self {
        Self::Encode(value)
    }
}

impl From<io::Error> for SaveError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Encode(e) => write!(f, "保存エラー: {e}"),
            Self::Io(e) => write!(f, "保存エラー: {e}"),
        }
    }
}

impl Error for SaveError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Encode(e) => Some(e),
            Self::Io(e) => Some(e),
        }
    }
}

// ======================================================================
// 画像保存
// ======================================================================
/// 画像をファイルへ保存する (既存ファイルは上書き)
///
/// 形式は拡張子から判定する。JPEG は透過を `JPEG_FLATTEN_BACKGROUND` に合成して
/// RGB 化し、品質 `JPEG_QUALITY` でエンコードする
/// エンコードをメモリ上で済ませてから書き込むため、エンコード失敗時に既存ファイルを壊さない
///
/// # Arguments
/// * `img`  - 保存する画像
/// * `path` - 保存先パス
///
/// # Errors
/// 拡張子から形式を判定できない、エンコードまたは書き込みに失敗した場合
pub fn save_image(img: &DynamicImage, path: &Path) -> Result<(), SaveError> {
    let bytes = encode_image(img, ImageFormat::from_path(path)?)?;
    fs::write(path, bytes)?;
    Ok(())
}

/// 画像を `out_dir` に新規ファイルとして保存する (既存ファイルは上書きしない)
///
/// `{stem}.{ext}` が既に存在する場合は `{stem}_2.{ext}`, `_3`, ... と連番を付ける
/// 存在確認と作成を `create_new` で同時に行うため、並行する保存とも衝突しない
///
/// # Arguments
/// * `img`     - 保存する画像
/// * `out_dir` - 出力先ディレクトリ (作成済みであること)
/// * `stem`    - 拡張子を除くファイル名
/// * `ext`     - 拡張子 (ドットなし。形式の判定にも使う)
///
/// # Returns
/// * `(PathBuf, bool)` - 保存したパスと、連番サフィックスで回避したかどうか
///
/// # Errors
/// 形式を判定できない、エンコードまたは書き込みに失敗した場合
pub fn save_image_unique(
    img: &DynamicImage,
    out_dir: &Path,
    stem: &str,
    ext: &str,
) -> Result<(PathBuf, bool), SaveError> {
    let base = out_dir.join(format!("{stem}.{ext}"));
    let bytes = encode_image(img, ImageFormat::from_path(&base)?)?;

    if write_new_file(&base, &bytes)? {
        return Ok((base, false));
    }
    for n in 2..=OUTPUT_SUFFIX_MAX {
        let candidate = out_dir.join(format!("{stem}_{n}.{ext}"));
        if write_new_file(&candidate, &bytes)? {
            return Ok((candidate, true));
        }
    }

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let fallback = out_dir.join(format!("{stem}_{ts}.{ext}"));
    if write_new_file(&fallback, &bytes)? {
        return Ok((fallback, true));
    }
    Err(io::Error::from(io::ErrorKind::AlreadyExists).into())
}

/// 一括トリミングの結果を `out_dir` に保存する
///
/// ファイル名は `batch_trim_output_path` と同じ規則で、衝突時は連番サフィックスを付ける
///
/// # Arguments
/// * `img`     - トリミング後の画像
/// * `source`  - 元画像ファイルのパス
/// * `out_dir` - 出力先ディレクトリ (作成済みであること)
///
/// # Returns
/// * `(PathBuf, bool)` - 保存したパスと、連番サフィックスで回避したかどうか
///
/// # Errors
/// エンコードまたは書き込みに失敗した場合
pub fn save_batch_output(
    img: &DynamicImage,
    source: &Path,
    out_dir: &Path,
) -> Result<(PathBuf, bool), SaveError> {
    let stem = format!("{}_trimmed", source_file_stem(source));
    save_image_unique(img, out_dir, &stem, &batch_output_extension(source))
}

/// 画像を指定形式でメモリ上にエンコードする
fn encode_image(img: &DynamicImage, format: ImageFormat) -> Result<Vec<u8>, ImageError> {
    let mut buf = Vec::new();
    if format == ImageFormat::Jpeg {
        let encoder = JpegEncoder::new_with_quality(&mut buf, JPEG_QUALITY);
        to_jpeg_compatible(img).write_with_encoder(encoder)?;
    } else {
        img.write_to(&mut Cursor::new(&mut buf), format)?;
    }
    Ok(buf)
}

/// JPEG エンコーダが受け付ける 8bit RGB / グレースケールへ変換する
///
/// 透過ピクセルは `JPEG_FLATTEN_BACKGROUND` に合成する
/// (アルファを捨てるだけでは透明部分の RGB 値 (多くは黒) が露出するため)
fn to_jpeg_compatible(img: &DynamicImage) -> Cow<'_, DynamicImage> {
    match img {
        DynamicImage::ImageRgb8(_) | DynamicImage::ImageLuma8(_) => Cow::Borrowed(img),
        _ if !img.color().has_alpha() => Cow::Owned(DynamicImage::ImageRgb8(img.to_rgb8())),
        _ => {
            let rgba = img.to_rgba8();
            let [bg_r, bg_g, bg_b] = JPEG_FLATTEN_BACKGROUND;
            let rgb = RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
                let [r, g, b, a] = rgba.get_pixel(x, y).0;
                Rgb([
                    blend_over(r, bg_r, a),
                    blend_over(g, bg_g, a),
                    blend_over(b, bg_b, a),
                ])
            });
            Cow::Owned(DynamicImage::ImageRgb8(rgb))
        }
    }
}

/// 前景チャネル値 `fg` をアルファ `alpha` で背景 `bg` に合成する
fn blend_over(fg: u8, bg: u8, alpha: u8) -> u8 {
    let (fg, bg, alpha) = (u32::from(fg), u32::from(bg), u32::from(alpha));
    // 255 で割る際に四捨五入する
    let v = (fg * alpha + bg * (255 - alpha) + 127) / 255;
    u8::try_from(v).unwrap_or(u8::MAX)
}

/// `path` が存在しない場合のみ作成して書き込む
///
/// 既に存在する場合は何もせず `Ok(false)` を返す
/// 書き込み途中で失敗した場合は作成したファイルを削除する
fn write_new_file(path: &Path, bytes: &[u8]) -> io::Result<bool> {
    let mut file = match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => return Ok(false),
        Err(e) => return Err(e),
    };
    if let Err(e) = file.write_all(bytes) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(e);
    }
    Ok(true)
}

// ======================================================================
// パス生成
// ======================================================================
/// 一括トリミングの出力ファイルパスを生成する
///
/// `{元ファイル名}_trimmed.{拡張子}` 形式で `out_dir` 配下のパスを返す
/// 元ファイルの拡張子を維持する。拡張子がない、または未対応の場合は PNG とする
/// 実際の保存は衝突時に連番を付ける `save_batch_output` で行う
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

    use image::{Rgba, RgbaImage};

    /// テストごとに一意な一時ディレクトリを作成する
    fn temp_dir(prefix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "{prefix}_{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    /// 左半分が完全透明、右半分が不透明な赤の RGBA 画像
    fn half_transparent_image() -> DynamicImage {
        DynamicImage::ImageRgba8(RgbaImage::from_fn(8, 8, |x, _| {
            if x < 4 {
                Rgba([0, 0, 0, 0])
            } else {
                Rgba([255, 0, 0, 255])
            }
        }))
    }

    // ======================================================================
    // パス生成
    // ======================================================================
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

    // ======================================================================
    // エラー型
    // ======================================================================
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

    // ======================================================================
    // JPEG 変換
    // ======================================================================
    /// 半透明の黒を白背景へ合成すると中間の灰色になることを確認する
    #[test]
    fn blend_over_mixes_with_background() {
        assert_eq!(blend_over(0, 255, 0), 255);
        assert_eq!(blend_over(0, 255, 255), 0);
        assert_eq!(blend_over(0, 255, 128), 127);
    }

    /// 透過を含む画像を JPEG で保存でき、透明部分が白になることを確認する
    #[test]
    fn save_image_flattens_alpha_for_jpeg() {
        let dir = temp_dir("edge_cut_jpeg_alpha");
        let path = dir.join("out.jpg");
        save_image(&half_transparent_image(), &path).expect("JPEG 保存");

        // 透明だった左側は白、不透明だった右側は赤として再読み込みされる
        let reloaded = image::open(&path).expect("再読み込み").to_rgb8();
        let left = reloaded.get_pixel(1, 4).0;
        let right = reloaded.get_pixel(6, 4).0;
        assert!(left.iter().all(|&c| c > 240), "left = {left:?}");
        assert!(
            right[0] > 200 && right[1] < 60 && right[2] < 60,
            "right = {right:?}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// 16bit 画像も JPEG で保存できることを確認する
    #[test]
    fn save_image_converts_16bit_for_jpeg() {
        let dir = temp_dir("edge_cut_jpeg_16bit");
        let path = dir.join("out.jpeg");
        let img = DynamicImage::ImageRgba16(image::ImageBuffer::from_pixel(
            4,
            4,
            Rgba([65535u16, 0, 0, 65535]),
        ));
        save_image(&img, &path).expect("16bit 画像の JPEG 保存");
        assert!(path.is_file());
        let _ = fs::remove_dir_all(&dir);
    }

    /// 拡張子から形式を判定できない場合はエラーとなり、既存ファイルを壊さないことを確認する
    #[test]
    fn save_image_unknown_extension_keeps_existing_file() {
        let dir = temp_dir("edge_cut_unknown_ext");
        let path = dir.join("out.xyz");
        fs::write(&path, b"keep").expect("placeholder");

        let result = save_image(&half_transparent_image(), &path);
        assert!(matches!(result, Err(SaveError::Encode(_))));
        assert_eq!(fs::read(&path).expect("read"), b"keep");
        let _ = fs::remove_dir_all(&dir);
    }

    // ======================================================================
    // 連番保存
    // ======================================================================
    /// 同名ファイルが無いときは指定名のまま保存することを確認する
    #[test]
    fn save_unique_uses_base_name_when_not_exists() {
        let dir = temp_dir("edge_cut_unique_base");
        let (path, renamed) =
            save_image_unique(&half_transparent_image(), &dir, "photo", "png").expect("保存");
        assert_eq!(path, dir.join("photo.png"));
        assert!(!renamed);
        assert!(path.is_file());
        let _ = fs::remove_dir_all(&dir);
    }

    /// 既存ファイルを上書きせず連番サフィックスで回避することを確認する
    #[test]
    fn save_unique_appends_suffix_without_overwriting() {
        let dir = temp_dir("edge_cut_unique_col");
        let existing = dir.join("photo.png");
        fs::write(&existing, b"x").expect("placeholder");

        let (path, renamed) =
            save_image_unique(&half_transparent_image(), &dir, "photo", "png").expect("保存");
        assert_eq!(path, dir.join("photo_2.png"));
        assert!(renamed);
        // 既存ファイルの内容は変わらない
        assert_eq!(fs::read(&existing).expect("read"), b"x");
        let _ = fs::remove_dir_all(&dir);
    }

    /// 一括出力が `{名前}_trimmed.{拡張子}` で保存され、2 回目は連番になることを確認する
    #[test]
    fn save_batch_output_names_and_suffixes() {
        let dir = temp_dir("edge_cut_batch_out");
        let img = half_transparent_image();

        let (first, renamed_first) =
            save_batch_output(&img, Path::new("in/photo.JPG"), &dir).expect("1 回目");
        assert_eq!(first, dir.join("photo_trimmed.jpg"));
        assert!(!renamed_first);

        let (second, renamed_second) =
            save_batch_output(&img, Path::new("in/photo.JPG"), &dir).expect("2 回目");
        assert_eq!(second, dir.join("photo_trimmed_2.jpg"));
        assert!(renamed_second);
        let _ = fs::remove_dir_all(&dir);
    }
}
