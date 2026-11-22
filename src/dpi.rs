//! 解像度 (DPI) の読み取りと保存データへの書き込み
//!
//! PNG の `pHYs`、JPEG の JFIF 密度 (無ければ EXIF)、TIFF の解像度タグを読み取る。
//! 書き込みは JPEG が `image` のエンコーダ経由、PNG / TIFF はエンコード済みのバイト列への挿入で行う。

use std::io::{self, Read, Seek, SeekFrom};

use image::ImageFormat;
use image::codecs::jpeg::{PixelDensity, PixelDensityUnit};
use image::metadata::Orientation;

/// 扱う解像度の上限 (dpi)。JPEG の密度フィールド (16bit) に収まる値
const MAX_DPI: f64 = 65535.0;
/// 1 インチあたりのメートル (PNG の `pHYs` は 1 メートルあたりの画素数)
const METERS_PER_INCH: f64 = 0.0254;
/// 1 インチあたりのセンチメートル
const CM_PER_INCH: f64 = 2.54;
/// 走査する PNG のチャンク数の上限
const MAX_PNG_CHUNKS: usize = 256;
/// 走査する JPEG のセグメント数の上限
const MAX_JPEG_SEGMENTS: usize = 64;
/// 読み取る TIFF の IFD エントリ数の上限
const MAX_IFD_ENTRIES: u16 = 1024;
/// TIFF に書き込む解像度の分母 (小数第 2 位まで保持する)
const TIFF_RESOLUTION_DENOMINATOR: u32 = 100;

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
/// PNG の署名と IHDR チャンク (長さ 4 + 種別 4 + データ 13 + CRC 4) を合わせた長さ
const PNG_IHDR_END: usize = 8 + 25;

const TAG_X_RESOLUTION: u16 = 282;
const TAG_Y_RESOLUTION: u16 = 283;
const TAG_RESOLUTION_UNIT: u16 = 296;
const TYPE_SHORT: u16 = 3;
const TYPE_RATIONAL: u16 = 5;

// ======================================================================
// 解像度
// ======================================================================
/// 画像の解像度 (1 インチあたりの画素数)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dpi {
    /// 水平方向の解像度 (dpi)
    pub x: f64,
    /// 垂直方向の解像度 (dpi)
    pub y: f64,
}

impl Dpi {
    /// 解像度を生成する
    ///
    /// # Arguments
    /// * `x` - 水平方向の解像度 (dpi)
    /// * `y` - 垂直方向の解像度 (dpi)
    ///
    /// # Returns
    /// * `Option<Dpi>` - 両方が 0 より大きく `65535` 以下の有限値なら `Some`
    #[must_use]
    pub fn new(x: f64, y: f64) -> Option<Self> {
        let valid = |v: f64| v.is_finite() && v > 0.0 && v <= MAX_DPI;
        (valid(x) && valid(y)).then_some(Self { x, y })
    }

    /// 90 度単位の回転で縦横が入れ替わる向きの場合に、水平と垂直を入れ替える
    ///
    /// # Arguments
    /// * `orientation` - 画像へ適用した EXIF の向き
    ///
    /// # Returns
    /// * `Dpi` - 表示上の向きに合わせた解像度
    #[must_use]
    pub fn oriented(self, orientation: Orientation) -> Self {
        match orientation {
            Orientation::Rotate90
            | Orientation::Rotate270
            | Orientation::Rotate90FlipH
            | Orientation::Rotate270FlipH => Self {
                x: self.y,
                y: self.x,
            },
            _ => self,
        }
    }

    /// JPEG エンコーダへ渡す密度 (dpi)
    pub(crate) fn to_jpeg_density(self) -> PixelDensity {
        PixelDensity {
            density: (round_to_u16(self.x), round_to_u16(self.y)),
            unit: PixelDensityUnit::Inches,
        }
    }
}

