//! 画像余白トリミングロジックを提供

use std::collections::HashMap;

use image::{DynamicImage, GenericImageView, ImageBuffer, Pixel, Rgba, RgbaImage, imageops};
use rayon::prelude::*;

use crate::consts::{
    ALPHA_TRANSPARENT_THRESHOLD, BG_DETECT_QUANT_SHIFT, MAX_RGB_SQ_DIST, TOL_MAX, TOL_SCALE_DENOM,
    TRIM_PARALLEL_MIN_PIXELS, TRIM_ROW_BLOCK_PIXELS,
};

// ======================================================================
// トリミング結果
// ======================================================================
/// トリミング結果の情報
///
/// 余白が元画像の端を越える場合、越えた分は `extend_*` に持ち、
/// 切り出し後に `fill` で塗った領域として四辺へ追加する
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrimInfo {
    /// 元画像の幅 (px)
    pub orig_w: u32,
    /// 元画像の高さ (px)
    pub orig_h: u32,
    /// トリミング後の幅 (px)。元画像の範囲外へ拡張した余白を含む
    pub trim_w: u32,
    /// トリミング後の高さ (px)。元画像の範囲外へ拡張した余白を含む
    pub trim_h: u32,
    /// 元画像から切り出す開始位置の X オフセット (px)
    pub offset_x: u32,
    /// 元画像から切り出す開始位置の Y オフセット (px)
    pub offset_y: u32,
    /// 元画像の左端より外側へ拡張する余白 (px)
    pub extend_left: u32,
    /// 元画像の上端より外側へ拡張する余白 (px)
    pub extend_top: u32,
    /// 元画像の右端より外側へ拡張する余白 (px)
    pub extend_right: u32,
    /// 元画像の下端より外側へ拡張する余白 (px)
    pub extend_bottom: u32,
    /// 範囲外へ拡張した余白を塗る色 (RGBA)。トリミングに使用した背景色
    pub fill: [u8; 4],
}

impl TrimInfo {
    /// 元画像から切り出す幅を返す
    ///
    /// # Arguments
    /// * `self` - トリミング情報
    ///
    /// # Returns
    /// * `u32` - 切り出す幅 (px)。範囲外へ拡張した余白を含まない
    #[must_use]
    pub fn crop_w(&self) -> u32 {
        self.trim_w
            .saturating_sub(self.extend_left)
            .saturating_sub(self.extend_right)
    }

    /// 元画像から切り出す高さを返す
    ///
    /// # Arguments
    /// * `self` - トリミング情報
    ///
    /// # Returns
    /// * `u32` - 切り出す高さ (px)。範囲外へ拡張した余白を含まない
    #[must_use]
    pub fn crop_h(&self) -> u32 {
        self.trim_h
            .saturating_sub(self.extend_top)
            .saturating_sub(self.extend_bottom)
    }

