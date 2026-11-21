//! 大容量画像によるメモリ枯渇を防ぐための安全な画像読み込み

use std::error::Error;
use std::fmt;
use std::fs;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::num::NonZeroU64;
use std::path::Path;

use gif::{DecodeOptions, MemoryLimit};
use image::codecs::png::PngDecoder;
use image::codecs::webp::WebPDecoder;
use image::error::LimitErrorKind;
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageError, ImageFormat, ImageReader, Limits};

use crate::consts::{
    MAX_ICC_PROFILE_BYTES, MAX_IMAGE_FILE_BYTES, MAX_IMAGE_HEIGHT, MAX_IMAGE_PIXELS,
    MAX_IMAGE_WIDTH,
};
use crate::dpi::{Dpi, read_dpi};
use crate::image_output::OutputFormat;

/// デコーダ内部の作業領域として、画像バッファとは別に確保を許す量
const DECODE_ALLOC_MARGIN_BYTES: u64 = 16 * 1024 * 1024;
/// PNG のデコーダ生成時に許す確保量
///
/// 圧縮済みの iCCP チャンクと展開結果を同時に保持できるよう、ICC プロファイルの上限の 2 倍に余裕を足す
const PNG_HEADER_ALLOC_BYTES: u64 = MAX_ICC_PROFILE_BYTES * 2 + 1024 * 1024;

// ======================================================================
// 読み込み結果
// ======================================================================
/// 読み込んだ画像と、保存時に引き継ぐメタデータ
#[derive(Debug, Clone)]
pub struct LoadedImage {
    /// EXIF Orientation を適用済みの画像
    pub image: DynamicImage,
    /// 埋め込まれていた ICC プロファイル。無い、取得に失敗した、または `MAX_ICC_PROFILE_BYTES` を超える場合は `None`
    pub icc_profile: Option<Vec<u8>>,
    /// 埋め込まれていた解像度 (表示上の向きに合わせた値)。PNG / JPEG / TIFF 以外、記録が無い、または長さの単位でない場合は `None`
    pub dpi: Option<Dpi>,
    /// 先頭フレーム以外にも画像を含むファイルか (アニメーション GIF / WebP / APNG、複数ページ TIFF)
    ///
    /// 読み込むのは先頭の 1 フレームだけで、残りは無視される。判定に失敗した場合は `false`
    pub has_extra_frames: bool,
    /// 読み込み時にファイルの中身から判定した形式。出力非対応の形式・判定できない場合は `None`
    ///
    /// 出力形式が `Original` のときの保存形式の決定に使い、保存時にファイルを開き直さずに済ませる
    pub source_format: Option<OutputFormat>,
}

// ======================================================================
// エラー型
// ======================================================================
/// 画像読み込み時のエラー
#[derive(Debug)]
pub enum LoadError {
    /// ファイルサイズが上限を超えている
    FileTooLarge { size: u64, limit: u64 },
    /// 画像の幅・高さ・ピクセル数が上限を超えている
    DimensionsTooLarge {
        width: u32,
        height: u32,
        kind: DimensionLimitKind,
    },
    /// クリップボード画像のバイト列長が RGBA として不正
    ClipboardSizeMismatch {
        width: u32,
        height: u32,
        bytes: usize,
    },
    /// ファイル I/O エラー
    Io(io::Error),
    /// 画像デコードエラー
    Decode(ImageError),
}

/// 寸法チェックで抵触した上限の種類
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DimensionLimitKind {
    Width,
    Height,
    Pixels,
}

// ======================================================================
// エラー表示
// ======================================================================
impl From<ImageError> for LoadError {
    fn from(value: ImageError) -> Self {
        Self::Decode(value)
    }
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileTooLarge { size, limit } => write!(
                f,
                "ファイルサイズが大きすぎます ({actual} / 上限 {limit_str})",
                actual = format_bytes(*size),
                limit_str = format_bytes(*limit),
            ),
            Self::DimensionsTooLarge {
                width,
                height,
                kind,
            } => {
                let pixels = u64::from(*width) * u64::from(*height);
                let detail = match kind {
                    DimensionLimitKind::Width => {
                        format!("幅 {width} px が上限 {MAX_IMAGE_WIDTH} px を超えています")
                    }
                    DimensionLimitKind::Height => {
                        format!("高さ {height} px が上限 {MAX_IMAGE_HEIGHT} px を超えています")
                    }
                    DimensionLimitKind::Pixels => format!(
                        "ピクセル数 {dims} ({mp}) が上限 {limit_mp} を超えています",
                        dims = format_dimensions(*width, *height),
                        mp = format_megapixels(pixels),
                        limit_mp = format_megapixels(MAX_IMAGE_PIXELS),
                    ),
                };
                write!(f, "画像サイズが大きすぎます: {detail}")
            }
            Self::ClipboardSizeMismatch {
                width,
                height,
                bytes,
            } => write!(
                f,
                "クリップボード画像のデータサイズが不正です ({width}×{height} に対し {bytes} バイト)",
            ),
            Self::Io(e) => write!(f, "読み込みエラー: {e}"),
            Self::Decode(ImageError::Limits(e)) => match e.kind() {
                LimitErrorKind::InsufficientMemory => write!(
                    f,
                    "画像のデコードに必要なメモリが上限を超えたため読み込めません",
                ),
                _ => write!(
                    f,
                    "画像が大きすぎるため読み込めません (上限: 幅 {MAX_IMAGE_WIDTH} / 高さ {MAX_IMAGE_HEIGHT} / {limit_mp})",
                    limit_mp = format_megapixels(MAX_IMAGE_PIXELS),
                ),
            },
            Self::Decode(e) => write!(f, "読み込みエラー: {}", image_error_message(e)),
        }
    }
}

