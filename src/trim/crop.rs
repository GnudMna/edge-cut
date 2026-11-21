//! トリミング情報に従った画像の切り出し

use image::{DynamicImage, ImageBuffer, Pixel, Rgba, RgbaImage, imageops};

use super::types::TrimInfo;

// ======================================================================
// 切り出し
// ======================================================================
/// トリミング情報に従って画像を切り出す
///
/// 矩形が画像全体のときは切り出さずに複製を返す
/// 範囲外へ拡張する余白があれば、元画像と同じ色形式のキャンバスを `info.fill` で塗って追加する
/// `info` が別の画像のものでも、`crop_imm` が画像範囲へクランプするためパニックしない
///
/// # Arguments
/// * `img`  - 切り出し元の画像 (`info` を求めた画像)
/// * `info` - [`super::detect_trim`] で求めたトリミング情報
///
/// # Returns
/// * `DynamicImage` - 切り出した画像
#[must_use]
pub fn apply_trim(img: &DynamicImage, info: &TrimInfo) -> DynamicImage {
    let (crop_w, crop_h) = (info.crop_w(), info.crop_h());
    let cropped = if info.offset_x == 0
        && info.offset_y == 0
        && crop_w == img.width()
        && crop_h == img.height()
    {
        img.clone()
    } else {
        img.crop_imm(info.offset_x, info.offset_y, crop_w, crop_h)
    };
    if info.is_extended() {
        extend_canvas(&cropped, info)
    } else {
        cropped
    }
}

/// 切り出した画像を `info.fill` で塗ったキャンバスへ配置し、範囲外の余白を追加する
///
/// 16bit や浮動小数点の画像も精度を落とさないよう、元と同じ色形式のキャンバスを使う
/// 塗り色は RGBA8 から各色形式へ変換する (アルファのない形式では不透明として扱う)
fn extend_canvas(src: &DynamicImage, info: &TrimInfo) -> DynamicImage {
    let fill = DynamicImage::ImageRgba8(RgbaImage::from_pixel(1, 1, Rgba(info.fill)));
    macro_rules! extend {
        ($variant:ident, $buf:expr, $convert:ident) => {
            DynamicImage::$variant(extend_buffer($buf, *fill.$convert().get_pixel(0, 0), info))
        };
    }
    match src {
        DynamicImage::ImageLuma8(buf) => extend!(ImageLuma8, buf, to_luma8),
        DynamicImage::ImageLumaA8(buf) => extend!(ImageLumaA8, buf, to_luma_alpha8),
        DynamicImage::ImageRgb8(buf) => extend!(ImageRgb8, buf, to_rgb8),
        DynamicImage::ImageLuma16(buf) => extend!(ImageLuma16, buf, to_luma16),
        DynamicImage::ImageLumaA16(buf) => extend!(ImageLumaA16, buf, to_luma_alpha16),
        DynamicImage::ImageRgb16(buf) => extend!(ImageRgb16, buf, to_rgb16),
        DynamicImage::ImageRgba16(buf) => extend!(ImageRgba16, buf, to_rgba16),
        DynamicImage::ImageRgb32F(buf) => extend!(ImageRgb32F, buf, to_rgb32f),
        DynamicImage::ImageRgba32F(buf) => extend!(ImageRgba32F, buf, to_rgba32f),
        DynamicImage::ImageRgba8(buf) => extend!(ImageRgba8, buf, to_rgba8),
        other => extend!(ImageRgba8, &other.to_rgba8(), to_rgba8),
    }
}

/// `fill` で塗った出力サイズのバッファへ `src` を余白分ずらして配置する
fn extend_buffer<P: Pixel>(
    src: &ImageBuffer<P, Vec<P::Subpixel>>,
    fill: P,
    info: &TrimInfo,
) -> ImageBuffer<P, Vec<P::Subpixel>> {
    let mut canvas = ImageBuffer::from_pixel(info.trim_w, info.trim_h, fill);
    imageops::replace(
        &mut canvas,
        src,
        i64::from(info.extend_left),
        i64::from(info.extend_top),
    );
    canvas
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{bordered_rgba, solid_rgba};
    use crate::trim::{detect_trim, sample_pixel};

    /// `apply_trim` が矩形どおりの位置・サイズで切り出すこと
    #[test]
    fn apply_trim_crops_to_info() {
        let img = bordered_rgba(10, 10, 2, [255, 255, 255, 255], [255, 0, 0, 255]);
        let bounds = detect_trim(&img, 0, 1, None);
        let cropped = apply_trim(&img, &bounds.info);
        assert_eq!(cropped.width(), bounds.info.trim_w);
        assert_eq!(cropped.height(), bounds.info.trim_h);
        // padding 1px 分の背景が左上に残り、その内側から内容が始まる
        assert_eq!(sample_pixel(&cropped, 0, 0), Some([255, 255, 255, 255]));
        assert_eq!(sample_pixel(&cropped, 1, 1), Some([255, 0, 0, 255]));
    }

    /// 画像全体の矩形では内容を変えずに返すこと
    #[test]
    fn apply_trim_full_rect_returns_same_image() {
        let img = solid_rgba(6, 4, [10, 20, 30, 255]);
        let bounds = detect_trim(&img, 0, 0, None);
        assert_eq!(apply_trim(&img, &bounds.info), img);
    }
}