    /// 元画像の範囲外へ余白を拡張するかどうかを判定する
    ///
    /// # Arguments
    /// * `self` - トリミング情報
    ///
    /// # Returns
    /// * `bool` - いずれかの辺を範囲外へ拡張する場合は `true`
    #[must_use]
    pub fn is_extended(&self) -> bool {
        self.extend_left > 0
            || self.extend_top > 0
            || self.extend_right > 0
            || self.extend_bottom > 0
    }
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

/// `detect_trim` の戻り値 (切り出し前のトリミング矩形)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrimBounds {
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
fn detect_background_with(w: u32, h: u32, pixel: impl Fn(u32, u32) -> [u8; 4]) -> [u8; 4] {
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

/// 画像上の指定座標の RGBA 値を取得する
///
/// 座標が画像範囲外のときは `None` を返す
/// RGBA8 以外の画像も全体を変換せず、指定ピクセルのみを RGBA8 に変換する
///
/// # Arguments
/// * `img` - 対象の画像
/// * `px` - x 座標 (px)
/// * `py` - y 座標 (px)
///
/// # Returns
/// * `Option<[u8; 4]>` - 指定座標の RGBA 値。範囲外なら `None`
#[must_use]
pub fn sample_pixel(img: &DynamicImage, px: u32, py: u32) -> Option<[u8; 4]> {
    if !img.in_bounds(px, py) {
        return None;
    }
    Some(img.get_pixel(px, py).0)
}

/// 画像の四辺から背景余白をトリミングする
///
/// 許容度 `tol` は 0–100 の範囲で指定する
/// 0 のとき背景色と完全一致するピクセルのみを除去し、
/// 100 のときすべての色を背景とみなす
/// 背景が不透明なときは RGBA 各チャネルの差の二乗和を、
/// 最大 RGB 二乗距離 (195,075) を基準にした閾値と比較する
/// `padding` はトリミング後に四辺に追加する余白ピクセル数
/// 元画像の端を越える分は背景色で塗って追加するため、内容から四辺まで常に `padding` の幅になる
/// 背景しかない画像には余白を追加しない
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
    let bounds = detect_trim(img, tol, padding, bg_override);
    TrimResult {
        image: apply_trim(img, &bounds.info),
        info: bounds.info,
        background: bounds.background,
    }
}

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
fn u16_to_u8(c: u16) -> u8 {
    // 最大でも (65535 + 128) / 257 = 255 に収まる
    ((u32::from(c) + 128) / 257) as u8
}

/// 浮動小数点の成分 (0.0–1.0) を 8bit へ丸めて変換する
///
/// 範囲外は 0.0–1.0 に丸め、NaN は白 (1.0) として扱う。`image` クレートの変換に合わせる
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn f32_to_u8(c: f32) -> u8 {
    let clamped = if c < 1.0 { c.max(0.0) } else { 1.0 };
    // clamped は 0.0–1.0 なので 0–255 に収まる
    (clamped * f32::from(u8::MAX)).round() as u8
}

/// トリミング情報に従って画像を切り出す
///
/// 矩形が画像全体のときは切り出さずに複製を返す
/// 範囲外へ拡張する余白があれば、元画像と同じ色形式のキャンバスを `info.fill` で塗って追加する
/// `info` が別の画像のものでも、`crop_imm` が画像範囲へクランプするためパニックしない
///
/// # Arguments
/// * `img`  - 切り出し元の画像 (`info` を求めた画像)
/// * `info` - [`detect_trim`] で求めたトリミング情報
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
    use image::RgbaImage;

    use super::*;
    use crate::test_helpers::{bordered_rgba, solid_rgba};

    // ======================================================================
    // トリム
    // ======================================================================
    /// トリミング前後で元画像の幅と高さが保持されていること
    #[test]
    fn trim_info_preserves_original_dimensions() {
        let img = bordered_rgba(20, 16, 3, [255, 255, 255, 255], [0, 0, 0, 255]);
        let result = trim(&img, 0, 0, None);
        assert_eq!(result.info.orig_w, 20);
        assert_eq!(result.info.orig_h, 16);
    }

    /// 縁が正しく削除されること
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

    /// トリミング後の画像サイズが情報と一致すること
    #[test]
    fn trim_result_image_size_matches_info() {
        let img = bordered_rgba(10, 10, 2, [255, 255, 255, 255], [255, 0, 0, 255]);
        let trimmed = trim(&img, 0, 0, None);
        assert_eq!(trimmed.image.width(), trimmed.info.trim_w);
        assert_eq!(trimmed.image.height(), trimmed.info.trim_h);
    }

    /// 全ピクセルが背景色の場合、トリミング後もサイズが変わらないこと
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

    /// 並列走査の対象サイズでも全面背景の画像が元のサイズのままであること
    #[test]
    fn trim_parallel_solid_background_returns_original_size() {
        let img = solid_rgba(1024, 768, [255, 255, 255, 255]);
        let info = detect_trim(&img, 0, 0, None).info;
        assert_eq!((info.trim_w, info.trim_h), (1024, 768));
        assert_eq!((info.offset_x, info.offset_y), (0, 0));
    }

