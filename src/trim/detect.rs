//! 余白の矩形検出
//!
//! 色形式ごとに生バッファから画素を読み出し、背景色との差から内容の最小矩形を求める。

use image::{DynamicImage, GenericImageView};
use rayon::prelude::*;

use super::background::detect_background_with;
use super::types::{TrimBounds, TrimInfo};
use crate::consts::{
    ALPHA_TRANSPARENT_THRESHOLD, MAX_RGB_SQ_DIST, TOL_MAX, TOL_SCALE_DENOM,
    TRIM_PARALLEL_MIN_PIXELS, TRIM_ROW_BLOCK_PIXELS,
};

// ======================================================================
// 矩形検出
// ======================================================================
/// 画像を切り出さずにトリミング矩形と背景色だけを求める
///
/// 判定規則は [`trim`] と同じ。プレビューのように矩形だけが必要な場面で
/// 切り抜き画像の複製を省くために使う
///
/// # Arguments
/// * `img`         - トリミング元の画像
/// * `tol`         - 余白検出の許容度 (0–100)
/// * `padding`     - トリミング後に残す余白ピクセル数
/// * `bg_override` - 手動指定の背景色。`None` のとき四辺から自動検出する
///
/// # Returns
/// * `TrimBounds` - トリミング情報と使用した背景色
#[must_use]
pub fn detect_trim(
    img: &DynamicImage,
    tol: u32,
    padding: u32,
    bg_override: Option<[u8; 4]>,
) -> TrimBounds {
    let (w, h) = (img.width(), img.height());

    // 0x0 画像は演算がアンダーフローするため全体を対象とする
    if w == 0 || h == 0 {
        let background = bg_override.unwrap_or([255, 255, 255, 255]);
        return TrimBounds {
            info: full_info(w, h, background),
            background,
        };
    }

    // 画像全体を RGBA8 へ変換すると大きな画像で操作ごとに巨大な確保が走るため、
    // 色形式ごとに生バッファから直接読み、画素単位で RGBA8 へ変換して判定する
    macro_rules! detect_raw {
        ($buf:expr, $channels:literal, $to_rgba8:expr) => {{
            let raw = $buf.as_raw();
            detect_trim_with(w, h, tol, padding, bg_override, |x, y| {
                $to_rgba8(raw_pixel::<_, $channels>(raw, w, x, y))
            })
        }};
    }
    match img {
        DynamicImage::ImageRgba8(buf) => detect_raw!(buf, 4, |p: [u8; 4]| p),
        DynamicImage::ImageRgb8(buf) => {
            detect_raw!(buf, 3, |[r, g, b]: [u8; 3]| [r, g, b, u8::MAX])
        }
        DynamicImage::ImageLuma8(buf) => detect_raw!(buf, 1, |[v]: [u8; 1]| [v, v, v, u8::MAX]),
        DynamicImage::ImageLumaA8(buf) => detect_raw!(buf, 2, |[v, a]: [u8; 2]| [v, v, v, a]),
        DynamicImage::ImageRgba16(buf) => {
            detect_raw!(buf, 4, |p: [u16; 4]| p.map(u16_to_u8))
        }
        DynamicImage::ImageRgb16(buf) => {
            detect_raw!(buf, 3, |[r, g, b]: [u16; 3]| [
                u16_to_u8(r),
                u16_to_u8(g),
                u16_to_u8(b),
                u8::MAX
            ])
        }
        DynamicImage::ImageLuma16(buf) => {
            detect_raw!(buf, 1, |[v]: [u16; 1]| {
                let v = u16_to_u8(v);
                [v, v, v, u8::MAX]
            })
        }
        DynamicImage::ImageLumaA16(buf) => {
            detect_raw!(buf, 2, |[v, a]: [u16; 2]| {
                let v = u16_to_u8(v);
                [v, v, v, u16_to_u8(a)]
            })
        }
        DynamicImage::ImageRgba32F(buf) => {
            detect_raw!(buf, 4, |p: [f32; 4]| p.map(f32_to_u8))
        }
        DynamicImage::ImageRgb32F(buf) => {
            detect_raw!(buf, 3, |[r, g, b]: [f32; 3]| {
                [f32_to_u8(r), f32_to_u8(g), f32_to_u8(b), u8::MAX]
            })
        }
        other => detect_trim_with(w, h, tol, padding, bg_override, |x, y| {
            other.get_pixel(x, y).0
        }),
    }
}

