//! 壊れた画像ファイルに対する `open_image_with_icc` の異常系テスト
//!
//! 破損・切り詰め・寸法偽装・巨大 ICC などの入力で、panic せず `Err` または妥当な画像を返し、
//! 上限を超えるメモリ確保や長時間のデコードに入らないことを確認する

use std::fs;
use std::io::Cursor;
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use edge_cut::consts::{MAX_ICC_PROFILE_BYTES, MAX_IMAGE_HEIGHT, MAX_IMAGE_WIDTH};
use edge_cut::{LoadError, LoadedImage, open_image_with_icc};
use image::codecs::png::PngEncoder;
use image::{DynamicImage, ExtendedColorType, ImageEncoder, ImageFormat, Rgb, RgbImage};

// ======================================================================
// 定数
// ======================================================================
/// 破損入力 1 件のデコードに許す時間
///
/// 上限を無視した巨大確保やループがあると大きく超えるため、余裕を持たせた値にする
const DECODE_TIME_BUDGET: Duration = Duration::from_secs(10);
/// 乱数破損テストで形式ごとに試す回数
const MUTATION_ROUNDS: usize = 100;
/// 切り詰めテストで 1 バイトずつ試すファイル先頭の長さ
const HEADER_SCAN_BYTES: usize = 64;
/// 切り詰めテストでヘッダ以降を分割する数
const TRUNCATION_BODY_STEPS: usize = 40;
/// 乱数破損テストの固定シード (失敗を再現できるようにする)
const MUTATION_SEED: u64 = 0x5EED_CAFE_F00D_0001;

// ======================================================================
// テスト用ヘルパー
// ======================================================================
/// 破損入力のテーブルに載せる形式
const FORMATS: [(&str, ImageFormat); 6] = [
    ("png", ImageFormat::Png),
    ("jpeg", ImageFormat::Jpeg),
    ("gif", ImageFormat::Gif),
    ("bmp", ImageFormat::Bmp),
    ("tiff", ImageFormat::Tiff),
    ("webp", ImageFormat::WebP),
];

/// 一時ファイルへ書き出して `open_image_with_icc` で読み込む
///
/// 読み込み時間が `DECODE_TIME_BUDGET` を超えたら失敗とする
///
/// # Arguments
/// * `label` - 失敗時に表示する入力の名前
/// * `bytes` - ファイルの内容
///
/// # Returns
/// * `Result<LoadedImage, LoadError>` - 読み込み結果
fn load_bytes(label: &str, bytes: &[u8]) -> Result<LoadedImage, LoadError> {
    let dir = tempfile::tempdir().expect("一時ディレクトリの作成に失敗");
    // 拡張子は形式判定に使われない (先頭バイトで判定する) ため固定でよい
    let path: PathBuf = dir.path().join("input.bin");
    fs::write(&path, bytes).expect("入力ファイルの書き込みに失敗");

    let started = Instant::now();
    // デコーダ内部の panic は入力名つきで報告し直す
    let result = panic::catch_unwind(AssertUnwindSafe(|| open_image_with_icc(&path)))
        .unwrap_or_else(|_| panic!("{label}: デコード中に panic した"));
    let elapsed = started.elapsed();
    assert!(
        elapsed < DECODE_TIME_BUDGET,
        "{label}: デコードに {elapsed:?} かかった"
    );
    result
}

/// 読み込みに成功した画像が上限内の寸法であることを検証する
fn assert_within_limits(label: &str, loaded: &LoadedImage) {
    assert!(
        loaded.image.width() <= MAX_IMAGE_WIDTH && loaded.image.height() <= MAX_IMAGE_HEIGHT,
        "{label}: 上限を超える画像を返した ({}x{})",
        loaded.image.width(),
        loaded.image.height(),
    );
}