/// `image` クレートのエラーを利用者向けの日本語メッセージへ変換する
///
/// 元のエラー文は英語のため表示に使わない。詳細は `source()` から辿れる
///
/// # Arguments
/// * `error` - 変換する `image` クレートのエラー
///
/// # Returns
/// * `String` - 日本語のエラー概要
pub(crate) fn image_error_message(error: &ImageError) -> String {
    match error {
        ImageError::Decoding(_) => {
            "画像データを解析できません (ファイルが破損しているか、対応していない形式の可能性があります)"
                .to_owned()
        }
        ImageError::Encoding(_) => "画像のエンコードに失敗しました".to_owned(),
        ImageError::Parameter(_) => "画像の処理パラメータが不正です".to_owned(),
        ImageError::Limits(_) => "画像が大きすぎて処理できません".to_owned(),
        ImageError::Unsupported(_) => "対応していない画像形式または機能です".to_owned(),
        ImageError::IoError(e) => e.to_string(),
    }
}

impl From<io::Error> for LoadError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl Error for LoadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Decode(e) => Some(e),
            Self::FileTooLarge { .. }
            | Self::DimensionsTooLarge { .. }
            | Self::ClipboardSizeMismatch { .. } => None,
        }
    }
}

// ======================================================================
// 画像読み込み
// ======================================================================
/// ファイルから画像を安全に読み込む
///
/// ファイルサイズ・ヘッダ上の寸法・デコード時の `image::Limits` を順に検証する
/// EXIF の Orientation があれば回転・反転を適用し、表示上の向きで返す
/// ICC プロファイルも必要な場合は `open_image_with_icc` を使う
///
/// # Arguments
/// * `path` - 読み込む画像ファイルのパス
///
/// # Returns
/// * `Result<DynamicImage, LoadError>` - 向きを補正した画像
///
/// # Errors
/// ファイルサイズ・寸法が上限を超える、または読み込み・デコードに失敗した場合
pub fn open_image(path: &Path) -> Result<DynamicImage, LoadError> {
    open_image_with_icc(path).map(|loaded| loaded.image)
}

/// ファイルから画像を ICC プロファイルとともに安全に読み込む
///
/// 検証と向きの補正は `open_image` と同じ
/// ICC プロファイルは保存時に再埋め込みし、色の見え方を元画像と揃えるために使う
///
/// # Arguments
/// * `path` - 読み込む画像ファイルのパス
///
/// # Returns
/// * `Result<LoadedImage, LoadError>` - 向きを補正した画像と ICC プロファイル (埋め込みがなければ `None`)
///
/// # Errors
/// ファイルサイズ・寸法が上限を超える、または読み込み・デコードに失敗した場合
pub fn open_image_with_icc(path: &Path) -> Result<LoadedImage, LoadError> {
    open_image_with_icc_reserving(path, |_| ()).map(|(loaded, ())| loaded)
}

