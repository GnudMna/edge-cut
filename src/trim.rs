//! 画像余白トリミングロジックを提供

use std::collections::HashMap;

use crate::consts::{
    ALPHA_TRANSPARENT_THRESHOLD, BG_DETECT_QUANT_SHIFT, MAX_RGB_SQ_DIST, TOL_MAX, TOL_SCALE_DENOM,
    TRIM_PARALLEL_MIN_PIXELS,
};

use image::{DynamicImage, RgbaImage};
use rayon::prelude::*;

// ======================================================================
// トリミング結果
// ======================================================================
/// トリミング結果の情報
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// `trim` の戻り値
#[derive(Debug)]
pub struct TrimResult {
    /// トリミング後の画像
    pub image: DynamicImage,
    /// トリミング情報
    pub info: TrimInfo,
    /// 使用した背景色 (RGBA)
    pub background: [u8; 4],
}

// ======================================================================
// 画像処理
// ======================================================================
/// 四辺のピクセルを走査して最頻色を背景色として検出する
///
/// 四隅 4 点のみ参照する旧実装と異なり、上・下・左・右の全辺ピクセルを
/// サンプリングするため、角が丸い素材や隅に異なる色が置かれた画像でも
/// 誤検出しにくい
///
/// 色を量子化したビンと隣接ビンの合計票で背景の色クラスタを決めるため、
/// JPEG ノイズや緩いグラデーションで同系色の票が割れても安定して検出する
/// 返す値はクラスタ内で最も多い実在色とし、ノイズのない画像では完全一致する色になる
/// ほぼ完全に透明なピクセルは RGB に関係なく 1 つのクラスタとして集計する
///
/// # Arguments
/// * `rgba` - 対象画像の RGBA バッファ
/// * `w`    - 画像の幅 (px)
/// * `h`    - 画像の高さ (px)
///
/// # Returns
/// * `[u8; 4]` - 背景と推定される RGBA 値
#[must_use]
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

    let mut bins: HashMap<[u8; 4], u32> = HashMap::new();
    for (&color, &count) in &freq {
        *bins.entry(bg_bin(color)).or_default() += count;
    }

    // 隣接ビンを含めた票数が最大のビンを背景クラスタとする (同票はキー順で決定的に選ぶ)
    let Some((_, best_bin)) = bins
        .keys()
        .map(|&bin| (neighborhood_votes(&bins, bin), bin))
        .max()
    else {
        return [255, 255, 255, 255];
    };

    freq.into_iter()
        .filter(|&(color, _)| bins_are_adjacent(bg_bin(color), best_bin))
        .map(|(color, count)| (count, color))
        .max()
        .map_or([255, 255, 255, 255], |(_, color)| color)
}

/// ほぼ完全に透明なピクセルをまとめるビン (量子化後の値は最大 `255 >> shift` のため衝突しない)
const TRANSPARENT_BIN: [u8; 4] = [u8::MAX; 4];

/// 背景色検出用に色を量子化したビンを返す
fn bg_bin(color: [u8; 4]) -> [u8; 4] {
    if color[3] < ALPHA_TRANSPARENT_THRESHOLD {
        return TRANSPARENT_BIN;
    }
    color.map(|c| c >> BG_DETECT_QUANT_SHIFT)
}

/// 2 つのビンが同一または各チャネル ±1 以内で隣接しているか
///
/// 透明ビンは自身とのみ隣接する
fn bins_are_adjacent(a: [u8; 4], b: [u8; 4]) -> bool {
    if a == TRANSPARENT_BIN || b == TRANSPARENT_BIN {
        return a == b;
    }
    a.iter().zip(b).all(|(&x, y)| x.abs_diff(y) <= 1)
}

/// `bin` と隣接ビンの票数の合計
fn neighborhood_votes(bins: &HashMap<[u8; 4], u32>, bin: [u8; 4]) -> u32 {
    if bin == TRANSPARENT_BIN {
        return bins.get(&bin).copied().unwrap_or(0);
    }
    // 各チャネル -1, 0, +1 の 3^4 通りを列挙する
    let mut total = 0;
    for code in 0..81u32 {
        let mut neighbor = bin;
        let mut rest = code;
        let mut valid = true;
        for c in &mut neighbor {
            let shifted = match rest % 3 {
                0 => Some(*c),
                1 => c.checked_sub(1),
                _ => c.checked_add(1),
            };
            rest /= 3;
            match shifted {
                Some(v) => *c = v,
                None => valid = false,
            }
        }
        if valid {
            total += bins.get(&neighbor).copied().unwrap_or(0);
        }
    }
    total
}

