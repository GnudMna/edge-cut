//! 永続化する設定

use std::path::PathBuf;

use crate::app::save_dir::default_auto_save_dir;
use crate::consts::{
    ASPECT_PART_MAX, ASPECT_PART_MIN, JPEG_QUALITY_DEFAULT, JPEG_QUALITY_MAX, JPEG_QUALITY_MIN,
    PADDING_MAX, PADDING_MIN, TOL_MAX, TOL_MIN, WEBP_QUALITY_DEFAULT, WEBP_QUALITY_MAX,
    WEBP_QUALITY_MIN,
};
use crate::image_output::OutputFormat;
use crate::trim::{AspectFit, AspectRatio, Margins, TrimOptions, TrimSides};

// ======================================================================
// 設定
// ======================================================================
/// 永続化する設定キー
pub(in crate::app) const SETTINGS_KEY: &str = "settings";

/// UI・トリミング設定
///
/// アプリ状態 (`App::prefs`) としてそのまま保持し、終了時に正規化して保存する。
/// 実行中のみ有効な `bg_override` は保存対象外
///
/// 保存データに無いフィールドは `Default` の値で補う。フィールドの追加・削除・改名があっても
/// 残りの設定を読み込める
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
// 独立した ON/OFF の設定項目であり、状態の組み合わせではない
#[allow(clippy::struct_excessive_bools)]
pub(in crate::app) struct Settings {
    /// 余白検出の許容度 (0–100)
    pub tolerance: u32,
    /// トリミング後に残す余白ピクセル数。`link_margins` が有効なときの四辺共通の値
    ///
    /// 辺ごとの余白を導入する前から保存している設定をそのまま読めるよう、名前と意味を維持する
    pub padding: u32,
    /// 余白を四辺で連動させる (`padding` を使う)。無効のとき `margins` を使う
    pub link_margins: bool,
    /// 辺ごとの余白。`link_margins` が無効なときだけ使う
    pub margins: Margins,
    /// 余白を除去する辺。無効にした辺は元画像の端を維持する
    pub sides: TrimSides,
    /// アスペクト比の固定を有効にする
    pub aspect_enabled: bool,
    /// 固定するアスペクト比の幅
    pub aspect_w: u32,
    /// 固定するアスペクト比の高さ
    pub aspect_h: u32,
    /// アスペクト比の合わせ方
    pub aspect_fit: AspectFit,
    /// ダークモードの有効/無効
    pub dark_mode: bool,
    /// 自動保存モードの有効/無効
    pub auto_save: bool,
    /// 自動保存先ディレクトリ
    pub auto_save_dir: PathBuf,
    /// コンパクト UI モードの有効/無効
    pub compact_mode: bool,
    /// 手動指定の背景色。`None` のとき四辺から自動検出する
    ///
    /// 永続化しない。手動指定の解除を忘れたまま再起動して「自動検出が効かない」と
    /// 誤解するのを防ぐため、起動時は常に自動検出から始める
    #[serde(skip)]
    pub bg_override: Option<[u8; 4]>,
    /// 保存・自動保存・一括トリミングの出力形式
    pub output_format: OutputFormat,
    /// JPEG 保存時のエンコード品質 (1–100)
    pub jpeg_quality: u8,
    /// WebP 保存時のエンコード品質 (1–100)。最大値のときだけロスレス
    pub webp_quality: u8,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            tolerance: 0,
            padding: 0,
            link_margins: true,
            margins: Margins::default(),
            sides: TrimSides::ALL,
            aspect_enabled: false,
            aspect_w: 1,
            aspect_h: 1,
            aspect_fit: AspectFit::default(),
            dark_mode: true,
            auto_save: false,
            auto_save_dir: default_auto_save_dir(),
            compact_mode: false,
            bg_override: None,
            output_format: OutputFormat::default(),
            jpeg_quality: JPEG_QUALITY_DEFAULT,
            webp_quality: WEBP_QUALITY_DEFAULT,
        }
    }
}

impl Settings {
    /// 現在の設定からトリミングの設定を組み立てる
    ///
    /// # Returns
    /// * `TrimOptions` - プレビュー・保存・一括トリミングで共通に使うトリミング設定
    pub(in crate::app) fn trim_options(&self) -> TrimOptions {
        TrimOptions {
            tolerance: self.tolerance,
            margins: if self.link_margins {
                Margins::uniform(self.padding)
            } else {
                self.margins
            },
            sides: self.sides,
            aspect: self.aspect_enabled.then_some(AspectRatio {
                w: self.aspect_w,
                h: self.aspect_h,
                fit: self.aspect_fit,
            }),
            bg_override: self.bg_override,
        }
    }

