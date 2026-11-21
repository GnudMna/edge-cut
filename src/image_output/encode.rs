//! 保存形式に合わせた画像のエンコード
//!
//! JPEG の透過合成、エンコーダ非対応のカラータイプの変換、ICC プロファイルと解像度の再埋め込み、
//! WebP のロスレス / ロッシーの切り替えを担う。

use std::borrow::Cow;
use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::codecs::tiff::TiffEncoder;
use image::codecs::webp::WebPEncoder;
use image::{
    ColorType, DynamicImage, GrayImage, ImageEncoder, ImageError, ImageFormat, Luma, Pixel, Rgb,
    RgbImage,
};

use super::save::SaveOptions;
use super::webp_lossy::encode_lossy;
use crate::consts::{JPEG_FLATTEN_BACKGROUND, WEBP_QUALITY_MAX};
use crate::dpi::{Dpi, embed_png, embed_tiff};

// ======================================================================
// エンコード
// ======================================================================
/// 画像を指定形式でメモリ上にエンコードする
///
/// ICC プロファイルを埋め込める形式はエンコーダを直接生成して設定する
/// それ以外の形式は `DynamicImage::write_to` に任せ、プロファイルは破棄する
/// 解像度は JPEG ではエンコーダへ設定し、PNG / TIFF ではエンコード後のバイト列へ書き足す
pub(super) fn encode_image(
    img: &DynamicImage,
    icc_profile: Option<&[u8]>,
    dpi: Option<Dpi>,
    format: ImageFormat,
    options: SaveOptions,
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
            let mut encoder =
                JpegEncoder::new_with_quality(&mut cursor, options.clamped_jpeg_quality());
            if let Some(dpi) = dpi {
                encoder.set_pixel_density(dpi.to_jpeg_density());
            }
            write_with_icc(&img, icc_profile, encoder)?;
        }
        ImageFormat::Png => write_with_icc(&img, icc_profile, PngEncoder::new(&mut cursor))?,
        ImageFormat::Tiff => write_with_icc(&img, icc_profile, TiffEncoder::new(&mut cursor))?,
        // 最大品質のときだけロスレス (`write_to` と同じ設定)、それ以外は libwebp のロッシー
        ImageFormat::WebP => {
            let quality = options.clamped_webp_quality();
            if quality >= WEBP_QUALITY_MAX {
                write_with_icc(&img, icc_profile, WebPEncoder::new_lossless(&mut cursor))?;
            } else {
                // libwebp は RGB / RGBA のみ受け付けるため、グレースケールは RGB 化する
                // (RGB 画像に GRAY のプロファイルは付けられないため、プロファイルは外れる)
                let img = match img.color() {
                    ColorType::L8 => Cow::Owned(DynamicImage::ImageRgb8(img.to_rgb8())),
                    ColorType::La8 => Cow::Owned(DynamicImage::ImageRgba8(img.to_rgba8())),
                    _ => img,
                };
                let icc = icc_profile.filter(|icc| icc_matches_color(icc, &img));
                return encode_lossy(&img, icc, quality);
            }
        }
        _ => img.write_to(&mut cursor, format)?,
    }
    if let Some(dpi) = dpi {
        match format {
            ImageFormat::Png => embed_png(&mut buf, dpi),
            ImageFormat::Tiff => embed_tiff(&mut buf, dpi),
            _ => {}
        }
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
/// PNG / TIFF は 16bit までそのまま保存し (TIFF は 32bit float も保存する)、
/// 未対応のカラータイプは元のチャンネル幅に応じて 16bit か 8bit の RGB / RGBA に変換する
/// BMP / WebP は 8bit グレースケールも維持し、それ以外の形式は 8bit RGB / RGBA に変換する
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
                | ColorType::Rgb32F
                | ColorType::Rgba32F
        ),
        // BMP エンコーダとロスレス WebP エンコーダは 8bit グレースケールも直接書ける
        // (ロッシー WebP は呼び出し側で RGB / RGBA に変換する)
        ImageFormat::Bmp | ImageFormat::WebP => matches!(
            img.color(),
            ColorType::L8 | ColorType::La8 | ColorType::Rgb8 | ColorType::Rgba8
        ),
        _ => matches!(img.color(), ColorType::Rgb8 | ColorType::Rgba8),
    };
    if supported {
        Cow::Borrowed(img)
    } else {
        // 8bit を超えるチャンネル幅は PNG / TIFF のみ 16bit を維持する
        let keep_16bit = matches!(format, ImageFormat::Png | ImageFormat::Tiff)
            && img.color().bytes_per_pixel() / img.color().channel_count() > 1;
        Cow::Owned(match (img.color().has_alpha(), keep_16bit) {
            (true, true) => DynamicImage::ImageRgba16(img.to_rgba16()),
            (true, false) => DynamicImage::ImageRgba8(img.to_rgba8()),
            (false, true) => DynamicImage::ImageRgb16(img.to_rgb16()),
            (false, false) => DynamicImage::ImageRgb8(img.to_rgb8()),
        })
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

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;

    /// 半透明の黒を白背景へ合成すると中間の灰色になること
    #[test]
    fn blend_over_mixes_with_background() {
        assert_eq!(blend_over(0, 255, 0), 255);
        assert_eq!(blend_over(0, 255, 255), 0);
        assert_eq!(blend_over(0, 255, 128), 127);
    }

    /// 16bit グレー+アルファと 32bit float が PNG / TIFF で 8bit に落ちないこと
    #[test]
    fn high_bit_depth_is_kept_for_png_and_tiff() {
        let la16 = DynamicImage::ImageLumaA16(image::ImageBuffer::new(2, 2));
        let rgb32f = DynamicImage::ImageRgb32F(image::ImageBuffer::new(2, 2));

        // TIFF: La16 は Rgba16、32bit float はそのまま
        assert_eq!(
            to_encoder_compatible(&la16, ImageFormat::Tiff).color(),
            ColorType::Rgba16
        );
        assert_eq!(
            to_encoder_compatible(&rgb32f, ImageFormat::Tiff).color(),
            ColorType::Rgb32F
        );
        // PNG: 32bit float は Rgb16
        assert_eq!(
            to_encoder_compatible(&rgb32f, ImageFormat::Png).color(),
            ColorType::Rgb16
        );
        // それ以外の形式は 8bit
        assert_eq!(
            to_encoder_compatible(&rgb32f, ImageFormat::WebP).color(),
            ColorType::Rgb8
        );
    }

    /// 8bit グレースケールが BMP / ロスレス WebP で RGB 化されず、GIF では RGB 系になること
    #[test]
    fn gray8_is_kept_for_bmp_and_webp() {
        let gray = DynamicImage::ImageLuma8(GrayImage::new(2, 2));
        for format in [ImageFormat::Bmp, ImageFormat::WebP] {
            assert_eq!(to_encoder_compatible(&gray, format).color(), ColorType::L8);
        }
        assert_eq!(
            to_encoder_compatible(&gray, ImageFormat::Gif).color(),
            ColorType::Rgb8
        );
    }

    /// グレースケール画像の GRAY プロファイルがロスレス WebP に残り、BMP はグレーで書けること
    #[test]
    fn gray_icc_survives_lossless_webp() {
        use image::ImageDecoder;
        use image::codecs::webp::WebPDecoder;

        let mut icc = vec![0u8; 128];
        icc[16..20].copy_from_slice(b"GRAY");
        let gray = DynamicImage::ImageLuma8(GrayImage::from_pixel(4, 4, Luma([90])));

        let options = SaveOptions::default();
        let webp_options = SaveOptions {
            webp_quality: WEBP_QUALITY_MAX,
            ..options
        };
        let bytes = encode_image(&gray, Some(&icc), None, ImageFormat::WebP, webp_options).unwrap();
        let mut decoder = WebPDecoder::new(Cursor::new(&bytes)).unwrap();
        assert_eq!(decoder.icc_profile().unwrap(), Some(icc.clone()));

        // BMP は 8bit グレーのまま書けること (ヘッダのビット深度で確認する)
        let bmp = encode_image(&gray, None, None, ImageFormat::Bmp, options).unwrap();
        assert_eq!(u16::from_le_bytes([bmp[28], bmp[29]]), 8);
    }
}