/// 指定形式で正常な 24×16 の画像をエンコードする
fn encode_sample(format: ImageFormat) -> Vec<u8> {
    let img = RgbImage::from_fn(24, 16, |x, y| {
        let x = u8::try_from(x).expect("x は u8 に収まる");
        let y = u8::try_from(y).expect("y は u8 に収まる");
        Rgb([x * 10, y * 15, x ^ y])
    });
    let mut out = Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(img)
        .write_to(&mut out, format)
        .expect("サンプル画像のエンコードに失敗");
    out.into_inner()
}

/// 乱数列 (xorshift64)。依存を増やさず、シード固定で同じ破損を再現するために使う
struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// `0..bound` の乱数を返す
    fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next() % bound as u64).expect("bound 未満は usize に収まる")
    }
}

/// PNG のチャンク CRC-32 を計算する
fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// PNG の IHDR を指定した幅・高さへ書き換える (CRC は計算し直す)
fn png_with_declared_size(width: u32, height: u32) -> Vec<u8> {
    let mut png = encode_sample(ImageFormat::Png);
    // シグネチャ 8 + 長さ 4 + "IHDR" 4 の後ろに幅・高さが続く
    png[16..20].copy_from_slice(&width.to_be_bytes());
    png[20..24].copy_from_slice(&height.to_be_bytes());
    let crc = crc32(&png[12..29]);
    png[29..33].copy_from_slice(&crc.to_be_bytes());
    png
}

/// 指定した ICC プロファイルを埋め込んだ PNG を作る
fn png_with_icc(icc: Vec<u8>) -> Vec<u8> {
    let img = RgbImage::from_pixel(8, 8, Rgb([10, 20, 30]));
    let mut out = Vec::new();
    let mut encoder = PngEncoder::new(&mut out);
    encoder.set_icc_profile(icc).expect("ICC の設定に失敗");
    encoder
        .write_image(img.as_raw(), 8, 8, ExtendedColorType::Rgb8)
        .expect("PNG のエンコードに失敗");
    out
}

// ======================================================================
// テスト: 内容が画像ではないファイル
// ======================================================================
/// 画像ではない内容のファイルは読み込みエラーになること
///
/// 空ファイル・極端に短いファイル・テキスト・ゼロ埋め・シグネチャだけのファイルを試す
#[test]
fn non_image_files_are_rejected() {
    let png_signature: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let cases: [(&str, Vec<u8>); 8] = [
        ("空ファイル", Vec::new()),
        ("1 バイト", vec![0x89]),
        ("テキスト", "これは画像ではない\n".as_bytes().to_vec()),
        ("ゼロ埋め 4 KiB", vec![0; 4096]),
        ("0xFF 埋め 4 KiB", vec![0xFF; 4096]),
        ("PNG シグネチャのみ", png_signature.to_vec()),
        ("JPEG SOI のみ", vec![0xFF, 0xD8]),
        ("GIF89a ヘッダのみ", b"GIF89a".to_vec()),
    ];

    for (label, bytes) in cases {
        let result = load_bytes(label, &bytes);
        assert!(
            matches!(result, Err(LoadError::Decode(_) | LoadError::Io(_))),
            "{label}: Err を期待したが {:?}",
            result.map(|loaded| (loaded.image.width(), loaded.image.height())),
        );
    }
}

/// 存在しないパスとディレクトリは `Io` エラーになること
#[test]
fn missing_path_and_directory_are_io_errors() {
    let dir = tempfile::tempdir().expect("一時ディレクトリの作成に失敗");

    let missing = open_image_with_icc(&dir.path().join("missing.png"));
    assert!(matches!(missing, Err(LoadError::Io(_))));

    // ディレクトリはファイルとして開けない、または読み取りに失敗する
    let directory = open_image_with_icc(dir.path());
    assert!(matches!(
        directory,
        Err(LoadError::Io(_) | LoadError::Decode(_))
    ));
}

