//! 画像ファイルの保存と出力パス生成
//!
//! 出力形式 (`OutputFormat`) の決定、保存先の形式に合わせた変換 (JPEG の透過合成など)、
//! ICC プロファイルの再埋め込み、
//! 既存ファイルを上書きしない排他作成による連番保存を担う

use std::borrow::Cow;
use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Cursor, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::consts::{
    DEFAULT_IMAGE_EXTENSION, JPEG_FLATTEN_BACKGROUND, JPEG_QUALITY, OUTPUT_SUFFIX_MAX,
    is_supported_image_extension,
};

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::codecs::tiff::TiffEncoder;
use image::codecs::webp::WebPEncoder;
use image::{
    ColorType, DynamicImage, GrayImage, ImageEncoder, ImageError, ImageFormat, Luma, Pixel, Rgb,
    RgbImage,
};
use serde::{Deserialize, Serialize};

/// 上書き保存用の一時ファイル名が衝突した場合に試す回数
const TEMP_FILE_ATTEMPTS: u32 = 100;

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
// 出力形式
// ======================================================================
/// トリミング結果の出力形式
///
/// 自動保存・一括トリミングの保存形式と、手動保存ダイアログの既定値に使う
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum OutputFormat {
    /// PNG
    Png,
    /// JPEG (透過は `JPEG_FLATTEN_BACKGROUND` に合成する)
    Jpeg,
    /// WebP (ロスレス)
    Webp,
    /// BMP
    Bmp,
    /// TIFF
    Tiff,
    /// GIF
    Gif,
    /// 元画像の形式を維持する。拡張子がない・未対応の場合は PNG
    ///
    /// 未知の値を読み込んだ場合もこの値とする
    #[default]
    #[serde(other)]
    Original,
}

impl OutputFormat {
    /// 選択肢として表示する順序の全形式
    pub const ALL: [Self; 7] = [
        Self::Original,
        Self::Png,
        Self::Jpeg,
        Self::Webp,
        Self::Bmp,
        Self::Tiff,
        Self::Gif,
    ];

    /// 表示名
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Original => "元の形式",
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::Webp => "WebP",
            Self::Bmp => "BMP",
            Self::Tiff => "TIFF",
            Self::Gif => "GIF",
        }
    }

    /// 形式に対応する拡張子 (小文字、ドットなし)。先頭が出力時の拡張子
    ///
    /// `Original` は固有の拡張子を持たないため空を返す
    #[must_use]
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Original => &[],
            Self::Png => &["png"],
            Self::Jpeg => &["jpg", "jpeg"],
            Self::Webp => &["webp"],
            Self::Bmp => &["bmp"],
            Self::Tiff => &["tiff", "tif"],
            Self::Gif => &["gif"],
        }
    }

    /// 拡張子から形式を判定する (大文字小文字は区別しない)
    ///
    /// # Returns
    /// * `Option<Self>` - 対応形式なら `Some`。`Original` を返すことはない
    #[must_use]
    pub fn from_extension(ext: &str) -> Option<Self> {
        let ext = ext.to_ascii_lowercase();
        Self::ALL
            .into_iter()
            .find(|f| f.extensions().contains(&ext.as_str()))
    }

    /// 元画像 `source` をこの形式で出力するときの拡張子
    ///
    /// `Original` は元の拡張子を小文字にして維持し、拡張子がない・未対応の場合は
    /// `DEFAULT_IMAGE_EXTENSION` とする
    #[must_use]
    pub fn output_extension(self, source: &Path) -> String {
        if let Some(ext) = self.extensions().first() {
            return (*ext).into();
        }
        source
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .filter(|ext| is_supported_image_extension(ext))
            .unwrap_or_else(|| DEFAULT_IMAGE_EXTENSION.into())
    }
}

