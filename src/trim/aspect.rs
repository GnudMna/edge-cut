//! アスペクト比の固定
//!
//! 元画像の範囲外も含む矩形 (`Rect`) を、中央を基準に指定の比率へ広げる・切り詰める。

use super::options::{AspectFit, AspectRatio, TrimSides};
use super::types::AspectSkip;
use crate::consts::{MAX_IMAGE_HEIGHT, MAX_IMAGE_PIXELS, MAX_IMAGE_WIDTH};

// ======================================================================
// 矩形
// ======================================================================
/// 元画像の座標系での矩形 (終端は含まない)
///
/// 余白により元画像の範囲外 (負の座標や幅・高さ以上) へ出ることがある
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Rect {
    pub x0: i64,
    pub y0: i64,
    pub x1: i64,
    pub y1: i64,
}

impl Rect {
    fn width(self) -> u64 {
        self.x1.abs_diff(self.x0)
    }

    fn height(self) -> u64 {
        self.y1.abs_diff(self.y0)
    }

    /// 幅 `w`・高さ `h` の画像 (原点 0,0) と 1 画素以上重なるかどうか
    fn overlaps_image(self, w: u32, h: u32) -> bool {
        self.x0 < i64::from(w) && self.x1 > 0 && self.y0 < i64::from(h) && self.y1 > 0
    }
}

// ======================================================================
// 比率の調整
// ======================================================================
/// 矩形を指定のアスペクト比へ合わせる
///
/// 中央を基準に増減し、奇数のときは右・下を 1 px 多くする (増やす場合) か
/// 左・上を 1 px 多く削る (減らす場合)。比率は整数の丸めにより ±1 px の差が出る
/// 除去しない辺 (`sides` が `false`) は動かさず、同じ軸のもう一方の辺だけで増減する
/// 軸の両辺とも除去しない場合は、固定する辺がないため中央を基準にする
///
/// 次の場合は比率を合わせず `rect` をそのまま返し、理由を `AspectSkip` で返す
/// * 結果が `MAX_IMAGE_WIDTH` / `MAX_IMAGE_HEIGHT` / `MAX_IMAGE_PIXELS` を超える
/// * 切り詰めた結果が元画像と重ならない
///
/// # Arguments
/// * `rect`  - 比率を合わせる前の矩形 (幅・高さとも 1 以上)
/// * `ratio` - 固定する比率
/// * `sides` - 除去する辺
/// * `img_w` - 元画像の幅 (px)
/// * `img_h` - 元画像の高さ (px)
///
/// # Returns
/// * `(Rect, Option<AspectSkip>)` - 比率を合わせた矩形と、適用しなかった場合の理由
pub(super) fn fit_aspect(
    rect: Rect,
    ratio: AspectRatio,
    sides: TrimSides,
    img_w: u32,
    img_h: u32,
) -> (Rect, Option<AspectSkip>) {
    let (w, h) = (rect.width(), rect.height());
    let (aw, ah) = (u64::from(ratio.w.max(1)), u64::from(ratio.h.max(1)));
    // w / h と aw / ah の大小は交差積で比べる
    let too_narrow = w * ah < h * aw;
    let (new_w, new_h) = match ratio.fit {
        AspectFit::Expand if too_narrow => ((h * aw).div_ceil(ah), h),
        AspectFit::Expand => (w, (w * ah).div_ceil(aw)),
        AspectFit::Crop if too_narrow => (w, ((w * ah) / aw).max(1)),
        AspectFit::Crop => (((h * aw) / ah).max(1), h),
    };
    if new_w > u64::from(MAX_IMAGE_WIDTH)
        || new_h > u64::from(MAX_IMAGE_HEIGHT)
        || new_w * new_h > MAX_IMAGE_PIXELS
    {
        return (rect, Some(AspectSkip::SizeLimit));
    }

    let fitted = resize(rect, new_w, new_h, sides);
    if fitted.overlaps_image(img_w, img_h) {
        (fitted, None)
    } else {
        (rect, Some(AspectSkip::OutsideImage))
    }
}

/// 除去しない辺を動かさずに矩形の幅と高さを変える
fn resize(rect: Rect, new_w: u64, new_h: u64, sides: TrimSides) -> Rect {
    // 上限チェック済みのため i64 に収まる
    let (new_w, new_h) = (
        i64::try_from(new_w).unwrap_or(i64::MAX),
        i64::try_from(new_h).unwrap_or(i64::MAX),
    );
    let (x0, x1) = resize_axis(rect.x0, rect.x1, new_w, sides.left, sides.right);
    let (y0, y1) = resize_axis(rect.y0, rect.y1, new_h, sides.top, sides.bottom);
    Rect { x0, y0, x1, y1 }
}