// ======================================================================
// テスト: 切り詰め
// ======================================================================
/// どの位置で切り詰めても panic せず、成功した場合は元の寸法を保つこと
///
/// ヘッダ付近は 1 バイトずつ、それ以降は `TRUNCATION_BODY_STEPS` 分割した位置で切り詰める
#[test]
fn truncated_files_never_panic() {
    for (name, format) in FORMATS {
        let full = encode_sample(format);
        let step = (full.len() / TRUNCATION_BODY_STEPS).max(1);
        let lengths = (0..full.len()).filter(|len| *len < HEADER_SCAN_BYTES || len % step == 0);
        for len in lengths {
            let label = format!("{name} を {len}/{} バイトで切り詰め", full.len());
            if let Ok(loaded) = load_bytes(&label, &full[..len]) {
                // 一部のデコーダは欠けた画素を埋めて成功するが、寸法は偽らない
                assert_eq!((loaded.image.width(), loaded.image.height()), (24, 16));
            }
        }
    }
}

/// ヘッダが欠けるほど短い切り詰めは必ずエラーになること
#[test]
fn files_truncated_inside_header_are_rejected() {
    for (name, format) in FORMATS {
        let full = encode_sample(format);
        for len in 0..4 {
            let label = format!("{name} を {len} バイトで切り詰め");
            assert!(load_bytes(&label, &full[..len]).is_err(), "{label}");
        }
    }
}

// ======================================================================
// テスト: ランダム破損
// ======================================================================
/// 数バイトを書き換えた入力でも panic や上限超過をしないこと
///
/// 固定シードの乱数で各形式を `MUTATION_ROUNDS` 回破損させる (cargo-fuzz の代わりの決定的な簡易版)
#[test]
fn byte_corruption_never_panics() {
    let mut rng = XorShift(MUTATION_SEED);
    for (name, format) in FORMATS {
        let original = encode_sample(format);
        for round in 0..MUTATION_ROUNDS {
            let mut bytes = original.clone();
            for _ in 0..=rng.below(8) {
                let at = rng.below(bytes.len());
                bytes[at] = u8::try_from(rng.next() & 0xFF).expect("下位 8 bit は u8 に収まる");
            }
            let label = format!("{name} の破損 #{round}");
            if let Ok(loaded) = load_bytes(&label, &bytes) {
                assert_within_limits(&label, &loaded);
            }
        }
    }
}

/// 末尾を切り詰めたうえで中央付近を破損させた入力でも panic しないこと
#[test]
fn truncation_combined_with_corruption_never_panics() {
    let mut rng = XorShift(MUTATION_SEED ^ 0xFFFF);
    for (name, format) in FORMATS {
        let original = encode_sample(format);
        for round in 0..MUTATION_ROUNDS {
            let keep = rng.below(original.len()) + 1;
            let mut bytes = original[..keep].to_vec();
            let at = rng.below(bytes.len());
            bytes[at] ^= 0xFF;
            let label = format!("{name} の切り詰め+破損 #{round}");
            if let Ok(loaded) = load_bytes(&label, &bytes) {
                assert_within_limits(&label, &loaded);
            }
        }
    }
}

// ======================================================================
// テスト: 寸法の偽装
// ======================================================================
/// ヘッダが巨大な寸法を宣言する PNG は、画素を確保する前に拒否されること
#[test]
fn png_declaring_huge_dimensions_is_rejected() {
    let cases = [
        ("幅だけ上限超過", MAX_IMAGE_WIDTH + 1, 8),
        ("高さだけ上限超過", 8, MAX_IMAGE_HEIGHT + 1),
        ("ピクセル数が上限超過", MAX_IMAGE_WIDTH, MAX_IMAGE_HEIGHT),
        ("u32 の最大値", u32::MAX, u32::MAX),
        ("i32 の最大値", i32::MAX as u32, i32::MAX as u32),
    ];

    for (label, width, height) in cases {
        let result = load_bytes(label, &png_with_declared_size(width, height));
        assert!(
            matches!(
                result,
                Err(LoadError::DimensionsTooLarge { .. } | LoadError::Decode(_))
            ),
            "{label}: 寸法超過のエラーを期待したが成功した",
        );
    }
}