    /// 並列走査でも離れた 2 点の内容から上下左右の端を正しく求めること
    #[test]
    fn trim_parallel_finds_bounds_of_sparse_content() {
        let mut buf = RgbaImage::from_pixel(1024, 768, Rgba([255, 255, 255, 255]));
        buf.put_pixel(900, 100, Rgba([255, 0, 0, 255]));
        buf.put_pixel(30, 700, Rgba([255, 0, 0, 255]));
        let img = DynamicImage::ImageRgba8(buf);
        let info = detect_trim(&img, 0, 0, None).info;
        // x: 30..=900 / y: 100..=700
        assert_eq!((info.offset_x, info.offset_y), (30, 100));
        assert_eq!((info.trim_w, info.trim_h), (871, 601));
    }

    /// 1x1 の画像でもパニックしないこと
    #[test]
    fn trim_1x1_image_does_not_panic() {
        let img = solid_rgba(1, 1, [255, 0, 0, 255]);
        let trimmed = trim(&img, 0, 0, None);
        assert_eq!(trimmed.info.orig_w, 1);
        assert_eq!(trimmed.info.orig_h, 1);
        assert_eq!(trimmed.image.width(), 1);
        assert_eq!(trimmed.image.height(), 1);
    }

    /// 非対称画像のトリミング後の寸法が正しいこと
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
    /// padding を指定したとき、トリミング結果が正しく拡張されること
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

    /// padding が画像端を越える分は範囲外の余白として追加されること
    #[test]
    fn trim_padding_extends_beyond_image_boundary() {
        // 10x10 / 縁 1px 白 / 中央 8x8 緑 / padding=10 → 8 + 10*2 = 28
        let img = bordered_rgba(10, 10, 1, [255, 255, 255, 255], [0, 255, 0, 255]);
        let result = trim(&img, 0, 10, None);
        let info = result.info;

        // 画像内は全体を切り出し、越えた 9px を四辺へ拡張する
        assert_eq!((info.offset_x, info.offset_y), (0, 0));
        assert_eq!((info.crop_w(), info.crop_h()), (10, 10));
        assert_eq!(
            (
                info.extend_left,
                info.extend_top,
                info.extend_right,
                info.extend_bottom
            ),
            (9, 9, 9, 9)
        );
        assert_eq!((info.trim_w, info.trim_h), (28, 28));
        assert_eq!(info.fill, [255, 255, 255, 255]);

        // 出力画像は背景色で塗られ、内容は padding の位置から始まる
        assert_eq!((result.image.width(), result.image.height()), (28, 28));
        assert_eq!(
            sample_pixel(&result.image, 0, 0),
            Some([255, 255, 255, 255])
        );
        assert_eq!(
            sample_pixel(&result.image, 9, 9),
            Some([255, 255, 255, 255])
        );
        assert_eq!(sample_pixel(&result.image, 10, 10), Some([0, 255, 0, 255]));
        assert_eq!(sample_pixel(&result.image, 17, 17), Some([0, 255, 0, 255]));
        assert_eq!(
            sample_pixel(&result.image, 18, 18),
            Some([255, 255, 255, 255])
        );
        assert_eq!(
            sample_pixel(&result.image, 27, 27),
            Some([255, 255, 255, 255])
        );
    }

    /// 内容が画像の片側に寄っている場合、越えた辺だけが拡張されること
    #[test]
    fn trim_padding_extends_only_exceeding_sides() {
        // 20x10 白 / (1,2)-(3,4) に黒 → 左 1px・上 2px しか余白がない
        let mut buf = RgbaImage::from_pixel(20, 10, image::Rgba([255, 255, 255, 255]));
        for y in 2..=4 {
            for x in 1..=3 {
                buf.put_pixel(x, y, image::Rgba([0, 0, 0, 255]));
            }
        }
        let img = DynamicImage::ImageRgba8(buf);
        let info = detect_trim(&img, 0, 3, None).info;

        // 左は 2px、上は 1px 不足し、右・下は画像内に収まる
        assert_eq!((info.offset_x, info.offset_y), (0, 0));
        assert_eq!(
            (
                info.extend_left,
                info.extend_top,
                info.extend_right,
                info.extend_bottom
            ),
            (2, 1, 0, 0)
        );
        assert_eq!((info.crop_w(), info.crop_h()), (7, 8));
        // 内容 3px + 余白 3px*2 で縦横とも 9px に揃う
        assert_eq!((info.trim_w, info.trim_h), (9, 9));
    }