/// 1 軸の区間 `[lo, hi)` の長さを `new_len` に変える
///
/// 片側だけ除去しない場合はその端を固定し、それ以外は中央を基準にする
fn resize_axis(lo: i64, hi: i64, new_len: i64, trim_lo: bool, trim_hi: bool) -> (i64, i64) {
    match (trim_lo, trim_hi) {
        (false, true) => (lo, lo + new_len),
        (true, false) => (hi - new_len, hi),
        _ => {
            let new_lo = lo - (new_len - (hi - lo)).div_euclid(2);
            (new_lo, new_lo + new_len)
        }
    }
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x0: i64, y0: i64, x1: i64, y1: i64) -> Rect {
        Rect { x0, y0, x1, y1 }
    }

    fn ratio(w: u32, h: u32, fit: AspectFit) -> AspectRatio {
        AspectRatio { w, h, fit }
    }

    /// 四辺とも除去する設定で比率を合わせ、矩形だけを返す
    fn fit_all(rect: Rect, ratio: AspectRatio, img_w: u32, img_h: u32) -> Rect {
        fit_aspect(rect, ratio, TrimSides::ALL, img_w, img_h).0
    }

    /// 横長の矩形を 1:1 に広げると高さが幅に揃い、上下へ均等に足すこと
    #[test]
    fn expand_wide_rect_to_square() {
        let fitted = fit_all(
            rect(10, 10, 30, 14),
            ratio(1, 1, AspectFit::Expand),
            100,
            100,
        );
        assert_eq!(fitted, rect(10, 2, 30, 22));
    }

    /// 縦長の矩形を 16:9 に広げると幅が切り上げた値になり、奇数分は右へ足すこと
    #[test]
    fn expand_tall_rect_to_landscape() {
        // 高さ 9 に対する幅は 16。幅 5 から 11 増えるため左へ 5、右へ 6
        let fitted = fit_all(
            rect(20, 0, 25, 9),
            ratio(16, 9, AspectFit::Expand),
            100,
            100,
        );
        assert_eq!((fitted.width(), fitted.height()), (16, 9));
        assert_eq!((fitted.x0, fitted.x1), (15, 31));
    }

    /// すでに指定の比率なら矩形を変えないこと
    #[test]
    fn exact_ratio_is_unchanged() {
        for fit in AspectFit::ALL {
            let r = rect(0, 0, 160, 90);
            assert_eq!(fit_all(r, ratio(16, 9, fit), 200, 200), r);
        }
    }

    /// 切り詰めでは長い辺だけを中央から削り、比率に合わせること
    #[test]
    fn crop_wide_rect_to_square() {
        let fitted = fit_all(rect(0, 0, 30, 10), ratio(1, 1, AspectFit::Crop), 30, 10);
        assert_eq!(fitted, rect(10, 0, 20, 10));
    }

    /// 切り詰めで奇数分は左・上を多く削ること
    #[test]
    fn crop_odd_difference_cuts_start_more() {
        let fitted = fit_all(rect(0, 0, 11, 4), ratio(1, 1, AspectFit::Crop), 11, 4);
        assert_eq!(fitted, rect(4, 0, 8, 4));
    }

    /// 比率の 0 は 1 として扱いパニックしないこと
    #[test]
    fn zero_ratio_part_is_treated_as_one() {
        let fitted = fit_all(rect(0, 0, 8, 4), ratio(0, 0, AspectFit::Expand), 8, 4);
        assert_eq!((fitted.width(), fitted.height()), (8, 8));
    }

    /// 上限サイズを超える場合は矩形を変えないこと
    #[test]
    fn oversized_result_keeps_rect() {
        let r = rect(0, 0, i64::from(MAX_IMAGE_WIDTH), 10);
        assert_eq!(
            fit_all(r, ratio(1, 1, AspectFit::Expand), MAX_IMAGE_WIDTH, 10),
            r
        );
    }

    /// 切り詰めた結果が元画像と重ならない場合は矩形を変えないこと
    #[test]
    fn crop_outside_image_keeps_rect() {
        // 元画像は 1x1。極端な比率へ切り詰めると、矩形が画像の上側へ外れる
        let r = rect(-50, -50, 51, 1);
        assert_eq!(fit_all(r, ratio(100, 1, AspectFit::Crop), 1, 1), r);
    }

    /// 除去しない辺は動かさず、反対側だけで広げること
    #[test]
    fn expand_keeps_pinned_side() {
        // 上辺を除去しない (元画像の端 0 を維持)。高さ 4 を 10 へ広げると下だけが伸びる
        let r = rect(0, 0, 10, 4);
        let (fitted, skip) = fit_aspect(
            r,
            ratio(1, 1, AspectFit::Expand),
            TrimSides {
                top: false,
                ..TrimSides::ALL
            },
            10,
            4,
        );
        assert_eq!(fitted, rect(0, 0, 10, 10));
        assert_eq!(skip, None);
    }

    /// 除去しない辺は動かさず、反対側だけを削ること
    #[test]
    fn crop_keeps_pinned_side() {
        // 左辺を除去しない。幅 30 を 10 へ削ると右だけが削れる
        let (fitted, skip) = fit_aspect(
            rect(0, 0, 30, 10),
            ratio(1, 1, AspectFit::Crop),
            TrimSides {
                left: false,
                ..TrimSides::ALL
            },
            30,
            10,
        );
        assert_eq!(fitted, rect(0, 0, 10, 10));
        assert_eq!(skip, None);
    }

    /// 軸の両辺とも除去しない場合は中央を基準にすること
    #[test]
    fn both_pinned_axis_uses_center() {
        let (fitted, _) = fit_aspect(
            rect(10, 10, 30, 14),
            ratio(1, 1, AspectFit::Expand),
            TrimSides {
                top: false,
                bottom: false,
                ..TrimSides::ALL
            },
            100,
            100,
        );
        assert_eq!(fitted, rect(10, 2, 30, 22));
    }

    /// 上限を超えて適用しない場合に理由を返すこと
    #[test]
    fn skipped_result_reports_reason() {
        let r = rect(0, 0, i64::from(MAX_IMAGE_WIDTH), 10);
        let (_, skip) = fit_aspect(
            r,
            ratio(1, 1, AspectFit::Expand),
            TrimSides::ALL,
            MAX_IMAGE_WIDTH,
            10,
        );
        assert_eq!(skip, Some(AspectSkip::SizeLimit));

        let (_, skip) = fit_aspect(
            rect(-50, -50, 51, 1),
            ratio(100, 1, AspectFit::Crop),
            TrimSides::ALL,
            1,
            1,
        );
        assert_eq!(skip, Some(AspectSkip::OutsideImage));
    }
}