/// `open_image_with_icc` と同じ読み込みを、デコード前の確保処理つきで行う
///
/// ヘッダの検証後、デコード先バッファのバイト数を `reserve` へ渡してからデコードする
/// 並列処理でメモリ使用量を見積もって確保するために使う
/// ファイルは 1 回だけ開き、検証・デコード・フレーム数の判定で同じハンドルを使い回す
///
/// # Arguments
/// * `path` - 読み込む画像ファイルのパス
/// * `reserve` - デコード先バッファのバイト数 (`ImageDecoder::total_bytes`) を受け取る処理
///
/// # Returns
/// * `Result<(LoadedImage, G), LoadError>` - 読み込んだ画像と、`reserve` が返した値
///
/// # Errors
/// ファイルサイズ・寸法が上限を超える、または読み込み・デコードに失敗した場合
/// いずれも `reserve` を呼ぶ前に検出できるものは `reserve` を呼ばずに返す
pub(crate) fn open_image_with_icc_reserving<G>(
    path: &Path,
    reserve: impl FnOnce(u64) -> G,
) -> Result<(LoadedImage, G), LoadError> {
    let mut file = fs::File::open(path)?;
    check_file_size(file.metadata()?.len())?;

    let mut limits = decode_limits();

    let mut reader = ImageReader::new(BufReader::new(&file)).with_guessed_format()?;
    let format = reader.format();
    reader.limits(construction_limits(format));
    let mut decoder = reader.into_decoder()?;
    let (width, height) = decoder.dimensions();
    check_dimensions(width, height)?;

    // 寸法は検証済みのため、16bit・浮動小数点画像でもデコード先バッファが収まるよう上限を広げる
    // その上で `ImageReader::decode()` と同様にデコード先バッファ分を上限から差し引く
    let total_bytes = decoder.total_bytes();
    let reserved = reserve(total_bytes);
    limits.max_alloc = Some(decode_alloc_budget(total_bytes));
    limits.reserve(total_bytes)?;
    decoder.set_limits(limits)?;

    // 壊れた EXIF で読み込み自体を失敗させないよう、取得できなければ無変換とする
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    // ICC プロファイルも同様に、壊れていれば埋め込みなしとして扱う
    // 上限を超えるものも保存時に再埋め込みしないよう、埋め込みなしとする
    let icc_profile = decoder
        .icc_profile()
        .ok()
        .flatten()
        .filter(|icc| !icc.is_empty() && icc.len() as u64 <= MAX_ICC_PROFILE_BYTES);
    let mut image = DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    let has_extra_frames = format.is_some_and(|format| detect_extra_frames(&mut file, format));
    // 解像度の取得に失敗しても読み込みは成功とする (保存時に書き込まないだけ)
    let dpi = format
        .and_then(|format| read_dpi(&mut BufReader::new(&file), format))
        .map(|dpi| dpi.oriented(orientation));
    let loaded = LoadedImage {
        image,
        icc_profile,
        dpi,
        has_extra_frames,
        source_format: format.and_then(OutputFormat::from_image_format),
    };
    Ok((loaded, reserved))
}

/// 先頭フレーム以外にも画像を含むかを開いたままのファイルから判定する
///
/// 読み込み自体は先頭フレームだけで成立するため、判定に失敗した場合は `false` とする
/// ファイルの読み取り位置は先頭へ戻してから使う
///
/// # Arguments
/// * `file` - 読み込みに使った画像ファイル
/// * `format` - 読み込み時に判定した画像形式
///
/// # Returns
/// * `bool` - 2 フレーム (ページ) 以上を含む場合は `true`
fn detect_extra_frames(file: &mut fs::File, format: ImageFormat) -> bool {
    if !matches!(
        format,
        ImageFormat::Gif | ImageFormat::WebP | ImageFormat::Tiff | ImageFormat::Png
    ) || file.seek(SeekFrom::Start(0)).is_err()
    {
        return false;
    }
    let reader = BufReader::new(&*file);
    match format {
        ImageFormat::Gif => gif_has_extra_frames(reader),
        ImageFormat::WebP => WebPDecoder::new(reader).is_ok_and(|decoder| decoder.has_animation()),
        // APNG はヘッダの acTL チャンクの有無で判定する
        // `PngDecoder::new` は上限なしで iCCP を展開するため、読み込みと同じ上限を渡す
        ImageFormat::Png => {
            PngDecoder::with_limits(reader, construction_limits(Some(ImageFormat::Png)))
                .is_ok_and(|decoder| decoder.is_apng().unwrap_or(false))
        }
        _ => tiff::decoder::Decoder::new(reader).is_ok_and(|decoder| decoder.more_images()),
    }
}

/// GIF が 2 フレーム以上を含むかを、画素を展開せずに判定する
///
/// フレームのメタデータだけを読み進め、LZW 展開とフレームバッファの確保は行わない
/// デコーダの確保量は `DECODE_ALLOC_MARGIN_BYTES` までに制限する
///
/// # Arguments
/// * `reader` - GIF を読み取るリーダー
///
/// # Returns
/// * `bool` - 2 フレーム以上を含む場合は `true`。読み取りに失敗した場合は `false`
fn gif_has_extra_frames(reader: impl Read) -> bool {
    let mut options = DecodeOptions::new();
    if let Some(limit) = NonZeroU64::new(DECODE_ALLOC_MARGIN_BYTES) {
        options.set_memory_limit(MemoryLimit::Bytes(limit));
    }
    let Ok(mut decoder) = options.read_info(reader) else {
        return false;
    };
    // 2 枚目のメタデータまで読めれば複数フレームと分かる
    matches!(decoder.next_frame_info(), Ok(Some(_)))
        && matches!(decoder.next_frame_info(), Ok(Some(_)))
}