/// 幅または高さが 0 の PNG は成功扱いにならないこと
#[test]
fn png_declaring_zero_dimension_is_rejected() {
    for (label, width, height) in [("幅 0", 0, 8), ("高さ 0", 8, 0), ("幅高さ 0", 0, 0)] {
        let result = load_bytes(label, &png_with_declared_size(width, height));
        assert!(result.is_err(), "{label}: Err を期待したが成功した");
    }
}

/// ヘッダが巨大な寸法を宣言する BMP は拒否されること
#[test]
fn bmp_declaring_huge_dimensions_is_rejected() {
    let mut bmp = encode_sample(ImageFormat::Bmp);
    // BITMAPINFOHEADER の幅 (オフセット 18) と高さ (オフセット 22)
    bmp[18..22].copy_from_slice(&i32::MAX.to_le_bytes());
    bmp[22..26].copy_from_slice(&i32::MAX.to_le_bytes());

    let result = load_bytes("巨大寸法の BMP", &bmp);
    assert!(result.is_err());
}

/// ヘッダが巨大な寸法を宣言する GIF は上限内の寸法で成功しないこと
#[test]
fn gif_declaring_huge_screen_never_exceeds_limits() {
    let mut gif = encode_sample(ImageFormat::Gif);
    // Logical Screen Descriptor の幅 (オフセット 6) と高さ (オフセット 8)
    gif[6..8].copy_from_slice(&u16::MAX.to_le_bytes());
    gif[8..10].copy_from_slice(&u16::MAX.to_le_bytes());

    if let Ok(loaded) = load_bytes("巨大寸法の GIF", &gif) {
        assert_within_limits("巨大寸法の GIF", &loaded);
    }
}

/// SOF の寸法を書き換えた JPEG は上限を超える画像を返さないこと
#[test]
fn jpeg_declaring_huge_dimensions_never_exceeds_limits() {
    let mut jpeg = encode_sample(ImageFormat::Jpeg);
    let sof = jpeg
        .windows(2)
        .position(|marker| marker == [0xFF, 0xC0])
        .expect("SOF0 マーカーが見つからない");
    // マーカー 2 + 長さ 2 + 精度 1 の後ろに高さ・幅 (各 2 バイト、ビッグエンディアン) が続く
    jpeg[sof + 5..sof + 7].copy_from_slice(&u16::MAX.to_be_bytes());
    jpeg[sof + 7..sof + 9].copy_from_slice(&u16::MAX.to_be_bytes());

    if let Ok(loaded) = load_bytes("巨大寸法の JPEG", &jpeg) {
        assert_within_limits("巨大寸法の JPEG", &loaded);
    }
}

// ======================================================================
// テスト: ICC プロファイル
// ======================================================================
/// 壊れた ICC プロファイルが埋め込まれていても画像は読み込めること
///
/// ICC は付随情報のため、プロファイルが読めなくても読み込み全体は失敗させない
#[test]
fn garbage_icc_profile_does_not_fail_load() {
    let cases: [(&str, Vec<u8>); 3] = [
        ("3 バイト", vec![1, 2, 3]),
        ("ヘッダ長だけのゼロ埋め", vec![0; 128]),
        ("0xFF 埋め 4 KiB", vec![0xFF; 4096]),
    ];

    for (label, icc) in cases {
        let loaded = load_bytes(label, &png_with_icc(icc))
            .unwrap_or_else(|err| panic!("{label}: 画像の読み込みに失敗した: {err}"));
        assert_eq!((loaded.image.width(), loaded.image.height()), (8, 8));
    }
}

/// 展開後に上限を超える ICC プロファイルは、読み捨てて画像だけ読み込むこと
///
/// ゼロ埋めの 64 MiB は zlib で数十 KiB に圧縮され、小さなファイルが巨大な展開を要求する
/// プロファイルは保持せず、展開のために上限を超えて確保することもない
#[test]
fn decompression_bomb_icc_profile_is_dropped() {
    let png = png_with_icc(vec![0; 64 * 1024 * 1024]);
    assert!(
        png.len() < 1024 * 1024,
        "圧縮後は小さいファイルのはず: {} バイト",
        png.len(),
    );

    let loaded = load_bytes("展開爆弾 ICC", &png).expect("画像は読み込めるはず");
    assert_eq!((loaded.image.width(), loaded.image.height()), (8, 8));
    assert_eq!(loaded.icc_profile, None);
}