// ======================================================================
// 画像保存
// ======================================================================
/// 画像をファイルへ保存する (既存ファイルは上書き)
///
/// 形式は拡張子から判定する。JPEG は透過を `JPEG_FLATTEN_BACKGROUND` に合成して
/// RGB 化し、品質 `JPEG_QUALITY` でエンコードする
/// ICC プロファイルは PNG / JPEG / TIFF / WebP に埋め込む。BMP / GIF など非対応の形式や、
/// プロファイルの色空間が出力画像と合わない場合は埋め込まない
/// エンコードをメモリ上で済ませ、同じディレクトリの一時ファイルへ書き込んでから置き換えるため、
/// エンコードや書き込みに失敗しても既存ファイルを壊さない
///
/// # Arguments
/// * `img`         - 保存する画像
/// * `icc_profile` - 埋め込む ICC プロファイル (元画像から引き継いだもの)
/// * `path`        - 保存先パス
///
/// # Errors
/// 拡張子から形式を判定できない、エンコードまたは書き込みに失敗した場合
pub fn save_image(
    img: &DynamicImage,
    icc_profile: Option<&[u8]>,
    path: &Path,
) -> Result<(), SaveError> {
    let bytes = encode_image(img, icc_profile, ImageFormat::from_path(path)?)?;
    replace_file(path, &bytes)?;
    Ok(())
}

/// 画像を `out_dir` に新規ファイルとして保存する (既存ファイルは上書きしない)
///
/// `{stem}.{ext}` が既に存在する場合は `{stem}_2.{ext}`, `_3`, ... と連番を付ける
/// 存在確認と作成を `create_new` で同時に行うため、並行する保存とも衝突しない
///
/// # Arguments
/// * `img`         - 保存する画像
/// * `icc_profile` - 埋め込む ICC プロファイル (扱いは `save_image` と同じ)
/// * `out_dir`     - 出力先ディレクトリ (作成済みであること)
/// * `stem`        - 拡張子を除くファイル名
/// * `ext`         - 拡張子 (ドットなし。形式の判定にも使う)
///
/// # Returns
/// * `(PathBuf, bool)` - 保存したパスと、連番サフィックスで回避したかどうか
///
/// # Errors
/// 形式を判定できない、エンコードまたは書き込みに失敗した場合
pub fn save_image_unique(
    img: &DynamicImage,
    icc_profile: Option<&[u8]>,
    out_dir: &Path,
    stem: &str,
    ext: &str,
) -> Result<(PathBuf, bool), SaveError> {
    let base = out_dir.join(format!("{stem}.{ext}"));
    let bytes = encode_image(img, icc_profile, ImageFormat::from_path(&base)?)?;

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

/// トリミング結果を `out_dir` に新規ファイルとして保存する
///
/// ファイル名は `trimmed_file_name` と同じ規則で、衝突時は連番サフィックスを付ける
/// 自動保存と一括トリミングで共通に使う
///
/// # Arguments
/// * `img`         - トリミング後の画像
/// * `icc_profile` - 元画像の ICC プロファイル (扱いは `save_image` と同じ)
/// * `source`      - 元画像ファイルのパス (名前と `Original` 時の形式に使う)
/// * `out_dir`     - 出力先ディレクトリ (作成済みであること)
/// * `format`      - 出力形式
///
/// # Returns
/// * `(PathBuf, bool)` - 保存したパスと、連番サフィックスで回避したかどうか
///
/// # Errors
/// エンコードまたは書き込みに失敗した場合
pub fn save_trimmed_output(
    img: &DynamicImage,
    icc_profile: Option<&[u8]>,
    source: &Path,
    out_dir: &Path,
    format: OutputFormat,
) -> Result<(PathBuf, bool), SaveError> {
    let stem = trimmed_file_stem(source);
    save_image_unique(
        img,
        icc_profile,
        out_dir,
        &stem,
        &format.output_extension(source),
    )
}

/// 画像を指定形式でメモリ上にエンコードする
///
/// ICC プロファイルを埋め込める形式はエンコーダを直接生成して設定する
/// それ以外の形式は `DynamicImage::write_to` に任せ、プロファイルは破棄する
fn encode_image(
    img: &DynamicImage,
    icc_profile: Option<&[u8]>,
    format: ImageFormat,
) -> Result<Vec<u8>, ImageError> {
    let mut buf = Vec::new();
    let mut cursor = Cursor::new(&mut buf);
    let img = if format == ImageFormat::Jpeg {
        to_jpeg_compatible(img)
    } else {
        to_encoder_compatible(img, format)
    };
    match format {
        ImageFormat::Jpeg => {
            let encoder = JpegEncoder::new_with_quality(&mut cursor, JPEG_QUALITY);
            write_with_icc(&img, icc_profile, encoder)?;
        }
        ImageFormat::Png => write_with_icc(&img, icc_profile, PngEncoder::new(&mut cursor))?,
        ImageFormat::Tiff => write_with_icc(&img, icc_profile, TiffEncoder::new(&mut cursor))?,
        // `write_to` と同じロスレス設定
        ImageFormat::WebP => {
            write_with_icc(&img, icc_profile, WebPEncoder::new_lossless(&mut cursor))?;
        }
        _ => img.write_to(&mut cursor, format)?,
    }
    Ok(buf)
}

/// 色空間が合う場合のみ ICC プロファイルを設定してエンコードする
///
/// # Arguments
/// * `img`         - エンコードする画像 (形式に合わせた変換済みであること)
/// * `icc_profile` - 埋め込む ICC プロファイル
/// * `encoder`     - ICC プロファイルに対応したエンコーダ
fn write_with_icc(
    img: &DynamicImage,
    icc_profile: Option<&[u8]>,
    mut encoder: impl ImageEncoder,
) -> Result<(), ImageError> {
    if let Some(icc) = icc_profile.filter(|icc| icc_matches_color(icc, img)) {
        encoder
            .set_icc_profile(icc.to_vec())
            .map_err(ImageError::Unsupported)?;
    }
    img.write_with_encoder(encoder)
}

/// ICC プロファイルのデータ色空間が画像のカラーモデルと一致するか判定する
///
/// CMYK の JPEG は RGB に変換して読み込まれるため、元の CMYK プロファイルを
/// そのまま埋め込むと色が大きく崩れる。ヘッダの色空間 (オフセット 16〜19) が
/// カラー画像なら `RGB `、グレースケール画像なら `GRAY` の場合のみ一致とみなす
fn icc_matches_color(icc: &[u8], img: &DynamicImage) -> bool {
    let expected: &[u8; 4] = if img.color().has_color() {
        b"RGB "
    } else {
        b"GRAY"
    };
    icc.get(16..20) == Some(expected.as_slice())
}

/// JPEG 以外のエンコーダが受け付けるカラータイプへ変換する
///
/// PNG / TIFF は 16bit までそのまま保存し、それ以外の形式と未対応のカラータイプは
/// 8bit RGB / RGBA に変換する
fn to_encoder_compatible(img: &DynamicImage, format: ImageFormat) -> Cow<'_, DynamicImage> {
    let supported = match format {
        ImageFormat::Png => matches!(
            img.color(),
            ColorType::L8
                | ColorType::La8
                | ColorType::Rgb8
                | ColorType::Rgba8
                | ColorType::L16
                | ColorType::La16
                | ColorType::Rgb16
                | ColorType::Rgba16
        ),
        ImageFormat::Tiff => matches!(
            img.color(),
            ColorType::L8
                | ColorType::Rgb8
                | ColorType::Rgba8
                | ColorType::L16
                | ColorType::Rgb16
                | ColorType::Rgba16
        ),
        _ => matches!(img.color(), ColorType::Rgb8 | ColorType::Rgba8),
    };
    if supported {
        Cow::Borrowed(img)
    } else if img.color().has_alpha() {
        Cow::Owned(DynamicImage::ImageRgba8(img.to_rgba8()))
    } else {
        Cow::Owned(DynamicImage::ImageRgb8(img.to_rgb8()))
    }
}

