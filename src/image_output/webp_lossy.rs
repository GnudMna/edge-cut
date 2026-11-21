//! ロッシー WebP のエンコード
//!
//! `image` クレートの WebP エンコーダはロスレス専用のため、ロッシーは libwebp で行う。
//! libwebp は ICC プロファイルを書き出せないため、RIFF コンテナへ `ICCP` チャンクを後から挿入する。

use image::error::{EncodingError, ImageFormatHint};
use image::{DynamicImage, ImageError, ImageFormat};
use webp::Encoder;

/// VP8X チャンクのフラグのうち、ICC プロファイルの有無を示すビット
const VP8X_FLAG_ICC: u8 = 0x20;

/// RIFF ヘッダ (`RIFF` + サイズ + `WEBP`) の長さ
const RIFF_HEADER_LEN: usize = 12;

/// VP8X チャンク全体 (チャンクヘッダ 8 + ペイロード 10) の長さ
const VP8X_CHUNK_LEN: usize = 18;

// ======================================================================
// エンコード
// ======================================================================
/// 画像をロッシー WebP としてメモリ上にエンコードする
///
/// # Arguments
/// * `img`         - エンコードする画像 (8bit RGB / RGBA であること)
/// * `icc_profile` - 埋め込む ICC プロファイル (色空間の一致確認は呼び出し側で済ませる)
/// * `quality`     - エンコード品質 (1-100)
///
/// # Returns
/// * `Result<Vec<u8>, ImageError>` - WebP ファイルのバイト列
pub(super) fn encode_lossy(
    img: &DynamicImage,
    icc_profile: Option<&[u8]>,
    quality: u8,
) -> Result<Vec<u8>, ImageError> {
    let (width, height) = (img.width(), img.height());
    let source = match img {
        DynamicImage::ImageRgb8(rgb) => Encoder::from_rgb(rgb.as_raw(), width, height),
        DynamicImage::ImageRgba8(rgba) => Encoder::from_rgba(rgba.as_raw(), width, height),
        _ => return Err(encoding_error("ロッシー WebP は 8bit RGB / RGBA のみ対応")),
    };
    let encoded = source
        .encode_simple(false, f32::from(quality))
        .map_err(|e| encoding_error(format!("WebP のエンコードに失敗: {e:?}")))?;
    let bytes = encoded.to_vec();

    Ok(icc_profile
        .and_then(|icc| embed_icc(&bytes, icc, width, height))
        .unwrap_or(bytes))
}

