//! トリミング結果の型

use image::DynamicImage;

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

/// アスペクト比の固定が適用されなかった理由
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AspectSkip {
    /// 結果が出力サイズの上限 (幅・高さ・画素数) を超える
    SizeLimit,
    /// 切り詰めた結果が元画像と重ならない
    OutsideImage,
}

/// `detect_trim` の戻り値 (切り出し前のトリミング矩形)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrimBounds {
    /// トリミング情報
    pub info: TrimInfo,
    /// 使用した背景色 (RGBA)
    pub background: [u8; 4],
    /// アスペクト比の固定が適用されなかった理由。適用した場合・指定がない場合は `None`
    pub aspect_skip: Option<AspectSkip>,
}