/// JPEG エンコーダが受け付ける 8bit RGB / グレースケールへ変換する
///
/// グレースケール画像は GRAY の ICC プロファイルを引き継げるよう 8bit グレースケールのまま保つ
/// 透過ピクセルは `JPEG_FLATTEN_BACKGROUND` に合成する
/// (アルファを捨てるだけでは透明部分の RGB 値 (多くは黒) が露出するため)
fn to_jpeg_compatible(img: &DynamicImage) -> Cow<'_, DynamicImage> {
    match img {
        DynamicImage::ImageRgb8(_) | DynamicImage::ImageLuma8(_) => Cow::Borrowed(img),
        _ if !img.color().has_color() && !img.color().has_alpha() => {
            Cow::Owned(DynamicImage::ImageLuma8(img.to_luma8()))
        }
        _ if !img.color().has_color() => {
            let luma_alpha = img.to_luma_alpha8();
            let [bg] = Rgb(JPEG_FLATTEN_BACKGROUND).to_luma().0;
            let luma = GrayImage::from_fn(luma_alpha.width(), luma_alpha.height(), |x, y| {
                let [l, a] = luma_alpha.get_pixel(x, y).0;
                Luma([blend_over(l, bg, a)])
            });
            Cow::Owned(DynamicImage::ImageLuma8(luma))
        }
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

/// `bytes` を一時ファイルへ書き込んでから `path` へ置き換える
///
/// 一時ファイルは rename が同一ボリューム内で完結するよう `path` と同じディレクトリに作る
/// 書き込み・同期・置き換えのいずれかに失敗した場合は一時ファイルを削除し、`path` は元のまま残す
fn replace_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    };
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;

    for n in 0..TEMP_FILE_ATTEMPTS {
        let mut temp_name = OsString::from(".");
        temp_name.push(name);
        temp_name.push(format!(".{}.{n}.tmp", process::id()));
        let temp = dir.join(temp_name);

        let mut file = match OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        };
        // rename 後に中身が欠けないよう、置き換え前にディスクへ反映させる
        let written = file.write_all(bytes).and_then(|()| file.sync_all());
        drop(file);
        let result = written.and_then(|()| fs::rename(&temp, path));
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        return result;
    }
    Err(io::Error::from(io::ErrorKind::AlreadyExists))
}

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
#[must_use]
pub fn trimmed_output_path(source: &Path, out_dir: &Path, format: OutputFormat) -> PathBuf {
    out_dir.join(trimmed_file_name(source, format))
}