/// クリップボード由来の RGBA 画像寸法を検証する
///
/// # Arguments
/// * `width` - 画像の幅 (px)
/// * `height` - 画像の高さ (px)
/// * `byte_len` - RGBA バイト列の長さ
///
/// # Returns
/// * `Result<(), LoadError>` - 寸法とバイト列長が妥当なら `Ok(())`
///
/// # Errors
/// 寸法が上限を超える、またはバイト列長が RGBA として不正な場合
pub fn validate_clipboard_rgba(width: u32, height: u32, byte_len: usize) -> Result<(), LoadError> {
    check_dimensions(width, height)?;
    let expected = rgba_buffer_len(width, height)?;
    if byte_len != expected {
        return Err(LoadError::ClipboardSizeMismatch {
            width,
            height,
            bytes: byte_len,
        });
    }
    Ok(())
}

/// ファイルサイズが上限内か検証する
///
/// # Arguments
/// * `size` - ファイルサイズ (バイト)
///
/// # Returns
/// * `Result<(), LoadError>` - 上限内なら `Ok(())`
///
/// # Errors
/// ファイルサイズが上限を超えている場合
pub fn check_file_size(size: u64) -> Result<(), LoadError> {
    if size > MAX_IMAGE_FILE_BYTES {
        return Err(LoadError::FileTooLarge {
            size,
            limit: MAX_IMAGE_FILE_BYTES,
        });
    }
    Ok(())
}

/// 画像の幅・高さ・ピクセル数が上限内か検証する
///
/// # Arguments
/// * `width` - 画像の幅 (px)
/// * `height` - 画像の高さ (px)
///
/// # Returns
/// * `Result<(), LoadError>` - すべて上限内なら `Ok(())`
///
/// # Errors
/// 幅・高さ・ピクセル数のいずれかが上限を超えている場合
pub fn check_dimensions(width: u32, height: u32) -> Result<(), LoadError> {
    if width > MAX_IMAGE_WIDTH {
        return Err(LoadError::DimensionsTooLarge {
            width,
            height,
            kind: DimensionLimitKind::Width,
        });
    }
    if height > MAX_IMAGE_HEIGHT {
        return Err(LoadError::DimensionsTooLarge {
            width,
            height,
            kind: DimensionLimitKind::Height,
        });
    }
    let pixels = u64::from(width) * u64::from(height);
    if pixels > MAX_IMAGE_PIXELS {
        return Err(LoadError::DimensionsTooLarge {
            width,
            height,
            kind: DimensionLimitKind::Pixels,
        });
    }
    Ok(())
}

// ======================================================================
// 内部ヘルパー
// ======================================================================
/// `image` クレート向けのデコード上限を構築する
fn decode_limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_WIDTH);
    limits.max_image_height = Some(MAX_IMAGE_HEIGHT);
    limits.max_alloc = Some(decode_alloc_budget(0));
    limits
}

/// デコーダ生成時に使うデコード上限を形式に応じて構築する
///
/// PNG は生成時にヘッダのチャンクを読み、iCCP を zlib 展開する
/// 展開サイズの上限は生成時の `max_alloc` で決まり、後から `set_limits` で広げても反映されない
/// 小さなファイルが巨大な展開を要求できるため、`PNG_HEADER_ALLOC_BYTES` まで絞る
/// 上限を超えた iCCP はデコーダが読み捨て、プロファイルなしとして扱われる
///
/// # Arguments
/// * `format` - 判定した画像形式
///
/// # Returns
/// * `Limits` - デコーダ生成時に渡す上限
fn construction_limits(format: Option<ImageFormat>) -> Limits {
    let mut limits = decode_limits();
    if format == Some(ImageFormat::Png) {
        limits.max_alloc = Some(PNG_HEADER_ALLOC_BYTES);
    }
    limits
}

/// デコード時に許すメモリ確保量の上限を求める
///
/// 最大ピクセル数の RGBA8 バッファ 1 枚分を基準とし、
/// 16bit・浮動小数点画像などデコード先バッファがそれを超える場合はその大きさに合わせる
///
/// # Arguments
/// * `decode_buffer_bytes` - デコード先バッファのバイト数 (`ImageDecoder::total_bytes`)
///
/// # Returns
/// * `u64` - デコード先バッファとデコーダ内部の作業領域を合わせた上限バイト数
fn decode_alloc_budget(decode_buffer_bytes: u64) -> u64 {
    MAX_IMAGE_PIXELS
        .saturating_mul(4)
        .max(decode_buffer_bytes)
        .saturating_add(DECODE_ALLOC_MARGIN_BYTES)
}

