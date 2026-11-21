//! 画像余白トリミングロジックを提供

use std::collections::HashMap;

use crate::consts::{ALPHA_TRANSPARENT_THRESHOLD, MAX_RGB_SQ_DIST, TOL_SCALE_DENOM};

use eframe::egui;
use egui::{Color32, ColorImage};
use image::{DynamicImage, RgbaImage};
use rayon::prelude::*;

// ======================================================================
// トリミング結果
// ======================================================================
/// トリミング結果の情報
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrimInfo {
    /// 元画像の幅 (px)
    pub orig_w: u32,
    /// 元画像の高さ (px)
    pub orig_h: u32,
    /// トリミング後の幅 (px)
    pub trim_w: u32,
    /// トリミング後の高さ (px)
    pub trim_h: u32,
    /// トリミング開始位置の X オフセット (px)
    pub offset_x: u32,
    /// トリミング開始位置の Y オフセット (px)
    pub offset_y: u32,
}

// ======================================================================
// 画像処理
// ======================================================================
/// `DynamicImage` を egui 描画用の `ColorImage` へ変換する
///
/// # Arguments
/// * `img` - 変換元の画像
///
/// # Returns
/// * `egui::ColorImage` - RGBA ピクセル列を持つ egui 用画像
pub fn to_color_image(img: &DynamicImage) -> ColorImage {
    let rgba = img.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let pixels: Vec<Color32> = rgba
        .as_raw()
        .par_chunks_exact(4)
        .map(|c| Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]))
        .collect();
    ColorImage::new(size, pixels)
}

/// 四辺のピクセルを走査して最頻色を背景色として検出する
///
/// 四隅 4 点のみ参照する旧実装と異なり、上・下・左・右の全辺ピクセルを
/// サンプリングするため、角が丸い素材や隅に異なる色が置かれた画像でも
/// 誤検出しにくい
///
/// # Arguments
/// * `rgba` - 対象画像の RGBA バッファ
/// * `w`    - 画像の幅 (px)
/// * `h`    - 画像の高さ (px)
///
/// # Returns
/// * `[u8; 4]` - 背景と推定される RGBA 値
pub fn detect_background_rgba(rgba: &RgbaImage, w: u32, h: u32) -> [u8; 4] {
    let raw = rgba.as_raw();
    let stride = (w * 4) as usize;
    let pixel = |x: u32, y: u32| -> [u8; 4] {
        let i = y as usize * stride + x as usize * 4;
        [raw[i], raw[i + 1], raw[i + 2], raw[i + 3]]
    };
    let mut freq: HashMap<[u8; 4], u32> = HashMap::new();
    // 上辺・下辺
    for x in 0..w {
        *freq.entry(pixel(x, 0)).or_default() += 1;
        *freq.entry(pixel(x, h - 1)).or_default() += 1;
    }
    // 左辺・右辺 (角は上辺・下辺のループで計上済みのため 1..h-1)
    for y in 1..h.saturating_sub(1) {
        *freq.entry(pixel(0, y)).or_default() += 1;
        *freq.entry(pixel(w - 1, y)).or_default() += 1;
    }
    freq.into_iter()
        .max_by_key(|(_, v)| *v)
        .map_or([255, 255, 255, 255], |(k, _)| k)
}

/// 画像上の指定座標の RGBA 値を取得する
///
/// 座標が画像範囲外のときは `None` を返す
pub fn sample_pixel(img: &DynamicImage, px: u32, py: u32) -> Option<[u8; 4]> {
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    if px >= width || py >= height {
        return None;
    }
    let pixel = rgba.get_pixel(px, py).0;
    Some(pixel)
}