/// 上限ちょうどの ICC プロファイルは保持し、1 バイト超えると読み捨てること
#[test]
fn icc_profile_size_limit_is_enforced() {
    let limit = usize::try_from(MAX_ICC_PROFILE_BYTES).expect("上限は usize に収まる");

    let at_limit = load_bytes("上限ちょうど", &png_with_icc(vec![1; limit])).expect("load");
    assert_eq!(at_limit.icc_profile.map(|icc| icc.len()), Some(limit));

    let over_limit = load_bytes("上限超過", &png_with_icc(vec![1; limit + 1])).expect("load");
    assert_eq!(over_limit.icc_profile, None);
}

/// 上限内の大きな ICC プロファイルはそのまま保持すること
#[test]
fn large_icc_profile_within_limit_is_kept() {
    let icc = vec![7u8; 4 * 1024 * 1024];
    let loaded = load_bytes("4 MiB の ICC", &png_with_icc(icc.clone())).expect("load");
    assert_eq!(loaded.icc_profile, Some(icc));
}

/// ICC 以外の巨大な補助チャンクがあっても、画像は読み込めること
#[test]
fn oversized_ancillary_chunk_does_not_fail_load() {
    let mut png = encode_sample(ImageFormat::Png);
    let mut chunk = Vec::new();
    let data = vec![0u8; 20 * 1024 * 1024];
    chunk.extend_from_slice(&u32::try_from(data.len()).expect("len").to_be_bytes());
    chunk.extend_from_slice(b"prVt");
    chunk.extend_from_slice(&data);
    chunk.extend_from_slice(&crc32(&chunk[4..]).to_be_bytes());
    // IHDR (33 バイト) の直後に差し込む
    png.splice(33..33, chunk);

    let loaded = load_bytes("巨大な補助チャンク", &png).expect("画像は読み込めるはず");
    assert_eq!((loaded.image.width(), loaded.image.height()), (24, 16));
}

/// JPEG の ICC 用 APP2 セグメントが壊れていても panic しないこと
///
/// 分割数・通し番号の不整合、長さの偽装、重複を試す
#[test]
fn malformed_jpeg_icc_segments_never_panic() {
    let original = encode_sample(ImageFormat::Jpeg);
    // SOI の直後に差し込む
    let (soi, rest) = original.split_at(2);

    let app2 = |payload: &[u8]| -> Vec<u8> {
        let mut segment = vec![0xFF, 0xE2];
        let len = u16::try_from(payload.len() + 2).expect("セグメント長が u16 に収まる");
        segment.extend_from_slice(&len.to_be_bytes());
        segment.extend_from_slice(payload);
        segment
    };
    let icc_payload = |seq: u8, total: u8, body: &[u8]| -> Vec<u8> {
        let mut payload = b"ICC_PROFILE\0".to_vec();
        payload.extend_from_slice(&[seq, total]);
        payload.extend_from_slice(body);
        payload
    };

    let cases: [(&str, Vec<u8>); 5] = [
        ("通し番号 0", app2(&icc_payload(0, 1, &[0; 200]))),
        ("通し番号が総数を超過", app2(&icc_payload(3, 2, &[0; 200]))),
        ("総数 0", app2(&icc_payload(1, 0, &[0; 200]))),
        (
            "同じ番号が重複",
            [
                app2(&icc_payload(1, 2, &[0; 200])),
                app2(&icc_payload(1, 2, &[0; 200])),
            ]
            .concat(),
        ),
        ("識別子だけで本体なし", app2(b"ICC_PROFILE\0")),
    ];

    for (label, segment) in cases {
        let bytes = [soi, segment.as_slice(), rest].concat();
        if let Ok(loaded) = load_bytes(label, &bytes) {
            assert_eq!((loaded.image.width(), loaded.image.height()), (24, 16));
        }
    }
}