    /// 範囲外の余白が透明な背景色で塗られること
    #[test]
    fn trim_padding_extension_uses_transparent_background() {
        let img = bordered_rgba(6, 6, 1, [0, 0, 0, 0], [255, 0, 0, 255]);
        let result = trim(&img, 0, 3, None);
        assert_eq!((result.image.width(), result.image.height()), (10, 10));
        assert_eq!(sample_pixel(&result.image, 0, 0).map(|p| p[3]), Some(0));
        assert_eq!(sample_pixel(&result.image, 3, 3), Some([255, 0, 0, 255]));
    }

    /// ほぼ透明な背景色が検出されても範囲外の余白は完全透明で塗られること
    #[test]
    fn trim_padding_extension_on_nearly_transparent_background_is_fully_transparent() {
        let img = bordered_rgba(6, 6, 1, [0, 0, 0, 3], [255, 0, 0, 255]);
        let result = trim(&img, 0, 3, None);
        // 背景色は実際の色のまま、塗り色だけアルファを 0 にする
        assert_eq!(result.background, [0, 0, 0, 3]);
        assert_eq!(result.info.fill, [0, 0, 0, 0]);
        assert_eq!(sample_pixel(&result.image, 0, 0), Some([0, 0, 0, 0]));
        assert_eq!(sample_pixel(&result.image, 3, 3), Some([255, 0, 0, 255]));
    }

    /// 範囲外の余白を追加しても元画像の色形式 (16bit) が保たれること
    #[test]
    fn trim_padding_extension_preserves_color_type() {
        let mut rgb = image::ImageBuffer::from_pixel(4, 4, image::Rgb([65535u16, 65535, 65535]));
        rgb.put_pixel(1, 1, image::Rgb([1000, 2000, 3000]));
        let img = DynamicImage::ImageRgb16(rgb);
        let result = trim(&img, 0, 2, None);

        let DynamicImage::ImageRgb16(out) = &result.image else {
            panic!("色形式が変わった: {:?}", result.image.color());
        };
        // 内容 1px + 余白 2px*2 → 左上 1px 分だけ拡張する
        assert_eq!(out.dimensions(), (5, 5));
        assert_eq!(out.get_pixel(0, 0).0, [65535, 65535, 65535]);
        assert_eq!(out.get_pixel(2, 2).0, [1000, 2000, 3000]);
    }

    /// 背景しかない画像には padding を指定しても余白を追加しないこと
    #[test]
    fn trim_padding_not_applied_to_solid_background() {
        let img = solid_rgba(8, 8, [255, 255, 255, 255]);
        let result = trim(&img, 0, 10, None);
        assert!(!result.info.is_extended());
        assert_eq!((result.image.width(), result.image.height()), (8, 8));
    }

    // ======================================================================
    // トリム - 許容誤差
    // ======================================================================
    /// tol=0 のとき、正確なマッチングでトリミングが行われること
    #[test]
    fn trim_tolerance_zero_performs_exact_match_trim() {
        let img = bordered_rgba(10, 10, 2, [255, 255, 255, 255], [255, 0, 0, 255]);
        let result = trim(&img, 0, 0, None);
        assert_eq!(result.info.trim_w, 6);
        assert_eq!(result.info.trim_h, 6);
    }

    /// tol=100 のとき、すべての色が背景として扱われること
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
    /// 完全に透明なピクセルが背景として扱われること
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

    /// 四隅がすべて透明な画像は透明を背景色とし、半透明の内側を保持すること
    ///
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