/// 出力ファイル名の拡張子を除く部分 (`{元ファイル名}_trimmed`)。元の名前の欠落時は `image`
fn trimmed_file_stem(source: &Path) -> String {
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
    use std::path::Path;

    use super::*;
    use crate::image_load::open_image_with_icc;

    use image::{LumaA, Rgba, RgbaImage};

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

    /// 指定した色空間をヘッダに持つ ICC プロファイル風のバイト列
    ///
    /// 埋め込み・取り出しの検証用で、プロファイルとしての妥当性は問わない
    fn fake_icc(color_space: [u8; 4]) -> Vec<u8> {
        let mut icc = vec![0u8; 132];
        icc[0..4].copy_from_slice(&132u32.to_be_bytes());
        icc[16..20].copy_from_slice(&color_space);
        icc[36..40].copy_from_slice(b"acsp");
        icc
    }

    // ======================================================================
    // パス生成
    // ======================================================================
    /// PNG 拡張子が維持されることを確認する
    #[test]
    fn preserves_png_extension() {
        let path = trimmed_output_path(
            Path::new("photo.png"),
            Path::new("/out"),
            OutputFormat::Original,
        );
        assert_eq!(path, Path::new("/out/photo_trimmed.png"));
    }

    /// JPEG 拡張子が維持されることを確認する
    #[test]
    fn preserves_jpeg_extension() {
        let path = trimmed_output_path(
            Path::new("a.jpeg"),
            Path::new("C:\\out"),
            OutputFormat::Original,
        );
        assert_eq!(path, Path::new("C:\\out/a_trimmed.jpeg"));
    }

    /// 大文字 `.JPG` が小文字 `.jpg` に正規化されることを確認する
    #[test]
    fn preserves_jpg_extension() {
        let path = trimmed_output_path(
            Path::new("a.JPG"),
            Path::new("/out"),
            OutputFormat::Original,
        );
        assert_eq!(path, Path::new("/out/a_trimmed.jpg"));
    }

    /// `.tif` 拡張子が維持されることを確認する
    #[test]
    fn preserves_tif_extension() {
        let path = trimmed_output_path(
            Path::new("scan.TIF"),
            Path::new("/out"),
            OutputFormat::Original,
        );
        assert_eq!(path, Path::new("/out/scan_trimmed.tif"));
    }

    /// WebP と BMP 拡張子がそれぞれ維持されることを確認する
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

    /// 未対応拡張子のとき PNG にフォールバックすることを確認する
    #[test]
    fn unknown_extension_falls_back_to_png() {
        let path = trimmed_output_path(
            Path::new("doc.pdf"),
            Path::new("/out"),
            OutputFormat::Original,
        );
        assert_eq!(path, Path::new("/out/doc_trimmed.png"));
    }

    /// 拡張子なしのファイル名でも PNG にフォールバックすることを確認する
    #[test]
    fn missing_extension_falls_back_to_png() {
        let path = trimmed_output_path(
            Path::new("noext"),
            Path::new("/out"),
            OutputFormat::Original,
        );
        assert_eq!(path, Path::new("/out/noext_trimmed.png"));
    }

    /// 形式指定時は元の拡張子に関係なく形式の拡張子になることを確認する
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

    /// 元の名前がない場合は `image_trimmed` を使うことを確認する
    #[test]
    fn empty_source_uses_fallback_stem() {
        assert_eq!(
            trimmed_file_name(Path::new(""), OutputFormat::Original),
            "image_trimmed.png"
        );
    }

    // ======================================================================
    // 出力形式
    // ======================================================================
    /// 拡張子から形式を大文字小文字を区別せず判定し、未対応は `None` とすることを確認する
    #[test]
    fn output_format_from_extension() {
        assert_eq!(OutputFormat::from_extension("PNG"), Some(OutputFormat::Png));
        assert_eq!(
            OutputFormat::from_extension("jpeg"),
            Some(OutputFormat::Jpeg)
        );
        assert_eq!(
            OutputFormat::from_extension("tif"),
            Some(OutputFormat::Tiff)
        );
        assert_eq!(OutputFormat::from_extension("pdf"), None);
        assert_eq!(OutputFormat::from_extension(""), None);
    }

    /// 全形式の出力拡張子が対応拡張子であり、`from_extension` で同じ形式に戻ることを確認する
    #[test]
    fn output_format_extensions_round_trip() {
        for format in OutputFormat::ALL {
            if format == OutputFormat::Original {
                continue;
            }
            let ext = format.output_extension(Path::new("a.png"));
            assert!(is_supported_image_extension(&ext), "{format:?}: {ext}");
            assert_eq!(OutputFormat::from_extension(&ext), Some(format));
        }
    }

    /// 未知の保存値は `Original` として読み込むことを確認する
    #[test]
    fn output_format_unknown_value_deserializes_to_original() {
        let value: OutputFormat =
            serde::de::Deserialize::deserialize(serde::de::value::StrDeserializer::<
                serde::de::value::Error,
            >::new("Avif"))
            .expect("deserialize");
        assert_eq!(value, OutputFormat::Original);
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
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let path = dir.join("out.jpg");
        save_image(&half_transparent_image(), None, &path).expect("JPEG 保存");

        // 透明だった左側は白、不透明だった右側は赤として再読み込みされる
        let reloaded = image::open(&path).expect("再読み込み").to_rgb8();
        let left = reloaded.get_pixel(1, 4).0;
        let right = reloaded.get_pixel(6, 4).0;
        assert!(left.iter().all(|&c| c > 240), "left = {left:?}");
        assert!(
            right[0] > 200 && right[1] < 60 && right[2] < 60,
            "right = {right:?}"
        );
    }

    /// 16bit 画像も JPEG で保存できることを確認する
    #[test]
    fn save_image_converts_16bit_for_jpeg() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let path = dir.join("out.jpeg");
        let img = DynamicImage::ImageRgba16(image::ImageBuffer::from_pixel(
            4,
            4,
            Rgba([65535u16, 0, 0, 65535]),
        ));
        save_image(&img, None, &path).expect("16bit 画像の JPEG 保存");
        assert!(path.is_file());
    }

    /// 全出力形式で各カラータイプの画像を保存・再読み込みできることを確認する
    #[test]
    fn save_image_supports_all_formats_and_color_types() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let rgba = half_transparent_image();
        let images = [
            DynamicImage::ImageLuma8(rgba.to_luma8()),
            DynamicImage::ImageLumaA8(rgba.to_luma_alpha8()),
            DynamicImage::ImageRgb8(rgba.to_rgb8()),
            rgba.clone(),
            DynamicImage::ImageLuma16(rgba.to_luma16()),
            DynamicImage::ImageLumaA16(rgba.to_luma_alpha16()),
            DynamicImage::ImageRgb16(rgba.to_rgb16()),
            DynamicImage::ImageRgba16(rgba.to_rgba16()),
            DynamicImage::ImageRgb32F(rgba.to_rgb32f()),
            DynamicImage::ImageRgba32F(rgba.to_rgba32f()),
        ];
        for format in OutputFormat::ALL {
            if format == OutputFormat::Original {
                continue;
            }
            for img in &images {
                let path = dir.join(format!(
                    "{:?}_{:?}.{}",
                    format,
                    img.color(),
                    format.output_extension(Path::new(""))
                ));
                save_image(img, None, &path)
                    .unwrap_or_else(|e| panic!("{format:?} / {:?}: {e}", img.color()));
                let reloaded = image::open(&path).expect("再読み込み");
                assert_eq!((reloaded.width(), reloaded.height()), (8, 8));
            }
        }
    }

    /// 拡張子から形式を判定できない場合はエラーとなり、既存ファイルを壊さないことを確認する
    #[test]
    fn save_image_unknown_extension_keeps_existing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let path = dir.join("out.xyz");
        fs::write(&path, b"keep").expect("placeholder");

        let result = save_image(&half_transparent_image(), None, &path);
        assert!(matches!(result, Err(SaveError::Encode(_))));
        assert_eq!(fs::read(&path).expect("read"), b"keep");
    }

    /// 既存ファイルを新しい内容で上書きし、一時ファイルを残さないことを確認する
    #[test]
    fn save_image_overwrites_without_leaving_temp_file() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let path = dir.join("out.png");
        fs::write(&path, b"old").expect("placeholder");

        save_image(&half_transparent_image(), None, &path).expect("上書き保存");
        assert!(image::open(&path).is_ok());
        // ディレクトリには保存先のファイルだけが残る
        assert_eq!(fs::read_dir(dir).expect("read_dir").count(), 1);
    }

    /// 置き換えに失敗した場合はエラーとなり、一時ファイルを残さないことを確認する
    #[test]
    fn save_image_replace_failure_cleans_up_temp_file() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        // 保存先がディレクトリのため rename が失敗する
        let path = dir.join("out.png");
        fs::create_dir(&path).expect("blocking dir");

        let result = save_image(&half_transparent_image(), None, &path);
        assert!(matches!(result, Err(SaveError::Io(_))));
        assert!(path.is_dir());
        assert_eq!(fs::read_dir(dir).expect("read_dir").count(), 1);
    }

    // ======================================================================
    // ICC プロファイル
    // ======================================================================
    /// ICC 対応形式で保存すると元のプロファイルがそのまま埋め込まれることを確認する
    #[test]
    fn save_image_embeds_icc_profile() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let icc = fake_icc(*b"RGB ");
        // `image` の TIFF デコーダはタグ値の読み込み上限を画像バッファサイズに合わせるため、
        // 極小画像だと ICC タグを読めない。実用的な大きさの画像で検証する
        let img = DynamicImage::ImageRgba8(RgbaImage::from_pixel(64, 64, Rgba([255, 0, 0, 255])));
        for ext in ["png", "jpg", "tif", "webp"] {
            let path = dir.join(format!("out.{ext}"));
            save_image(&img, Some(&icc), &path).expect("保存");

            // 再読み込みで同じバイト列が取り出せる
            let loaded = open_image_with_icc(&path).expect("再読み込み");
            assert_eq!(loaded.icc_profile.as_deref(), Some(icc.as_slice()), "{ext}");
        }
    }

    /// ICC プロファイル未指定なら埋め込まないことを確認する
    #[test]
    fn save_image_without_icc_profile() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let path = dir.join("out.png");
        save_image(&half_transparent_image(), None, &path).expect("保存");
        assert!(
            open_image_with_icc(&path)
                .expect("再読み込み")
                .icc_profile
                .is_none()
        );
    }

    /// CMYK プロファイルは RGB 画像に埋め込まないことを確認する
    #[test]
    fn save_image_skips_mismatched_icc_color_space() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let path = dir.join("out.jpg");
        save_image(&half_transparent_image(), Some(&fake_icc(*b"CMYK")), &path).expect("保存");
        assert!(
            open_image_with_icc(&path)
                .expect("再読み込み")
                .icc_profile
                .is_none()
        );
    }

    /// グレースケールのプロファイルは、JPEG 化で RGB になった画像には埋め込まないことを確認する
    #[test]
    fn save_image_skips_gray_icc_after_rgb_conversion() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let path = dir.join("out.jpg");
        save_image(&half_transparent_image(), Some(&fake_icc(*b"GRAY")), &path).expect("保存");
        assert!(
            open_image_with_icc(&path)
                .expect("再読み込み")
                .icc_profile
                .is_none()
        );
    }

    /// グレースケール画像は JPEG でもグレースケールのまま保存し、GRAY プロファイルを引き継ぐことを確認する
    #[test]
    fn save_image_keeps_gray_icc_for_grayscale_jpeg() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let icc = fake_icc(*b"GRAY");
        let images = [
            DynamicImage::ImageLuma8(image::ImageBuffer::from_pixel(4, 4, Luma([128]))),
            DynamicImage::ImageLumaA8(image::ImageBuffer::from_pixel(4, 4, LumaA([128, 255]))),
            DynamicImage::ImageLuma16(image::ImageBuffer::from_pixel(4, 4, Luma([32896]))),
            DynamicImage::ImageLumaA16(image::ImageBuffer::from_pixel(4, 4, LumaA([32896, 65535]))),
        ];
        for img in &images {
            let path = dir.join(format!("{:?}.jpg", img.color()));
            save_image(img, Some(&icc), &path).expect("JPEG 保存");

            // 8bit グレースケールとして保存され、プロファイルも埋め込まれる
            let loaded = open_image_with_icc(&path).expect("再読み込み");
            assert_eq!(loaded.image.color(), ColorType::L8, "{:?}", img.color());
            assert_eq!(
                loaded.icc_profile.as_deref(),
                Some(icc.as_slice()),
                "{:?}",
                img.color()
            );
        }
    }

    /// グレースケールの透過画像を JPEG で保存すると、透明部分が背景色に合成されることを確認する
    #[test]
    fn save_image_flattens_gray_alpha_for_jpeg() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let path = dir.join("out.jpg");
        // 左半分は透明な黒、右半分は不透明な黒
        let img = DynamicImage::ImageLumaA8(image::ImageBuffer::from_fn(8, 8, |x, _| {
            LumaA([0, if x < 4 { 0 } else { 255 }])
        }));
        save_image(&img, None, &path).expect("JPEG 保存");

        let reloaded = image::open(&path).expect("再読み込み").to_luma8();
        let left = reloaded.get_pixel(1, 4).0[0];
        let right = reloaded.get_pixel(6, 4).0[0];
        assert!(left > 240, "left = {left}");
        assert!(right < 15, "right = {right}");
    }

    /// ICC 非対応形式ではプロファイルを破棄して保存に成功することを確認する
    #[test]
    fn save_image_ignores_icc_for_unsupported_format() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let path = dir.join("out.bmp");
        save_image(&half_transparent_image(), Some(&fake_icc(*b"RGB ")), &path).expect("保存");
        assert!(path.is_file());
    }

    // ======================================================================
    // 連番保存
    // ======================================================================
    /// 同名ファイルが無いときは指定名のまま保存することを確認する
    #[test]
    fn save_unique_uses_base_name_when_not_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let (path, renamed) =
            save_image_unique(&half_transparent_image(), None, dir, "photo", "png").expect("保存");
        assert_eq!(path, dir.join("photo.png"));
        assert!(!renamed);
        assert!(path.is_file());
    }

    /// 既存ファイルを上書きせず連番サフィックスで回避することを確認する
    #[test]
    fn save_unique_appends_suffix_without_overwriting() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let existing = dir.join("photo.png");
        fs::write(&existing, b"x").expect("placeholder");

        let (path, renamed) =
            save_image_unique(&half_transparent_image(), None, dir, "photo", "png").expect("保存");
        assert_eq!(path, dir.join("photo_2.png"));
        assert!(renamed);
        // 既存ファイルの内容は変わらない
        assert_eq!(fs::read(&existing).expect("read"), b"x");
    }

    /// トリミング結果が `{名前}_trimmed.{拡張子}` で保存され、2 回目は連番になることを確認する
    #[test]
    fn save_trimmed_output_names_and_suffixes() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let img = half_transparent_image();
        let src = Path::new("in/photo.JPG");

        let (first, renamed_first) =
            save_trimmed_output(&img, None, src, dir, OutputFormat::Original).expect("1 回目");
        assert_eq!(first, dir.join("photo_trimmed.jpg"));
        assert!(!renamed_first);

        let (second, renamed_second) =
            save_trimmed_output(&img, None, src, dir, OutputFormat::Original).expect("2 回目");
        assert_eq!(second, dir.join("photo_trimmed_2.jpg"));
        assert!(renamed_second);

        // 形式を指定すると元の拡張子に関係なくその形式で保存する
        let (webp, _) =
            save_trimmed_output(&img, None, src, dir, OutputFormat::Webp).expect("WebP 保存");
        assert_eq!(webp, dir.join("photo_trimmed.webp"));
        assert_eq!(
            ImageFormat::from_path(&webp).expect("形式"),
            image::guess_format(&fs::read(&webp).expect("read")).expect("推定")
        );
    }
}