/// 画像上の指定座標の RGBA 値を取得する
///
/// 座標が画像範囲外のときは `None` を返す
#[must_use]
pub fn sample_pixel(img: &DynamicImage, px: u32, py: u32) -> Option<[u8; 4]> {
    match img {
        DynamicImage::ImageRgba8(rgba) => {
            if px >= rgba.width() || py >= rgba.height() {
                return None;
            }
            Some(rgba.get_pixel(px, py).0)
        }
        other => {
            let rgba = other.to_rgba8();
            if px >= rgba.width() || py >= rgba.height() {
                return None;
            }
            Some(rgba.get_pixel(px, py).0)
        }
    }
}

/// 画像の四辺から背景余白をトリミングする
///
/// 許容度 `tol` は 0–100 の範囲で指定する
/// 0 のとき背景色と完全一致するピクセルのみを除去し、
/// 100 のときすべての色を背景とみなす
/// 背景が不透明なときは RGBA 各チャネルの差の二乗和を、
/// 最大 RGB 二乗距離 (195,075) を基準にした閾値と比較する
/// `padding` はトリミング後に四辺に追加する余白ピクセル数
///
/// # Arguments
/// * `img`         - トリミング元の画像
/// * `tol`         - 余白検出の許容度 (0–100)
/// * `padding`     - トリミング後に残す余白ピクセル数
/// * `bg_override` - 手動指定の背景色。`None` のとき四辺から自動検出する
///
/// # Returns
/// * `TrimResult` - トリミング後の画像、トリミング情報、使用した背景色
#[must_use]
pub fn trim(
    img: &DynamicImage,
    tol: u32,
    padding: u32,
    bg_override: Option<[u8; 4]>,
) -> TrimResult {
    let (w, h) = (img.width(), img.height());

    // 0x0 画像は演算がアンダーフローするためそのまま返す
    if w == 0 || h == 0 {
        let bg = bg_override.unwrap_or([255, 255, 255, 255]);
        return TrimResult {
            image: img.clone(),
            info: TrimInfo {
                orig_w: w,
                orig_h: h,
                trim_w: w,
                trim_h: h,
                offset_x: 0,
                offset_y: 0,
            },
            background: bg,
        };
    }

    match img {
        DynamicImage::ImageRgba8(rgba) => trim_rgba(rgba, img, tol, padding, bg_override),
        other => {
            let rgba = other.to_rgba8();
            trim_rgba(&rgba, img, tol, padding, bg_override)
        }
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
    let row_has_content = |y: u32| -> bool {
        if parallel {
            (0..w).into_par_iter().any(|x| !is_bg(x, y))
        } else {
            (0..w).any(|x| !is_bg(x, y))
        }
    };

    let (top_opt, bottom_opt) = if parallel {
        rayon::join(
            || (0..h).find(|&y| row_has_content(y)),
            || (0..h).rev().find(|&y| row_has_content(y)),
        )
    } else {
        (
            (0..h).find(|&y| row_has_content(y)),
            (0..h).rev().find(|&y| row_has_content(y)),
        )
    };

    let (top, bottom) = (top_opt?, bottom_opt?);

    let col_has_content = |x: u32| -> bool {
        if parallel {
            (top..=bottom).into_par_iter().any(|y| !is_bg(x, y))
        } else {
            (top..=bottom).any(|y| !is_bg(x, y))
        }
    };

    let (left, right) = if parallel {
        rayon::join(
            || (0..w).find(|&x| col_has_content(x)).unwrap_or(0),
            || (0..w).rev().find(|&x| col_has_content(x)).unwrap_or(w - 1),
        )
    } else {
        (
            (0..w).find(|&x| col_has_content(x)).unwrap_or(0),
            (0..w).rev().find(|&x| col_has_content(x)).unwrap_or(w - 1),
        )
    };

    Some(ContentBounds {
        top,
        bottom,
        left,
        right,
    })
}

/// `RgbaImage` バッファ上で余白トリミング境界を求め、`source` から切り出す
fn trim_rgba(
    rgba: &RgbaImage,
    source: &DynamicImage,
    tol: u32,
    padding: u32,
    bg_override: Option<[u8; 4]>,
) -> TrimResult {
    let (w, h) = rgba.dimensions();
    let bg = bg_override.unwrap_or_else(|| detect_background_rgba(rgba, w, h));

    // RGBA 各チャネルの差の二乗和がこの値以下なら背景とみなす
    // 尺度は RGB の最大二乗距離基準のままとし、不透明画像の結果を従来と揃える
    let sq_thresh =
        u32::try_from(u64::from(tol) * u64::from(tol) * MAX_RGB_SQ_DIST / TOL_SCALE_DENOM)
            .unwrap_or(u32::MAX);
    let raw = rgba.as_raw();
    let stride = (w * 4) as usize;
    let parallel = u64::from(w) * u64::from(h) >= TRIM_PARALLEL_MIN_PIXELS;

    let is_bg = |x: u32, y: u32| -> bool {
        let i = y as usize * stride + x as usize * 4;
        let p = [raw[i], raw[i + 1], raw[i + 2], raw[i + 3]];
        // ほぼ完全に透明なピクセル → 背景とみなす
        if p[3] < ALPHA_TRANSPARENT_THRESHOLD {
            return true;
        }
        // ほぼ完全に透明なキャンバス → 透明でないピクセルはすべて内容
        if bg[3] < ALPHA_TRANSPARENT_THRESHOLD {
            return false;
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
        // 背景しかない → 元画像をそのまま返す
        return TrimResult {
            image: source.clone(),
            info: TrimInfo {
                orig_w: w,
                orig_h: h,
                trim_w: w,
                trim_h: h,
                offset_x: 0,
                offset_y: 0,
            },
            background: bg,
        };
    };

    // padding を四辺に加算し、画像境界でクランプ
    let px_top = bounds.top.saturating_sub(padding);
    let px_bottom = (bounds.bottom + padding).min(h - 1);
    let px_left = bounds.left.saturating_sub(padding);
    let px_right = (bounds.right + padding).min(w - 1);

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
    TrimResult {
        image: source.crop_imm(px_left, px_top, tw, th),
        info,
        background: bg,
    }
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::{bordered_rgba, solid_rgba};

    use image::RgbaImage;

    // ======================================================================
    // トリム
    // ======================================================================
    /// トリミング前後で元画像の幅と高さが保持されていることを確認する
    #[test]
    fn trim_info_preserves_original_dimensions() {
        let img = bordered_rgba(20, 16, 3, [255, 255, 255, 255], [0, 0, 0, 255]);
        let result = trim(&img, 0, 0, None);
        assert_eq!(result.info.orig_w, 20);
        assert_eq!(result.info.orig_h, 16);
    }

    /// 縁が正しく削除されることを確認する
    #[test]
    fn trim_removes_white_border() {
        // 10x10 / 縁 2px 白 / 中央 6x6 赤
        let img = bordered_rgba(10, 10, 2, [255, 255, 255, 255], [255, 0, 0, 255]);
        let result = trim(&img, 0, 0, None);
        assert_eq!(result.info.offset_x, 2);
        assert_eq!(result.info.offset_y, 2);
        assert_eq!(result.info.trim_w, 6);
        assert_eq!(result.info.trim_h, 6);
    }

    /// トリミング後の画像サイズが情報と一致することを確認する
    #[test]
    fn trim_result_image_size_matches_info() {
        let img = bordered_rgba(10, 10, 2, [255, 255, 255, 255], [255, 0, 0, 255]);
        let trimmed = trim(&img, 0, 0, None);
        assert_eq!(trimmed.image.width(), trimmed.info.trim_w);
        assert_eq!(trimmed.image.height(), trimmed.info.trim_h);
    }

    /// 全ピクセルが背景色の場合、トリミング後もサイズが変わらないことを確認する
    #[test]
    fn trim_solid_background_returns_original_size() {
        // 全ピクセルが背景色 → トリミング後もサイズ変わらず
        let img = solid_rgba(8, 8, [255, 255, 255, 255]);
        let trimmed = trim(&img, 0, 0, None);
        assert_eq!(trimmed.info.trim_w, 8);
        assert_eq!(trimmed.info.trim_h, 8);
        assert_eq!(trimmed.image.width(), 8);
        assert_eq!(trimmed.image.height(), 8);
    }

    /// 1x1 の画像でもパニックしないことを確認する
    #[test]
    fn trim_1x1_image_does_not_panic() {
        let img = solid_rgba(1, 1, [255, 0, 0, 255]);
        let trimmed = trim(&img, 0, 0, None);
        assert_eq!(trimmed.info.orig_w, 1);
        assert_eq!(trimmed.info.orig_h, 1);
        assert_eq!(trimmed.image.width(), 1);
        assert_eq!(trimmed.image.height(), 1);
    }

    /// 非対称画像のトリミング後の寸法が正しいことを確認する
    #[test]
    fn trim_asymmetric_image_correct_dimensions() {
        // 30x20 / 縁 3px 白 / 中央 24x14 黒
        let img = bordered_rgba(30, 20, 3, [255, 255, 255, 255], [0, 0, 0, 255]);
        let result = trim(&img, 0, 0, None);
        assert_eq!(result.info.orig_w, 30);
        assert_eq!(result.info.orig_h, 20);
        assert_eq!(result.info.trim_w, 24);
        assert_eq!(result.info.trim_h, 14);
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
        let result = trim(&img, 0, 2, None);
        assert_eq!(result.info.offset_x, 3);
        assert_eq!(result.info.offset_y, 3);
        assert_eq!(result.info.trim_w, 14);
        assert_eq!(result.info.trim_h, 14);
    }

    /// padding が画像端でクランプされることを確認する
    #[test]
    fn trim_padding_clamped_at_image_boundary() {
        // 10x10 / 縁 1px 白 / padding=10 → 画像端でクランプ
        let img = bordered_rgba(10, 10, 1, [255, 255, 255, 255], [0, 255, 0, 255]);
        let result = trim(&img, 0, 10, None);
        assert_eq!(result.info.offset_x, 0);
        assert_eq!(result.info.offset_y, 0);
        assert_eq!(result.info.trim_w, 10);
        assert_eq!(result.info.trim_h, 10);
    }

    // ======================================================================
    // トリム - 許容誤差
    // ======================================================================
    /// tol=0 のとき、正確なマッチングでトリミングが行われることを確認する
    #[test]
    fn trim_tolerance_zero_performs_exact_match_trim() {
        let img = bordered_rgba(10, 10, 2, [255, 255, 255, 255], [255, 0, 0, 255]);
        let result = trim(&img, 0, 0, None);
        assert_eq!(result.info.trim_w, 6);
        assert_eq!(result.info.trim_h, 6);
    }

    /// tol=100 のとき、すべての色が背景として扱われることを確認する
    #[test]
    fn trim_max_tolerance_treats_all_colors_as_background() {
        // tol=100 のとき背景(白)との RGB 二乗距離 130050 ≤ 195075 となり、
        // 赤 [255,0,0] も背景とみなされ → 全体が bg → サイズ変わらず
        let img = bordered_rgba(10, 10, 2, [255, 255, 255, 255], [255, 0, 0, 255]);
        let trimmed = trim(&img, 100, 0, None);
        assert_eq!(trimmed.info.trim_w, trimmed.info.orig_w);
        assert_eq!(trimmed.info.trim_h, trimmed.info.orig_h);
        assert_eq!(trimmed.image.width(), 10);
        assert_eq!(trimmed.image.height(), 10);
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
        let result = trim(&img, 0, 0, None);
        assert_eq!(result.info.offset_x, 2);
        assert_eq!(result.info.offset_y, 2);
        assert_eq!(result.info.trim_w, 6);
        assert_eq!(result.info.trim_h, 6);
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
        let result = trim(&img, 0, 0, None);
        assert_eq!(result.info.offset_x, 1);
        assert_eq!(result.info.offset_y, 1);
        assert_eq!(result.info.trim_w, 4);
        assert_eq!(result.info.trim_h, 4);
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

    /// JPEG ノイズで同系色の票が割れても、完全一致の多い別色より同系色クラスタが選ばれることを確認する
    #[test]
    fn detect_bg_groups_noisy_near_white_edges() {
        // 辺の約 3 割を完全一致の黒、残りを多数の異なる近似白にする
        let buf = RgbaImage::from_fn(40, 40, |x, y| {
            let k = x * 7 + y * 13;
            if k % 10 < 3 {
                image::Rgba([0, 0, 0, 255])
            } else {
                let r = 248 + u8::try_from(k % 8).unwrap();
                let g = 248 + u8::try_from(k / 8 % 8).unwrap();
                let b = 250 + u8::try_from(k % 5).unwrap();
                image::Rgba([r, g, b, 255])
            }
        });
        let bg = detect_background_rgba(&buf, 40, 40);
        // 近似白の実在色のいずれかが選ばれる
        assert!(bg[..3].iter().all(|&c| c >= 248), "bg = {bg:?}");
        assert_eq!(bg[3], 255);
    }

    /// クラスタ内では最も多い実在色がそのまま返ることを確認する
    #[test]
    fn detect_bg_returns_exact_majority_color_within_cluster() {
        // 白が多数、近似色 250 が少数 → 平均ではなく白そのものを返す
        let buf = RgbaImage::from_fn(12, 12, |x, y| {
            if (x + y) % 4 == 0 {
                image::Rgba([250, 250, 250, 255])
            } else {
                image::Rgba([255, 255, 255, 255])
            }
        });
        let bg = detect_background_rgba(&buf, 12, 12);
        assert_eq!(bg, [255, 255, 255, 255]);
    }

    /// RGB の異なる透明ピクセルが 1 つのクラスタとして集計されることを確認する
    #[test]
    fn detect_bg_groups_transparent_pixels_regardless_of_rgb() {
        // 辺の約 1/3 を不透明な赤、残りを RGB の異なる 3 種の透明色にする
        let transparent = [[0, 0, 0, 0], [255, 255, 255, 0], [10, 20, 30, 8]];
        let buf = RgbaImage::from_fn(30, 30, |x, y| {
            let k = x * 7 + y * 13;
            if k % 9 < 3 {
                image::Rgba([255, 0, 0, 255])
            } else {
                image::Rgba(transparent[(k % 3) as usize])
            }
        });
        let bg = detect_background_rgba(&buf, 30, 30);
        assert!(bg[3] < ALPHA_TRANSPARENT_THRESHOLD, "bg = {bg:?}");
    }

    // ======================================================================
    // トリム - 不透明な背景上の半透明ピクセル
    // ======================================================================
    /// 不透明な背景と同じ RGB の半透明ピクセルが内容として残ることを確認する
    #[test]
    fn trim_semi_transparent_content_on_opaque_bg_is_kept() {
        // 10x10 / 縁 2px 不透明白 / 中央 6x6 半透明白 (alpha=100)
        let img = bordered_rgba(10, 10, 2, [255, 255, 255, 255], [255, 255, 255, 100]);
        let result = trim(&img, 0, 0, None);
        assert_eq!(result.background, [255, 255, 255, 255]);
        assert_eq!(result.info.offset_x, 2);
        assert_eq!(result.info.offset_y, 2);
        assert_eq!(result.info.trim_w, 6);
        assert_eq!(result.info.trim_h, 6);
    }

    /// 許容度最大ではアルファ差があっても全体を背景とみなすことを確認する
    #[test]
    fn trim_max_tolerance_treats_semi_transparent_as_background() {
        // 不透明白の背景と半透明黒の内容は RGBA 距離が RGB 基準の上限を超えるが、
        // tol=100 では全体が背景 → サイズ変わらず
        let img = bordered_rgba(10, 10, 2, [255, 255, 255, 255], [0, 0, 0, 100]);
        let result = trim(&img, 100, 0, None);
        assert_eq!(result.info.trim_w, 10);
        assert_eq!(result.info.trim_h, 10);
    }

    /// 手動指定の背景色がトリミング結果に反映されることを確認する
    #[test]
    fn trim_manual_background_returns_specified_color() {
        let img = bordered_rgba(10, 10, 2, [255, 255, 255, 255], [255, 0, 0, 255]);
        let manual = [10, 20, 30, 255];
        let result = trim(&img, 0, 0, Some(manual));
        assert_eq!(result.background, manual);
    }

    /// 手動で白を指定したとき、自動検出と同じトリミング結果になることを確認する
    #[test]
    fn trim_manual_white_matches_auto_detection() {
        let img = bordered_rgba(10, 10, 2, [255, 255, 255, 255], [255, 0, 0, 255]);
        let auto = trim(&img, 0, 0, None);
        let manual = trim(&img, 0, 0, Some([255, 255, 255, 255]));
        assert_eq!(auto.info.trim_w, manual.info.trim_w);
        assert_eq!(auto.info.trim_h, manual.info.trim_h);
        assert_eq!(auto.info.offset_x, manual.info.offset_x);
        assert_eq!(auto.info.offset_y, manual.info.offset_y);
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
