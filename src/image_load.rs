//! 大容量画像によるメモリ枯渇を防ぐための安全な画像読み込み

use std::error::Error;
use std::fmt;
use std::fs;
use std::io::{self, BufReader, Seek, SeekFrom};
use std::path::Path;

use crate::consts::{MAX_IMAGE_FILE_BYTES, MAX_IMAGE_HEIGHT, MAX_IMAGE_PIXELS, MAX_IMAGE_WIDTH};

use image::error::LimitErrorKind;
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageError, ImageReader, Limits};

/// デコーダ内部の作業領域として、画像バッファとは別に確保を許す量
const DECODE_ALLOC_MARGIN_BYTES: u64 = 16 * 1024 * 1024;

// ======================================================================
// 読み込み結果
// ======================================================================
/// 読み込んだ画像と、保存時に引き継ぐメタデータ
#[derive(Debug, Clone)]
pub struct LoadedImage {
    /// EXIF Orientation を適用済みの画像
    pub image: DynamicImage,
    /// 埋め込まれていた ICC プロファイル。無い、または取得に失敗した場合は `None`
    pub icc_profile: Option<Vec<u8>>,
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
            Self::Decode(e) => write!(f, "読み込みエラー: {e}"),
        }
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
/// # Errors
/// ファイルサイズ・寸法が上限を超える、または読み込み・デコードに失敗した場合
pub fn open_image_with_icc(path: &Path) -> Result<LoadedImage, LoadError> {
    let mut file = fs::File::open(path)?;
    let file_size = file.metadata()?.len();
    check_file_size(file_size)?;

    let mut limits = decode_limits();

    let mut probe = ImageReader::new(BufReader::new(&file));
    probe.limits(limits.clone());
    let (width, height) = probe.with_guessed_format()?.into_dimensions()?;
    check_dimensions(width, height)?;

    file.seek(SeekFrom::Start(0))?;
    let mut reader = ImageReader::new(BufReader::new(&file));
    reader.limits(limits.clone());
    let mut decoder = reader.with_guessed_format()?.into_decoder()?;

    // 寸法は検証済みのため、16bit・浮動小数点画像でもデコード先バッファが収まるよう上限を広げる
    // その上で `ImageReader::decode()` と同様にデコード先バッファ分を上限から差し引く
    let total_bytes = decoder.total_bytes();
    limits.max_alloc = Some(decode_alloc_budget(total_bytes));
    limits.reserve(total_bytes)?;
    decoder.set_limits(limits)?;

    // 壊れた EXIF で読み込み自体を失敗させないよう、取得できなければ無変換とする
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    // ICC プロファイルも同様に、壊れていれば埋め込みなしとして扱う
    let icc_profile = decoder
        .icc_profile()
        .ok()
        .flatten()
        .filter(|icc| !icc.is_empty());
    let mut image = DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    Ok(LoadedImage { image, icc_profile })
}

/// クリップボード由来の RGBA 画像寸法を検証する
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
#[allow(clippy::cast_precision_loss)]
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

