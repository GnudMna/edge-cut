//! egui 描画用の画像変換

use eframe::egui;
use egui::{Color32, ColorImage};
use image::{DynamicImage, RgbaImage};
use moxcms::{ColorProfile, DataColorSpace, Layout, TransformOptions};
use rayon::prelude::*;

use crate::consts::{PREVIEW_MAX_DIMENSION, TRIM_PARALLEL_MIN_PIXELS};

// ======================================================================
// 画像変換
// ======================================================================
/// `DynamicImage` をプレビュー用の `ColorImage` へ変換する
///
/// 長辺が [`PREVIEW_MAX_DIMENSION`] を超える画像は縮小してから変換し、
/// GPU テクスチャ用のメモリ使用量を抑える。
/// RGB / GRAY の ICC プロファイルがあれば sRGB へ変換し、広色域の画像の色を実際の見え方に近づける
///
/// # Arguments
/// * `img` - 変換元の画像
/// * `icc_profile` - 元画像の ICC プロファイル。`None` や変換できないプロファイルは sRGB として扱う
///
/// # Returns
/// * `egui::ColorImage` - RGBA ピクセル列を持つ egui 用画像
pub(super) fn to_preview_color_image(img: &DynamicImage, icc_profile: Option<&[u8]>) -> ColorImage {
    let mut rgba = if img.width() <= PREVIEW_MAX_DIMENSION && img.height() <= PREVIEW_MAX_DIMENSION
    {
        img.to_rgba8()
    } else {
        img.thumbnail(PREVIEW_MAX_DIMENSION, PREVIEW_MAX_DIMENSION)
            .to_rgba8()
    };
    if let Some(icc) = icc_profile {
        convert_to_srgb(&mut rgba, icc);
    }
    rgba8_to_color_image(&rgba)
}

/// 余白の塗り色を画像本体と同じ ICC 変換でプレビュー用の sRGB へ変換する
///
/// # Arguments
/// * `fill` - 変換前の RGBA 塗り色
/// * `icc_profile` - 元画像の ICC プロファイル。`None` や変換できないプロファイルは変換しない
///
/// # Returns
/// * `[u8; 4]` - プレビューで塗る RGBA 色
pub(super) fn to_preview_fill(fill: [u8; 4], icc_profile: Option<&[u8]>) -> [u8; 4] {
    let Some(icc) = icc_profile else {
        return fill;
    };
    let mut px = RgbaImage::from_pixel(1, 1, image::Rgba(fill));
    convert_to_srgb(&mut px, icc);
    px.get_pixel(0, 0).0
}

/// RGBA8 画像を ICC プロファイルの色空間から sRGB へ変換する
///
/// RGB と GRAY 以外のプロファイルや壊れたプロファイル、変換の失敗時は画像を変更しない
///
/// # Arguments
/// * `rgba` - 変換する画像 (その場で書き換える)
/// * `icc` - 元画像の ICC プロファイル
pub(super) fn convert_to_srgb(rgba: &mut RgbaImage, icc: &[u8]) {
    let Ok(src) = ColorProfile::new_from_slice(icc) else {
        return;
    };
    match src.color_space {
        DataColorSpace::Rgb => convert_rgb_to_srgb(rgba, &src),
        DataColorSpace::Gray => convert_gray_to_srgb(rgba, &src),
        _ => {}
    }
}

/// RGBA8 画像を RGB プロファイルの色空間から sRGB へ変換する
///
/// # Arguments
/// * `rgba` - 変換する画像 (その場で書き換える)
/// * `src` - 元画像の RGB プロファイル
fn convert_rgb_to_srgb(rgba: &mut RgbaImage, src: &ColorProfile) {
    let Ok(transform) = src.create_transform_8bit(
        Layout::Rgba,
        &ColorProfile::new_srgb(),
        Layout::Rgba,
        TransformOptions::default(),
    ) else {
        return;
    };
    let mut converted = vec![0u8; rgba.as_raw().len()];
    if transform.transform(rgba.as_raw(), &mut converted).is_ok() {
        rgba.copy_from_slice(&converted);
    }
}