/// 画像の四辺から背景余白をトリミングする
///
/// 許容度 `tol` は 0–100 の範囲で指定する
/// 0 のとき背景色と完全一致するピクセルのみを除去し、
/// 100 のとき最大 RGB 二乗距離 (195,075) 以内のすべての色を背景とみなす
/// `padding` はトリミング後に四辺に追加する余白ピクセル数
///
/// # Arguments
/// * `img`         - トリミング元の画像
/// * `tol`         - 余白検出の許容度 (0–100)
/// * `padding`     - トリミング後に残す余白ピクセル数
/// * `bg_override` - 手動指定の背景色。`None` のとき四辺から自動検出する
///
/// # Returns
/// * `(DynamicImage, TrimInfo, [u8; 4])` - トリミング後の画像、トリミング情報、使用した背景色
pub fn trim(
    img: &DynamicImage,
    tol: u32,
    padding: u32,
    bg_override: Option<[u8; 4]>,
) -> (DynamicImage, TrimInfo, [u8; 4]) {
    let rgba = img.to_rgba8();
    let (w, h) = (img.width(), img.height());

    // 0x0 画像は演算がアンダーフローするためそのまま返す
    if w == 0 || h == 0 {
        let bg = bg_override.unwrap_or([255, 255, 255, 255]);
        return (
            img.clone(),
            TrimInfo {
                orig_w: w,
                orig_h: h,
                trim_w: w,
                trim_h: h,
                offset_x: 0,
                offset_y: 0,
            },
            bg,
        );
    }

    let bg = bg_override.unwrap_or_else(|| detect_background_rgba(&rgba, w, h));

    // RGB 各チャネルの差の二乗和がこの値以下なら背景とみなす
    let sq_thresh =
        u32::try_from(u64::from(tol) * u64::from(tol) * MAX_RGB_SQ_DIST / TOL_SCALE_DENOM)
            .unwrap_or(u32::MAX);
    let raw = rgba.as_raw();
    let stride = (w * 4) as usize;

    let is_bg = |x: u32, y: u32| -> bool {
        let i = y as usize * stride + x as usize * 4;
        let p = [raw[i], raw[i + 1], raw[i + 2], raw[i + 3]];
        if p[3] < ALPHA_TRANSPARENT_THRESHOLD {
            return true;
        } // ほぼ完全に透明なピクセル → 背景とみなす
        if bg[3] < ALPHA_TRANSPARENT_THRESHOLD {
            return p[3] < ALPHA_TRANSPARENT_THRESHOLD;
        } // ほぼ完全に透明なキャンバス → アルファのみで判定
        let dr = u32::from(p[0]).abs_diff(u32::from(bg[0]));
        let dg = u32::from(p[1]).abs_diff(u32::from(bg[1]));
        let db = u32::from(p[2]).abs_diff(u32::from(bg[2]));
        dr * dr + dg * dg + db * db <= sq_thresh
    };

    // top/bottom を並列に走査する
    let (top_opt, bottom_opt) = rayon::join(
        || (0..h).find(|&y| (0..w).into_par_iter().any(|x| !is_bg(x, y))),
        || {
            (0..h)
                .rev()
                .find(|&y| (0..w).into_par_iter().any(|x| !is_bg(x, y)))
        },
    );

    let (Some(top), Some(bottom)) = (top_opt, bottom_opt) else {
        // 背景しかない → 元画像をそのまま返す
        return (
            img.clone(),
            TrimInfo {
                orig_w: w,
                orig_h: h,
                trim_w: w,
                trim_h: h,
                offset_x: 0,
                offset_y: 0,
            },
            bg,
        );
    };

    // left/right を並列に走査する
    let (left, right) = rayon::join(
        || {
            (0..w)
                .find(|&x| (top..=bottom).into_par_iter().any(|y| !is_bg(x, y)))
                .unwrap_or(0)
        },
        || {
            (0..w)
                .rev()
                .find(|&x| (top..=bottom).into_par_iter().any(|y| !is_bg(x, y)))
                .unwrap_or(w - 1)
        },
    );

    // padding を四辺に加算し、画像境界でクランプ
    let px_top = top.saturating_sub(padding);
    let px_bottom = (bottom + padding).min(h - 1);
    let px_left = left.saturating_sub(padding);
    let px_right = (right + padding).min(w - 1);

    let tw = px_right - px_left + 1;
    let th = px_bottom - px_top + 1;
    let info = TrimInfo {
        orig_w: w,
        orig_h: h,
        trim_w: tw,
        trim_h: th,
        offset_x: px_left,
        offset_y: px_top,
    };
    (img.crop_imm(px_left, px_top, tw, th), info, bg)
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;

    use eframe::egui;
    use image::{DynamicImage, RgbaImage};

    // ======================================================================
    // ヘルパー関数
    // ======================================================================
    /// 単色の RGBA 画像を生成する
    fn solid_rgba(w: u32, h: u32, color: [u8; 4]) -> DynamicImage {
        let buf: Vec<u8> = std::iter::repeat_n(color, (w * h) as usize)
            .flatten()
            .collect();
        DynamicImage::ImageRgba8(RgbaImage::from_raw(w, h, buf).unwrap())
    }

    /// 縁 (border px) と内側で色が異なる RGBA 画像を生成する
    fn bordered_rgba(w: u32, h: u32, border: u32, bg: [u8; 4], fg: [u8; 4]) -> DynamicImage {
        let mut img = RgbaImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let color = if x < border || x >= w - border || y < border || y >= h - border {
                    image::Rgba(bg)
                } else {
                    image::Rgba(fg)
                };
                img.put_pixel(x, y, color);
            }
        }
        DynamicImage::ImageRgba8(img)
    }

    // ======================================================================
    // 画像変換
    // ======================================================================
    /// サイズが元画像と一致することを確認する
    #[test]
    fn color_image_size_matches_source() {
        let img = solid_rgba(4, 3, [255, 0, 0, 255]);
        let ci = to_color_image(&img);
        assert_eq!(ci.size, [4, 3]);
    }

    /// ピクセル数が幅×高さと一致することを確認する
    #[test]
    fn color_image_pixel_count_equals_width_times_height() {
        let img = solid_rgba(5, 7, [0, 0, 0, 255]);
        let ci = to_color_image(&img);
        assert_eq!(ci.pixels.len(), 5 * 7);
    }

    /// RGBA 値が正しく変換されていることを確認する
    #[test]
    fn color_image_pixel_values_match_source() {
        let img = solid_rgba(2, 2, [100, 150, 200, 255]);
        let ci = to_color_image(&img);
        let expected = egui::Color32::from_rgba_unmultiplied(100, 150, 200, 255);
        assert!(ci.pixels.iter().all(|&p| p == expected));
    }

    /// 完全に透明なピクセルが正しく変換されていることを確認する
    #[test]
    fn color_image_transparent_pixel_preserved() {
        let img = solid_rgba(1, 1, [0, 0, 0, 0]);
        let ci = to_color_image(&img);
        assert_eq!(
            ci.pixels[0],
            egui::Color32::from_rgba_unmultiplied(0, 0, 0, 0)
        );
    }

    // ======================================================================
    // トリム
    // ======================================================================
    /// トリミング前後で元画像の幅と高さが保持されていることを確認する
    #[test]
    fn trim_info_preserves_original_dimensions() {
        let img = bordered_rgba(20, 16, 3, [255, 255, 255, 255], [0, 0, 0, 255]);
        let (_, info, _) = trim(&img, 0, 0, None);
        assert_eq!(info.orig_w, 20);
        assert_eq!(info.orig_h, 16);
    }

    /// 縁が正しく削除されることを確認する
    #[test]
    fn trim_removes_white_border() {
        // 10x10 / 縁 2px 白 / 中央 6x6 赤
        let img = bordered_rgba(10, 10, 2, [255, 255, 255, 255], [255, 0, 0, 255]);
        let (_, info, _) = trim(&img, 0, 0, None);
        assert_eq!(info.offset_x, 2);
        assert_eq!(info.offset_y, 2);
        assert_eq!(info.trim_w, 6);
        assert_eq!(info.trim_h, 6);
    }

    /// トリミング後の画像サイズが情報と一致することを確認する
    #[test]
    fn trim_result_image_size_matches_info() {
        let img = bordered_rgba(10, 10, 2, [255, 255, 255, 255], [255, 0, 0, 255]);
        let (result, info, _) = trim(&img, 0, 0, None);
        assert_eq!(result.width(), info.trim_w);
        assert_eq!(result.height(), info.trim_h);
    }

    /// 全ピクセルが背景色の場合、トリミング後もサイズが変わらないことを確認する
    #[test]
    fn trim_solid_background_returns_original_size() {
        // 全ピクセルが背景色 → トリミング後もサイズ変わらず
        let img = solid_rgba(8, 8, [255, 255, 255, 255]);
        let (result, info, _) = trim(&img, 0, 0, None);
        assert_eq!(info.trim_w, 8);
        assert_eq!(info.trim_h, 8);
        assert_eq!(result.width(), 8);
        assert_eq!(result.height(), 8);
    }

    /// 1x1 の画像でもパニックしないことを確認する
    #[test]
    fn trim_1x1_image_does_not_panic() {
        let img = solid_rgba(1, 1, [255, 0, 0, 255]);
        let (result, info, _) = trim(&img, 0, 0, None);
        assert_eq!(info.orig_w, 1);
        assert_eq!(info.orig_h, 1);
        assert_eq!(result.width(), 1);
        assert_eq!(result.height(), 1);
    }

    /// 非対称画像のトリミング後の寸法が正しいことを確認する
    #[test]
    fn trim_asymmetric_image_correct_dimensions() {
        // 30x20 / 縁 3px 白 / 中央 24x14 黒
        let img = bordered_rgba(30, 20, 3, [255, 255, 255, 255], [0, 0, 0, 255]);
        let (_, info, _) = trim(&img, 0, 0, None);
        assert_eq!(info.orig_w, 30);
        assert_eq!(info.orig_h, 20);
        assert_eq!(info.trim_w, 24);
        assert_eq!(info.trim_h, 14);
    }

    // ======================================================================
    // トリム - パディング
    // ======================================================================
    /// padding を指定したとき、トリミング結果が正しく拡張されることを確認する
    #[test]
    fn trim_padding_expands_result() {
        // 20x20 / 縁 5px 白 / 中央 10x10 青
        // padding=2 → offset=3, size=14
        let img = bordered_rgba(20, 20, 5, [255, 255, 255, 255], [0, 0, 255, 255]);
        let (_, info, _) = trim(&img, 0, 2, None);
        assert_eq!(info.offset_x, 3);
        assert_eq!(info.offset_y, 3);
        assert_eq!(info.trim_w, 14);
        assert_eq!(info.trim_h, 14);
    }

    /// padding が画像端でクランプされることを確認する
    #[test]
    fn trim_padding_clamped_at_image_boundary() {
        // 10x10 / 縁 1px 白 / padding=10 → 画像端でクランプ
        let img = bordered_rgba(10, 10, 1, [255, 255, 255, 255], [0, 255, 0, 255]);
        let (_, info, _) = trim(&img, 0, 10, None);
        assert_eq!(info.offset_x, 0);
        assert_eq!(info.offset_y, 0);
        assert_eq!(info.trim_w, 10);
        assert_eq!(info.trim_h, 10);
    }

    // ======================================================================
    // トリム - 許容誤差
    // ======================================================================
    /// tol=0 のとき、正確なマッチングでトリミングが行われることを確認する
    #[test]
    fn trim_tolerance_zero_performs_exact_match_trim() {
        let img = bordered_rgba(10, 10, 2, [255, 255, 255, 255], [255, 0, 0, 255]);
        let (_, info, _) = trim(&img, 0, 0, None);
        assert_eq!(info.trim_w, 6);
        assert_eq!(info.trim_h, 6);
    }

    /// tol=100 のとき、すべての色が背景として扱われることを確認する
    #[test]
    fn trim_max_tolerance_treats_all_colors_as_background() {
        // tol=100 のとき背景(白)との RGB 二乗距離 130050 ≤ 195075 となり、
        // 赤 [255,0,0] も背景とみなされ → 全体が bg → サイズ変わらず
        let img = bordered_rgba(10, 10, 2, [255, 255, 255, 255], [255, 0, 0, 255]);
        let (result, info, _) = trim(&img, 100, 0, None);
        assert_eq!(info.trim_w, info.orig_w);
        assert_eq!(info.trim_h, info.orig_h);
        assert_eq!(result.width(), 10);
        assert_eq!(result.height(), 10);
    }

    // ======================================================================
    // トリム - 透明ピクセル
    // ======================================================================
    /// 完全に透明なピクセルが背景として扱われることを確認する
    #[test]
    fn trim_fully_transparent_border_treated_as_background() {
        let mut buf = RgbaImage::new(10, 10);
        for y in 0..10u32 {
            for x in 0..10u32 {
                let px = if !(2..8).contains(&x) || !(2..8).contains(&y) {
                    image::Rgba([0u8, 0, 0, 0]) // 完全透明
                } else {
                    image::Rgba([255u8, 0, 0, 255]) // 不透明赤
                };
                buf.put_pixel(x, y, px);
            }
        }
        let img = DynamicImage::ImageRgba8(buf);
        let (_, info, _) = trim(&img, 0, 0, None);
        assert_eq!(info.offset_x, 2);
        assert_eq!(info.offset_y, 2);
        assert_eq!(info.trim_w, 6);
        assert_eq!(info.trim_h, 6);
    }

    /// 四隅がすべて透明 → bg = 透明キャンバス
    /// 内側は半透明(alpha=100 >= 16) → 背景とみなされず保持される
    #[test]
    fn trim_transparent_canvas_uses_alpha_only_for_bg_detection() {
        let mut buf = RgbaImage::new(6, 6);
        for y in 0..6u32 {
            for x in 0..6u32 {
                let px = if x == 0 || x == 5 || y == 0 || y == 5 {
                    image::Rgba([0u8, 0, 0, 0])
                } else {
                    image::Rgba([128u8, 128, 128, 100])
                };
                buf.put_pixel(x, y, px);
            }
        }
        let img = DynamicImage::ImageRgba8(buf);
        let (_, info, _) = trim(&img, 0, 0, None);
        assert_eq!(info.offset_x, 1);
        assert_eq!(info.offset_y, 1);
        assert_eq!(info.trim_w, 4);
        assert_eq!(info.trim_h, 4);
    }

    // ======================================================================
    // detect_bg (private 関数の間接テスト)
    // ======================================================================
    /// 四辺のうち 3 辺を白・1 辺を黒にした場合、白が背景色として選ばれることを確認する
    #[test]
    fn detect_bg_picks_most_frequent_edge_color() {
        // 10x6 の画像。上・左・右辺を白、下辺だけ黒にする
        let mut buf = RgbaImage::new(10, 6);
        let white = image::Rgba([255u8, 255, 255, 255]);
        let black = image::Rgba([0u8, 0, 0, 255]);
        // 全辺を白で埋める
        for x in 0..10u32 {
            buf.put_pixel(x, 0, white);
            buf.put_pixel(x, 5, white);
        }
        for y in 1..5u32 {
            buf.put_pixel(0, y, white);
            buf.put_pixel(9, y, white);
        }
        // 下辺だけ黒に上書き (白 28px 、黒 10px → 白が勝つ)
        for x in 0..10u32 {
            buf.put_pixel(x, 5, black);
        }
        let bg = detect_background_rgba(&buf, 10, 6);
        assert_eq!(bg, [255, 255, 255, 255]);
    }

    /// 全辺のピクセルが同じ色の場合、その色が背景色として選ばれることを確認する
    #[test]
    fn detect_bg_all_same_edges_returns_that_color() {
        // 5x5 の画像。全辺を赤で埋める
        let mut buf = RgbaImage::new(5, 5);
        let red = image::Rgba([200u8, 50, 50, 255]);
        for x in 0..5u32 {
            buf.put_pixel(x, 0, red); // 上辺
            buf.put_pixel(x, 4, red); // 下辺
        }
        for y in 1..4u32 {
            buf.put_pixel(0, y, red); // 左辺
            buf.put_pixel(4, y, red); // 右辺
        }
        let bg = detect_background_rgba(&buf, 5, 5);
        assert_eq!(bg, [200, 50, 50, 255]);
    }

    /// 手動指定の背景色がトリミング結果に反映されることを確認する
    #[test]
    fn trim_manual_background_returns_specified_color() {
        let img = bordered_rgba(10, 10, 2, [255, 255, 255, 255], [255, 0, 0, 255]);
        let manual = [10, 20, 30, 255];
        let (_, _, used_bg) = trim(&img, 0, 0, Some(manual));
        assert_eq!(used_bg, manual);
    }

    /// 手動で白を指定したとき、自動検出と同じトリミング結果になることを確認する
    #[test]
    fn trim_manual_white_matches_auto_detection() {
        let img = bordered_rgba(10, 10, 2, [255, 255, 255, 255], [255, 0, 0, 255]);
        let (_, info_auto, _) = trim(&img, 0, 0, None);
        let (_, info_manual, _) = trim(&img, 0, 0, Some([255, 255, 255, 255]));
        assert_eq!(info_auto.trim_w, info_manual.trim_w);
        assert_eq!(info_auto.trim_h, info_manual.trim_h);
        assert_eq!(info_auto.offset_x, info_manual.offset_x);
        assert_eq!(info_auto.offset_y, info_manual.offset_y);
    }

    /// `sample_pixel` が画像内の色を正しく返すことを確認する
    #[test]
    fn sample_pixel_returns_rgba_at_coordinates() {
        let img = bordered_rgba(10, 10, 2, [255, 255, 255, 255], [255, 0, 0, 255]);
        assert_eq!(sample_pixel(&img, 0, 0), Some([255, 255, 255, 255]));
        assert_eq!(sample_pixel(&img, 5, 5), Some([255, 0, 0, 255]));
        assert_eq!(sample_pixel(&img, 10, 0), None);
    }
}