fn encoding_error(message: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> ImageError {
    ImageError::Encoding(EncodingError::new(
        ImageFormatHint::Exact(ImageFormat::WebP),
        message,
    ))
}

// ======================================================================
// ICC プロファイルの埋め込み
// ======================================================================
/// WebP のバイト列へ `ICCP` チャンクを挿入する
///
/// 拡張形式 (`VP8X`) ならフラグを立てて直後に挿入し、単純形式なら `VP8X` を補って挿入する
///
/// # Arguments
/// * `webp`   - libwebp が出力した WebP ファイルのバイト列
/// * `icc`    - 埋め込む ICC プロファイル
/// * `width`  - 画像の幅 (単純形式から `VP8X` を作るときのキャンバスサイズ)
/// * `height` - 画像の高さ
///
/// # Returns
/// * `Option<Vec<u8>>` - 挿入後のバイト列。入力が WebP として不正な場合は `None`
fn embed_icc(webp: &[u8], icc: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    if webp.len() < RIFF_HEADER_LEN + 8 || &webp[0..4] != b"RIFF" || &webp[8..12] != b"WEBP" {
        return None;
    }
    let mut out = Vec::with_capacity(webp.len() + icc.len() + 2 * VP8X_CHUNK_LEN);
    out.extend_from_slice(&webp[..RIFF_HEADER_LEN]);

    let rest = if &webp[RIFF_HEADER_LEN..RIFF_HEADER_LEN + 4] == b"VP8X" {
        let vp8x_end = RIFF_HEADER_LEN + VP8X_CHUNK_LEN;
        let mut vp8x = webp.get(RIFF_HEADER_LEN..vp8x_end)?.to_vec();
        vp8x[8] |= VP8X_FLAG_ICC;
        out.extend_from_slice(&vp8x);
        push_chunk(&mut out, *b"ICCP", icc)?;
        &webp[vp8x_end..]
    } else {
        // 単純形式にはアルファが無いため、フラグは ICC のみ。キャンバスサイズは各 24bit で「値 - 1」
        let mut payload = [0u8; 10];
        payload[0] = VP8X_FLAG_ICC;
        payload[4..7].copy_from_slice(&width.checked_sub(1)?.to_le_bytes()[..3]);
        payload[7..10].copy_from_slice(&height.checked_sub(1)?.to_le_bytes()[..3]);
        push_chunk(&mut out, *b"VP8X", &payload)?;
        push_chunk(&mut out, *b"ICCP", icc)?;
        &webp[RIFF_HEADER_LEN..]
    };
    out.extend_from_slice(rest);

    let riff_size = u32::try_from(out.len() - 8).ok()?;
    out[4..8].copy_from_slice(&riff_size.to_le_bytes());
    Some(out)
}

/// RIFF チャンク (タグ + サイズ + データ + 奇数長のときのパディング) を追記する
///
/// # Returns
/// * `Option<()>` - データ長が 32bit に収まらない場合は `None`
fn push_chunk(out: &mut Vec<u8>, tag: [u8; 4], data: &[u8]) -> Option<()> {
    out.extend_from_slice(&tag);
    out.extend_from_slice(&u32::try_from(data.len()).ok()?.to_le_bytes());
    out.extend_from_slice(data);
    if data.len() % 2 == 1 {
        out.push(0);
    }
    Some(())
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use image::codecs::webp::WebPDecoder;
    use image::{ImageDecoder, Rgb, RgbImage, Rgba, RgbaImage};

    use super::*;

    /// ICC プロファイルを埋め込んだ WebP から同じプロファイルを取り出せること
    ///
    /// アルファなし (単純形式) とアルファあり (拡張形式) の両方を検証する。
    /// プロファイルは奇数長にして、パディングの扱いも確かめる
    #[test]
    fn encode_lossy_embeds_icc_profile() {
        let icc: Vec<u8> = (0..101u8).collect();
        let images = [
            DynamicImage::ImageRgb8(RgbImage::from_pixel(8, 5, Rgb([10, 200, 30]))),
            DynamicImage::ImageRgba8(RgbaImage::from_fn(8, 5, |x, _| {
                Rgba([10, 200, 30, if x < 4 { 0 } else { 255 }])
            })),
        ];
        for img in &images {
            let bytes = encode_lossy(img, Some(&icc), 80).expect("エンコード");

            let mut decoder = WebPDecoder::new(Cursor::new(&bytes)).expect("デコーダ");
            assert_eq!(decoder.icc_profile().expect("取得"), Some(icc.clone()));
            assert_eq!(decoder.dimensions(), (8, 5));
            // 埋め込み後も画像として読み込める
            let reloaded = image::load_from_memory(&bytes).expect("デコード");
            assert_eq!((reloaded.width(), reloaded.height()), (8, 5));
        }
    }

    /// ICC プロファイルが無い場合はコンテナを書き換えないこと
    #[test]
    fn encode_lossy_without_icc_has_no_icc_chunk() {
        let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(8, 5, Rgb([10, 200, 30])));
        let bytes = encode_lossy(&img, None, 80).expect("エンコード");

        let mut decoder = WebPDecoder::new(Cursor::new(&bytes)).expect("デコーダ");
        assert_eq!(decoder.icc_profile().expect("取得"), None);
    }

    /// WebP として不正なバイト列には ICC プロファイルを挿入しないこと
    #[test]
    fn embed_icc_rejects_invalid_container() {
        assert!(embed_icc(b"not a webp file at all", &[0; 4], 1, 1).is_none());
        assert!(embed_icc(b"RIFF", &[0; 4], 1, 1).is_none());
    }

    /// 8bit RGB / RGBA 以外の画像はエラーになること
    #[test]
    fn encode_lossy_rejects_unsupported_color_type() {
        let img = DynamicImage::ImageLuma8(image::GrayImage::new(2, 2));
        assert!(encode_lossy(&img, None, 80).is_err());
    }
}
