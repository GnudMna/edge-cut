//! 画像余白トリミングロジックを提供
//!
//! 背景色の検出 (`background`)、余白の矩形検出 (`detect`)、アスペクト比の固定 (`aspect`)、
//! 切り出し (`crop`)、設定 (`options`)、結果の型 (`types`) に分け、
//! ここでは公開 API の再エクスポートと `trim` を持つ。

mod aspect;
mod background;
mod crop;
mod detect;
mod options;
mod types;

use image::{DynamicImage, GenericImageView};

pub use self::background::detect_background_rgba;
pub use self::crop::apply_trim;
pub use self::detect::{detect_trim, detect_trim_with_options};
pub use self::options::{AspectFit, AspectRatio, Margins, TrimOptions, TrimSides};
pub use self::types::{AspectSkip, TrimBounds, TrimInfo, TrimResult};

// ======================================================================
// トリミング
// ======================================================================
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
/// 辺ごとの余白・辺の指定・アスペクト比は [`trim_with_options`] で指定する
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
    trim_with_options(img, &TrimOptions::new(tol, padding, bg_override))
}

/// 設定に従って画像をトリミングする
///
/// 余白の検出と判定規則は [`trim`] と同じ。次の指定を加えられる
/// * 辺ごとの余白 (`margins`)
/// * 余白を除去する辺 (`sides`)。除去しない辺は元画像の端を維持し、その辺の余白も加えない
/// * アスペクト比 (`aspect`)。余白を加えた後の矩形を、中央を基準に指定の比率へ合わせる
///
/// # Arguments
/// * `img`  - トリミング元の画像
/// * `opts` - トリミングの設定
///
/// # Returns
/// * `TrimResult` - トリミング後の画像、トリミング情報、使用した背景色
#[must_use]
pub fn trim_with_options(img: &DynamicImage, opts: &TrimOptions) -> TrimResult {
    let bounds = detect_trim_with_options(img, opts);
    TrimResult {
        image: apply_trim(img, &bounds.info),
        info: bounds.info,
        background: bounds.background,
    }
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests;