/// 解像度を 16bit 整数へ丸める (範囲は `Dpi::new` が保証する)
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "`Dpi::new` が 0 より大きく 65535 以下に制限している"
)]
fn round_to_u16(v: f64) -> u16 {
    v.round().max(1.0) as u16
}

/// 小数第 1 位に丸める (単位換算で生じる 299.9994 のような端数を除く)
fn round_tenth(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

// ======================================================================
// 読み取り
// ======================================================================
/// 画像ファイルから解像度を読み取る
///
/// 壊れている・解像度の記録が無い・単位が長さでない場合は `None` とする
/// 読み取り位置はファイルの先頭へ戻してから使う
///
/// # Arguments
/// * `reader` - 画像ファイルのリーダー
/// * `format` - 画像形式
///
/// # Returns
/// * `Option<Dpi>` - 読み取った解像度
pub(crate) fn read_dpi(reader: &mut (impl Read + Seek), format: ImageFormat) -> Option<Dpi> {
    reader.seek(SeekFrom::Start(0)).ok()?;
    match format {
        ImageFormat::Png => read_png(reader),
        ImageFormat::Jpeg => read_jpeg(reader),
        ImageFormat::Tiff => read_tiff(reader),
        _ => None,
    }
}

/// PNG の `pHYs` チャンクから読み取る
fn read_png(reader: &mut (impl Read + Seek)) -> Option<Dpi> {
    let mut signature = [0u8; 8];
    reader.read_exact(&mut signature).ok()?;
    if signature != PNG_SIGNATURE {
        return None;
    }
    // pHYs は IDAT より前に置かれる
    for _ in 0..MAX_PNG_CHUNKS {
        let mut header = [0u8; 8];
        reader.read_exact(&mut header).ok()?;
        let len = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
        match &header[4..8] {
            b"pHYs" if len == 9 => {
                let mut data = [0u8; 9];
                reader.read_exact(&mut data).ok()?;
                // 単位 1 のみがメートル (0 は縦横比だけを表す)
                if data[8] != 1 {
                    return None;
                }
                let per_meter = |b: &[u8]| f64::from(u32::from_be_bytes([b[0], b[1], b[2], b[3]]));
                return Dpi::new(
                    round_tenth(per_meter(&data[0..4]) * METERS_PER_INCH),
                    round_tenth(per_meter(&data[4..8]) * METERS_PER_INCH),
                );
            }
            b"IDAT" | b"IEND" => return None,
            _ => {
                // データと CRC を読み飛ばす
                reader.seek(SeekFrom::Current(i64::from(len) + 4)).ok()?;
            }
        }
    }
    None
}

/// JPEG の JFIF 密度から読み取る。無ければ EXIF の解像度タグを使う
fn read_jpeg(reader: &mut (impl Read + Seek)) -> Option<Dpi> {
    let mut soi = [0u8; 2];
    reader.read_exact(&mut soi).ok()?;
    if soi != [0xFF, 0xD8] {
        return None;
    }
    let mut exif = None;
    for _ in 0..MAX_JPEG_SEGMENTS {
        let mut marker = [0u8; 2];
        if reader.read_exact(&mut marker).is_err() || marker[0] != 0xFF {
            break;
        }
        // マーカー前の 0xFF 埋めバイトは読み飛ばす
        while marker[1] == 0xFF {
            if reader.read_exact(&mut marker[1..]).is_err() {
                return exif;
            }
        }
        match marker[1] {
            // SOS (画像データ) と EOI
            0xDA | 0xD9 => break,
            // 長さを持たないマーカー
            0x01 | 0xD0..=0xD8 => {}
            code => {
                let mut len = [0u8; 2];
                reader.read_exact(&mut len).ok()?;
                let len = usize::from(u16::from_be_bytes(len)).checked_sub(2)?;
                if code != 0xE0 && code != 0xE1 {
                    reader
                        .seek(SeekFrom::Current(i64::try_from(len).ok()?))
                        .ok()?;
                    continue;
                }
                let mut data = vec![0u8; len];
                reader.read_exact(&mut data).ok()?;
                if code == 0xE0 {
                    if let Some(dpi) = parse_jfif(&data) {
                        return Some(dpi);
                    }
                } else if exif.is_none() && data.starts_with(b"Exif\0\0") {
                    exif = read_tiff(&mut io::Cursor::new(&data[6..]));
                }
            }
        }
    }
    exif
}

/// JFIF の APP0 セグメントから密度を取り出す
///
/// 単位が 0 (縦横比のみ) の場合は `None`
fn parse_jfif(data: &[u8]) -> Option<Dpi> {
    let body = data.strip_prefix(b"JFIF\0")?;
    let [_, _, unit, xh, xl, yh, yl, ..] = *body else {
        return None;
    };
    let x = f64::from(u16::from_be_bytes([xh, xl]));
    let y = f64::from(u16::from_be_bytes([yh, yl]));
    match unit {
        1 => Dpi::new(x, y),
        2 => Dpi::new(round_tenth(x * CM_PER_INCH), round_tenth(y * CM_PER_INCH)),
        _ => None,
    }
}

/// TIFF (または EXIF) の先頭 IFD にある解像度タグから読み取る
fn read_tiff(reader: &mut (impl Read + Seek)) -> Option<Dpi> {
    let mut header = [0u8; 8];
    reader.read_exact(&mut header).ok()?;
    let endian = match &header[0..2] {
        b"II" => Endian::Little,
        b"MM" => Endian::Big,
        _ => return None,
    };
    if endian.u16(&header[2..4]) != 42 {
        return None;
    }
    reader
        .seek(SeekFrom::Start(u64::from(endian.u32(&header[4..8]))))
        .ok()?;
    let mut count = [0u8; 2];
    reader.read_exact(&mut count).ok()?;
    let count = endian.u16(&count).min(MAX_IFD_ENTRIES);
    let mut entries = vec![0u8; usize::from(count) * 12];
    reader.read_exact(&mut entries).ok()?;

    let (mut x, mut y, mut unit) = (None, None, 2u16);
    for entry in entries.as_chunks::<12>().0 {
        let (tag, kind) = (endian.u16(&entry[0..2]), endian.u16(&entry[2..4]));
        match tag {
            TAG_X_RESOLUTION | TAG_Y_RESOLUTION if kind == TYPE_RATIONAL => {
                // 有理数 (8 バイト) は値フィールドのオフセット先にある
                reader
                    .seek(SeekFrom::Start(u64::from(endian.u32(&entry[8..12]))))
                    .ok()?;
                let mut rational = [0u8; 8];
                reader.read_exact(&mut rational).ok()?;
                let (num, den) = (endian.u32(&rational[0..4]), endian.u32(&rational[4..8]));
                let value = (den != 0).then(|| f64::from(num) / f64::from(den));
                if tag == TAG_X_RESOLUTION {
                    x = value;
                } else {
                    y = value;
                }
            }
            TAG_RESOLUTION_UNIT if kind == TYPE_SHORT => unit = endian.u16(&entry[8..10]),
            _ => {}
        }
    }
    let (x, y) = (x?, y?);
    match unit {
        2 => Dpi::new(x, y),
        3 => Dpi::new(round_tenth(x * CM_PER_INCH), round_tenth(y * CM_PER_INCH)),
        _ => None,
    }
}

/// TIFF のバイトオーダー
#[derive(Debug, Clone, Copy)]
enum Endian {
    Little,
    Big,
}

impl Endian {
    fn u16(self, b: &[u8]) -> u16 {
        let b = [b[0], b[1]];
        match self {
            Self::Little => u16::from_le_bytes(b),
            Self::Big => u16::from_be_bytes(b),
        }
    }

    fn u32(self, b: &[u8]) -> u32 {
        let b = [b[0], b[1], b[2], b[3]];
        match self {
            Self::Little => u32::from_le_bytes(b),
            Self::Big => u32::from_be_bytes(b),
        }
    }

    fn put_u16(self, v: u16) -> [u8; 2] {
        match self {
            Self::Little => v.to_le_bytes(),
            Self::Big => v.to_be_bytes(),
        }
    }

    fn put_u32(self, v: u32) -> [u8; 4] {
        match self {
            Self::Little => v.to_le_bytes(),
            Self::Big => v.to_be_bytes(),
        }
    }
}

// ======================================================================
// 書き込み
// ======================================================================
/// エンコード済みの PNG へ `pHYs` チャンクを挿入する
///
/// PNG として不正なバイト列は変更しない
///
/// # Arguments
/// * `png` - エンコード済みの PNG (`pHYs` を含まないこと)
/// * `dpi` - 書き込む解像度
pub(crate) fn embed_png(png: &mut Vec<u8>, dpi: Dpi) {
    if !png.starts_with(&PNG_SIGNATURE) || png.get(12..16) != Some(b"IHDR".as_slice()) {
        return;
    }
    let mut chunk = Vec::with_capacity(21);
    chunk.extend_from_slice(&9u32.to_be_bytes());
    chunk.extend_from_slice(b"pHYs");
    chunk.extend_from_slice(&per_meter(dpi.x).to_be_bytes());
    chunk.extend_from_slice(&per_meter(dpi.y).to_be_bytes());
    chunk.push(1);
    let crc = crc32(&chunk[4..]);
    chunk.extend_from_slice(&crc.to_be_bytes());
    // IHDR の直後なら PLTE / IDAT より前の規定位置になる
    png.splice(PNG_IHDR_END..PNG_IHDR_END, chunk);
}

/// 1 インチあたりの画素数を 1 メートルあたりの画素数へ換算する
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "上限 65535 dpi は u32 に収まる"
)]
fn per_meter(dpi: f64) -> u32 {
    (dpi / METERS_PER_INCH).round() as u32
}

