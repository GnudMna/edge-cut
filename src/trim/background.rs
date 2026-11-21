//! 背景色の検出
//!
//! 四辺の画素を量子化したビンで集計し、最多クラスタ内の最頻色を背景色とする。

use std::collections::HashMap;

use image::RgbaImage;

use crate::consts::{ALPHA_TRANSPARENT_THRESHOLD, BG_DETECT_QUANT_SHIFT};

// ======================================================================
// 背景色検出
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
/// 幅または高さが 0 の画像では白 (不透明) を返す
///
/// # Arguments
/// * `rgba` - 対象画像の RGBA バッファ
///
/// # Returns
/// * `[u8; 4]` - 背景と推定される RGBA 値
#[must_use]
pub fn detect_background_rgba(rgba: &RgbaImage) -> [u8; 4] {
    let (w, h) = rgba.dimensions();
    detect_background_with(w, h, |x, y| rgba.get_pixel(x, y).0)
}

/// 画素読み出し関数を使って四辺の最頻色を背景色として検出する
///
/// 判定規則は [`detect_background_rgba`] と同じ。色形式ごとのバッファを
/// RGBA8 へ一括変換せずに検出するために使う
pub(super) fn detect_background_with(
    w: u32,
    h: u32,
    pixel: impl Fn(u32, u32) -> [u8; 4],
) -> [u8; 4] {
    if w == 0 || h == 0 {
        return [255, 255, 255, 255];
    }
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

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use image::RgbaImage;

    use super::*;

    /// 幅または高さが 0 の画像でも panic せず白を返すこと
    #[test]
    fn detect_bg_empty_image_returns_white() {
        for (w, h) in [(0, 0), (0, 5), (5, 0)] {
            let buf = RgbaImage::new(w, h);
            assert_eq!(
                detect_background_rgba(&buf),
                [255, 255, 255, 255],
                "{w}x{h}"
            );
        }
    }

    /// 1x1 画像では唯一のピクセルの色を返すこと
    #[test]
    fn detect_bg_single_pixel_returns_that_color() {
        let buf = RgbaImage::from_pixel(1, 1, image::Rgba([10, 20, 30, 255]));
        assert_eq!(detect_background_rgba(&buf), [10, 20, 30, 255]);
    }

    /// 四辺のうち 3 辺を白・1 辺を黒にした場合、白が背景色として選ばれること
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
        let bg = detect_background_rgba(&buf);
        assert_eq!(bg, [255, 255, 255, 255]);
    }

    /// 全辺のピクセルが同じ色の場合、その色が背景色として選ばれること
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
        let bg = detect_background_rgba(&buf);
        assert_eq!(bg, [200, 50, 50, 255]);
    }

    /// JPEG ノイズで同系色の票が割れても、完全一致の多い別色より同系色クラスタが選ばれること
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
        let bg = detect_background_rgba(&buf);
        // 近似白の実在色のいずれかが選ばれる
        assert!(bg[..3].iter().all(|&c| c >= 248), "bg = {bg:?}");
        assert_eq!(bg[3], 255);
    }

    /// クラスタ内では最も多い実在色がそのまま返ること
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
        let bg = detect_background_rgba(&buf);
        assert_eq!(bg, [255, 255, 255, 255]);
    }

    /// RGB の異なる透明ピクセルが 1 つのクラスタとして集計されること
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
        let bg = detect_background_rgba(&buf);
        assert!(bg[3] < ALPHA_TRANSPARENT_THRESHOLD, "bg = {bg:?}");
    }
}