    /// 透明な背景上の半透明な影が許容度に応じて背景とみなされること
    #[test]
    fn trim_transparent_canvas_tolerance_removes_semi_transparent_shadow() {
        // 10x10 / 縁 1px 透明 / その内側 1px に半透明の影 (alpha=40) / 中央 6x6 不透明赤
        let mut buf = RgbaImage::new(10, 10);
        for y in 0..10u32 {
            for x in 0..10u32 {
                let px = if !(1..9).contains(&x) || !(1..9).contains(&y) {
                    image::Rgba([0u8, 0, 0, 0])
                } else if !(2..8).contains(&x) || !(2..8).contains(&y) {
                    image::Rgba([0u8, 0, 0, 40])
                } else {
                    image::Rgba([255u8, 0, 0, 255])
                };
                buf.put_pixel(x, y, px);
            }
        }
        let img = DynamicImage::ImageRgba8(buf);

        // tol=0 では影も内容として残る
        let result = trim(&img, 0, 0, None);
        assert_eq!((result.info.offset_x, result.info.trim_w), (1, 8));

        // tol=15 (アルファ差 38 まで) では影 (アルファ差 40) はまだ残る
        let result = trim(&img, 15, 0, None);
        assert_eq!((result.info.offset_x, result.info.trim_w), (1, 8));

        // tol=16 (アルファ差 40 まで) で影が背景とみなされ、不透明な内容だけが残る
        let result = trim(&img, 16, 0, None);
        assert_eq!((result.info.offset_x, result.info.trim_w), (2, 6));

        // tol=99 (アルファ差 252 まで) でも不透明な内容 (アルファ差 255) は残る
        let result = trim(&img, 99, 0, None);
        assert_eq!((result.info.offset_x, result.info.trim_w), (2, 6));
    }

    // ======================================================================
    // detect_bg (private 関数の間接テスト)
    // ======================================================================
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

    // ======================================================================
    // トリム - 不透明な背景上の半透明ピクセル
    // ======================================================================
    /// 不透明な背景と同じ RGB の半透明ピクセルが内容として残ること
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

    /// 許容度最大ではアルファ差があっても全体を背景とみなすこと
    #[test]
    fn trim_max_tolerance_treats_semi_transparent_as_background() {
        // 不透明白の背景と半透明黒の内容は RGBA 距離が RGB 基準の上限を超えるが、
        // tol=100 では全体が背景 → サイズ変わらず
        let img = bordered_rgba(10, 10, 2, [255, 255, 255, 255], [0, 0, 0, 100]);
        let result = trim(&img, 100, 0, None);
        assert_eq!(result.info.trim_w, 10);
        assert_eq!(result.info.trim_h, 10);
    }

    /// 手動指定の背景色がトリミング結果に反映されること
    #[test]
    fn trim_manual_background_returns_specified_color() {
        let img = bordered_rgba(10, 10, 2, [255, 255, 255, 255], [255, 0, 0, 255]);
        let manual = [10, 20, 30, 255];
        let result = trim(&img, 0, 0, Some(manual));
        assert_eq!(result.background, manual);
    }

    /// 手動で白を指定したとき、自動検出と同じトリミング結果になること
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

    // ======================================================================
    // detect_trim / apply_trim
    // ======================================================================
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

    /// `sample_pixel` が画像内の色を正しく返すこと
    #[test]
    fn sample_pixel_returns_rgba_at_coordinates() {
        let img = bordered_rgba(10, 10, 2, [255, 255, 255, 255], [255, 0, 0, 255]);
        assert_eq!(sample_pixel(&img, 0, 0), Some([255, 255, 255, 255]));
        assert_eq!(sample_pixel(&img, 5, 5), Some([255, 0, 0, 255]));
        assert_eq!(sample_pixel(&img, 10, 0), None);
    }

    /// RGBA8 以外の画像でも `sample_pixel` が RGBA に変換した色を返すこと
    #[test]
    fn sample_pixel_converts_non_rgba8_pixel() {
        let mut rgb = image::RgbImage::from_pixel(4, 3, image::Rgb([10, 20, 30]));
        rgb.put_pixel(3, 2, image::Rgb([200, 100, 50]));
        let img = DynamicImage::ImageRgb8(rgb);

        // アルファは不透明として補われる
        assert_eq!(sample_pixel(&img, 0, 0), Some([10, 20, 30, 255]));
        assert_eq!(sample_pixel(&img, 3, 2), Some([200, 100, 50, 255]));

        // 範囲外は幅・高さどちらの超過でも `None`
        assert_eq!(sample_pixel(&img, 4, 0), None);
        assert_eq!(sample_pixel(&img, 0, 3), None);
    }
}