/// CRC-32 (PNG チャンクの検査値)
fn crc32(data: &[u8]) -> u32 {
    let mut crc = u32::MAX;
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

/// エンコード済みの TIFF へ解像度タグを追加する
///
/// 先頭の IFD を、解像度の 3 タグ (既存の同タグは置き換え) を加えた形でファイル末尾へ複製し、
/// ヘッダの IFD オフセットを付け替える。画像データなど既存のオフセットは動かさない
/// TIFF として不正なバイト列は変更しない
///
/// # Arguments
/// * `tiff` - エンコード済みの TIFF
/// * `dpi`  - 書き込む解像度
pub(crate) fn embed_tiff(tiff: &mut Vec<u8>, dpi: Dpi) {
    let _ = try_embed_tiff(tiff, dpi);
}

fn try_embed_tiff(tiff: &mut Vec<u8>, dpi: Dpi) -> Option<()> {
    let endian = match tiff.get(0..2)? {
        b"II" => Endian::Little,
        b"MM" => Endian::Big,
        _ => return None,
    };
    if endian.u16(tiff.get(2..4)?) != 42 {
        return None;
    }
    let ifd = usize::try_from(endian.u32(tiff.get(4..8)?)).ok()?;
    let count = usize::from(endian.u16(tiff.get(ifd..ifd.checked_add(2)?)?));
    let entries_end = ifd.checked_add(2 + count * 12)?;
    let entries = tiff.get(ifd + 2..entries_end)?;
    let next_ifd = tiff.get(entries_end..entries_end.checked_add(4)?)?.to_vec();

    // 解像度の既存タグを除いた上で、昇順を保って 3 タグを加える
    let mut all: Vec<(u16, [u8; 12])> = entries
        .as_chunks::<12>()
        .0
        .iter()
        .map(|e| (endian.u16(&e[0..2]), *e))
        .filter(|(tag, _)| {
            !matches!(
                *tag,
                TAG_X_RESOLUTION | TAG_Y_RESOLUTION | TAG_RESOLUTION_UNIT
            )
        })
        .collect();

    // 有理数 2 つ (16 バイト) を末尾へ置く。TIFF の値は偶数オフセットに置く
    if tiff.len() % 2 == 1 {
        tiff.push(0);
    }
    let rational_offset = u32::try_from(tiff.len()).ok()?;
    for v in [dpi.x, dpi.y] {
        tiff.extend_from_slice(&endian.put_u32(tiff_rational_numerator(v)));
        tiff.extend_from_slice(&endian.put_u32(TIFF_RESOLUTION_DENOMINATOR));
    }

    let rational_entry = |tag: u16, offset: u32| {
        let mut e = [0u8; 12];
        e[0..2].copy_from_slice(&endian.put_u16(tag));
        e[2..4].copy_from_slice(&endian.put_u16(TYPE_RATIONAL));
        e[4..8].copy_from_slice(&endian.put_u32(1));
        e[8..12].copy_from_slice(&endian.put_u32(offset));
        (tag, e)
    };
    let mut unit = [0u8; 12];
    unit[0..2].copy_from_slice(&endian.put_u16(TAG_RESOLUTION_UNIT));
    unit[2..4].copy_from_slice(&endian.put_u16(TYPE_SHORT));
    unit[4..8].copy_from_slice(&endian.put_u32(1));
    // インチ。SHORT の値は値フィールドの先頭に置く
    unit[8..10].copy_from_slice(&endian.put_u16(2));
    all.push(rational_entry(TAG_X_RESOLUTION, rational_offset));
    all.push(rational_entry(TAG_Y_RESOLUTION, rational_offset + 8));
    all.push((TAG_RESOLUTION_UNIT, unit));
    all.sort_by_key(|(tag, _)| *tag);

    let new_ifd = u32::try_from(tiff.len()).ok()?;
    tiff.extend_from_slice(&endian.put_u16(u16::try_from(all.len()).ok()?));
    for (_, entry) in &all {
        tiff.extend_from_slice(entry);
    }
    tiff.extend_from_slice(&next_ifd);
    tiff[4..8].copy_from_slice(&endian.put_u32(new_ifd));
    Some(())
}

/// TIFF の有理数 (分母 `TIFF_RESOLUTION_DENOMINATOR`) の分子へ換算する
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "上限 65535 dpi に分母 100 を掛けても u32 に収まる"
)]
fn tiff_rational_numerator(dpi: f64) -> u32 {
    (dpi * f64::from(TIFF_RESOLUTION_DENOMINATOR)).round() as u32
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    /// 範囲外・非有限の値から `Dpi` を作れないこと
    #[test]
    fn dpi_new_rejects_invalid_values() {
        assert!(Dpi::new(300.0, 300.0).is_some());
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY, 70000.0] {
            assert!(Dpi::new(bad, 300.0).is_none(), "{bad}");
            assert!(Dpi::new(300.0, bad).is_none(), "{bad}");
        }
    }

    /// 90 度回転する向きでだけ水平と垂直が入れ替わること
    #[test]
    fn oriented_swaps_axes_only_for_quarter_turns() {
        let dpi = Dpi::new(300.0, 600.0).unwrap();
        assert_eq!(dpi.oriented(Orientation::NoTransforms), dpi);
        assert_eq!(dpi.oriented(Orientation::Rotate180), dpi);
        assert_eq!(dpi.oriented(Orientation::FlipHorizontal), dpi);
        let swapped = Dpi::new(600.0, 300.0).unwrap();
        assert_eq!(dpi.oriented(Orientation::Rotate90), swapped);
        assert_eq!(dpi.oriented(Orientation::Rotate270FlipH), swapped);
    }

    /// CRC-32 が既知の値と一致すること
    #[test]
    fn crc32_matches_known_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    /// `pHYs` を挿入した PNG から同じ解像度を読み戻せること
    #[test]
    fn png_density_round_trips() {
        let mut png = Vec::new();
        image::DynamicImage::new_rgb8(2, 2)
            .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
            .unwrap();
        embed_png(&mut png, Dpi::new(300.0, 600.0).unwrap());
        let dpi = read_dpi(&mut Cursor::new(&png), ImageFormat::Png).unwrap();
        assert_eq!(dpi, Dpi::new(300.0, 600.0).unwrap());
        // 挿入後も通常どおりデコードできる
        assert!(image::load_from_memory(&png).is_ok());
    }

    /// 解像度を追加した TIFF を読み戻せ、画像もデコードできること
    #[test]
    fn tiff_density_round_trips() {
        let mut tiff = Vec::new();
        image::DynamicImage::new_rgb8(3, 2)
            .write_to(&mut Cursor::new(&mut tiff), ImageFormat::Tiff)
            .unwrap();
        embed_tiff(&mut tiff, Dpi::new(300.0, 300.0).unwrap());
        let dpi = read_dpi(&mut Cursor::new(&tiff), ImageFormat::Tiff).unwrap();
        assert_eq!(dpi, Dpi::new(300.0, 300.0).unwrap());
        assert_eq!(image::load_from_memory(&tiff).unwrap().width(), 3);
    }

    /// JPEG の JFIF 密度を書き込み、読み戻せること
    #[test]
    fn jpeg_density_round_trips() {
        use image::codecs::jpeg::JpegEncoder;

        let img = image::DynamicImage::new_rgb8(8, 8);
        let mut jpeg = Vec::new();
        let mut encoder = JpegEncoder::new(&mut jpeg);
        encoder.set_pixel_density(Dpi::new(72.0, 96.0).unwrap().to_jpeg_density());
        img.write_with_encoder(encoder).unwrap();
        let dpi = read_dpi(&mut Cursor::new(&jpeg), ImageFormat::Jpeg).unwrap();
        assert_eq!(dpi, Dpi::new(72.0, 96.0).unwrap());
    }

    /// マーカー前に 0xFF の埋めバイトがあっても JFIF 密度を読めること
    #[test]
    fn jpeg_density_survives_fill_bytes() {
        use image::codecs::jpeg::JpegEncoder;

        let img = image::DynamicImage::new_rgb8(8, 8);
        let mut jpeg = Vec::new();
        let mut encoder = JpegEncoder::new(&mut jpeg);
        encoder.set_pixel_density(Dpi::new(72.0, 96.0).unwrap().to_jpeg_density());
        img.write_with_encoder(encoder).unwrap();
        // SOI と APP0 の間に埋めバイトを挿入する
        jpeg.splice(2..2, [0xFF, 0xFF]);
        let dpi = read_dpi(&mut Cursor::new(&jpeg), ImageFormat::Jpeg).unwrap();
        assert_eq!(dpi, Dpi::new(72.0, 96.0).unwrap());
    }

    /// 壊れたバイト列や単位なしの記録から解像度を読まないこと
    #[test]
    fn read_dpi_ignores_garbage() {
        for format in [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::Tiff] {
            assert!(read_dpi(&mut Cursor::new(b"garbage".as_slice()), format).is_none());
            assert!(read_dpi(&mut Cursor::new(Vec::new()), format).is_none());
        }
        // 縦横比のみの JFIF
        assert!(parse_jfif(b"JFIF\0\x01\x01\x00\x00\x01\x00\x01").is_none());
        // dpcm の JFIF は dpi へ換算する
        let dpi = parse_jfif(b"JFIF\0\x01\x01\x02\x00\x75\x00\x75").unwrap();
        assert_eq!(dpi, Dpi::new(297.2, 297.2).unwrap());
    }

    /// 不正なバイト列には書き込まないこと
    #[test]
    fn embed_ignores_invalid_container() {
        let dpi = Dpi::new(300.0, 300.0).unwrap();
        let mut bytes = b"not an image".to_vec();
        embed_png(&mut bytes, dpi);
        embed_tiff(&mut bytes, dpi);
        assert_eq!(bytes, b"not an image");
    }
}