/// 生バッファから `N` チャネルの画素を読み出す
fn raw_pixel<T: Copy + Default, const N: usize>(raw: &[T], w: u32, x: u32, y: u32) -> [T; N] {
    let start = (y as usize * w as usize + x as usize) * N;
    let mut pixel = [T::default(); N];
    pixel.copy_from_slice(&raw[start..start + N]);
    pixel
}

/// 16bit の成分を 8bit へ丸めて変換する
///
/// `image` クレートの `DynamicImage::get_pixel` と同じ `round(c * 255 / 65535)` になる
#[allow(clippy::cast_possible_truncation)]
pub(super) fn u16_to_u8(c: u16) -> u8 {
    // 最大でも (65535 + 128) / 257 = 255 に収まる
    ((u32::from(c) + 128) / 257) as u8
}

/// 浮動小数点の成分 (0.0–1.0) を 8bit へ丸めて変換する
///
/// 範囲外は 0.0–1.0 に丸め、NaN は白 (1.0) として扱う。`image` クレートの変換に合わせる
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(super) fn f32_to_u8(c: f32) -> u8 {
    let clamped = if c < 1.0 { c.max(0.0) } else { 1.0 };
    // clamped は 0.0–1.0 なので 0–255 に収まる
    (clamped * f32::from(u8::MAX)).round() as u8
}
/// 画像全体を対象とするトリミング情報
fn full_info(w: u32, h: u32, fill: [u8; 4]) -> TrimInfo {
    TrimInfo {
        orig_w: w,
        orig_h: h,
        trim_w: w,
        trim_h: h,
        offset_x: 0,
        offset_y: 0,
        extend_left: 0,
        extend_top: 0,
        extend_right: 0,
        extend_bottom: 0,
        fill,
    }
}

/// 背景以外のピクセルが存在する矩形境界
struct ContentBounds {
    top: u32,
    bottom: u32,
    left: u32,
    right: u32,
}

/// 背景以外のピクセルが存在する最小矩形を走査する
fn find_content_bounds(
    w: u32,
    h: u32,
    parallel: bool,
    is_bg: impl Fn(u32, u32) -> bool + Sync,
) -> Option<ContentBounds> {
    // 行内は逐次に走査し、並列化は複数行をまとめたブロック単位で行う
    let row_has_content = |y: u32| (0..w).any(|x| !is_bg(x, y));
    let block_rows = usize::try_from(TRIM_ROW_BLOCK_PIXELS / u64::from(w))
        .unwrap_or(usize::MAX)
        .max(1);

    // 上端を先に求め、下端は上端以降だけを探す (全面背景の画像は上端の走査 1 回で終わる)
    // 上端行は内容を持つため、下端探索は必ず見つかる
    let (top, bottom) = if parallel {
        let top = (0..h)
            .into_par_iter()
            .with_min_len(block_rows)
            .find_first(|&y| row_has_content(y))?;
        let bottom = (top..h)
            .into_par_iter()
            .with_min_len(block_rows)
            .find_last(|&y| row_has_content(y))?;
        (top, bottom)
    } else {
        let top = (0..h).find(|&y| row_has_content(y))?;
        let bottom = (top..h).rev().find(|&y| row_has_content(y))?;
        (top, bottom)
    };

    // 左右端は行ごとに両端から走査して最小・最大の x を集める (列方向の走査はキャッシュに不利)
    let row_extent = |y: u32| -> Option<(u32, u32)> {
        let l = (0..w).find(|&x| !is_bg(x, y))?;
        let r = (l..w).rev().find(|&x| !is_bg(x, y))?;
        Some((l, r))
    };
    let merge = |a: (u32, u32), b: (u32, u32)| (a.0.min(b.0), a.1.max(b.1));

    // 上端行は内容を持つため、実際には必ず更新される
    let (left, right) = if parallel {
        (top..=bottom)
            .into_par_iter()
            .filter_map(row_extent)
            .reduce(|| (w - 1, 0), merge)
    } else {
        (top..=bottom)
            .filter_map(row_extent)
            .fold((w - 1, 0), merge)
    };

    Some(ContentBounds {
        top,
        bottom,
        left,
        right,
    })
}