/// RGBA 画像 1 枚分のバイト数
fn rgba_buffer_len(width: u32, height: u32) -> Result<usize, LoadError> {
    let bytes = u64::from(width)
        .saturating_mul(u64::from(height))
        .saturating_mul(4);
    usize::try_from(bytes).map_err(|_| LoadError::DimensionsTooLarge {
        width,
        height,
        kind: DimensionLimitKind::Pixels,
    })
}

/// バイト数を `B` / `KB` / `MB` 単位の表示用文字列にする
#[expect(
    clippy::cast_precision_loss,
    reason = "表示用の概算値で、精度の損失は問題にならない"
)]
pub(crate) fn format_bytes(n: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * 1024;
    if n >= MIB {
        format!("{:.1} MB", n as f64 / MIB as f64)
    } else if n >= KIB {
        format!("{:.1} KB", n as f64 / KIB as f64)
    } else {
        format!("{n} B")
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "表示用の概算値で、精度の損失は問題にならない"
)]
fn format_megapixels(pixels: u64) -> String {
    if pixels >= 1_000_000 {
        format!("{:.1} MP", pixels as f64 / 1_000_000.0)
    } else {
        format!("{pixels} px")
    }
}

fn format_dimensions(width: u32, height: u32) -> String {
    format!("{width}×{height}")
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use image::codecs::gif::GifEncoder;
    use image::codecs::png::PngEncoder;
    use image::{DynamicImage, ExtendedColorType, Frame, ImageEncoder, Rgba, RgbaImage};

    use super::*;

    const RED: Rgba<u8> = Rgba([255, 0, 0, 255]);

    // ======================================================================
    // ヘルパー関数
    // ======================================================================
    /// 指定寸法の空 PNG を一時パスへ書き出す
    fn write_test_png(path: &Path, width: u32, height: u32) {
        let img = DynamicImage::ImageRgba8(RgbaImage::new(width, height));
        img.save(path).expect("test png write");
    }

    /// Orientation タグのみを持つ little-endian の EXIF (TIFF 形式) を生成する
    fn exif_with_orientation(value: u16) -> Vec<u8> {
        let mut exif = Vec::new();
        exif.extend_from_slice(b"II");
        exif.extend_from_slice(&42u16.to_le_bytes());
        exif.extend_from_slice(&8u32.to_le_bytes());
        // IFD0: エントリ数 1、Orientation (0x0112) / SHORT / 1 個
        exif.extend_from_slice(&1u16.to_le_bytes());
        exif.extend_from_slice(&0x0112u16.to_le_bytes());
        exif.extend_from_slice(&3u16.to_le_bytes());
        exif.extend_from_slice(&1u32.to_le_bytes());
        exif.extend_from_slice(&value.to_le_bytes());
        exif.extend_from_slice(&[0, 0]);
        // 次の IFD なし
        exif.extend_from_slice(&0u32.to_le_bytes());
        exif
    }

    /// 左上 1 px だけ赤い 3×2 の PNG を EXIF Orientation 付きで書き出す
    fn write_png_with_orientation(path: &Path, orientation: u16) {
        let mut img = RgbaImage::from_pixel(3, 2, Rgba([255, 255, 255, 255]));
        img.put_pixel(0, 0, RED);
        let mut encoder = PngEncoder::new(fs::File::create(path).expect("create png"));
        encoder
            .set_exif_metadata(exif_with_orientation(orientation))
            .expect("set exif");
        encoder
            .write_image(img.as_raw(), 3, 2, ExtendedColorType::Rgba8)
            .expect("write png");
    }

    // ======================================================================
    // check_dimensions
    // ======================================================================
    /// 上限内の寸法が受理されること
    #[test]
    fn check_dimensions_accepts_within_limits() {
        assert!(check_dimensions(4000, 3000).is_ok());
    }

    /// 幅が上限を超えると `DimensionLimitKind::Width` で拒否されること
    #[test]
    fn check_dimensions_rejects_excessive_width() {
        let err = check_dimensions(MAX_IMAGE_WIDTH + 1, 100).unwrap_err();
        assert!(matches!(
            err,
            LoadError::DimensionsTooLarge {
                kind: DimensionLimitKind::Width,
                ..
            }
        ));
    }

    /// 高さが上限を超えると `DimensionLimitKind::Height` で拒否されること
    #[test]
    fn check_dimensions_rejects_excessive_height() {
        let err = check_dimensions(100, MAX_IMAGE_HEIGHT + 1).unwrap_err();
        assert!(matches!(
            err,
            LoadError::DimensionsTooLarge {
                kind: DimensionLimitKind::Height,
                ..
            }
        ));
    }

    /// ピクセル数が上限を超えると `DimensionLimitKind::Pixels` で拒否されること
    #[test]
    fn check_dimensions_rejects_excessive_pixels() {
        let w = 10_000;
        let h = 10_000;
        let err = check_dimensions(w, h).unwrap_err();
        assert!(matches!(
            err,
            LoadError::DimensionsTooLarge {
                kind: DimensionLimitKind::Pixels,
                ..
            }
        ));
    }

    // ======================================================================
    // validate_clipboard_rgba
    // ======================================================================
    /// RGBA バッファ長が不一致のとき `ClipboardSizeMismatch` を返すこと
    #[test]
    fn validate_clipboard_rgba_rejects_size_mismatch() {
        let err = validate_clipboard_rgba(10, 10, 100).unwrap_err();
        assert!(matches!(err, LoadError::ClipboardSizeMismatch { .. }));
    }

    /// 期待バイト数と一致する RGBA バッファが受理されること
    #[test]
    fn validate_clipboard_rgba_accepts_valid_buffer() {
        assert!(validate_clipboard_rgba(2, 2, 16).is_ok());
    }

    // ======================================================================
    // check_file_size
    // ======================================================================
    /// ファイルサイズが上限超過のとき `FileTooLarge` を返すこと
    #[test]
    fn check_file_size_rejects_oversized() {
        let err = check_file_size(MAX_IMAGE_FILE_BYTES + 1).unwrap_err();
        assert!(matches!(err, LoadError::FileTooLarge { .. }));
    }

    /// 上限ちょうどのファイルサイズが受理されること
    #[test]
    fn check_file_size_accepts_within_limit() {
        assert!(check_file_size(MAX_IMAGE_FILE_BYTES).is_ok());
    }

    // ======================================================================
    // open_image
    // ======================================================================
    /// 存在しないファイルを開くと I/O またはデコードエラーになること
    #[test]
    fn open_image_rejects_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing.png");
        let err = open_image(&path).unwrap_err();
        assert!(matches!(err, LoadError::Io(_) | LoadError::Decode(_)));
    }

    /// 小さな PNG ファイルが正常に読み込めること
    #[test]
    fn open_image_loads_small_png() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("small.png");
        write_test_png(&path, 8, 8);
        let img = open_image(&path).expect("open small png");
        assert_eq!(img.width(), 8);
        assert_eq!(img.height(), 8);
    }

    /// EXIF Orientation が 6 (時計回り 90°) のとき回転して読み込まれること
    #[test]
    fn open_image_applies_exif_rotate90() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("orient6.png");
        write_png_with_orientation(&path, 6);
        let img = open_image(&path).expect("open png").to_rgba8();

        // 幅と高さが入れ替わり、左上の赤が右上へ移る
        assert_eq!(img.dimensions(), (2, 3));
        assert_eq!(*img.get_pixel(1, 0), RED);
        assert_ne!(*img.get_pixel(0, 0), RED);
    }

    /// EXIF Orientation が 2 (左右反転) のとき反転して読み込まれること
    #[test]
    fn open_image_applies_exif_flip_horizontal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("orient2.png");
        write_png_with_orientation(&path, 2);
        let img = open_image(&path).expect("open png").to_rgba8();

        // 寸法は変わらず、左上の赤が右上へ移る
        assert_eq!(img.dimensions(), (3, 2));
        assert_eq!(*img.get_pixel(2, 0), RED);
        assert_ne!(*img.get_pixel(0, 0), RED);
    }

    /// EXIF Orientation が 1 (無変換) のとき元の向きのまま読み込まれること
    #[test]
    fn open_image_keeps_orientation_when_no_transform() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("orient1.png");
        write_png_with_orientation(&path, 1);
        let img = open_image(&path).expect("open png").to_rgba8();

        assert_eq!(img.dimensions(), (3, 2));
        assert_eq!(*img.get_pixel(0, 0), RED);
    }

    /// 16bit 画像を開けること
    #[test]
    fn open_image_loads_16bit_png() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rgba16.png");
        DynamicImage::new_rgba16(8, 8)
            .save(&path)
            .expect("test png write");
        let img = open_image(&path).expect("open 16bit png");

        assert_eq!(img.color(), image::ColorType::Rgba16);
        assert_eq!((img.width(), img.height()), (8, 8));
    }

    // ======================================================================
    // open_image_with_icc_reserving
    // ======================================================================
    /// デコード先バッファのバイト数を色の型に応じて `reserve` へ渡すこと
    #[test]
    fn reserving_passes_decoded_bytes_by_color_type() {
        let dir = tempfile::tempdir().unwrap();
        let rgba8 = dir.path().join("rgba8.png");
        write_test_png(&rgba8, 10, 6);
        let rgba16 = dir.path().join("rgba16.png");
        DynamicImage::new_rgba16(10, 6)
            .save(&rgba16)
            .expect("test png write");

        // RGBA8 は 1 ピクセル 4 バイト、RGBA16 は 8 バイト
        let (loaded, bytes) =
            open_image_with_icc_reserving(&rgba8, |bytes| bytes).expect("open rgba8");
        assert_eq!(bytes, 10 * 6 * 4);
        assert_eq!((loaded.image.width(), loaded.image.height()), (10, 6));
        let (_, bytes) =
            open_image_with_icc_reserving(&rgba16, |bytes| bytes).expect("open rgba16");
        assert_eq!(bytes, 10 * 6 * 8);
    }

    /// 存在しないファイルは `reserve` を呼ばずに I/O エラーになること
    #[test]
    fn reserving_rejects_missing_file_without_calling_reserve() {
        let dir = tempfile::tempdir().unwrap();
        let mut called = false;
        let err = open_image_with_icc_reserving(&dir.path().join("missing.png"), |_| called = true)
            .unwrap_err();
        assert!(matches!(err, LoadError::Io(_)));
        assert!(!called);
    }

    /// アニメーション GIF は先頭フレームだけが読み込まれること
    #[test]
    fn open_image_reads_only_first_frame_of_animated_gif() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("anim.gif");
        let mut encoder = GifEncoder::new(fs::File::create(&path).expect("create gif"));
        for color in [Rgba([255, 0, 0, 255]), Rgba([0, 0, 255, 255])] {
            let frame = Frame::new(RgbaImage::from_pixel(4, 3, color));
            encoder.encode_frame(frame).expect("encode frame");
        }
        drop(encoder);
        let img = open_image(&path).expect("open gif").to_rgba8();

        // 2 枚目の青ではなく、1 枚目の赤が得られる
        assert_eq!(img.dimensions(), (4, 3));
        let px = img.get_pixel(0, 0);
        assert!(px[0] > 200 && px[2] < 50, "{px:?}");
    }

    // ======================================================================
    // 複数フレームの検出
    // ======================================================================
    /// 指定フレーム数の GIF を一時パスへ保存する
    fn write_gif(path: &Path, frames: usize) {
        let mut encoder = GifEncoder::new(fs::File::create(path).expect("create gif"));
        for _ in 0..frames {
            let frame = Frame::new(RgbaImage::from_pixel(4, 3, RED));
            encoder.encode_frame(frame).expect("encode frame");
        }
    }

    /// 指定ページ数の TIFF を一時パスへ保存する
    fn write_tiff(path: &Path, pages: usize) {
        let mut encoder =
            tiff::encoder::TiffEncoder::new(fs::File::create(path).expect("create tiff"))
                .expect("tiff encoder");
        for _ in 0..pages {
            encoder
                .write_image::<tiff::encoder::colortype::RGB8>(2, 2, &[255; 12])
                .expect("write page");
        }
    }

    /// 静止画 PNG の IHDR 直後へ acTL チャンクを挿入して APNG とする
    fn write_apng(path: &Path) {
        write_test_png(path, 4, 4);
        let mut bytes = fs::read(path).expect("read png");
        let mut chunk = Vec::from(*b"acTL");
        chunk.extend_from_slice(&2u32.to_be_bytes());
        chunk.extend_from_slice(&0u32.to_be_bytes());
        // CRC-32 (チャンク種別とデータが対象)
        let mut crc = u32::MAX;
        for byte in &chunk {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        // シグネチャ + IHDR (長さ・種別・データ・CRC) の直後
        let out = 8 + 8 + 13 + 4;
        let mut insert = 8u32.to_be_bytes().to_vec();
        insert.extend_from_slice(&chunk);
        insert.extend_from_slice(&(!crc).to_be_bytes());
        bytes.splice(out..out, insert);
        fs::write(path, bytes).expect("write apng");
    }

    /// APNG は追加フレームありと判定すること
    #[test]
    fn open_image_flags_extra_frames_for_apng() {
        let dir = tempfile::tempdir().unwrap();
        let apng = dir.path().join("anim.png");
        write_apng(&apng);

        assert!(open_image_with_icc(&apng).unwrap().has_extra_frames);
    }

    /// アニメーション GIF と複数ページ TIFF は追加フレームありと判定すること
    #[test]
    fn open_image_flags_extra_frames_for_animated_gif_and_multipage_tiff() {
        let dir = tempfile::tempdir().unwrap();
        let gif = dir.path().join("anim.gif");
        write_gif(&gif, 2);
        let tiff = dir.path().join("pages.tiff");
        write_tiff(&tiff, 2);

        assert!(open_image_with_icc(&gif).unwrap().has_extra_frames);
        assert!(open_image_with_icc(&tiff).unwrap().has_extra_frames);
    }

    /// 壊れた GIF や 1 枚目の途中で途切れた GIF は追加フレームなしと判定すること
    #[test]
    fn gif_has_extra_frames_returns_false_for_broken_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("anim.gif");
        write_gif(&path, 2);
        let bytes = fs::read(&path).unwrap();

        assert!(gif_has_extra_frames(bytes.as_slice()));
        assert!(!gif_has_extra_frames(&bytes[..bytes.len() / 4]));
        assert!(!gif_has_extra_frames(&b"GIF89a"[..]));
    }

    /// 1 フレームの GIF・1 ページの TIFF・PNG は追加フレームなしと判定すること
    #[test]
    fn open_image_does_not_flag_single_frame_files() {
        let dir = tempfile::tempdir().unwrap();
        let gif = dir.path().join("still.gif");
        write_gif(&gif, 1);
        let tiff = dir.path().join("page.tiff");
        write_tiff(&tiff, 1);
        let png = dir.path().join("still.png");
        write_test_png(&png, 4, 4);

        for path in [gif, tiff, png] {
            assert!(!open_image_with_icc(&path).unwrap().has_extra_frames);
        }
    }

    // ======================================================================
    // decode_alloc_budget
    // ======================================================================
    /// 寸法上限ちょうどの 16bit・32bit float RGBA 画像でもデコード先バッファを確保できること
    #[test]
    fn decode_alloc_budget_covers_high_bit_depth_at_pixel_limit() {
        for bytes_per_pixel in [8, 16] {
            let total = MAX_IMAGE_PIXELS * bytes_per_pixel;
            let mut limits = Limits::default();
            limits.max_alloc = Some(decode_alloc_budget(total));
            assert!(limits.reserve(total).is_ok(), "{bytes_per_pixel} byte/px");
        }
    }

    /// 小さな画像でも RGBA8 最大寸法分の上限を下回らないこと
    #[test]
    fn decode_alloc_budget_keeps_rgba8_baseline() {
        let baseline = MAX_IMAGE_PIXELS * 4 + DECODE_ALLOC_MARGIN_BYTES;
        assert_eq!(decode_alloc_budget(0), baseline);
        assert_eq!(decode_alloc_budget(1024), baseline);
    }

    // ======================================================================
    // LoadError 表示
    // ======================================================================
    /// メモリ上限による `Limits` エラーが寸法上限ではなくメモリ不足として表示されること
    #[test]
    fn display_distinguishes_memory_limit_from_dimension_limit() {
        use image::error::LimitError;

        let memory = LoadError::Decode(ImageError::Limits(LimitError::from_kind(
            LimitErrorKind::InsufficientMemory,
        )))
        .to_string();
        assert!(memory.contains("メモリ"));
        assert!(!memory.contains("16384"));

        let dimension = LoadError::Decode(ImageError::Limits(LimitError::from_kind(
            LimitErrorKind::DimensionError,
        )))
        .to_string();
        assert!(dimension.contains("16384"));
    }

    /// デコードエラーの表示が日本語になり、英語の元エラー文を含まないこと
    #[test]
    fn display_decode_error_is_japanese() {
        use image::error::{ParameterError, ParameterErrorKind};

        let err = LoadError::Decode(ImageError::Parameter(ParameterError::from_kind(
            ParameterErrorKind::DimensionMismatch,
        )));
        let msg = err.to_string();
        assert!(msg.starts_with("読み込みエラー: "));
        assert!(msg.contains("パラメータ"));
        assert!(!msg.contains("dimension"));
    }

    /// `FileTooLarge` の表示文字列に上限情報が含まれること
    #[test]
    fn display_includes_limit_for_oversized_file() {
        let err = LoadError::FileTooLarge {
            size: MAX_IMAGE_FILE_BYTES + 1,
            limit: MAX_IMAGE_FILE_BYTES,
        };
        let msg = err.to_string();
        assert!(msg.contains("ファイルサイズ"));
        assert!(msg.contains("上限"));
    }

    /// `Io` バリアントが元エラーを `source()` で返すこと
    #[test]
    fn error_source_returns_io_inner() {
        let inner = io::Error::new(io::ErrorKind::NotFound, "missing");
        let err = LoadError::Io(inner);
        let source = std::error::Error::source(&err).expect("source");
        assert!(source.to_string().contains("missing"));
    }

    /// `Decode` バリアントが元エラーを `source()` で返すこと
    #[test]
    fn error_source_returns_decode_inner() {
        use image::error::{ParameterError, ParameterErrorKind};

        let inner = ImageError::Parameter(ParameterError::from_kind(
            ParameterErrorKind::DimensionMismatch,
        ));
        let err = LoadError::Decode(inner);
        assert!(std::error::Error::source(&err).is_some());
    }
}