#[allow(clippy::cast_precision_loss)]
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
    use super::*;

    use image::codecs::png::PngEncoder;
    use image::{DynamicImage, ExtendedColorType, ImageEncoder, Rgba, RgbaImage};

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
    /// 上限内の寸法が受理されることを確認する
    #[test]
    fn check_dimensions_accepts_within_limits() {
        assert!(check_dimensions(4000, 3000).is_ok());
    }

    /// 幅が上限を超えると `DimensionLimitKind::Width` で拒否されることを確認する
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

    /// 高さが上限を超えると `DimensionLimitKind::Height` で拒否されることを確認する
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

    /// ピクセル数が上限を超えると `DimensionLimitKind::Pixels` で拒否されることを確認する
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
    /// RGBA バッファ長が不一致のとき `ClipboardSizeMismatch` を返すことを確認する
    #[test]
    fn validate_clipboard_rgba_rejects_size_mismatch() {
        let err = validate_clipboard_rgba(10, 10, 100).unwrap_err();
        assert!(matches!(err, LoadError::ClipboardSizeMismatch { .. }));
    }

    /// 期待バイト数と一致する RGBA バッファが受理されることを確認する
    #[test]
    fn validate_clipboard_rgba_accepts_valid_buffer() {
        assert!(validate_clipboard_rgba(2, 2, 16).is_ok());
    }

    // ======================================================================
    // check_file_size
    // ======================================================================
    /// ファイルサイズが上限超過のとき `FileTooLarge` を返すことを確認する
    #[test]
    fn check_file_size_rejects_oversized() {
        let err = check_file_size(MAX_IMAGE_FILE_BYTES + 1).unwrap_err();
        assert!(matches!(err, LoadError::FileTooLarge { .. }));
    }

    /// 上限ちょうどのファイルサイズが受理されることを確認する
    #[test]
    fn check_file_size_accepts_within_limit() {
        assert!(check_file_size(MAX_IMAGE_FILE_BYTES).is_ok());
    }

    // ======================================================================
    // open_image
    // ======================================================================
    /// 存在しないファイルを開くと I/O またはデコードエラーになることを確認する
    #[test]
    fn open_image_rejects_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing.png");
        let err = open_image(&path).unwrap_err();
        assert!(matches!(err, LoadError::Io(_) | LoadError::Decode(_)));
    }

    /// 小さな PNG ファイルが正常に読み込めることを確認する
    #[test]
    fn open_image_loads_small_png() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("small.png");
        write_test_png(&path, 8, 8);
        let img = open_image(&path).expect("open small png");
        assert_eq!(img.width(), 8);
        assert_eq!(img.height(), 8);
    }

    /// EXIF Orientation が 6 (時計回り 90°) のとき回転して読み込まれることを確認する
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

    /// EXIF Orientation が 2 (左右反転) のとき反転して読み込まれることを確認する
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

    /// EXIF Orientation が 1 (無変換) のとき元の向きのまま読み込まれることを確認する
    #[test]
    fn open_image_keeps_orientation_when_no_transform() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("orient1.png");
        write_png_with_orientation(&path, 1);
        let img = open_image(&path).expect("open png").to_rgba8();

        assert_eq!(img.dimensions(), (3, 2));
        assert_eq!(*img.get_pixel(0, 0), RED);
    }

    /// 16bit 画像を開けることを確認する
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
    // decode_alloc_budget
    // ======================================================================
    /// 寸法上限ちょうどの 16bit・32bit float RGBA 画像でもデコード先バッファを確保できることを確認する
    #[test]
    fn decode_alloc_budget_covers_high_bit_depth_at_pixel_limit() {
        for bytes_per_pixel in [8, 16] {
            let total = MAX_IMAGE_PIXELS * bytes_per_pixel;
            let mut limits = Limits::default();
            limits.max_alloc = Some(decode_alloc_budget(total));
            assert!(limits.reserve(total).is_ok(), "{bytes_per_pixel} byte/px");
        }
    }

    /// 小さな画像でも RGBA8 最大寸法分の上限を下回らないことを確認する
    #[test]
    fn decode_alloc_budget_keeps_rgba8_baseline() {
        let baseline = MAX_IMAGE_PIXELS * 4 + DECODE_ALLOC_MARGIN_BYTES;
        assert_eq!(decode_alloc_budget(0), baseline);
        assert_eq!(decode_alloc_budget(1024), baseline);
    }

    // ======================================================================
    // LoadError 表示
    // ======================================================================
    /// メモリ上限による `Limits` エラーが寸法上限ではなくメモリ不足として表示されることを確認する
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

    /// `FileTooLarge` の表示文字列に上限情報が含まれることを確認する
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

    /// `Io` バリアントが元エラーを `source()` で返すことを確認する
    #[test]
    fn error_source_returns_io_inner() {
        let inner = io::Error::new(io::ErrorKind::NotFound, "missing");
        let err = LoadError::Io(inner);
        let source = std::error::Error::source(&err).expect("source");
        assert!(source.to_string().contains("missing"));
    }

    /// `Decode` バリアントが元エラーを `source()` で返すことを確認する
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