/// RGBA8 画像を GRAY プロファイルのガンマから sRGB へ変換する
///
/// グレースケール画像は読み込み時に R = G = B へ展開されているため、R チャンネルを輝度として変換する。
/// アルファは変換の対象外とし、元の値を保つ
///
/// # Arguments
/// * `rgba` - 変換する画像 (その場で書き換える)
/// * `src` - 元画像の GRAY プロファイル
fn convert_gray_to_srgb(rgba: &mut RgbaImage, src: &ColorProfile) {
    let Ok(transform) = src.create_transform_8bit(
        Layout::Gray,
        &ColorProfile::new_srgb(),
        Layout::Rgba,
        TransformOptions::default(),
    ) else {
        return;
    };
    let gray: Vec<u8> = rgba
        .as_raw()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|px| px[0])
        .collect();
    let mut converted = vec![0u8; rgba.as_raw().len()];
    if transform.transform(&gray, &mut converted).is_err() {
        return;
    }
    for (dst, src) in rgba
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(converted.as_chunks::<4>().0)
    {
        dst[..3].copy_from_slice(&src[..3]);
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
        raw.as_chunks::<4>()
            .0
            .iter()
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

    /// サイズが元画像と一致すること
    #[test]
    fn color_image_size_matches_source() {
        let img = solid_rgba(4, 3, [255, 0, 0, 255]);
        let ci = to_preview_color_image(&img, None);
        assert_eq!(ci.size, [4, 3]);
    }

    /// ピクセル数が幅×高さと一致すること
    #[test]
    fn color_image_pixel_count_equals_width_times_height() {
        let img = solid_rgba(5, 7, [0, 0, 0, 255]);
        let ci = to_preview_color_image(&img, None);
        assert_eq!(ci.pixels.len(), 5 * 7);
    }

    /// RGBA 値が正しく変換されていること
    #[test]
    fn color_image_pixel_values_match_source() {
        let img = solid_rgba(2, 2, [100, 150, 200, 255]);
        let ci = to_preview_color_image(&img, None);
        let expected = egui::Color32::from_rgba_unmultiplied(100, 150, 200, 255);
        assert!(ci.pixels.iter().all(|&p| p == expected));
    }

    /// 完全に透明なピクセルが正しく変換されていること
    #[test]
    fn color_image_transparent_pixel_preserved() {
        let img = solid_rgba(1, 1, [0, 0, 0, 0]);
        let ci = to_preview_color_image(&img, None);
        assert_eq!(
            ci.pixels[0],
            egui::Color32::from_rgba_unmultiplied(0, 0, 0, 0)
        );
    }

    /// 長辺上限を超える画像はプレビュー用に縮小されること
    #[test]
    fn preview_downscales_oversized_image() {
        let img = solid_rgba(PREVIEW_MAX_DIMENSION + 100, 100, [255, 0, 0, 255]);
        let ci = to_preview_color_image(&img, None);
        assert!(ci.size[0] <= PREVIEW_MAX_DIMENSION as usize);
        assert!(ci.size[1] <= 100);
    }

    /// Display P3 のプロファイルを指定すると sRGB へ変換され、アルファは保たれること
    #[test]
    fn wide_gamut_profile_is_converted_to_srgb() {
        let icc = ColorProfile::new_display_p3().encode().unwrap();
        let img = solid_rgba(2, 2, [200, 100, 50, 128]);
        let plain = to_preview_color_image(&img, None);
        let converted = to_preview_color_image(&img, Some(&icc));
        assert_ne!(converted.pixels[0], plain.pixels[0]);
        assert_eq!(converted.pixels[0].a(), plain.pixels[0].a());
    }

    /// 余白の塗り色が画像本体と同じ ICC 変換を受けること
    #[test]
    fn preview_fill_matches_converted_image_pixel() {
        let icc = ColorProfile::new_display_p3().encode().unwrap();
        let fill = [200, 100, 50, 255];
        let body = to_preview_color_image(&solid_rgba(1, 1, fill), Some(&icc)).pixels[0];
        assert_eq!(to_preview_fill(fill, Some(&icc)), body.to_array());
        // プロファイルが無い場合は変換しない
        assert_eq!(to_preview_fill(fill, None), fill);
    }

    /// リニアな GRAY プロファイルを指定すると sRGB へ変換され、明るくなること
    ///
    /// アルファは保たれ、R = G = B のグレーのままであること
    #[test]
    fn linear_gray_profile_is_converted_to_srgb() {
        let icc = ColorProfile::new_gray_with_gamma(1.0).encode().unwrap();
        let opaque = solid_rgba(2, 2, [128, 128, 128, 255]);
        let px = to_preview_color_image(&opaque, Some(&icc)).pixels[0];
        // リニア 0.5 は sRGB で約 188 になる
        assert!(px.r() > 128 + 40, "r = {}", px.r());
        assert_eq!((px.r(), px.r()), (px.g(), px.b()));

        // 半透明でもアルファは変わらない
        let translucent = solid_rgba(2, 2, [128, 128, 128, 200]);
        let px = to_preview_color_image(&translucent, Some(&icc)).pixels[0];
        assert_eq!(px.a(), 200);
    }

    /// 壊れた ICC プロファイルでは色を変換しないこと
    #[test]
    fn invalid_profile_leaves_colors_unchanged() {
        let img = solid_rgba(2, 2, [200, 100, 50, 255]);
        let plain = to_preview_color_image(&img, None);
        let converted = to_preview_color_image(&img, Some(&[1, 2, 3]));
        assert_eq!(converted.pixels, plain.pixels);
    }

    /// 切り詰めた・破損させた ICC プロファイルでも panic せず、画像の寸法を保つこと
    ///
    /// 正常なプロファイルを 1 バイトずつ切り詰めた全長と、1 バイトずつ反転した全位置を試す
    #[test]
    fn truncated_or_corrupted_profile_never_panics() {
        let icc = ColorProfile::new_display_p3().encode().unwrap();
        let img = solid_rgba(2, 2, [200, 100, 50, 255]);

        for len in 0..icc.len() {
            let ci = to_preview_color_image(&img, Some(&icc[..len]));
            assert_eq!(ci.size, [2, 2], "{len} バイトで切り詰め");
        }
        for at in 0..icc.len() {
            let mut broken = icc.clone();
            broken[at] ^= 0xFF;
            let ci = to_preview_color_image(&img, Some(&broken));
            assert_eq!(ci.size, [2, 2], "{at} バイト目を反転");
        }
    }

    /// 巨大なゼロ埋めのプロファイルでも色を変換せずに返ること
    #[test]
    fn oversized_zero_profile_leaves_colors_unchanged() {
        let img = solid_rgba(2, 2, [200, 100, 50, 255]);
        let plain = to_preview_color_image(&img, None);
        let converted = to_preview_color_image(&img, Some(&vec![0u8; 16 * 1024 * 1024]));
        assert_eq!(converted.pixels, plain.pixels);
    }
}
