//! egui 描画用の画像変換

use crate::consts::{PREVIEW_MAX_DIMENSION, TRIM_PARALLEL_MIN_PIXELS};

use eframe::egui;
use egui::{Color32, ColorImage};
use image::{DynamicImage, RgbaImage};
use rayon::prelude::*;

// ======================================================================
// 画像変換
// ======================================================================
/// `DynamicImage` をプレビュー用の `ColorImage` へ変換する
///
/// 長辺が [`PREVIEW_MAX_DIMENSION`] を超える画像は縮小してから変換し、
/// GPU テクスチャ用のメモリ使用量を抑える
///
/// # Arguments
/// * `img` - 変換元の画像
///
/// # Returns
/// * `egui::ColorImage` - RGBA ピクセル列を持つ egui 用画像
pub(super) fn to_preview_color_image(img: &DynamicImage) -> ColorImage {
    if img.width() <= PREVIEW_MAX_DIMENSION && img.height() <= PREVIEW_MAX_DIMENSION {
        dynamic_to_color_image(img)
    } else {
        let thumb = img.thumbnail(PREVIEW_MAX_DIMENSION, PREVIEW_MAX_DIMENSION);
        dynamic_to_color_image(&thumb)
    }
}

/// `DynamicImage` を egui 描画用の `ColorImage` へ変換する
fn dynamic_to_color_image(img: &DynamicImage) -> ColorImage {
    match img {
        DynamicImage::ImageRgba8(rgba) => rgba8_to_color_image(rgba),
        other => {
            let rgba = other.to_rgba8();
            rgba8_to_color_image(&rgba)
        }
    }
}

/// `RgbaImage` を egui 描画用の `ColorImage` へ変換する
fn rgba8_to_color_image(rgba: &RgbaImage) -> ColorImage {
    let (w, h) = rgba.dimensions();
    let size = [w as usize, h as usize];
    let raw = rgba.as_raw();
    let parallel = u64::from(w) * u64::from(h) >= TRIM_PARALLEL_MIN_PIXELS;
    let pixels: Vec<Color32> = if parallel {
        raw.par_chunks_exact(4)
            .map(|c| Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]))
            .collect()
    } else {
        raw.chunks_exact(4)
            .map(|c| Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]))
            .collect()
    };
    ColorImage::new(size, pixels)
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::solid_rgba;

    /// サイズが元画像と一致することを確認する
    #[test]
    fn color_image_size_matches_source() {
        let img = solid_rgba(4, 3, [255, 0, 0, 255]);
        let ci = to_preview_color_image(&img);
        assert_eq!(ci.size, [4, 3]);
    }

    /// ピクセル数が幅×高さと一致することを確認する
    #[test]
    fn color_image_pixel_count_equals_width_times_height() {
        let img = solid_rgba(5, 7, [0, 0, 0, 255]);
        let ci = to_preview_color_image(&img);
        assert_eq!(ci.pixels.len(), 5 * 7);
    }

    /// RGBA 値が正しく変換されていることを確認する
    #[test]
    fn color_image_pixel_values_match_source() {
        let img = solid_rgba(2, 2, [100, 150, 200, 255]);
        let ci = to_preview_color_image(&img);
        let expected = egui::Color32::from_rgba_unmultiplied(100, 150, 200, 255);
        assert!(ci.pixels.iter().all(|&p| p == expected));
    }

    /// 完全に透明なピクセルが正しく変換されていることを確認する
    #[test]
    fn color_image_transparent_pixel_preserved() {
        let img = solid_rgba(1, 1, [0, 0, 0, 0]);
        let ci = to_preview_color_image(&img);
        assert_eq!(
            ci.pixels[0],
            egui::Color32::from_rgba_unmultiplied(0, 0, 0, 0)
        );
    }

    /// 長辺上限を超える画像はプレビュー用に縮小されることを確認する
    #[test]
    fn preview_downscales_oversized_image() {
        let img = solid_rgba(PREVIEW_MAX_DIMENSION + 100, 100, [255, 0, 0, 255]);
        let ci = to_preview_color_image(&img);
        assert!(ci.size[0] <= PREVIEW_MAX_DIMENSION as usize);
        assert!(ci.size[1] <= 100);
    }
}