/// 画素読み出し関数を使って余白トリミング境界を求める
///
/// # Arguments
/// * `w`, `h`      - 画像の幅と高さ (px)。いずれも 1 以上
/// * `tol`         - 余白検出の許容度 (0–100)
/// * `padding`     - トリミング後に残す余白ピクセル数
/// * `bg_override` - 手動指定の背景色。`None` のとき四辺から自動検出する
/// * `pixel`       - 座標 `(x, y)` の RGBA8 値を返す関数
///
/// # Returns
/// * `TrimBounds` - トリミング情報と使用した背景色
fn detect_trim_with(
    w: u32,
    h: u32,
    tol: u32,
    padding: u32,
    bg_override: Option<[u8; 4]>,
    pixel: impl Fn(u32, u32) -> [u8; 4] + Sync,
) -> TrimBounds {
    let bg = bg_override.unwrap_or_else(|| detect_background_with(w, h, &pixel));
    // ほぼ透明な背景色は最頻の実際の色 (例: [0,0,0,3]) になりうるため、余白は完全透明で塗る
    let fill = if bg[3] < ALPHA_TRANSPARENT_THRESHOLD {
        [bg[0], bg[1], bg[2], 0]
    } else {
        bg
    };

    // RGBA 各チャネルの差の二乗和がこの値以下なら背景とみなす
    // 尺度は RGB の最大二乗距離基準のままとし、不透明画像の結果を従来と揃える
    let sq_thresh =
        u32::try_from(u64::from(tol) * u64::from(tol) * MAX_RGB_SQ_DIST / TOL_SCALE_DENOM)
            .unwrap_or(u32::MAX);
    // 透明な背景ではアルファ差がこの値以下なら背景とみなす
    // RGB の距離と同じく許容度に比例させ、許容度最大で全アルファ (0–255) を覆う
    let alpha_thresh = u64::from(tol) * u64::from(u8::MAX) / u64::from(TOL_MAX);
    let parallel = u64::from(w) * u64::from(h) >= TRIM_PARALLEL_MIN_PIXELS;

    let is_bg = |x: u32, y: u32| -> bool {
        let p = pixel(x, y);
        // ほぼ完全に透明なピクセル → 背景とみなす
        if p[3] < ALPHA_TRANSPARENT_THRESHOLD {
            return true;
        }
        // ほぼ完全に透明なキャンバス → 透明な画素の RGB は見えないためアルファ差だけで判定する
        // 半透明の影やアンチエイリアスのにじみを許容度で削れるようにする
        if bg[3] < ALPHA_TRANSPARENT_THRESHOLD {
            return u64::from(p[3].abs_diff(bg[3])) <= alpha_thresh;
        }
        // 不透明な背景上の半透明ピクセルを背景と誤判定しないようアルファ差も加える
        let sq_dist: u32 = p
            .iter()
            .zip(bg)
            .map(|(&channel, bg_channel)| {
                let diff = u32::from(channel.abs_diff(bg_channel));
                diff * diff
            })
            .sum();
        sq_dist <= sq_thresh
    };

    // 許容度最大はアルファ差を含めすべてのピクセルを背景とみなすため走査を省く
    let bounds = if tol >= TOL_MAX {
        None
    } else {
        find_content_bounds(w, h, parallel, is_bg)
    };
    let Some(bounds) = bounds else {
        // 背景しかない → 画像全体を対象とする
        return TrimBounds {
            info: full_info(w, h, fill),
            background: bg,
        };
    };

    // padding を四辺に加算し、画像内は切り出し、画像端を越えた分は範囲外の余白とする
    let px_top = bounds.top.saturating_sub(padding);
    let px_left = bounds.left.saturating_sub(padding);
    let px_bottom = bounds.bottom.saturating_add(padding).min(h - 1);
    let px_right = bounds.right.saturating_add(padding).min(w - 1);
    let extend_top = padding.saturating_sub(bounds.top);
    let extend_left = padding.saturating_sub(bounds.left);
    let extend_bottom = bounds.bottom.saturating_add(padding).saturating_sub(h - 1);
    let extend_right = bounds.right.saturating_add(padding).saturating_sub(w - 1);

    let tw = (px_right - px_left + 1)
        .saturating_add(extend_left)
        .saturating_add(extend_right);
    let th = (px_bottom - px_top + 1)
        .saturating_add(extend_top)
        .saturating_add(extend_bottom);
    let info = TrimInfo {
        orig_w: w,
        orig_h: h,
        trim_w: tw,
        trim_h: th,
        offset_x: px_left,
        offset_y: px_top,
        extend_left,
        extend_top,
        extend_right,
        extend_bottom,
        fill,
    };
    TrimBounds {
        info,
        background: bg,
    }
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use image::{Rgba, RgbaImage};

    use super::*;
    use crate::test_helpers::bordered_rgba;
    use crate::trim::trim;

    /// `detect_trim` が `trim` と同じ矩形と背景色を返すこと
    #[test]
    fn detect_trim_matches_trim() {
        let img = bordered_rgba(20, 16, 3, [255, 255, 255, 255], [0, 0, 255, 255]);
        for (tol, padding) in [(0, 0), (10, 2), (100, 0)] {
            let bounds = detect_trim(&img, tol, padding, None);
            let result = trim(&img, tol, padding, None);
            assert_eq!(bounds.info, result.info);
            assert_eq!(bounds.background, result.background);
        }
    }

    /// RGBA8 以外の画像でも `detect_trim` が矩形を求めること
    #[test]
    fn detect_trim_handles_non_rgba8_image() {
        let mut rgb = image::RgbImage::from_pixel(10, 8, image::Rgb([255, 255, 255]));
        rgb.put_pixel(4, 3, image::Rgb([0, 0, 0]));
        let bounds = detect_trim(&DynamicImage::ImageRgb8(rgb), 0, 0, None);
        assert_eq!(
            (
                bounds.info.offset_x,
                bounds.info.offset_y,
                bounds.info.trim_w,
                bounds.info.trim_h
            ),
            (4, 3, 1, 1)
        );
    }

    /// 各色形式で RGBA8 へ一括変換した画像と同じ矩形・背景色を返すこと
    #[test]
    fn detect_trim_matches_rgba8_conversion_for_all_color_types() {
        // 半透明の縁と、背景と僅かに異なる色の内容を持つ画像を各色形式へ変換する
        let mut base = RgbaImage::from_pixel(24, 18, Rgba([250, 248, 246, 255]));
        for y in 5..12 {
            for x in 6..17 {
                base.put_pixel(x, y, Rgba([40, 120, 200, 180]));
            }
        }
        base.put_pixel(0, 0, Rgba([0, 0, 0, 0]));
        let base = DynamicImage::ImageRgba8(base);
        let variants = [
            base.clone(),
            DynamicImage::ImageRgb8(base.to_rgb8()),
            DynamicImage::ImageLuma8(base.to_luma8()),
            DynamicImage::ImageLumaA8(base.to_luma_alpha8()),
            DynamicImage::ImageRgb16(base.to_rgb16()),
            DynamicImage::ImageRgba16(base.to_rgba16()),
            DynamicImage::ImageLuma16(base.to_luma16()),
            DynamicImage::ImageLumaA16(base.to_luma_alpha16()),
            DynamicImage::ImageRgb32F(base.to_rgb32f()),
            DynamicImage::ImageRgba32F(base.to_rgba32f()),
        ];
        for img in &variants {
            let expected_src = DynamicImage::ImageRgba8(img.to_rgba8());
            for (tol, padding, bg_override) in [
                (0, 0, None),
                (5, 3, None),
                (30, 30, None),
                (10, 2, Some([250, 248, 246, 255])),
            ] {
                let actual = detect_trim(img, tol, padding, bg_override);
                let expected = detect_trim(&expected_src, tol, padding, bg_override);
                assert_eq!(actual, expected, "{:?} tol={tol}", img.color());
            }
        }
    }

    /// 16bit・浮動小数点の成分変換が `image` クレートの RGBA8 変換と一致すること
    ///
    /// 16bit は全値、浮動小数点は範囲外・NaN・丸め境界を含めて比較する
    #[test]
    fn component_conversions_match_image_crate() {
        let all_u16: Vec<u16> = (0..=u16::MAX).collect();
        let luma16 = image::ImageBuffer::<image::Luma<u16>, _>::from_raw(65536, 1, all_u16)
            .expect("バッファサイズが一致する");
        let expected = DynamicImage::ImageLuma16(luma16).to_rgba8();
        for (c, px) in (0..=u16::MAX).zip(expected.pixels()) {
            assert_eq!(u16_to_u8(c), px[0], "u16={c}");
        }

        let floats = [
            -1.0,
            0.0,
            0.001,
            0.5,
            0.5 / 255.0,
            1.5 / 255.0,
            0.999,
            1.0,
            2.0,
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ];
        let rgb32 = image::ImageBuffer::<image::Rgb<f32>, _>::from_raw(
            u32::try_from(floats.len()).expect("要素数は u32 に収まる"),
            1,
            floats.iter().flat_map(|&f| [f, f, f]).collect::<Vec<f32>>(),
        )
        .expect("バッファサイズが一致する");
        let expected = DynamicImage::ImageRgb32F(rgb32).to_rgba8();
        for (f, px) in floats.iter().zip(expected.pixels()) {
            assert_eq!(f32_to_u8(*f), px[0], "f32={f}");
        }
    }
}