    /// 永続化データを有効範囲へ正規化する
    ///
    /// 手動編集や将来のバージョン差分で範囲外の値が保存されていても、
    /// 読み込み時にクランプして安全に復元する
    pub(super) fn normalize(mut self) -> Self {
        self.tolerance = self.tolerance.clamp(TOL_MIN, TOL_MAX);
        self.padding = self.padding.clamp(PADDING_MIN, PADDING_MAX);
        for side in [
            &mut self.margins.top,
            &mut self.margins.right,
            &mut self.margins.bottom,
            &mut self.margins.left,
        ] {
            *side = (*side).clamp(PADDING_MIN, PADDING_MAX);
        }
        self.aspect_w = self.aspect_w.clamp(ASPECT_PART_MIN, ASPECT_PART_MAX);
        self.aspect_h = self.aspect_h.clamp(ASPECT_PART_MIN, ASPECT_PART_MAX);
        self.jpeg_quality = self.jpeg_quality.clamp(JPEG_QUALITY_MIN, JPEG_QUALITY_MAX);
        self.webp_quality = self.webp_quality.clamp(WEBP_QUALITY_MIN, WEBP_QUALITY_MAX);
        if self.auto_save_dir.as_os_str().is_empty() {
            self.auto_save_dir = default_auto_save_dir();
        }
        self
    }
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;

    /// 範囲外の許容度が読み込み時にクランプされること
    #[test]
    fn settings_normalize_clamps_tolerance() {
        let s = Settings {
            tolerance: 999,
            ..Settings::default()
        }
        .normalize();
        assert_eq!(s.tolerance, TOL_MAX);

        let s = Settings {
            tolerance: TOL_MAX + 1,
            ..Settings::default()
        }
        .normalize();
        assert_eq!(s.tolerance, TOL_MAX);
    }

    /// 範囲外の余白が読み込み時にクランプされること
    #[test]
    fn settings_normalize_clamps_padding() {
        let s = Settings {
            padding: 200,
            ..Settings::default()
        }
        .normalize();
        assert_eq!(s.padding, PADDING_MAX);
    }

    /// 範囲外の JPEG 品質が読み込み時にクランプされること
    #[test]
    fn settings_normalize_clamps_jpeg_quality() {
        for (input, expected) in [(0, JPEG_QUALITY_MIN), (200, JPEG_QUALITY_MAX), (80, 80)] {
            let s = Settings {
                jpeg_quality: input,
                ..Settings::default()
            }
            .normalize();
            assert_eq!(s.jpeg_quality, expected);
        }
    }

    /// 範囲外の WebP 品質が読み込み時にクランプされること
    #[test]
    fn settings_normalize_clamps_webp_quality() {
        for (input, expected) in [(0, WEBP_QUALITY_MIN), (200, WEBP_QUALITY_MAX), (80, 80)] {
            let s = Settings {
                webp_quality: input,
                ..Settings::default()
            }
            .normalize();
            assert_eq!(s.webp_quality, expected);
        }
    }

    /// 範囲外の辺ごとの余白とアスペクト比が読み込み時にクランプされること
    #[test]
    fn settings_normalize_clamps_margins_and_aspect() {
        let s = Settings {
            margins: Margins {
                top: 500,
                ..Margins::default()
            },
            aspect_w: 0,
            aspect_h: 100_000,
            ..Settings::default()
        }
        .normalize();
        assert_eq!(s.margins.top, PADDING_MAX);
        assert_eq!(s.aspect_w, ASPECT_PART_MIN);
        assert_eq!(s.aspect_h, ASPECT_PART_MAX);
    }

    /// 余白を連動させると四辺とも `padding` になり、解除すると辺ごとの値になること
    #[test]
    fn trim_options_uses_linked_padding_or_margins() {
        let mut s = Settings {
            padding: 7,
            margins: Margins {
                top: 1,
                right: 2,
                bottom: 3,
                left: 4,
            },
            ..Settings::default()
        };
        assert_eq!(s.trim_options().margins, Margins::uniform(7));
        s.link_margins = false;
        assert_eq!(s.trim_options().margins, s.margins);
    }

    /// アスペクト比は有効なときだけトリミング設定へ反映されること
    #[test]
    fn trim_options_includes_aspect_only_when_enabled() {
        let mut s = Settings {
            aspect_w: 16,
            aspect_h: 9,
            aspect_fit: AspectFit::Crop,
            ..Settings::default()
        };
        assert_eq!(s.trim_options().aspect, None);
        s.aspect_enabled = true;
        assert_eq!(
            s.trim_options().aspect,
            Some(AspectRatio {
                w: 16,
                h: 9,
                fit: AspectFit::Crop
            })
        );
    }

    /// 辺ごとの設定を持たない旧バージョンの保存データを読み込めること
    #[test]
    fn settings_deserialize_legacy_without_new_fields() {
        let json = r#"{"tolerance":5,"padding":12}"#;
        let s: Settings = serde_json::from_str(json).expect("旧形式を読める");
        assert_eq!((s.tolerance, s.padding), (5, 12));
        assert!(s.link_margins);
        assert_eq!(s.sides, TrimSides::ALL);
        assert!(!s.aspect_enabled);
        assert_eq!(s.trim_options().margins, Margins::uniform(12));
    }

    /// 空の自動保存先がデフォルトパスへ置き換わること
    #[test]
    fn settings_normalize_empty_auto_save_dir_uses_default() {
        let s = Settings {
            auto_save_dir: PathBuf::new(),
            ..Settings::default()
        }
        .normalize();
        assert!(!s.auto_save_dir.as_os_str().is_empty());
    }
    /// デフォルト設定で手動背景色が未指定であること
    #[test]
    fn settings_default_bg_override_is_none() {
        let s = Settings::default();
        assert_eq!(s.bg_override, None);
    }
}
