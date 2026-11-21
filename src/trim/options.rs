//! トリミングの設定
//!
//! 許容度・ノイズ除外・辺ごとの余白・トリミングする辺・アスペクト比・背景色の指定をまとめる。

use serde::{Deserialize, Serialize};

// ======================================================================
// 余白・辺
// ======================================================================
/// 四辺それぞれの余白ピクセル数
///
/// 内容から各辺までに残す幅。元画像の端を越える分は背景色で塗って追加する
/// 検出時に `consts::PADDING_MAX` を超える値は `PADDING_MAX` として扱う
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Margins {
    /// 上辺の余白 (px)
    pub top: u32,
    /// 右辺の余白 (px)
    pub right: u32,
    /// 下辺の余白 (px)
    pub bottom: u32,
    /// 左辺の余白 (px)
    pub left: u32,
}

impl Margins {
    /// 四辺に同じ値を指定した余白を返す
    ///
    /// # Arguments
    /// * `value` - 四辺共通の余白 (px)
    ///
    /// # Returns
    /// * `Margins` - 四辺とも `value` の余白
    #[must_use]
    pub const fn uniform(value: u32) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }
}

/// 余白を除去する辺
///
/// `false` の辺は元画像の端を維持し、その辺の余白指定も適用しない
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "四辺それぞれの独立した指定であり、状態の組み合わせではない"
)]
pub struct TrimSides {
    /// 上辺を除去する
    pub top: bool,
    /// 右辺を除去する
    pub right: bool,
    /// 下辺を除去する
    pub bottom: bool,
    /// 左辺を除去する
    pub left: bool,
}

impl TrimSides {
    /// 四辺すべてを除去する指定
    pub const ALL: Self = Self {
        top: true,
        right: true,
        bottom: true,
        left: true,
    };

    /// いずれかの辺を除去するかどうかを返す
    ///
    /// # Returns
    /// * `bool` - 除去する辺が 1 つでもあれば `true`
    #[must_use]
    pub const fn any(self) -> bool {
        self.top || self.right || self.bottom || self.left
    }
}

impl Default for TrimSides {
    fn default() -> Self {
        Self::ALL
    }
}

// ======================================================================
// アスペクト比
// ======================================================================
/// アスペクト比を合わせる方法
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum AspectFit {
    /// 長い方の辺を削り、比率を合わせる。内容が欠ける場合がある
    Crop,
    /// 短い方の辺を背景色で塗って足し、内容を削らずに比率を合わせる
    ///
    /// 未知の値を読み込んだ場合もこの値とする
    #[default]
    #[serde(other)]
    Expand,
}

impl AspectFit {
    /// 選択肢として表示する順序の全方法
    pub const ALL: [Self; 2] = [Self::Expand, Self::Crop];

    /// UI に表示する名称を返す
    ///
    /// # Returns
    /// * `&'static str` - 表示名
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Expand => "余白を足す",
            Self::Crop => "切り詰める",
        }
    }
}

/// 固定するアスペクト比 (幅:高さ)
///
/// 比率は中央を基準に合わせる。結果が上限サイズ (`consts::MAX_IMAGE_*`) を超える場合や、
/// 切り詰めで画像と重ならなくなる場合は、比率を合わせずに元の矩形を使う
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AspectRatio {
    /// 幅の比。0 は 1 として扱う
    pub w: u32,
    /// 高さの比。0 は 1 として扱う
    pub h: u32,
    /// 比率の合わせ方
    pub fit: AspectFit,
}

// ======================================================================
// トリミング設定
// ======================================================================
/// トリミングの設定一式
///
/// 同じ画像に同じ設定を渡せば同じ結果になるため、プレビューの再計算を省く判定にも使う
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct TrimOptions {
    /// 余白検出の許容度 (0–100)
    pub tolerance: u32,
    /// 辺ごとに残す余白
    pub margins: Margins,
    /// 余白を除去する辺
    pub sides: TrimSides,
    /// 固定するアスペクト比。`None` のとき比率は固定しない
    pub aspect: Option<AspectRatio>,
    /// 手動指定の背景色。`None` のとき四辺から自動検出する
    pub bg_override: Option<[u8; 4]>,
    /// 無視するゴミの画素数 (行・列あたり)
    ///
    /// 背景以外の画素がこの数以下の行・列は内容とみなさない。0 のとき 1 画素でも内容とする
    /// スキャンの埃や JPEG ノイズのような孤立した小さなゴミで、余白が残るのを防ぐ
    /// 検出時に `consts::SPECK_MAX` を超える値は `SPECK_MAX` として扱う
    pub ignore_pixels: u32,
}

impl TrimOptions {
    /// 四辺共通の余白で、辺の指定・比率の固定を行わない設定を返す
    ///
    /// # Arguments
    /// * `tolerance`   - 余白検出の許容度 (0–100)
    /// * `padding`     - 四辺共通の余白ピクセル数
    /// * `bg_override` - 手動指定の背景色。`None` のとき四辺から自動検出する
    ///
    /// # Returns
    /// * `TrimOptions` - 四辺すべてを除去し、比率を固定しない設定
    #[must_use]
    pub const fn new(tolerance: u32, padding: u32, bg_override: Option<[u8; 4]>) -> Self {
        Self {
            tolerance,
            margins: Margins::uniform(padding),
            sides: TrimSides::ALL,
            aspect: None,
            bg_override,
            ignore_pixels: 0,
        }
    }
}
