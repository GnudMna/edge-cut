//! 大容量画像によるメモリ枯渇を防ぐための安全な画像読み込み

use std::error::Error;
use std::fmt;
use std::fs;
use std::io::{self, BufReader, Seek, SeekFrom};
use std::path::Path;

use crate::consts::{MAX_IMAGE_FILE_BYTES, MAX_IMAGE_HEIGHT, MAX_IMAGE_PIXELS, MAX_IMAGE_WIDTH};

use image::{DynamicImage, ImageError, ImageReader, Limits};

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
            Self::Decode(ImageError::Limits(_)) => write!(
                f,
                "画像が大きすぎるため読み込めません (上限: 幅 {MAX_IMAGE_WIDTH} / 高さ {MAX_IMAGE_HEIGHT} / {limit_mp})",
                limit_mp = format_megapixels(MAX_IMAGE_PIXELS),
            ),
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
///
/// # Errors
/// ファイルサイズ・寸法が上限を超える、または読み込み・デコードに失敗した場合
pub fn open_image(path: &Path) -> Result<DynamicImage, LoadError> {
    let mut file = fs::File::open(path)?;
    let file_size = file.metadata()?.len();
    check_file_size(file_size)?;

    let limits = decode_limits();

    let mut probe = ImageReader::new(BufReader::new(&file));
    probe.limits(limits.clone());
    let (width, height) = probe.with_guessed_format()?.into_dimensions()?;
    check_dimensions(width, height)?;

    file.seek(SeekFrom::Start(0))?;
    let mut reader = ImageReader::new(BufReader::new(&file));
    reader.limits(limits);
    Ok(reader.with_guessed_format()?.decode()?)
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
    limits.max_alloc = Some(max_decode_alloc_bytes());
    limits
}

/// 最大ピクセル数の RGBA バッファ 1 枚分 + デコーダ内部用の余裕
fn max_decode_alloc_bytes() -> u64 {
    MAX_IMAGE_PIXELS
        .saturating_mul(4)
        .saturating_add(16 * 1024 * 1024)
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

#[allow(clippy::cast_precision_loss)]
fn format_bytes(n: u64) -> String {
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
    use std::env;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    use image::{DynamicImage, RgbaImage};

    // ======================================================================
    // ヘルパー関数
    // ======================================================================
    /// テスト用の一時ファイルパスを生成する
    fn temp_path(name: &str) -> std::path::PathBuf {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        env::temp_dir().join(format!("edge_cut_load_{ts}_{name}"))
    }

    /// 指定寸法の空 PNG を一時パスへ書き出す
    fn write_test_png(path: &Path, width: u32, height: u32) {
        let img = DynamicImage::ImageRgba8(RgbaImage::new(width, height));
        img.save(path).expect("test png write");
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
        let path = temp_path("missing.png");
        let err = open_image(&path).unwrap_err();
        assert!(matches!(err, LoadError::Io(_) | LoadError::Decode(_)));
    }

    /// 小さな PNG ファイルが正常に読み込めることを確認する
    #[test]
    fn open_image_loads_small_png() {
        let path = temp_path("small.png");
        write_test_png(&path, 8, 8);
        let img = open_image(&path).expect("open small png");
        assert_eq!(img.width(), 8);
        assert_eq!(img.height(), 8);
        let _ = fs::remove_file(&path);
    }

    // ======================================================================
    // LoadError 表示
    // ======================================================================
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
