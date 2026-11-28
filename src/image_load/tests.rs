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
        open_image_with_icc_reserving(&rgba8, |w, h, bytes| (w, h, bytes)).expect("open rgba8");
    assert_eq!(bytes, (10, 6, 10 * 6 * 4));
    assert_eq!((loaded.image.width(), loaded.image.height()), (10, 6));
    let (_, bytes) =
        open_image_with_icc_reserving(&rgba16, |w, h, bytes| (w, h, bytes)).expect("open rgba16");
    assert_eq!(bytes, (10, 6, 10 * 6 * 8));
}

/// 存在しないファイルは `reserve` を呼ばずに I/O エラーになること
#[test]
fn reserving_rejects_missing_file_without_calling_reserve() {
    let dir = tempfile::tempdir().unwrap();
    let mut called = false;
    let err =
        open_image_with_icc_reserving(&dir.path().join("missing.png"), |_, _, _| called = true)
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
    let mut encoder = tiff::encoder::TiffEncoder::new(fs::File::create(path).expect("create tiff"))
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
